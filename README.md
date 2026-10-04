<p align="center">
  <img src="assets/icon.png" width="140" alt="Sidekick icon">
</p>

<h1 align="center">Sidekick</h1>

<p align="center">
  A proactive desktop companion for Windows.<br>
  It notices what you are doing and suggests the next step.
</p>

<p align="center">
  <a href="https://github.com/tmalik258/sidekick/releases/latest"><b>Download</b></a> ·
  <a href="docs/setup.md">Setup</a> ·
  <a href="docs/privacy.md">Privacy</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/tmalik258/sidekick/actions/workflows/ci.yml"><img src="https://github.com/tmalik258/sidekick/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4" alt="Windows 10 and 11">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license"></a>
</p>

## What it is

Sidekick lives in a small island at the top of your screen, with a friendly orb for a face. It watches for moments where it can help (a download finishing, a dev server starting, an error on the clipboard, a meeting about to begin) and offers the next useful action. Safe actions can run on their own; everything else waits for one click, and anything destructive always asks.

- **Local-first.** Events, history and settings stay on your PC. AI and online services are only used for what you turn on.
- **Works without AI.** Rules handle the everyday moments. Add Claude Code, the Anthropic API, Codex or a local model through Ollama for answers, drafts and multi-step tasks.
- **Yours to shape.** Switch skills on, off or to automatic, record your own shortcuts, and write your own skills in YAML.

## Install

