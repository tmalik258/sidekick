# Releasing

1. Bump the version in all three places:
   - `Cargo.toml` (`[workspace.package] version`)
   - `apps/desktop/src-tauri/tauri.conf.json` (`version`)
   - `apps/desktop/package.json` (`version`)
2. Commit, then tag and push:

```bash
git tag v0.2.0
git push origin v0.2.0
```

3. The Release workflow builds the NSIS and MSI installers on Windows and attaches them to a draft release with generated notes. Review the draft on GitHub and publish it.
4. Installed copies see the new version within a day (Settings > General > Tell me about new versions) and link to the release page. Nothing installs on its own.

To build installers locally on Windows:

```powershell
pnpm install
pnpm build
```

The installers land in `target\release\bundle\nsis` and `target\release\bundle\msi`.

## Code signing (optional)

Unsigned installers work but show a SmartScreen warning. To sign, add your certificate to the runner and set `bundle.windows.certificateThumbprint` (or a `signCommand`) in `apps/desktop/src-tauri/tauri.release.conf.json`. See the Tauri guide: https://v2.tauri.app/distribute/sign/windows/
