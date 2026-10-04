# Contributing

Thanks for helping. Sidekick is a Tauri 2 app: Rust crates in `crates/`, the island UI (Next.js static export) in `apps/desktop`, skills in `skills/`.

## Setup

Windows with Rust (stable), Node 22 and pnpm:

```powershell
pnpm install
pnpm dev
```

`pnpm web` previews the UI in a browser with a mock backend.

## Checks

Run these before a pull request; CI runs the same:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint
pnpm typecheck
pnpm --filter desktop build
```

Voice tests that need the speech models run when `SIDEKICK_VOICE_MODELS` points at a folder holding them.

## Writing a skill

Most new behavior is a skill: one YAML file that reacts to an event and offers buttons. The format, events and actions are in `skills/README.md`. Add the file under `skills/<area>/`, register it in `crates/skills/src/lib.rs`, and add a test if it uses new conditions.

Rules for skills:

- Destructive or outward actions (deleting, installing, sending) can never run at Auto.
- Use `open_folder`, never `open_path`, for paths that come from outside Sidekick.
- Never build a shell command from event data; actions take argument lists.

You can also ask Sidekick to write one: in Ask mode type what you want and pick "Teach Sidekick a skill".

## Security rules

See `CLAUDE.md` for the full list. In short: no reading browser password stores, cookies or Claude credentials; secrets never reach AI; localhost endpoints refuse web pages; models download from pinned URLs with checksums.

## License

Code is MIT (see `LICENSE`). UI sounds come from SND (snd.dev) under its own terms.
