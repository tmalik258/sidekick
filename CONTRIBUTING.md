# Contributing

Thanks for helping. Sidekick is a Tauri 2 app: Rust crates in `crates/`, the island UI (Next.js static export) in `apps/desktop`, skills in `skills/`.

## Setup

Windows with Rust (stable), Node 22 and pnpm:

```powershell
pnpm install
pnpm dev          # "Sidekick Dev" — separate data from the installed app
pnpm dev:fresh    # wipe Sidekick Dev data, then start (replay first-run)
```

`pnpm web` previews the UI in a browser with a mock backend. Dev vs production data paths are in [README.md](README.md#development).

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

### End-to-end tests (Windows)

`pnpm --filter desktop e2e` drives the real app (Playwright attached to its WebView2): it opens Ask, checks instant results, the tabs, every Settings tab and This PC only, and fails on a page error, a step over its time budget, or anything that held the UI thread over 250 ms. CI runs it on every pull request (job `e2e-windows`, screenshots in the `e2e-windows` artifact). To run it locally:

```powershell
cd apps/desktop
pnpm tauri build --debug --no-bundle --config src-tauri/tauri.e2e.conf.json
pnpm e2e
```

For the run it starts Sidekick onboarded with voice off; your own `settings.json` is backed up and put back afterwards. Quit Sidekick first.

### Freezes

Anything that holds the UI thread over 50 ms is written to the log ("UI thread held 120 ms by agents_status") and to Settings > Home > Copy diagnostics. Tauri runs commands that are not `async` on the UI thread: make a command `async` and move slow work into `spawn_blocking`.

## Writing a skill

Most new behavior is a skill: one YAML file that reacts to an event and offers buttons. The format, events and actions are in `skills/README.md`. Add the file under `skills/<area>/`, register it in `crates/skills/src/lib.rs`, and add a test if it uses new conditions.

Rules for skills:

- Destructive or outward actions (deleting, installing, sending) can never run at Auto.
- Use `open_folder`, never `open_path`, for paths that come from outside Sidekick.
- Never build a shell command from event data; actions take argument lists.

You can also ask Sidekick to write one: in Ask mode type what you want and pick "Teach Sidekick a skill".

## Security rules

See [docs/architecture.md](docs/architecture.md) for the full list. In short: no reading browser password stores, cookies or Claude credentials; secrets never reach AI; localhost endpoints refuse web pages; models download from pinned URLs with checksums.

## License

Code is MIT (see `LICENSE`). UI sounds come from SND (snd.dev) under its own terms.
