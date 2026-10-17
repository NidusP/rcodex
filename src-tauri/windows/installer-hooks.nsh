# Stop R Codex binaries before install/uninstall so NSIS can replace or delete them.
# Do not kill ChatGPT.exe / Codex.exe: those are the Codex desktop, not this installer.

!macro StopRCodexBinaries
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM r-codex.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "R Codex.exe"'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM codex-launcher.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM codex-gateway.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM codex-launcher-x86_64-pc-windows-msvc.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM codex-gateway-x86_64-pc-windows-msvc.exe'
  Pop $0
  Sleep 2000
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro StopRCodexBinaries
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopRCodexBinaries
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$INSTDIR"
!macroend
