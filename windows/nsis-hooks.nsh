!include "FileFunc.nsh"
!include "LogicLib.nsh"

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Closing running CC Desktop Switch process if needed..."
  nsExec::ExecToLog 'taskkill /IM "CC Desktop Switch.exe" /T /F'
  nsExec::ExecToLog 'taskkill /IM "CC-Desktop-Switch.exe" /T /F'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desktop Switch" "InstallLocation" "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Closing running CC Desktop Switch process if needed..."
  nsExec::ExecToLog 'taskkill /IM "CC Desktop Switch.exe" /T /F'
  nsExec::ExecToLog 'taskkill /IM "CC-Desktop-Switch.exe" /T /F'
!macroend
