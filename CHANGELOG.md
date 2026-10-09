# Changelog

All notable changes to Sidekick are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Faster Ask

- Claude Code and Codex keep a session warm per chat: one starts when Ask opens, and follow-up messages go into it instead of starting the CLI again (Codex through `codex app-server`, older Codex versions fall back to `codex exec`). Idle sessions close after 10 minutes, at most three per agent.
- The local model loads while you type (Ollama `keep_alive`), and the Composio connection and its tool list are kept between messages.
- The system prompt keeps its fixed rules first and the changing context last, so a local server reuses its cache.
- The local model gets about eight tools picked by meaning instead of twenty, and tools that only read run at the same time.
- The screen text is read in the background when Ask opens, for questions about the screen.
- Streamed answers update the screen once per frame.

### Faster voice

- The answer starts 0.6 s after you stop talking instead of 0.9 s, and can start at a short pause before that; it stays hidden and silent, and can only look things up, until the question is final.
- The first part of a spoken answer (up to the first comma) is spoken on its own, so the voice starts sooner.
- Talk over an answer to stop it and ask something else (Settings > Voice > Interrupt by talking; best with headphones).

### Timings

- Ask measures open to ready, Enter to first word and end of speech to first sound, keeps them in `timings.log` on this PC, and shows them in Ask with Ctrl+Alt+Shift+T.

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
