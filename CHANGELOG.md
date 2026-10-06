# Changelog

All notable changes to Sidekick are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Appearance

- A new Appearance tab in Settings for the mascot, the island and how the island behaves.
- Mascot colours: Pearl, Aurora, Chrome, Peach, Mint, Lilac and Onyx. Graphite and Midnight become Onyx.
- Island colours: Black glass (the new default, a deep tint with a light rim), Graphite, Midnight, Smoke, Warm graphite and Solid black.

### Code editor

- Sidekick finds Cursor, VS Code, Antigravity, Windsurf, VSCodium, Zed, JetBrains IDEs and Visual Studio, and opens projects in the one you used most this week. Settings > Apps > Code editor picks one for good.
- Buttons name your editor: "Open in Cursor" instead of "Open in VS Code".

### Morning setup

- Routines are now Morning setup, under Settings > Skills, with one switch: Ask first, Open by itself or Off. Click an app or site to take it out.

## [0.1.0] - Unreleased

The first public release.

### Island and mascot

- A small island at the top of the screen that suggests the next step for what you are doing. Safe actions can run on their own; everything else waits for one click.
- An orb mascot with expressions on springs, moods (happy, proud, sad, celebrating and more), Alive mode and its own sounds.
- Offline mode: the island says the moment the internet drops and when it is back.
- Hides in fullscreen apps and comes back after; Do Not Disturb is a switch in Settings.

### Suggestions

- Downloads: Open, Show in folder, Copy file and conversions that fit the file (images, videos, Office files to PDF, archives).
- Dev servers: open in your browser of choice within a second, FastAPI docs for Python servers, free a port on `EADDRINUSE`.
- Clipboard: passwords and API keys clear after 30 seconds; errors and stack traces offer Explain and fix.
- Screenshots: Copy, Copy text (OCR) and Show in folder.
- Claude Code sessions: a notice when a session finishes or waits for you.
- Meetings, morning brief and end of day through Composio (Google Calendar, Outlook, Fathom).
- Low disk or memory warnings, and suggestions that wait while you are away.
- Learns from Not now and Always do this; low-priority suggestions are saved for later.

### Ask mode

- Ctrl+Space turns the island into Ask mode: answers stream in place, with the app you were in, your clipboard or a screenshot attached on request.
- Works with Claude Code, the Anthropic API, Codex or a local model through Ollama ("This PC only").
- Actions are offered as confirmable options with Undo, and multi-step tasks, recipes and triggers can run them for you.
- Search over your history and chosen folders, by keyword or by meaning with local embeddings.

### Voice

- "Hey Sidekick" wake word, live speech to text and spoken answers with Supertonic (10 voices), all on this PC.

### Connections

- Browser extension that pairs on its own: tab cleanup, Upwork proposal drafts.
- Sidekick as an MCP server for Claude Code, and MCP clients for your own tools.

### Settings and updates

- Settings inside the island, with search, shortcuts you can record and your own YAML skills.
- Update checks once a day. A small sign on the island shows a waiting update; installs are checked against SHA256SUMS.txt and only run after your click.
- History with Undo for files Sidekick creates (24 hours).

[Unreleased]: https://github.com/tmalik258/sidekick/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tmalik258/sidekick/releases/tag/v0.1.0
