!include "LogicLib.nsh"

; Runs the cleanup executable before a real uninstall. The installer passes
; /UPDATE when it replaces an existing install, which is not a real removal
; and must keep the currently applied rules.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode != 1
    DetailPrint "Removing LobbyLocker-managed firewall rules..."
    StrCpy $0 -1
    ClearErrors
    ExecWait '"$INSTDIR\lobbylocker-app.exe" --reset-firewall' $0
    ${If} ${Errors}
    ${OrIf} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "LobbyLocker could not remove its firewall rules. Uninstall has stopped so you can retry Reset without losing the cleanup tool."
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
!macroend
