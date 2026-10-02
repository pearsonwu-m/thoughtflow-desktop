# Thoughtflow

A persistent, minimalist thinking companion for macOS. Press **⌥Space** from any app, dump a half-formed thought, and think it through with Claude. Thoughtflow asks one useful question at a time, organizes what you said, and turns it into a plan, a short task list, or a polished prompt for another AI.

```
Capture → Claude understands → one good question → you answer → Claude organizes
        → you choose a direction → a concrete plan → saved and searchable
```

Thoughtflow is a thinking layer over your desktop, not a chat app. There's no Dock icon and no chat bubbles. It's a small, typographic notebook page that appears when you need it and disappears when you don't.

## Features

- **Global shortcut.** ⌥Space toggles a frameless, always-on-top widget over whatever app you're using, including full-screen apps. It's draggable, remembers its position, and opens on the screen you're working on.
- **Menu-bar icon.** Open Thoughtflow, New Thought, Search Thoughts, Settings, Quit.
- **Five modes:**
  - **Think** (default): Claude acknowledges, summarizes, names the key tension, asks *one* question, and offers 2–4 directions.
  - **Plan** (⌘P): objective, why it matters, ordered steps, next action, obstacles, deadline. Save it with ⌘S.
  - **Task** (⌘T): concrete tasks with priority, effort, and optional due dates. Save them with ⌘S.
  - **Reflect** (⌘R): careful, non-judgmental observations about contradictions, assumptions, and recurring concerns.
  - **Prompt** (⌘⇧P): clarifies what's missing, then writes one high-quality prompt for Claude, ChatGPT, Gemini, Cursor, and others, with **Copy Prompt**, **Regenerate**, and **Edit**.
- **Local memory.** Every thought is saved in SQLite on your Mac with full-text search. When you start a new thought, related past notes are suggested as context. You see each one and can deselect it before anything is sent.
- **Archive.** A notebook-style history (Today / Yesterday / …) with search, plus saved plans (steps, status, deadline) and lightweight tasks (complete, edit, delete).
- **Keyboard-first.** Everything works without a mouse, and every in-app shortcut can be rebound.
- **API key or Claude Code.** Connect with an Anthropic API key, or with the Claude Code CLI already installed on your Mac (for example, signed in with your Claude subscription), so no API key is needed.
- **Private by default.** The API key lives in the macOS Keychain and never reaches the UI layer. Nothing leaves your Mac until you ask Claude for something. Export and delete-everything are built in.

## Requirements

