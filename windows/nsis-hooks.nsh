!include "FileFunc.nsh"
!include "LogicLib.nsh"

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Checking previous CC Desktop Switch installation..."
  ReadRegStr $R1 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desktop Switch" "InstallLocation"
  ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desktop Switch" "UninstallString"
  ${If} $R1 == ""
  ${AndIf} $R0 != ""
    ${GetParent} $R0 $R1
  ${EndIf}
  ${If} $R1 != ""
    DetailPrint "Using existing install location: $R1"
    StrCpy $INSTDIR $R1
  ${EndIf}
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
