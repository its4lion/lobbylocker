; LobbyLocker NSIS installer (Rune build).
; Assumes the payload/ folder next to this file's directory holds:
;   lobbylocker-app.exe, lobbylocker.exe, data/
; The uninstaller passes /UPDATE when it replaces an existing install; the
; firewall cleanup hook in hooks.nsh skips firewall changes for updates.
Unicode True
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\LobbyLocker"
InstallDirRegKey HKLM "Software\LobbyLocker" "InstallDir"
OutFile "../../release-assets/LobbyLocker-${VERSION}.exe"

!include "LogicLib.nsh"
!include "hooks.nsh"

Name "LobbyLocker"
BrandingText "LobbyLocker"

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\payload\lobbylocker-app.exe"
  File "..\..\payload\lobbylocker.exe"
  RMDir /r "$INSTDIR\data"
  CreateDirectory "$INSTDIR\data"
  File /r "..\..\payload\data\*.*" 
  CreateDirectory "$SMPROGRAMS\LobbyLocker"
  CreateShortcut "$SMPROGRAMS\LobbyLocker\LobbyLocker.lnk" "$INSTDIR\lobbylocker-app.exe"
  CreateShortcut "$SMPROGRAMS\LobbyLocker\LobbyLocker Cleanup (reset firewall).lnk" "$INSTDIR\lobbylocker.exe" "--reset-firewall"
  WriteRegStr HKLM "Software\LobbyLocker" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker" "DisplayName" "LobbyLocker"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  ; NSIS_HOOK_PREUNINSTALL runs here via the uninstaller's own logic? For
  ; manual NSIS builds the hook is invoked explicitly below.
  DetailPrint "Removing LobbyLocker-managed firewall rules..."
  StrCpy $0 -1
  ClearErrors
  ExecWait '"$INSTDIR\lobbylocker-app.exe" --reset-firewall' $0
  ${If} ${Errors}
  ${OrIf} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "LobbyLocker could not remove its firewall rules. The uninstall has stopped so you can retry Reset without losing the cleanup tool."
    SetErrorLevel 1
    Abort
  ${EndIf}
  RMDir /r "$INSTDIR"
  RMDir "$SMPROGRAMS\LobbyLocker"
  DeleteRegKey HKLM "Software\LobbyLocker"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker"
SectionEnd