- macOS 11 or later (Apple silicon or Intel)
- [Node.js](https://nodejs.org) 20+ and npm
- [Rust](https://rustup.rs) stable 1.85+ (`rustup default stable`)
- Xcode Command Line Tools (`xcode-select --install`)
- An [Anthropic API key](https://console.anthropic.com/settings/keys), **or** [Claude Code](https://claude.com/product/claude-code) installed and signed in

## Quick start

```bash
git clone https://github.com/pearsonwu-m/thoughtflow-desktop.git
cd thoughtflow-desktop
npm install
npm run dev
```

The first `npm run dev` compiles the Rust backend, which takes a few minutes. Thoughtflow then appears with its widget open and an icon in the menu bar.

1. Press **⌘,** (or use the menu-bar icon → Settings…) and open **Claude**. Either paste your API key and press **Save**, or choose **Connect with: Claude Code** to use the `claude` CLI you're already signed in to.
2. Press **⌥Space** anywhere, type a thought, and press **⌘↵**.

## Commands

| Command | What it does |
| --- | --- |
| `npm install` | Install JavaScript dependencies (Rust crates download on first build) |
| `npm run dev` | Run the full desktop app with hot reload |
| `npm run dev:web` | Run only the UI in a browser at http://localhost:1420, backed by an in-memory mock (no Rust, no API key, canned replies) |
| `npm test` | Frontend unit tests (Vitest) and backend tests (`cargo test`) |
| `npm run lint` | ESLint and `cargo clippy -D warnings` |
| `npm run typecheck` | Strict TypeScript check |
| `npm run check` | All of the above |
| `npm run build` | Production build: `src-tauri/target/release/bundle/macos/Thoughtflow.app` and a `.dmg` |
| `npm run icons` | Regenerate app and menu-bar icons from `src-tauri/icons/source/*.svg` |

### Production builds

`npm run build` produces an unsigned app. To run it on your own Mac, drag `Thoughtflow.app` into `/Applications` and open it. If macOS blocks a copy that was downloaded or moved between machines, right-click the app and choose **Open**. Signing and notarizing for distribution needs an Apple Developer ID; see the [Tauri macOS signing guide](https://v2.tauri.app/distribute/sign/macos/).

## How Claude authentication works

Settings → Claude → **Connect with** offers two options.

### API key

- In **Settings → Claude**, the key is handed straight to the Rust backend, which stores it in the **macOS Keychain** (service `com.thoughtflow.desktop`). The UI can only ask whether a key exists; it can never read it back.
- Every Claude request is made by the Rust backend (`src-tauri/src/ai/anthropic`). The webview never sees the key or talks to the network.
- For development, you can instead put `ANTHROPIC_API_KEY=…` in a `.env` file (copy `.env.example`). It's used only when no Keychain key exists.
- **Test connection** lists your available models. It costs nothing and sends no thought content.

### Claude Code (no API key)

Thoughtflow can send requests through the [Claude Code](https://claude.com/product/claude-code) CLI on your Mac, using whatever it's signed in with (for example, a Claude Pro or Max subscription).

- Thoughtflow finds `claude` automatically (`~/.local/bin`, Homebrew, or your login shell's PATH), or you can set its location in Settings.
- Each request runs `claude -p` in an empty folder inside Thoughtflow's data directory, with Claude Code's tools, MCP servers, plugins, hooks, skills, and CLAUDE.md turned off (`--tools "" --safe-mode --strict-mcp-config --disable-slash-commands`). Sessions aren't saved to your Claude Code history (`--no-session-persistence`).
- The prompt is passed on stdin, so thought text never appears in the process list. Any `ANTHROPIC_API_KEY` in Thoughtflow's environment is removed, so Claude Code always uses its own sign-in.
- **Check again** reads `claude --version` and `claude auth status`; it sends nothing to Claude.
- Requests through Claude Code count toward that account's usage limits. Model and Response depth apply; temperature and Longest reply are API-only.

### Model and behavior

The default model is **Claude Opus 5.5** at the **Quick** response depth (`effort: low`), which keeps replies fast. In Settings you can choose Claude Sonnet 5.5, Haiku 4.5, Fable 5.1, or any model id your key has access to, and set the response depth, temperature (only for models that accept it), and the longest reply. Request parameters adapt to each model's capabilities (`src-tauri/src/ai/anthropic/caps.rs`). For models that support it, refusals are automatically retried on a fallback model server-side (`fallbacks: "default"`).

## Keyboard

| Shortcut | Action |
| --- | --- |
| **⌥Space** | Open or close Thoughtflow from any app (focuses it if it's open in the background) |
| ⌘↵ | Send to Claude |
| Esc | Close the widget (or leave the archive) |
| ⌘K | Ask Claude for its one most useful clarifying question |
| ⌘I / ⌘P / ⌘T / ⌘R / ⌘⇧P | Think / Plan / Task / Reflect / Prompt mode. With an empty composer, also asks Claude to produce that output now |
| ⌘S | Save: a new thought without sending it, or the plan/tasks Claude just wrote |
| ⌘H | Thought history (⌘1 Thoughts, ⌘2 Plans, ⌘3 Tasks; ↑↓ to move, ↵ to open, ⌘⌫ to delete) |
| ⌘N | New thought |
| ⌘. | Stop Claude's reply |
| ⌘, | Settings (⌘1–⌘5 switch sections) |

Change any of these in **Settings → Keyboard**. If ⌥Space is taken by another app (some launchers and input-source switchers use it), Thoughtflow says so and you can pick a different shortcut in **Settings → General**.

### Scripting

While Thoughtflow is running, launching it again with a flag forwards the action to the running app. You can bind these in Raycast, Alfred, Shortcuts, or a shell alias:

```bash
/Applications/Thoughtflow.app/Contents/MacOS/thoughtflow --toggle   # also --hide, --new, --history, --settings
```

## Where your data lives

| What | Where |
| --- | --- |
| Thoughts, conversations, plans, tasks, prompts | `~/Library/Application Support/com.thoughtflow.desktop/thoughtflow.db` (SQLite) |
| Settings | `~/Library/Application Support/com.thoughtflow.desktop/settings.json` |
| API key | macOS Keychain, item `com.thoughtflow.desktop` / `anthropic-api-key` |

**Settings → Memory** shows the location, exports everything as JSON, clears history, or deletes all data (including settings and the Keychain item). Deleting is permanent: SQLite's `secure_delete` is on, and the write-ahead log is truncated afterwards.

See **[docs/PRIVACY.md](docs/PRIVACY.md)** for exactly what is sent to Anthropic, and when.

## Project layout

```
src/                     React + TypeScript UI (one bundle, two windows)
  widget/                floating widget: composer, conversation, archive
  settings/              settings window
  lib/                   typed command API, shortcuts, Markdown, formatting, browser mock
  state/                 widget reducer
  types.ts               Thought, Message, Conversation, Plan, Task, Prompt, Settings…
src-tauri/src/           Rust backend
  ai/                    provider-agnostic types and prompts
  ai/anthropic/          Claude client: HTTP + SSE streaming, errors, model capabilities
  conversation.rs        builds requests from memory, streams, persists turns
  db/                    SQLite schema, FTS5 search and memory retrieval, export
  desktop/               widget window, tray, app menu, global shortcut
  settings.rs, secrets.rs, commands.rs, lib.rs
docs/                    architecture and privacy notes
```

Read **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** for how the pieces fit together.

## Troubleshooting

- **⌥Space does nothing.** Another app may own it. Open the menu-bar icon → Settings… → General and record a different shortcut. The widget and settings show a notice when registration fails.
- **"Add your Anthropic API key"** means no key is stored yet. Use Settings → Claude.
- **Keychain prompt during development.** Each rebuild of the dev binary has a new code signature, so macOS may ask once whether Thoughtflow may read its own Keychain item. Choose *Always Allow*.
- **"Claude couldn't be reached."** Check your connection; Thoughtflow retries transient failures (rate limits, overloads, network) automatically before reporting them.

## License

MIT. See [LICENSE](LICENSE).
