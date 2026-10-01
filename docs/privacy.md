# Privacy

Sidekick runs on your PC and keeps what it sees there.

- **Events and history** are stored in a local SQLite database. You can pause all sensors from the tray or Ask mode, and switch each sensor off in Settings > Sensors.
- **Ignored apps and sites** (Settings > Sensors): nothing from them is stored, indexed or shown to skills. Password managers are on the list from the start.
- **Clipboard secrets** (keys, tokens, passwords) are detected and never stored, indexed or sent to AI.
- **Passwords** are filled from your own 1Password or Bitwarden CLI. Sidekick never reads browser password stores or cookies.
- **AI** only sees what you send it: your question, plus the window, clipboard, page or screenshot you choose to attach. Claude Code runs in an empty folder and Sidekick never reads its credential files.
- **Voice** is off until you turn it on. Speech recognition and the Kokoro voice run on this PC; audio is never saved or sent. Only the words you say go to your AI, like a typed question.
- **Search by meaning** uses an embedding model on your own local server; text never leaves the PC.
- **Calendar** links and the Fathom key stay on this PC (settings file and environment variable).
- **Localhost endpoints** (Claude Code hooks, browser extension, MCP) listen on 127.0.0.1 only, refuse web pages, and the browser and MCP ones require a token.
- **Updates**: once a day Sidekick asks GitHub for the latest release number. You can turn this off.
- **Backups** (Settings > About > Export) hold your settings, own skills and action history; calendar links are left out.
