# Debugging crashes and UI freezes

## Native crashes (`STATUS_HEAP_CORRUPTION` / `0xc0000374`)

Rust panics are written to `last-crash.txt` in the app data folder. **Windows heap
corruption does not run that path** — the process is killed by the OS — so you need
a dump.

### Local dumps (preferred)

In an **elevated** PowerShell (once per machine):

```powershell
$exe = "sidekick-desktop.exe"
$dumpDir = "$env:USERPROFILE\SidekickDumps"
New-Item -ItemType Directory -Force -Path $dumpDir | Out-Null
$reg = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\$exe"
New-Item -Force -Path $reg | Out-Null
Set-ItemProperty $reg -Name DumpFolder -Value $dumpDir
Set-ItemProperty $reg -Name DumpType -Value 2
Set-ItemProperty $reg -Name DumpCount -Value 5
```

Or run [`scripts/enable-local-dumps.ps1`](../scripts/enable-local-dumps.ps1) as Administrator.

Reproduce until Sidekick exits. Open the newest `.dmp` under `%USERPROFILE%\SidekickDumps`
in Visual Studio or WinDbg and note the faulting stack (`tao` / `tauri_runtime` /
WebView2 / `ntdll!RtlpHeap*`). Do not commit dump files.

To remove the rule later:

```powershell
Remove-Item -Recurse -Force "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\sidekick-desktop.exe"
```

### PageHeap (only if the dump stack is useless)

Full PageHeap catches the bad write at the cost of heavy memory and speed. Use it
only on a dedicated repro, then turn it off. Install the Windows SDK Debugging Tools
and use `gflags` / Application Verifier on `sidekick-desktop.exe` — never leave it
on for normal development.

## UI thread holds

Sidekick logs `UI thread held N ms by <name>` when the main thread is busy long
enough to hitch:

- **by a command name** — a sync Tauri invoke; move that work off the UI thread.
- **by stall** — something else (WebView paint, tray, native code); no invoke name.

Copy diagnostics (Settings) includes a short freeze summary. `freeze_report` returns
the same list for tools and tests.

### Freeze heartbeat A/B

The unlabeled-stall heartbeat calls `run_on_main_thread` on an interval. That path
has been linked to intermittent Windows `STATUS_HEAP_CORRUPTION` in Tauri/tao, so
the beat is intentionally slow, and you can turn it off:

```powershell
$env:SIDEKICK_NO_FREEZE_WATCH = "1"
pnpm dev
```

Sync command timing (`freeze::timed`) stays on either way. Unset the env (or open a
new shell) to restore the heartbeat after a long-session A/B.
