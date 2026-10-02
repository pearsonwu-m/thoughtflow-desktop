# Architecture

Thoughtflow is a Tauri 2 app. A Rust process owns everything sensitive or stateful: the API key, the network, the database, the global shortcut, and windows. The React UI is a thin client that calls typed Tauri commands.

```
┌──────────────────────── webview (React + TypeScript) ───────────────────────┐
│  WidgetApp ── Composer · ConversationView · PromptCard · Archive · PlanCard │
│  SettingsApp                                                                │
│  state/widget.ts (reducer)        lib/api.ts (typed invoke + event wrappers)│
└──────────────────────────────────────┬──────────────────────────────────────┘
                     Tauri commands (JSON) │ ▲ Channel<StreamEvent>, events
┌──────────────────────────────────────▼──┴───────── Rust ────────────────────┐
│ commands.rs ── thin: validate, lock, delegate                               │
│   ├─ conversation.rs ── build request from memory → stream → persist        │
│   │     └─ ai/ (provider-agnostic types, prompts)                           │
│   │           └─ ai/anthropic/ (HTTP, SSE, errors, model capabilities) ──────┼──► api.anthropic.com
│   ├─ db/ ── SQLite: thoughts, messages, plans, tasks, prompts, FTS5         │
│   ├─ settings.rs (JSON file)      secrets.rs (macOS Keychain)               │
│   └─ desktop/ ── widget window · tray · app menu · global shortcut          │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Layers

| Layer | Files | Responsibility |
| --- | --- | --- |
| UI | `src/widget`, `src/settings`, `src/components` | Rendering and keyboard handling. No network, no secrets. |
| Application state | `src/state/widget.ts` | Pure reducer for the widget: view, mode, draft, pending stream, errors. Unit-tested. |
| Command API | `src/lib/api.ts` ↔ `src-tauri/src/commands.rs` | The only bridge. Errors arrive as `{ kind, message, retryable }`. |
| Conversation | `src-tauri/src/conversation.rs` | Turns user input plus memory into a request, streams the reply, and saves the turn. |
| AI provider | `src-tauri/src/ai` | `ai/mod.rs` defines provider-neutral types (`Mode`, `Turn`, `StreamEvent`, `AiError`). Everything Anthropic-specific is in `ai/anthropic/`. |
| Memory | `src-tauri/src/db` | SQLite schema and migrations, FTS5 search, related-note retrieval, export, secure deletion. |
| Shortcuts | `src-tauri/src/desktop/shortcut.rs`, `src/lib/shortcuts.ts` | Global ⌥Space (Rust) and rebindable in-app shortcuts (TypeScript), using one accelerator format. |
| Settings | `src-tauri/src/settings.rs`, `src-tauri/src/secrets.rs` | Preferences as JSON (atomic writes, tolerant of corruption). The API key lives only in the Keychain. |

Shared shapes (`Thought`, `Message`/`Conversation`, `Plan`, `Task`, `Prompt`, `Settings`) are defined in `src/types.ts` and mirrored by serde structs in `src-tauri/src/db/models.rs` and `settings.rs` (camelCase on the wire).

## A Claude request, end to end

1. **⌘↵** in the composer. `WidgetApp.send` dispatches `sendStarted`, so the user's words and a "Sending to Claude · model" status appear at once. It then calls `send_message` with a request id, the mode, any related notes the user left selected, and the local date.
2. **`conversation::send`** loads the thought's stored turns and builds the new user turn: an optional `<related_notes>` block, then the user's text. If the mode changed (or this is the first turn), it appends an app instruction such as `Mode: Plan. … Today is …`.
3. **`ai::anthropic::request::chat_request`** renders the request:
   - The frozen `SYSTEM_PROMPT` (`ai/prompts.rs`) is sent unchanged on every request, with a cache breakpoint.
   - Stored turns are replayed **verbatim**, including `thinking` blocks and their signatures.
   - App instructions become `{"role": "system"}` messages on models that support mid-conversation system messages, and are folded into the user turn as `<thoughtflow_mode>` text otherwise.
   - Model capabilities (`caps.rs`) decide whether to send `output_config.effort`, `temperature`, and `fallbacks: "default"` (with its beta header).
4. **`AnthropicClient::stream_message`** POSTs with `stream: true`, retries 429/5xx/overload/network errors before any output (honoring `retry-after`), parses SSE (`sse.rs`), and rebuilds the full message (`accumulate.rs`). Visible text is forwarded to the UI over a Tauri `Channel` as `delta` events.
5. **Persistence happens only after the reply completes.** One transaction creates the thought (on first reply), inserts the user turn, the mode instruction, and the assistant turn, updates the summary used for memory, and saves any `<prompt>` block as a `Prompt`. A cancelled (⌘.) or failed request leaves nothing half-written, and the UI restores the user's text.

### Why history is append-only

Current Claude models bind their reasoning (`thinking` blocks) to the exact conversation that produced them. Editing earlier turns, rebuilding the system prompt, or dropping blocks from the middle invalidates that reasoning, and on newer accounts the request is rejected. The same discipline keeps prompt caching effective. So Thoughtflow:

- never edits or reorders stored turns; mode switches are *appended* as app messages;
- keeps the system prompt identical for every request;
- stores each turn's provider payload exactly as sent or received, and replays it unchanged;
- implements **Regenerate** by removing only the final assistant turn, which leaves every earlier turn and its reasoning intact.

### Structured extraction

**Save plan** and **Save tasks** make a separate, non-streaming request containing only that thought's transcript, with a JSON Schema (`plan_schema`, `tasks_schema`) passed as `output_config.format`. Models without structured outputs get the schema in the prompt instead, and the JSON is parsed tolerantly. Results are validated with serde, normalized (dates, enums, empty fields), and saved.

## Memory retrieval

Retrieval is deliberately simple and transparent. There's no vector database.

- `thoughts_fts` (title, raw input, summary, tags) and `messages_fts` (conversation text) are SQLite FTS5 tables, kept in sync by triggers.
- While you type the first message of a new thought (or anything in Reflect mode), the UI calls `find_related`. That command OR-queries the meaningful words of your draft (stopwords removed), ranks by BM25, excludes thoughts marked *Keep out of Claude's memory*, and returns at most three.
- The suggestions appear as chips under the composer ("Sending 2 related notes as context"). Each can be deselected. Only selected notes are sent: title, date, tags, an excerpt of the original thought, and its one-line takeaway (`summary`, derived locally from Claude's reply).
- History search uses the same indexes with AND semantics and prefix matching, plus a substring fallback for scripts FTS5 doesn't segment (for example Chinese).

## Windows and the desktop

- **Widget** (`label: widget`): frameless, transparent, always on top, never destroyed. Its AppKit collection behavior is set to *MoveToActiveSpace + FullScreenAuxiliary* (`configure_widget`), so each time it's summoned it appears on the current Space, including over full-screen apps. Tauri's "visible on all workspaces" option doesn't reach full-screen Spaces. Showing it unhides the app, orders the window front, and activates the app in one main-thread closure (`present`), because issuing those steps separately can order the window in while the app is still hidden. The visible card is drawn by CSS with a margin for its shadow. A `ResizeObserver` reports the card's height and `resize_widget` resizes the window to fit, keeping it on screen. On show it is placed on the monitor under the pointer: at the remembered position, or centered or top per settings. On hide it plays a short exit animation, saves its position, and returns focus to the previous app.
- **Settings** (`label: settings`): an ordinary window, created on demand. The same JS bundle serves both windows; `main.tsx` picks the root from the window label.
- **Activation policy** is `Accessory` (plus `LSUIElement` in `Info.plist`): no Dock icon, and the widget can float over full-screen apps.
- **App menu**: hidden, but it provides the standard Edit key equivalents (⌘C/⌘V/⌘Z) in text fields. "Hide" is deliberately absent so ⌘H reaches the widget.
- **Single instance**: launching again forwards to the running app. With `--toggle`, `--hide`, `--new`, `--history`, or `--settings` it performs that action; otherwise it shows the widget. Launch at login uses a LaunchAgent with `--background`, so a login launch stays in the menu bar.

## Errors

Every failure becomes a short sentence for the UI. Examples: *"Claude couldn't be reached. Check your connection and try again."*, invalid key, model not available, rate limit (with the retry delay), overload, refusal, malformed output, and database errors. A damaged database is set aside and replaced with a fresh one at startup instead of preventing launch. A global-shortcut conflict is reported in the widget and in Settings, and if a newly recorded shortcut fails, the previous one is restored. Nothing panics on a Claude failure. Raw thought content is never written to logs or error messages.

## Extension points

- **Another AI provider:** implement a sibling of `ai/anthropic` and switch on a setting in `conversation.rs`. Stored turns carry a `provider` column, and turns from a different provider degrade to their visible text (`turn_from_stored`).
- **Reminders (opt-in, not implemented):** tasks and plans already store due dates and deadlines. A reminder service would run a timer in `lib.rs`, query due items, and post a notification only when a future `general.reminders` setting is on. Thoughtflow is passive by default and never interrupts on its own.

## Testing

- `cargo test`: unit tests for SSE parsing, stream accumulation (thinking signatures, refusals, fallback sanitizing), request building per model, error mapping, settings persistence, Keychain key validation, migrations, FTS search (including CJK fallback), memory exclusions, cascading deletes, and export. An end-to-end client test runs against a local mock HTTP server that serves real Anthropic SSE frames, including a 529 retry with `retry-after`.
- `vitest`: shortcut parsing, matching, recording and conflicts, Markdown parsing and `<prompt>` splitting, date grouping, model capability gating, and the widget reducer.
- `npm run dev:web`: the full UI against an in-memory mock (`src/lib/mock.ts`) for fast visual iteration.
