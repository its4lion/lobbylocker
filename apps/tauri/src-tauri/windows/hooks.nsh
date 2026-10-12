!include "LogicLib.nsh"

!macro NSIS_HOOK_PREUNINSTALL
  ; Tauri's /UPDATE mode replaces the application; it is not a real removal.
  ${If} $UpdateMode != 1
    DetailPrint "Removing LobbyLocker-managed firewall rules..."
    StrCpy $0 -1
    ClearErrors
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --reset-firewall' $0
    ${If} ${Errors}
    ${OrIf} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "LobbyLocker could not remove its firewall rules. Uninstall has stopped so you can retry Reset without losing the cleanup tool."
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
!macroend
