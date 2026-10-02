# Setting up Sidekick

Sidekick works on its own after install. Each item below turns on more of it. The same list is in the app. The first-run welcome looks around first (your code folders, including WSL ones from VS Code, your documents, the Ollama models you have, Claude Code) and **Set it all up** applies what you tick. **Settings > Home > Setup** shows what is left, with the exact command for anything missing. **Run** opens a PowerShell window with that command so you can watch it, and **Check again** (or waiting a few seconds) turns finished items green. Sidekick only installs or changes things when you press a button, and backs up Claude Code's settings before it adds anything.

## 1. AI (pick at least one)

| What | Command | Unlocks |
| --- | --- | --- |
| Claude Code | `irm https://claude.ai/install.ps1 \| iex`, then run `claude` once to sign in | Chat, drafts and skills with your Claude plan |
| Ollama | `winget install -e --id Ollama.Ollama` | Free local AI on this PC |
| Local chat model | `ollama pull qwen3:4b` (about 2.5 GB) | Answers that never leave the PC |
| Search model | `ollama pull nomic-embed-text` (about 270 MB) | Search by meaning, not only exact words |
| Anthropic API key (optional) | `setx ANTHROPIC_API_KEY "your-key"`, then restart Sidekick | Pay as you go instead of a Claude plan |

## 2. Connections

| What | Where | Unlocks |
| --- | --- | --- |
| Claude Code hooks | Settings > Connections > Claude Code > **Add for me** (merges into `~/.claude/settings.json`, keeps your own hooks, saves a dated backup next to it) | Know when a session finishes or waits; allow or deny its permission requests from the island |
| Sidekick tools in Claude Code | Settings > Connections > Claude Code > **Add for me** (runs `claude mcp add` with your token) | Claude Code can search your history, notify you and open links |
| Composio | Settings > Connections > **Connect Composio**. Your browser opens; sign in and allow Sidekick, and it connects on its own. Then press **Connect** next to each app you use (Google Calendar, Outlook, Gmail, Slack, Jira, Fathom and more) | Meeting reminders, the morning brief, follow-ups from Fathom notes, and reading your apps in Ask mode |
| Browser extension | Settings > Connections > Browser > **Install** next to your browser. Sidekick copies the extension folder path and opens the browser's extensions page: turn on Developer mode, click Load unpacked, paste. The extension pairs on its own and the island asks you to Allow it | Page summaries, form help, duplicate tabs, saved sessions |
| Code folders | Settings > Home > Your code: tick the folders Sidekick found | Project status, the project launcher, the end-of-day check |
| Folders to search | Settings > Privacy and data > Search: tick the folders Sidekick found | Search inside your documents and notes |
| Voice (optional) | Settings > AI > Voice, about 180 MB once | "Hey Sidekick", spoken answers and follow-ups without the wake word, all on this PC |

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
| VS Code | `Microsoft.VisualStudioCode` | Open projects and files in your editor |
| Tesseract | `UB-Mannheim.TesseractOCR` | Copy text out of screenshots |
| Poppler (optional) | `oschwartz10612.Poppler` | Summarize PDFs |
| Pandoc (optional) | `JohnMacFarlane.Pandoc` | Summarize Word files, convert documents |
| FFmpeg (optional) | `Gyan.FFmpeg` | Convert videos and audio |
| ImageMagick (optional) | `ImageMagick.ImageMagick` | Convert and resize images |
| Docker Desktop (optional) | `Docker.DockerDesktop` | Start Docker when a project needs it |
| LibreOffice (optional) | `TheDocumentFoundation.LibreOffice` | Convert Office files to PDF |
| Bitwarden CLI (optional) | `Bitwarden.CLI` (or 1Password's `op`) | Fill logins from your password manager |

Install any of them with `winget install -e --id <package>`. Sidekick picks up new tools without a restart.

## Composio and the local model

With Composio on, the local model can read your connected apps in Ask mode, for example "what Jira issues are assigned to me?" or "any unread Slack messages from the client?". It can only read. When a request needs a change (sending, creating, moving, deleting), takes too many steps, or the model gives up, the answer shows **Continue in Claude Code**. That opens Claude Code in a terminal with the whole conversation, and Claude Code asks before it changes anything (or you allow it from the island).

Small models work best with one clear request at a time. If answers cut off or ignore the tools, give Ollama a bigger context window, then restart Ollama:

```powershell
setx OLLAMA_CONTEXT_LENGTH 16384
```

