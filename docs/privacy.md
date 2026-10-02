# Privacy

Sidekick runs on your PC and keeps what it sees there.

- **Events and history** are stored in a local SQLite database. You can pause all sensors from the tray or Ask mode, and switch each one off in Settings > Privacy and data.
- **Ignored apps and sites** (Settings > Privacy and data): nothing from them is stored, indexed or shown to skills. Password managers are on the list from the start.
- **Routines** (Settings > Privacy and data): the first time you open each app, and each site in your active tab (with the browser extension, domain only, never the page), during the first hour of a day. Kept for 60 days on this PC to suggest your usual start. Switch it off or press Forget routines any time.
- **Work log**: Save to work log appends to Documents\Sidekick\worklog.md, only when you press it.
- **Clipboard secrets** (keys, tokens, passwords) are detected and never stored, indexed or sent to AI.
- **Passwords** are filled from your own 1Password or Bitwarden CLI. Sidekick never reads browser password stores or cookies.
- **AI** only sees what you send it: your question, plus the window, clipboard, page or screenshot you choose to attach. Claude Code runs in an empty folder and Sidekick never reads its credential files.
- **Voice** is on by default and can be turned off in Settings > AI. Speech recognition and the Supertonic voice run on this PC; audio is never saved or sent. Only the words you say go to your AI, like a typed question.
- **Search by meaning** uses an embedding model on your own local server; text never leaves the PC.
- **Localhost endpoints** (Claude Code hooks, browser extension, MCP) listen on 127.0.0.1 only, refuse web pages, and the browser and MCP ones require a token.
- **Updates**: once a day Sidekick asks GitHub for the latest release number. You can turn this off.
- **Backups** (Settings > Home > Backup) hold your settings, own skills and action history; keys and sign-ins are left out.
- **Composio** (off until you connect it): calendar reminders, the morning brief and Fathom follow-ups read your apps through Composio, and in Ask mode the local model can use it as tools. Requests (for example a calendar or Jira search) go to Composio and on to that app. The local model can only read; changes go through Claude Code, which asks first. Sidekick connects to Composio Connect (connect.composio.dev), the same service Claude uses, so the apps already connected in your Composio account work without connecting them again. The sign-in (or a consumer key you paste) is kept in Windows Credential Manager, not in the settings file, and never in backups. Questions marked "This PC only" never use Composio.
- **Claude Code settings** are only changed when you press Add for me or Always in this project, and `~/.claude/settings.json` is backed up first.
- **Continue in Claude Code** writes the conversation to a file in Sidekick's AI folder and opens Claude Code with it, only when you press the button.
