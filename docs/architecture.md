# Architecture and project rules

How Duskie is put together, and the rules every change has to keep. Read this before a larger change; [CONTRIBUTING.md](../CONTRIBUTING.md) covers setup and pull requests.

## Architecture rules

- Rust owns everything that touches the OS and all state machines. The UI only renders state and calls named commands.
- Every OS signal becomes an `Event` on the `EventBus` (`crates/core`). Sensors check `SensorGate` before emitting, so pause and per-sensor switches are enforced in one place.
- Settings change only through `commands::apply_settings` (sanitize, side effects, save, gate update, `settings://changed`).
- Skills are YAML in `skills/` (compiled in via `crates/skills/src/lib.rs`). Actions live in `crates/actions`; add new ones there and to `SAFE` only if they are non-destructive.
- Suggestions go through `suggestions::offer`, which queues while the island is busy and expires ignored ones in Rust.
- Mascot transitions only through `mascot::dispatch` (or `force` for debug). Delayed follow-ups use `mascot::after`, which is cancelled by any newer transition.
- Never read cookies or Claude credential files. Never read or write browser password stores (`Login Data`, NSS `logins.json` / `key4.db`, or similar). Clipboard secrets are never logged, never sent to AI, and never uploaded. Never pass event data to a shell as a string.
- Destructive or outward-facing actions can never run at Auto trust level.
- All AI goes through `crates/ai` (`AiProvider` for chat, `Decider` for ranking suggestions). Prompts reach Claude Code on stdin, never as arguments. Decisions only use SemIf or a local model, never a cloud provider. A clipboard classified as a secret is never attached to a prompt.
- The Claude Code hook endpoint binds to 127.0.0.1 only. Duskie never reads Claude transcripts or credentials. User-initiated "Add for me" may merge Duskie's hook URLs into `~/.claude/settings.json` after writing a dated backup; other keys are left alone. Browsers cannot silent-install unpacked extensions: Duskie only stages the folder, copies the path, and opens the extensions page.
- Localhost endpoints (hooks 47821, browser 47822, MCP 47823) bind to 127.0.0.1, refuse requests with a web page origin, and the browser and MCP ones need their token. Use `open_folder`, never `open_path`, for paths from outside Duskie.
- MCP search only returns shareable sources (`search::SHAREABLE`): never clipboard or page text.
- Voice (`crates/voice`) runs fully on this PC (official sherpa-onnx crate, linked statically: no DLLs; Supertonic 3 voice). Voice is on by default. Models are prefetched at build/dev into `src-tauri/resources/voice-models` (SHA-256 checked), copied into app data on launch, and only downloaded from pinned GitHub release URLs when still missing (the voice first). First-run welcome waits until the voice can speak, then greets aloud. The microphone is open only while voice is on and Duskie is not paused; audio is never saved, logged or sent. Only the final transcript goes to the AI.
- Agent sessions (`sessions.rs`) run Claude Code (`claude -p`, stream-json in and out) or Codex (`codex app-server`) in a project the user picked. Claude Code's permission questions come through Duskie's MCP server (`sidekick_permission`, called by the CLI, not the model); Codex asks as JSON-RPC requests. Both wait for the user's answer in the island and are declined after 10 minutes. Before a session starts, `review.rs` snapshots the project with `git stash create` (which changes nothing); undo puts back only what changed since, and sends files the agent created to the Recycle Bin.
- Secrets live in the OS keychain (`secrets.rs`), never in settings, logs or prompts.
- Email, web page, file and MCP output text is untrusted data. It can never choose or run a tool by itself.
- Skills (`SKILL.md` folders) are untrusted instructions and possibly code. Nothing in a skill runs on import or sync; scripts only run under the agent's own permission prompts or an approved action.
- Undo only touches paths an action itself produced (`undo::UNDOABLE`), sends them to the Recycle Bin, and only within 24 hours.

## Frontend rules

- Next.js runs as a static export inside Tauri: no SSR, API routes, server actions, or middleware.
- Tauri APIs are imported dynamically through `src/lib/bridge.ts`; it falls back to `src/lib/mock.ts` in a plain browser.
- Keep `src/lib/types.ts` in sync with the Rust serde types.
- Pure TypeScript logic gets Vitest tests next to the file (`*.test.ts`).
- Honor `prefers-reduced-motion`. No visible scrollbars inside the island.
- Next.js 16 differs from older versions; check the guides in `node_modules/next/dist/docs/` before using an API.

## Checks before pushing

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint && pnpm typecheck && pnpm --filter desktop test && pnpm --filter desktop build
```

Native crashes and UI-thread freezes: [debugging.md](debugging.md).

## Style

- No em dashes or emojis in user-facing text or docs.
- Mascot art and sounds must be original or properly licensed and credited.
