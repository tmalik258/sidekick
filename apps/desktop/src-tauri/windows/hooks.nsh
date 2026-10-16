; Before installing, remove an installed Sidekick (the app's old name) so
; users end up with one app. Its uninstaller runs silently and keeps the
; user's data; Duskie moves that data over on its first launch.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Sidekick" "UninstallString"
  StrCmp $R0 "" sidekick_done
  ReadRegStr $R1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Sidekick" "InstallLocation"
  DetailPrint "Removing Sidekick, the app's old name"
  ExecWait '$R0 /S _?=$R1'
  Delete "$R1\uninstall.exe"
  RMDir "$R1"
  sidekick_done:
!macroend
