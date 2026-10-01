# Sidekick: notes for contributors and AI agents

Spec: the SRS linked in README.md. Requirement IDs (FR-UI-01, NFR-SEC-05, ...) in code comments refer to it.

## Architecture rules

- Rust owns everything that touches the OS and all state machines. The UI only renders state and calls named commands.
- Every OS signal becomes an `Event` on the `EventBus` (`crates/core`). Sensors check `SensorGate` before emitting, so pause and per-sensor switches are enforced in one place.
- Settings change only through `commands::apply_settings` (sanitize, side effects, save, gate update, `settings://changed`).
- Skills are YAML in `skills/` (compiled in via `crates/skills/src/lib.rs`). Actions live in `crates/actions`; add new ones there and to `SAFE` only if they are non-destructive.
- Suggestions go through `suggestions::offer`, which queues while the island is busy and expires ignored ones in Rust.
- Mascot transitions only through `mascot::dispatch` (or `force` for debug). Delayed follow-ups use `mascot::after`, which is cancelled by any newer transition.
- Never read browser password stores, cookies, or Claude credential files. Never pass event data to a shell as a string.
- Destructive or outward-facing actions can never run at Auto trust level.
- All AI goes through `crates/ai` (`AiProvider` for chat, `Decider` for T1). Prompts reach Claude Code on stdin, never as arguments. T1 decisions only use SemIf or a local model, never a cloud provider. A clipboard classified as a secret is never attached to a prompt.
- The Claude Code hook endpoint binds to 127.0.0.1 only. Sidekick never edits `~/.claude/settings.json` and never reads transcripts or credentials.
- Undo only touches paths an action itself produced (`undo::UNDOABLE`), sends them to the Recycle Bin, and only within 24 hours.

## Frontend rules

- Next.js runs as a static export inside Tauri: no SSR, API routes, server actions, or middleware.
- Tauri APIs are imported dynamically through `src/lib/bridge.ts`; it falls back to `src/lib/mock.ts` in a plain browser.
- Keep `src/lib/types.ts` in sync with the Rust serde types.
- Next.js 16 differs from older versions; see `apps/desktop/AGENTS.md`.

## Checks before pushing

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint && pnpm typecheck && pnpm --filter desktop build
```

## Style

- No em dashes in user-facing text or docs.
- Mascot art and sounds must be original. Do not copy Coucou's Mochi assets.
