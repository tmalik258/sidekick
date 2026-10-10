# Enable Windows Error Reporting local full dumps for Sidekick (debug only).
# Run once in an elevated PowerShell from the repo root:
#   powershell -ExecutionPolicy Bypass -File .\scripts\enable-local-dumps.ps1
#
# Dumps land in %USERPROFILE%\SidekickDumps after STATUS_HEAP_CORRUPTION etc.
# Remove the registry key when finished (see docs/debugging.md).

$ErrorActionPreference = "Stop"
$exe = "sidekick-desktop.exe"
$dumpDir = Join-Path $env:USERPROFILE "SidekickDumps"
New-Item -ItemType Directory -Force -Path $dumpDir | Out-Null

$reg = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\$exe"
try {
    New-Item -Force -Path $reg | Out-Null
    Set-ItemProperty $reg -Name DumpFolder -Value $dumpDir
    Set-ItemProperty $reg -Name DumpType -Value 2
    Set-ItemProperty $reg -Name DumpCount -Value 5
} catch {
    Write-Error "Need Administrator to write LocalDumps. Right-click PowerShell → Run as administrator, then retry."
    exit 1
}

Write-Host "Local dumps enabled for $exe"
Write-Host "Folder: $dumpDir"
Write-Host "Reproduce the crash, then open the newest .dmp in Visual Studio or WinDbg."
