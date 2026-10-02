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
crates/voice/            Hey Sidekick wake word, live speech to text, Kokoro speech (sherpa-onnx, on this PC)
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
pnpm dev          # Next.js + Tauri (Rust release; needed so sherpa DLLs match the CRT)
```

On Windows, a debug Rust build links the debug CRT while the sherpa/ONNX DLLs use the release CRT, which crashes at startup with a Visual C++ assert. `pnpm dev` therefore runs Tauri with `--release`.

Preview only the UI in a browser, with a mock core instead of Rust:

```powershell
pnpm web          # then open http://localhost:3002/island/ or /settings/
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
- **Press Ctrl+Space** (or the search button on the island) and the island becomes Ask mode: type a command or ask anything, and the answer streams right there. Attach the app you were in or your clipboard with the chips; "This PC only" keeps the chat on a local model. Esc or a click elsewhere folds it back; the conversation stays for next time.
- **Copy an error or stack trace.** The island offers Explain and fix, which asks AI with the error attached.
- **Claude Code sessions**: add the hook from Settings > AI and the island tells you when a session finishes or is waiting for you.
- **Undo**: files Sidekick creates (conversions, extracted folders) can be sent to the Recycle Bin from the island or Settings > History for 24 hours.
- **Low disk or memory**: the island warns once and offers Storage settings or Task Manager.
- **Step away** for 5 minutes and suggestions wait for you instead of expiring unseen.
- **Browser**: press Install next to your browser in Settings > Connections; Sidekick opens the extensions page with the folder path copied, and the extension pairs on its own. Sign-in pages offer a fill from 1Password or Bitwarden, too many tabs offer cleanup, Upwork jobs offer a proposal draft.
- **Screenshots** (Win+PrtScn): Copy, Copy text (with Tesseract), Show in folder.
- **Search my stuff**: type in Ask mode and pick Search. Pick folders under Settings > Privacy and data. With Ollama running, `ollama pull nomic-embed-text` adds search by meaning (embeddings stay on this PC).
- **Claude Code can use Sidekick** through MCP: press Add for me in Settings > Connections.
- **What's on my screen?**: pick it in Ask mode, or turn on the Screenshot chip to send one with your next question. Sidekick only captures when you ask.
- **Voice**: on by default (Settings > AI > Voice; about 180 MB of speech models download once). Say "Hey Sidekick" and your question; the island shows your words as you speak and the answer is read aloud with Kokoro. Talking over an answer stops it, and after an answer you can reply without the wake word. Suggestions are read out too, and you can answer them by voice ("the first one", "not now"). There is a mic button in Ask mode for push to talk.
- **Settings** open inside the island (tray, the island's Settings button, or "Open settings" in Ask mode).
- **Meetings**: connect Composio in Settings > Connections, then Google Calendar or Outlook. A few minutes before a meeting the island offers Join call and Prep with AI; afterwards it offers to draft the follow-up from your Fathom notes (connect Fathom too). Suggestions wait quietly while you are in a meeting.
- **Morning brief**: the first time you sit down each day, one card with yesterday's time, repos with unsaved work and PRs waiting on you (if the GitHub CLI `gh` is signed in). Plan my day hands it to your AI.
- **End of day**: a time summary to paste into your standup, and repos with unsaved work.
- **Learns**: three Not nows in a row quiet a skill for a while; **Always do this** on a suggestion makes it automatic (see Settings > Skills > Automations). Low-priority suggestions are saved for later instead of popping up; a small number on the island shows how many.
- **Shortcuts** for Talk, Accept, Not now, Ask about the screen, Clipboard history, Pause and Settings. Record your own in Settings > Home > Shortcuts.

## AI setup

Everything works without AI. The full checklist (AI, connections and tools, with every command) is in [docs/setup.md](docs/setup.md) and in the app under Settings > Home > Setup. In short:

```powershell
# Claude Code: answers come from your own Claude subscription
irm https://claude.ai/install.ps1 | iex
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

## More

- [Setup](docs/setup.md)
- [Privacy](docs/privacy.md)
- [Contributing and writing skills](CONTRIBUTING.md)
- [Releasing](docs/releasing.md)
