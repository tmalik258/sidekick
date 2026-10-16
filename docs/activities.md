# Live activity and download status

Activities live in memory in `apps/desktop/src-tauri/src/activity.rs`. The desktop listens before requesting a snapshot and accepts only newer registry revisions. Opening actions also keep a local record so they appear immediately; their IDs match the backend records.

The inactivity threshold is fixed at 120 seconds. Increasing transferred bytes, phase changes, and new substantive tool output count as progress. Identical reports, polling responses, and newly discovered totals do not reset the clock. Waiting never cancels or restarts an operation. Dismiss hides a record until meaningful progress or a terminal outcome; Check again reads the existing source.

Sources include suggestion and chat actions, agent steps, updater and voice downloads, paired-browser downloads, recognized partial files in Downloads, and Duskie-generated setup commands. Setup output is captured per step while its PowerShell terminal remains visible. Model pulls stream Ollama's pull API and accumulate bytes by layer digest. Since layers are discovered incrementally, a whole-model percentage is not guessed from one layer.

The paired extension requires the `downloads` permission. Existing installations need the updated extension and browser approval for that permission. Telemetry uses the authenticated local bridge, skips incognito records, and respects browser/download sensor switches and site exclusions. Records use the extension instance and browser download ID, plus observation revisions to reject delayed reports. Active downloads are queried every two seconds while the worker is running and paired. The existing long poll and alarm wake the worker; active downloads recover after reconnecting. Without browser telemetry, recognized partial-file growth shows received bytes without a guessed total.

An application or installer launch is acknowledged as a launch request, rather than proof that a window opened or installation finished. A completed setup script means its command finished; the setup checklist independently checks whether the tool is installed.

Focused checks (from `apps/desktop` unless noted):

```text
npm.cmd run typecheck
node scripts/activity-check.mjs
node scripts/extension-download-check.mjs
node scripts/state-order-check.mjs
python scripts/setup-output-check.py
node scripts/activity-browser-check.mjs
node scripts/update-check.mjs
```

The browser checks use the preview at port 3002 and accept `CHROMIUM` for an installed Chrome executable. From the repository root, run the Rust tests for `activity::tests`, `setup_progress::tests`, `updates::tests`, `browser::download_tests`, and `downloads::tests` in their respective desktop/sensors packages.
