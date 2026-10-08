# Releasing

1. Bump the version in all three places:
   - `Cargo.toml` (`[workspace.package] version`)
   - `apps/desktop/src-tauri/tauri.conf.json` (`version`)
   - `apps/desktop/package.json` (`version`)
2. In `CHANGELOG.md`, move the entries under Unreleased to a new version heading with today's date, and add its compare link at the bottom.
3. Run the tool-choice eval against the default local model. It sends the 100 prompts in `apps/desktop/src-tauri/evals/prompts.json` and fails below 85% right:

```bash
ollama pull qwen3:4b
SIDEKICK_EVAL_MODEL=qwen3:4b cargo test -p sidekick-desktop eval -- --ignored --nocapture
```

   Every wrong prompt is printed with what the model called. Fix the prompt or tool description before tagging.
4. Commit, then tag and push:

```bash
git tag v0.2.0
git push origin v0.2.0
```

5. The Release workflow builds the NSIS and MSI installers on Windows and attaches them to a draft release with generated notes. Review the draft on GitHub and publish it.
6. Installed copies see the new version within a day (Settings > Home > Tell me about new versions). The island offers Install: it downloads the installer, checks it against SHA256SUMS.txt from the release, then runs it. Nothing installs without that click.

To build installers locally on Windows:

```powershell
pnpm install
pnpm build
```

The installers land in `target\release\bundle\nsis` and `target\release\bundle\msi`.

## Code signing (optional)

Unsigned installers work but show a SmartScreen warning. To sign, add your certificate to the runner and set `bundle.windows.certificateThumbprint` (or a `signCommand`) in `apps/desktop/src-tauri/tauri.release.conf.json`. See the Tauri guide: https://v2.tauri.app/distribute/sign/windows/
