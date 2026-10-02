# Setting up Sidekick

Sidekick works on its own after install. Each item below turns on more of it. The same list is in the app: the first-run welcome walks through it, and **Settings > Setup** shows what is done, with the exact command for anything missing. **Run** opens a PowerShell window with that command so you can watch it, and **Check again** (or waiting a few seconds) turns finished items green. Sidekick never installs anything or edits other apps' settings on its own.

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
| Claude Code hooks | Copy the block from Settings > AI into `~/.claude/settings.json` (merge with any hooks you have) | Know when a session finishes or waits; allow or deny its permission requests from the island |
| Sidekick tools in Claude Code | Settings > AI shows the `claude mcp add ...` command with your token | Claude Code can search your history, notify you and open links |
| Browser extension | `chrome://extensions` (or Edge), Developer mode, Load unpacked `apps/extension`, then paste the pairing code from Settings > Browser | Page summaries, form help, duplicate tabs, saved sessions |
| Code folders | Settings > General | Project status, the project launcher, the end-of-day check |
| Calendar | Settings > Today: your private iCal link (Google: Settings > your calendar > Secret address in iCal format; Outlook: Settings > Calendar > Shared calendars > Publish) | Meeting reminders with Join and Prep |
| Folders to search | Settings > Search | Search inside your documents and notes |
| Voice (optional) | Settings > Voice, about 180 MB once | "Hey Sidekick" and spoken answers, all on this PC |
| Fathom (optional) | `setx FATHOM_API_KEY "your-key"`, then restart Sidekick | Follow-ups drafted from meeting notes |

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
