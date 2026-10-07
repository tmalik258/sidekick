# Setting up Sidekick

Sidekick works on its own after install. Each item below turns on more of it. The same list is in the app. The first-run welcome looks around first (your code folders, including WSL ones from VS Code, your documents, the Ollama models you have, Claude Code) and **Set it all up** applies what you tick. **Settings > Home > Setup** shows what is left, with the exact command for anything missing. **Run** opens a PowerShell window with that command so you can watch it, and **Check again** (or waiting a few seconds) turns finished items green. Sidekick only installs or changes things when you press a button, and backs up Claude Code's settings before it adds anything.

## 1. AI (pick at least one)

| What | Command | Unlocks |
| --- | --- | --- |
| Claude Code | `irm https://claude.ai/install.ps1 \| iex`, then run `claude` once to sign in | Chat, drafts and skills with your Claude plan |
| Ollama | `winget install -e --id Ollama.Ollama`. **Install** in Sidekick also starts Ollama and downloads the chat model below | Free local AI on this PC |
| Local chat model | Picked for your PC: `ollama pull qwen3:8b` (5.2 GB) with an 8 GB+ graphics card, `qwen3:4b` (2.5 GB) with a 4 GB+ card or 16 GB of memory, otherwise `qwen3:1.7b` (1.4 GB) | Answers that never leave the PC |
| Search model | `ollama pull nomic-embed-text` (about 270 MB) | Search by meaning, not only exact words |
| Anthropic API key (optional) | `setx ANTHROPIC_API_KEY "your-key"`, then restart Sidekick | Pay as you go instead of a Claude plan |

Sidekick talks to Ollama at `127.0.0.1:11434`, so "Expose Ollama to the network" can stay off.

## 2. Connections

| What | Where | Unlocks |
| --- | --- | --- |
| Claude Code hooks | Settings > Connections > Claude Code > **Add for me** (merges into `~/.claude/settings.json`, keeps your own hooks, saves a dated backup next to it) | Know when a session finishes or waits; allow or deny its permission requests from the island |
| Sidekick tools in Claude Code | Settings > Connections > Claude Code > **Add for me** (runs `claude mcp add` with your token) | Claude Code can search your history, notify you and open links |
| Composio | Settings > Connections > **Connect Composio**. Your browser opens; sign in and press Allow, the same as Claude's Composio connector. Every app already connected in your Composio account (Google Calendar, Gmail, Slack, Jira, Fathom and more) works right away. To add one more, use **Add** under the list, or paste a consumer key (`ck_...`) under Other ways to connect | Meeting reminders, the morning brief, follow-ups from Fathom notes, and reading your apps in Ask mode |
| Browser extension | Settings > Connections > Browser > **Install** next to your browser. Sidekick copies the extension folder path and opens the browser's extensions page: turn on Developer mode, click Load unpacked, paste. The extension pairs on its own and the island asks you to Allow it | Page summaries, form help, duplicate tabs, saved sessions |
| Code folders | Settings > Home > Your code: tick the folders Sidekick found | Project status, the project launcher, the end-of-day check |
| Folders to search | Settings > Privacy and data > Search: tick the folders Sidekick found | Search inside your documents and notes |
| Voice (optional) | Settings > AI > Voice, about 205 MB once | "Hey Sidekick", spoken answers and follow-ups without the wake word, all on this PC |

## 3. Tools

Recommended, in one go:

```powershell
winget install -e --id GitHub.cli --accept-source-agreements; winget install -e --id Git.Git --accept-source-agreements; winget install -e --id Microsoft.VisualStudioCode --accept-source-agreements; winget install -e --id UB-Mannheim.TesseractOCR --accept-source-agreements
```

Then sign in to GitHub once:

```powershell
gh auth login
```

| Tool | Package | Unlocks |
| --- | --- | --- |
| GitHub CLI | `GitHub.cli` | Open PRs in the morning brief |
| Git | `Git.Git` | Repo status and unsaved work |
| Code editor | `Microsoft.VisualStudioCode` | Open projects and files in your editor. Cursor, VS Code, Antigravity, Windsurf, VSCodium, Zed, JetBrains IDEs and Visual Studio are found on their own; pick one in Settings > Apps > Code editor |
| Tesseract | `UB-Mannheim.TesseractOCR` | Copy text out of screenshots |
| Poppler (optional) | `oschwartz10612.Poppler` | Summarize PDFs |
| Pandoc (optional) | `JohnMacFarlane.Pandoc` | Summarize Word files, convert documents |
| FFmpeg (optional) | `Gyan.FFmpeg` | Convert videos and audio |
| ImageMagick (optional) | `ImageMagick.ImageMagick` | Convert and resize images |
| Docker Desktop (optional) | `Docker.DockerDesktop` | Start Docker when a project needs it |
| LibreOffice (optional) | `TheDocumentFoundation.LibreOffice` | Convert Office files to PDF |

Install any of them with `winget install -e --id <package>`. Sidekick picks up new tools without a restart.

## Composio and the local model

With Composio on, the local model can read your connected apps in Ask mode, for example "what Jira issues are assigned to me?" or "any unread Slack messages from the client?". It can only read. When a request needs a change (sending, creating, moving, deleting), takes too many steps, or the model gives up, the answer shows **Continue in Claude Code**. That opens Claude Code in a terminal with the whole conversation, and Claude Code asks before it changes anything (or you allow it from the island).

Small models work best with one clear request at a time. If answers cut off or ignore the tools, give Ollama a bigger context window, then restart Ollama:

```powershell
setx OLLAMA_CONTEXT_LENGTH 16384
```

## Codex

Sidekick works with OpenAI's Codex CLI the same way as Claude Code, for people who use it instead or as well. Install it from Settings > Home > Setup (`npm install -g @openai/codex`) and run `codex` once to sign in. Then:

- **Chat**: Codex is in Settings > AI next to Claude Code. It runs read-only in an empty folder.
- **Continue in...**: Settings > AI > Coding agent picks who gets handoffs from Ask mode. Auto takes Claude Code when it is installed, else Codex.
- **Notifications and tools**: Settings > Connections > Codex > Add for me adds a `notify` script and Sidekick's MCP server to `~/.codex/config.toml` (backed up first), so the island tells you when a Codex turn is done.

## Supported versions

Sidekick checks for these when it starts a session and says so plainly when something is too old.

| What | Supported | Needs |
| --- | --- | --- |
| Windows | Windows 11, and Windows 10 22H2 | The island is a tinted capsule on both; nothing behind it is blurred. |
| Claude Code | A current release (`claude update` keeps it there) | `--input-format stream-json`, `--permission-prompt-tool` and `--resume`. An older CLI shows "Claude Code is too old for this" with the command to update. |
| Codex | A release with `codex app-server` (`npm install -g @openai/codex@latest`) | The app server's `thread/start`, `thread/resume` and `turn/steer`. Older versions fall back to `codex exec` in Ask; the Agents tab asks you to update. |
| Ollama | 0.5 or newer | `/api/ps` for the model's context size, and `keep_alive`. |
| Code editors | Cursor, VS Code, VSCodium, Antigravity, Windsurf, Zed, JetBrains IDEs (2023 or newer) and Visual Studio 2019 or newer | Found from Windows' list of installed apps; projects open with the editor's own command line. |
| git | 2.30 or newer, for reviewing and undoing agent changes | Projects without git are tracked in a private copy inside Sidekick's data folder, so review and undo work there too. |