1. Download the latest installer from [Releases](https://github.com/tmalik258/sidekick/releases/latest):
   - `Sidekick_x.y.z_x64-setup.exe` for most people.
   - `Sidekick_x.y.z_x64_en-US.msi` for managed or scripted installs.
2. Run it. Sidekick starts in the island at the top of the screen and walks you through setup.

Sidekick runs on Windows 10 and 11 (64-bit). The installer sets up the WebView2 runtime if it is missing.

### "Windows protected your PC"

The installers are not code signed yet, so Microsoft Defender SmartScreen may stop the first run with a blue "Windows protected your PC" window. To continue, click **More info**, check that the file name is the Sidekick installer you downloaded, then click **Run anyway**. This warning fades as more people install a release, and goes away once releases are signed.

To check the download first, compare its hash with `SHA256SUMS.txt` from the same release:

```powershell
Get-FileHash .\Sidekick_x.y.z_x64-setup.exe -Algorithm SHA256
```

Updates come to the island on their own (Settings > Home > Tell me about new versions) and are checked against `SHA256SUMS.txt` before they run. See [CHANGELOG.md](CHANGELOG.md) for what changed in each release.

## Things to try

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
- **Browser**: press Install next to your browser in Settings > Connections; Sidekick opens the extensions page with the folder path copied, and the extension pairs on its own. Sign-in pages offer fill from supported local Chromium password stores in each browser’s last-used profile. New entries have a five-second save countdown; different passwords require an explicit Override click within five seconds or are skipped. Local saving does not guarantee cloud sync. Too many tabs offer cleanup, Upwork jobs offer a proposal draft.
- **Screenshots** (Win+PrtScn): Copy, Copy text (with Tesseract), Show in folder.
- **Search my stuff**: type in Ask mode and pick Search. Pick folders under Settings > Privacy and data. With Ollama running, `ollama pull nomic-embed-text` adds search by meaning (embeddings stay on this PC).
- **Claude Code can use Sidekick** through MCP: press Add for me in Settings > Connections.
- **What's on my screen?**: pick it in Ask mode, or turn on the Screenshot chip to send one with your next question. Sidekick only captures when you ask.
- **Voice**: on by default (Settings > AI > Voice; about 205 MB of speech models download once). Say "Hey Sidekick" and your question; the island shows your words as you speak and the answer is read aloud with Supertonic (10 voices to pick from in Settings > AI > Voice). Talking over an answer stops it, and after an answer you can reply without the wake word. Suggestions are read out too, and you can answer them by voice ("the first one", "not now"). There is a mic button in Ask mode for push to talk.
- **Settings** open inside the island (tray, the island's Settings button, or "Open settings" in Ask mode).
- **Meetings**: connect Composio in Settings > Connections, then Google Calendar or Outlook. A few minutes before a meeting the island offers Join call and Prep with AI; afterwards it offers to draft the follow-up from your Fathom notes (connect Fathom too). Suggestions wait quietly while you are in a meeting.
- **Morning brief**: the first time you sit down each day, one card with yesterday's time, repos with unsaved work and PRs waiting on you (if the GitHub CLI `gh` is signed in). Plan my day hands it to your AI.
- **End of day**: a time summary to paste into your standup, and repos with unsaved work.
- **Learns**: three Not nows in a row quiet a skill for a while; **Always do this** on a suggestion makes it automatic (see Settings > Skills > Automations). Low-priority suggestions are saved for later instead of popping up; a small number on the island shows how many.
- **Shortcuts** for Talk, Accept, Not now, Ask about the screen, Clipboard history, Pause and Settings. Record your own in Settings > Home > Shortcuts.

## Setup

Sidekick works on its own after install. The first-run welcome looks around (code folders, documents, Ollama models, Claude Code) and **Set it all up** applies what you tick. **Settings > Home > Setup** shows what is left, with the exact command for anything missing. The full checklist is in [docs/setup.md](docs/setup.md). The short version for AI:

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

SemIf (optional) can rank suggestions from WSL. Install it there, then set its command in Settings > AI, for example `wsl.exe -d Ubuntu-22.04 -- /home/you/semif/.venv/bin/semif-score`.

## Privacy

Sidekick runs on your PC and keeps its data there. Passwords and API keys you copy are never stored, password managers are ignored from the start, and you can exclude any app or site. Screenshots are only taken when you ask. Details in [docs/privacy.md](docs/privacy.md).

## Development

### Prerequisites

- Windows 10 or 11
- Rust (stable) with the MSVC toolchain
- Node.js 22+ and pnpm 10 (`corepack enable`)

### Run

```powershell
pnpm install
pnpm dev          # Next.js + Tauri, debug Rust build
```

Preview only the UI in a browser, with a mock core instead of Rust:

```powershell
pnpm web          # then open http://localhost:3002/island/ or /settings/
```

In the browser preview, `window.sidekickMock.go("success")` switches mascot states from the console.

### Checks

CI runs the same:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint
pnpm typecheck
pnpm --filter desktop build
```

### Build an installer

```powershell
pnpm build        # NSIS and MSI installers under target/release/bundle
```

Releases are built by GitHub Actions from a version tag; see [docs/releasing.md](docs/releasing.md).

### Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 (Rust) |
| UI | Next.js 16 static export, React 19, TypeScript, Tailwind CSS v4, Motion, Zustand |
| Storage | SQLite (`rusqlite`, bundled) |
| Async | tokio |
| Voice | sherpa-onnx (wake word, speech to text, Supertonic speech), on this PC |
| Lint | clippy, rustfmt, Biome |

### Repository layout

```text
apps/desktop/            Next.js UI (island, Ask mode, settings) + src-tauri (the app)
  src/app/               routes: /island, /settings
  src/components/        Island, Orb mascot, Ask panel, settings
  src/lib/               bridge to Rust, browser mock, store, sounds
  src-tauri/src/         commands, pipeline, island window, tray, updates
crates/core/             event envelope, bus, mascot state machine, settings, storage
crates/sensors/          downloads, ports, clipboard, window, Claude Code hooks, disk and memory, away
crates/skills/           YAML skill engine
crates/actions/          built-in actions and detection of installed browsers and tools
crates/ai/               AI providers (Claude Code, Codex, local, Anthropic API), router, SemIf decisions
crates/voice/            Hey Sidekick wake word, live speech to text, Supertonic speech
skills/                  built-in skills
assets/                  app icon and social preview
```

## Contributing

Bug reports, ideas and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md), which also covers writing your own skills.

## License

MIT, see [LICENSE](LICENSE).

Credits:

- UI sounds: [SND](https://snd.dev/) by Dentsu Inc. and Starryworks Inc., installed from npm (`snd-lib`) and copied into `apps/desktop/public/sounds` at build time. Free to use; copyright of the audio belongs to the credited sound designers.
- Interface icons: [Solar](https://www.figma.com/community/file/1166831539721848736) by 480 Design, CC BY 4.0, via Iconify.
- The orb mascot is drawn with CSS and is original to this project.
