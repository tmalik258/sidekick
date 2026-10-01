# Sidekick

A proactive desktop companion for Windows. Sidekick lives in a small "island" at the top of the screen, notices what you are doing (downloads, dev servers, login pages, Claude Code sessions), and suggests the next useful action. Safe actions can run on their own; everything else waits for one click.

Local-first: events, history, and settings stay on your machine. AI runs in tiers: rules first, a local decision model (SemIf) next, and Claude Code for real work.

> Status: **P2**. Rule-based skills (T0), SemIf or a local model for ranking (T1), and chat through Claude Code, the Anthropic API or a local model (T2) work end to end. Browser extension and password manager fill come in P3.

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
crates/sensors/          downloads, ports, clipboard, window, Claude Code hooks, disk and memory, away
crates/skills/           YAML skill engine
crates/actions/          built-in actions and detection of installed browsers and tools
crates/ai/               AI providers (Claude Code, local, Anthropic API), router, SemIf decisions
skills/                  built-in skills
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
- **Press Alt+Space** (or the search button on the island) and the island becomes Ask mode: type a command or ask anything, and the answer streams right there. Attach the app you were in or your clipboard with the chips; "This PC only" keeps the chat on a local model. Esc or a click elsewhere folds it back; the conversation stays for next time.
- **Copy an error or stack trace.** The island offers Explain and fix, which asks AI with the error attached.
- **Claude Code sessions**: add the hook from Settings > AI and the island tells you when a session finishes or is waiting for you.
- **Undo**: files Sidekick creates (conversions, extracted folders) can be sent to the Recycle Bin from the island or Settings > History for 24 hours.
- **Low disk or memory**: the island warns once and offers Storage settings or Task Manager.
- **Step away** for 5 minutes and suggestions wait for you instead of expiring unseen.
- **Browser**: load `apps/extension` unpacked in Chrome, Edge or Zen and paste the pairing code from Settings > Browser. Sign-in pages offer a fill from 1Password or Bitwarden, too many tabs offer cleanup, Upwork jobs offer a proposal draft.
- **Screenshots** (Win+PrtScn): Copy, Copy text (with Tesseract), Show in folder.
- **Search my stuff**: type in Ask mode and pick Search. Add folders under Settings > Search.
- **Claude Code can use Sidekick** through MCP: copy the command from Settings > AI and run it once.
- **What's on my screen?**: pick it in Ask mode, or turn on the Screenshot chip to send one with your next question. Sidekick only captures when you ask.
- **End of day**: a time summary to paste into your standup, and repos with unsaved work.
- **Learns**: three Not nows in a row quiet a skill for a day; five identical picks offer to make it automatic.

## AI setup

Everything works without AI. Set up any of these, in any order (Settings > AI shows which are reachable):

```powershell
# Claude Code: answers come from your own Claude subscription
npm install -g @anthropic-ai/claude-code
claude   # sign in once

# Local model through Ollama (stays on this PC; also ranks suggestions)
winget install Ollama.Ollama
ollama pull qwen3:4b

# Or the Anthropic API
setx ANTHROPIC_API_KEY "your-key"
```

SemIf (optional, T1 decisions) runs from WSL. Install it there, then set the command in Settings > AI, for example `wsl.exe -d Ubuntu-22.04 -- /home/you/semif/.venv/bin/semif-score`, with the llamacpp backend and a GGUF file for the GTX 1650 Ti.

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
