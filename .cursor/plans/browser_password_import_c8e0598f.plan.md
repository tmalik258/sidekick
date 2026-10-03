---
name: Browser password import repair
overview: Safe local Chromium fill/save/mirror, explicit five-second override approval, transactional writes and ephemeral credentials.
isProject: false
---

# Local browser password repair

## Behavior

- Windows Chrome, Edge, Brave and Samsung Internet stores use each browser's last-used profile. Firefox/Zen NSS and unsupported encryption are explicitly unsupported.
- New submitted entries auto-save after a live five-second countdown; Cancel aborts. Different existing passwords require an explicit Override click within five seconds, otherwise skip that target.
- Edit pauses the countdown. Save new entries does not approve replacements; Save and override existing passwords explicitly approves them. Saved chips last 25 seconds, and open edit drafts expire after ten minutes.
- Mirror existing passwords snapshots every selected source before any write. Conflicting site/account groups prompt sequentially, require source-browser selection and skip the whole account after five seconds without approval. Cancel mirroring stops remaining work.
- All targets are enabled by default. Null means all detected stores; an explicit [] disables all. Legacy empty selections migrate once to null.
- Saving is local only. Direct database writes do not promise browser cloud synchronization or phone sync. CLI vaults remain removed.

## Implementation requirements

- SQLite read snapshots include WAL; IMMEDIATE write transactions replace file-copy writes. Existing URL/form metadata and row identity survive edits. Canonical realms, username collisions, unreadable entries and changed snapshots protect existing passwords.
- Every submitted credential is bounded in memory and keyed by its event id, excluded from events/history/logs/AI. Prompt commands require that same id.
- Rust owns countdown deadlines and starts them only after display acknowledgement. Failed saves retry manually; no automatic retry loop.
- Cancel, dismissal, queue eviction, pause, disabled browser sensing and expiry clear credentials and cancel future transactions.
- Resolve profiles once per operation, constrain paths within user data, and serialize writes per store. Report saved, unchanged, conflict, locked, unsupported, disabled, cancelled and failed outcomes separately.

## Verification

- Fixtures: crypto, realms, WAL snapshots, locks, rollback, unrelated-row preservation, URL-path updates, username collisions, changed snapshots and profile traversal.
- State tests: display starts the timer, deadline skips override, explicit override is timely, Edit pauses, stale/duplicate commands reject, independent ids and expiry.
- Mirror: immutable source snapshots, explicit source selection, conflict timeout, approved override, cancellation and aggregate outcomes.
- Settings: legacy migration, defaults, subsets, all disabled and disabled targets during countdown.
- Run Rust tests and desktop compile checks, TypeScript and formatting/lint checks. Use disposable browser profiles for integration checks; never mutate real browser stores for verification.

Implementation and validation results are reported separately; the original completed markers did not establish these repair criteria.

## Real-browser validation

- Chrome 154.0.8037.97: isolated browser-created profile, browser-native dummy seed, Sidekick insert, restart and native autofill, approved edit, second restart and native autofill all passed. Unrelated seed ciphertext remained unchanged and SQLite integrity checks passed.
- This exposed and fixed an obsolete `date_synced` insert column. A regression test covers current Chromium schemas without that column.
- The temporary browser-check script and disposable profiles were removed after validation at the user's request. The opt-in Rust integration test remains restricted to isolated profiles under `target/`.
- Edge, Brave and Samsung Internet have not received this real-browser restart validation.
