## What changes

<!-- One or two sentences on what this PR does and why. Link the issue it closes, if any: "Closes #123". -->

## How it works

<!-- The approach, and anything a reviewer should look at first. Skip for small fixes. -->

## How it was tested

<!-- What you ran and what you checked by hand. For island or settings changes, say which Windows version you tried it on. -->

## Screenshots

<!-- Before and after, for anything you can see. Delete this section if nothing visible changed. -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] `pnpm lint`, `pnpm typecheck` and `pnpm --filter desktop build` pass
- [ ] New skills follow the rules in [CONTRIBUTING.md](https://github.com/tmalik258/sidekick/blob/main/CONTRIBUTING.md#writing-a-skill): nothing destructive or outward runs at Auto, `open_folder` for outside paths, no shell commands built from event data
- [ ] Nothing here breaks the [security rules](https://github.com/tmalik258/sidekick/blob/main/docs/architecture.md): no browser passwords, cookies or Claude credentials; secrets never reach AI
- [ ] `CHANGELOG.md` has a line for anything a user will notice
