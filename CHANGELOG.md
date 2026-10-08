# Changelog

All notable changes to Sidekick are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Steadier answers on this PC

- The local model gets a prompt that fits its context: older turns, long tool results and attached text are trimmed first, so it no longer forgets the rules or your question, and answers start sooner.
- Paths like `%USERPROFILE%`, `$(USERPROFILE)`, `~` or a made-up `C:\Users\John` become your real folders, and a path that does not exist sends the model back to look it up instead of showing you the error.
- A new storage check answers "what is taking space": free space per drive, the biggest folders and files, and old files and installers in Downloads, with buttons to clear them to the Recycle Bin.
- Answers no longer talk about Sidekick's own tools or ask you for paths it can find.

### Plan first and instant results

- A task with several steps shows its plan first: Run (Enter) does the steps in order and ticks each off, and Undo all (Alt Z) puts them back.
- Typing in Ask shows installed apps and files by name before any model runs, the apps you use most first.

### More for agents

- Type `/` in the Agents composer for its commands (compact, clear, rewind and your project's own commands), and `@` to mention a file.
- Hover a message and Rewind to here puts the code and the conversation back to before it.
- A command's output shows folded under its step: the last three lines, the rest on click.
- Open in your editor (Alt 3) when a session is done.
- Sessions survive a restart: their history comes back, Resume carries on where it stopped, and Undo still works. Undo leaves a file alone when you edited it after the agent finished.
- Hover the island while agents work to see each one and Allow the waiting one there (Enter).

### Clearer states

- When an answer fails, Ask says what happened and offers the fix: Start Ollama, download the model, sign in again, ask the local model when a cloud one is busy, or ask again when you are back online. Retry is Alt R.
- With no model set up yet, Ask shows how Sidekick can answer (This PC, Claude Code, Codex or your API key) and sets up the one you pick; apps, files and commands work already.
- Offline, the context line says so and a web question can wait: Ask when I'm back online (Alt O).
- After three minutes away, hovering the island shows what finished or is waiting meanwhile, with Review, Answer and Clear all.

### Controls and the rest of the plan

- Settings use the new controls from the design: On/Off switches, a quieter dropdown with a tick on the current choice, and tiles for the mascot and island colours. Menus open upward when there is no room below.
- Code editor picker shows each editor with its letter, colour and time this week.
- The Agents tab picks the agent and project from the same menus instead of plain lists, and shows how much memory each session uses.
- Voice > While you talk: Compact (one slim line) or Full (a waveform, a timer and your words a size larger). Read answers aloud is now Speak replies.
- Click the context ring in a Claude Code session to compact it.
- Permission questions say Allow, Allow this session and Deny. Allow this session now also holds for Claude Code.
- Answers from the Anthropic API show what they cost.
- Typing a Windows setting in Ask ("bluetooth", "night light") opens that Settings page.
- Projects without git can be reviewed, undone and rewound too: Sidekick keeps a private copy in its own data folder and never adds git to the project.
- Windows High contrast mode is followed, sounds stay quiet in Do Not Disturb, and the halo stops turning while the island is at rest.

### Ask and agents, after first use on Windows

- Freeze watch: anything that holds the UI thread over 50 ms is logged with the command that caused it, and shows in Copy diagnostics.
- End-to-end tests drive the real app on Windows in CI: Ask, instant results, tabs, every Settings tab, This PC only, page errors and freezes.
- A model that says nothing for 90 seconds (or runs past 5 minutes) is stopped and Auto asks the next one, instead of Ask waiting forever.
- Instant results show on the first keystroke: the app and file lists are built in the background when Sidekick starts and when Ask opens, then matched in memory.
- Agents, chats, editors and the island's look are read off the UI thread, so a slow disk or process scan no longer freezes Sidekick. Session memory is no longer measured on the UI thread every 5 seconds.
- "Lighter Ollama" in Setup: one answer at a time, flash attention, an 8-bit cache and an 8K context (4K cut the start of chats with tools), about half the memory and faster answers. Setup says when Ollama's own Context length setting holds it at 4K. The local model now unloads after 10 idle minutes instead of 30.
- Local Qwen3 answers without hidden thinking unless the question needs it (why, compare, plan, maths, long questions), so the first word comes in seconds. "Think harder" (Alt H) under a local answer asks again with thinking on.
- After you send, the input clears ("Ask a follow-up") and your question shows above the answer.
- The app, what you copied and This PC only look like toggles again: a tick when on, an outline with + when left out.
- Holding Alt shows only the letter on each control, like Windows KeyTips.
- Ask turns Wi-Fi, Bluetooth, Mobile hotspot, airplane mode, Night light, Do Not Disturb and dark mode on or off. Typing "turn on hotspot" shows the switch as the first result, with no model involved, and "hotspot" always means this PC's own.
- In Auto, a Claude Code plan that is used up no longer ends the answer: Sidekick asks Codex or the next model, skips Claude Code until it resets, and hands coding work to Codex meanwhile.
- Agents shows how much of the 5-hour or weekly limit is used, in the session and on the island, and offers Compact (Alt K) once Claude Code's context is half full.
- Settings > Appearance scrolls smoothly; its mascot previews stand still.
- This PC only stays on for the rest of the chat. Before, clicking it left the keyboard on the button, so the next Enter switched it back off.
- Ctrl+Space opens Ask in one quick move. The input no longer scrolls the panel while it grows, so the tabs stop jumping.
- The copied pill names what you copied ("Copied: stack trace"); hover shows the text.
- History rows no longer run past the island: long titles and project paths are cut short.
- Settings > AI shows Start Ollama (or Install Ollama) when Ollama is not running, instead of an empty model list.
- A model that cannot be used yet shows why ("No key", "Not installed", "Not running", "Out of usage") instead of an On switch that did nothing.

- Sidekick reaches Ollama on `127.0.0.1` instead of `localhost`. Windows tried IPv6 first, and a fresh Ollama only listens on IPv4, so it looked stopped until "Expose Ollama to the network" was turned on.
- Installing Ollama from Sidekick also starts it and downloads the chat model, in one PowerShell window.
- The chat model suits the PC: `qwen3:8b` with an 8 GB graphics card, `qwen3:4b` with 4 GB or 16 GB of memory, otherwise `qwen3:1.7b`.

### Ready for release

- With Windows transparency effects off, the island is a solid colour; on Battery saver its halo is not blurred and the mascot stays still.
- Secrets (API keys, tokens, private keys, passwords) are shown as dots in saved chats, agent transcripts, command output and the change review.
- "This PC only" is tested: the local model gets no tools that go online, and one it names anyway is refused.
- Screen readers hear state changes, an agent that needs you, and answers a sentence or two at a time.
- An agent CLI too old for Sidekick says so and how to update it, instead of stopping without a word.
- Copy diagnostics in Settings > Home > About, for bug reports, and opt-in crash reports you read before sending.
- Faint text on the island is a little brighter, so every island colour passes a contrast check, now run in CI with speed targets and screenshots of each colour.


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

### Appearance

- A new Appearance tab in Settings for the mascot, the island and how the island behaves.
- Mascot colours: Pearl, Aurora, Chrome, Peach, Mint, Lilac and Onyx. Graphite and Midnight become Onyx.
- Island colours: Black glass (the new default, a deep tint with a light rim), Graphite, Midnight, Smoke, Warm graphite and Solid black.

### Code editor

- Sidekick finds Cursor, VS Code, Antigravity, Windsurf, VSCodium, Zed, JetBrains IDEs and Visual Studio, and opens projects in the one you used most this week. Settings > Apps > Code editor picks one for good.
- Buttons name your editor: "Open in Cursor" instead of "Open in VS Code".

### Morning setup

- Routines are now Morning setup, under Settings > Skills, with one switch: Ask first, Open by itself or Off. Click an app or site to take it out.

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
