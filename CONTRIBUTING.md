# Contributing

Thanks for helping. Thoughtflow aims to stay small, fast, and calm, so changes that add surface area need a strong reason.

## Setup

```bash
npm install
cp .env.example .env   # optional: ANTHROPIC_API_KEY for development
npm run dev            # full app
npm run dev:web        # UI only, in a browser, against an in-memory mock
```

## Before opening a pull request

```bash
npm run check          # typecheck, ESLint + clippy (-D warnings), Vitest + cargo test
npm run build          # make sure the bundle still builds
```

Then try the change in the running app: summon the widget with ⌥Space, send a thought, switch modes, and close it with Esc.

## Guidelines

- **Keep secrets in Rust.** The webview must never see the API key or call the network. New Claude features belong in `src-tauri/src/ai/anthropic` and are exposed through a command.
- **Keep conversation history append-only.** Never rewrite stored turns or the system prompt mid-conversation (see "Why history is append-only" in `docs/ARCHITECTURE.md`). Put new per-request context in the new turn.
- **Don't log thought content**, and keep user-facing errors as short sentences.
- **Match the design language:** typography first, near-monochrome, hairline rules, no chat bubbles, no gradients. Every action needs a keyboard path.
- **Types are mirrored.** If you change a struct in `src-tauri/src/db/models.rs` or `settings.rs`, update `src/types.ts`.
- **Schema changes are migrations.** Append to `MIGRATIONS` in `src-tauri/src/db/mod.rs`; never edit a shipped migration.
- Add tests next to the code: `*.test.ts` for the frontend and `#[cfg(test)]` modules in Rust.
- Format Rust with `npm run fmt:rust`.
