# Sidekick

A proactive desktop companion for Windows. Sidekick lives in a small "island" at the top of the screen, notices what you are doing (downloads, dev servers, login pages, Claude Code sessions), and suggests the next useful action. Safe actions can run on their own; everything else waits for one click.

Local-first: events, history, and settings stay on your machine. AI runs in tiers: rules first, a local decision model (SemIf) next, and Claude Code for real work.

> Status: **P1 MVP**. Real sensors (downloads, dev servers, clipboard, active window), a rule-based skill engine, and built-in actions work end to end. AI tiers (SemIf, local model, Claude Code) arrive in P2.

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

## Try it

- **Download a file.** The moment it finishes, the island offers Open, Show in folder, Copy file, and conversions that fit the file (images to WebP, PNG or JPG; videos to MP4 or MP3; Office files to PDF; archives extracted) when ffmpeg, ImageMagick, LibreOffice or tar are installed.
- **Start a dev server** (`pnpm dev`, `uvicorn`, `python -m http.server`, also from WSL). Within a second it offers Chrome, Incognito, Zen, Edge, Firefox or your default browser, and FastAPI docs for Python servers. The browser you pick moves to the front next time.
- **Copy an `EADDRINUSE` error.** It offers to free the port.
- **Copy a password or API key.** The clipboard clears itself after 30 seconds.
- **Go fullscreen** (video, game, slides). The island hides and comes back after.
- **Settings > Skills**: switch skills on or off, or set them to Auto. Destructive actions always ask.
- **Your own skills**: drop YAML files into the folder shown in Settings > Found on this PC (format in `skills/README.md`).

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

Code: MIT (see `LICENSE`).

Credits:

- UI sounds: [SND](https://snd.dev/) by Dentsu Inc. and Starryworks Inc., installed from npm (`snd-lib`) and copied into `apps/desktop/public/sounds` at build time. Free to use; copyright of the audio belongs to the credited sound designers.
- Interface icons: [Solar](https://www.figma.com/community/file/1166831539721848736) by 480 Design, CC BY 4.0, via Iconify.
- The orb mascot is drawn with CSS and is original to this project.
