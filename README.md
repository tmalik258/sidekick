# Sidekick

A proactive desktop companion for Windows. Sidekick lives in a small "island" at the top of the screen, notices what you are doing (downloads, dev servers, login pages, Claude Code sessions), and suggests the next useful action. Safe actions can run on their own; everything else waits for one click.

Local-first: events, history, and settings stay on your machine. AI runs in tiers: rules first, a local decision model (SemIf) next, and Claude Code for real work.

> Status: **P0 Foundation**. The island, mascot state machine, sound cues, settings, pause control, event bus, SQLite storage, and a test sensor work end to end. Real sensors and skills arrive in P1.

Full spec: [Desktop AI Assistant SRS](https://claude.ai/code/artifact/2f76a151-4e3f-4d9b-ab0c-bd1239d8ff69)

## Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 (Rust) |
| UI | Next.js 16 static export, React 19, TypeScript, Tailwind CSS v4, Motion, Zustand |
| Storage | SQLite (`rusqlite`, bundled) |
| Async | tokio |
| Lint | clippy, rustfmt, Biome |

## Repository layout

```text
apps/desktop/            Next.js UI (island, settings) + src-tauri (the app)
  src/app/               routes: /island, /settings
  src/components/        Island, Mascot (Canvas), SettingsPanel
  src/lib/               bridge to Rust, browser mock, store, sound cues
  src-tauri/src/         commands, mascot driver, pipeline, island window, tray
crates/core/             event envelope, bus, mascot state machine, settings, storage
crates/sensors/          Sensor trait, pause gate, heartbeat test sensor
skills/                  built-in skills (P1)
assets/                  mascot art and sounds (separate license)
```

## Prerequisites (Windows)

- Rust (stable) with the MSVC toolchain
- Node.js 22+ and pnpm 10 (`corepack enable`)
- WebView2 runtime (preinstalled on Windows 11)

## Run

```powershell
pnpm install
pnpm dev          # runs Next.js and the Tauri app together
```

Preview only the UI in a browser, with a mock core instead of Rust:

```powershell
pnpm web          # then open http://localhost:3000/island/ or /settings/
```

In the browser preview, `window.sidekickMock.go("success")` switches mascot states from the console.

## Try P0

- Hover the island to expand it. It collapses when the cursor leaves.
- Every 30 s the heartbeat sensor emits a test event: the mascot notices it, then settles.
- Tray menu: pause 15 min, 1 hour, or until resumed; run a demo suggestion; open settings.
- Settings > Debug: switch mascot states, emit a test event, run the demo suggestion, see stored events.

## Checks

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint
pnpm typecheck
pnpm --filter desktop build
```

## Build an installer

```powershell
pnpm build        # NSIS and MSI installers under target/release/bundle
```

## License

Code: MIT (see `LICENSE`). Mascot art and sounds in `assets/` will ship under a separate asset license. The current mascot and cues are placeholders drawn and synthesized in code.
