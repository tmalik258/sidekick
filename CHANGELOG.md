# Changelog

All notable changes to Sidekick are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Agents

- Ask has three tabs: Ask, Agents and History (Ctrl Tab). History holds past chats and agent sessions.
- The Agents tab runs Claude Code or Codex in a project inside the island: pick the project and a mode (Plan, Ask, Edit or Full), then watch its plan, each step and what it says. Steer it while it works, or ask for more after.
- Its questions come to the island: Allow (Alt A), Always (Alt Y) or No (Alt N).
- When it is done, review every change by file: keep or undo one change, a whole file, or everything. Files it created go to the Recycle Bin. This needs the project to be a git repository.
- A ring shows how much of the context is used; Open in terminal (Alt T) carries on in the CLI.
- Continue in Claude Code from Ask now carries on in the Agents tab instead of a terminal.
- With Ask closed, a small pill says an agent is working or needs you; hovering it opens the Agents tab.

### Ask

- The header holds everything you change often: speak replies (Alt S), talk (Alt V), history (Alt H) and the model (Alt M), in a Graphite menu that says where each model runs.
- What goes with a question is one quiet line under it: the app you were in and what you copied. Click one to add it or leave it out; This PC only sits at the end (Alt P).
- Typing shows matches at once, before any AI: commands, projects, settings screens and past chats. A short name selects its match; a question selects Ask, which is always on top.
- Hold Alt to see every key on its control, and the key legend at the bottom.
- While it works: one shimmering line with a timer. After: "Local model · 1.2 s" under the answer, and the steps fold to "3 steps · 2.4 s".
- At most three actions after an answer, the first one solid.
- Reopening Ask within 10 minutes brings back the last chat; later, a new one starts.

### Voice

- Listening shows rainbow bars and your words, in Ask and in the compact pill.
- The mascot talks along while it reads an answer aloud.

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
