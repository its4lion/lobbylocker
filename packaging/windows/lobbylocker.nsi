; LobbyLocker NSIS installer (Rune build).
; Assumes the payload/ folder next to this file's directory holds:
;   lobbylocker-app.exe, lobbylocker.exe, data/
; In-app upgrades run this installer directly and preserve the firewall rules.
Unicode True
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\LobbyLocker"
InstallDirRegKey HKLM "Software\LobbyLocker" "InstallDir"
OutFile "../../release-assets/LobbyLocker-${VERSION}.exe"

!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WordFunc.nsh"

Name "LobbyLocker"
BrandingText "LobbyLocker"
Icon "../brand/lobbylocker.ico"
UninstallIcon "../brand/lobbylocker.ico"
Page instfiles

Var UpdatePID
Var LegacyUpdate
Var InstallerMutex
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker"

Function .onInit
  ; Serialize installation so a second updater cannot overwrite a newer build.
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Global\LobbyLockerInstaller") p.r0 ?e'
  Pop $1
  StrCpy $InstallerMutex $0
  ${If} $InstallerMutex == 0
  ${OrIf} $1 == 183
    MessageBox MB_OK|MB_ICONSTOP "Another LobbyLocker installer is running."
    SetErrorLevel 1
    Abort
  ${EndIf}

  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATEPID=" $UpdatePID
  ${If} ${Errors}
    StrCpy $UpdatePID ""
    ; v0.0.5/6 pass /UPDATE and exit after launching the installer.
    ClearErrors
    ${GetOptions} $0 "/UPDATE" $1
    ${If} ${Errors}
      Return
    ${EndIf}
    StrCpy $LegacyUpdate 1
    SetAutoClose true
  ${EndIf}

  StrCpy $INSTDIR "$PROGRAMFILES64\LobbyLocker"
  ; SYNCHRONIZE, then WAIT_OBJECT_0. ERROR_INVALID_PARAMETER means the PID
  ; already exited before OpenProcess. Other failures must stop the installer.
  ${If} $UpdatePID != ""
    System::Call 'kernel32::OpenProcess(i 0x100000, i 0, i $UpdatePID) p.r0 ?e'
    Pop $1
    ${If} $0 != 0
      System::Call 'kernel32::WaitForSingleObject(p r0, i 30000) i.r1'
      System::Call 'kernel32::CloseHandle(p r0)'
    ${ElseIf} $1 == 87
      StrCpy $1 0
    ${EndIf}
    ${If} $1 != 0
      MessageBox MB_OK|MB_ICONSTOP "Close LobbyLocker before retrying the update."
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}

  ReadRegStr $0 HKLM "${UNINSTALL_KEY}" "DisplayVersion"
  ${If} $0 != ""
    ${VersionCompare} "$0" "${VERSION}" $1
    ${If} $1 != 2
      Exec '"$INSTDIR\lobbylocker-app.exe"'
      Quit
    ${EndIf}
  ${EndIf}
FunctionEnd

Function .onInstSuccess
  ${If} $UpdatePID != ""
  ${OrIf} $LegacyUpdate == 1
    Exec '"$INSTDIR\lobbylocker-app.exe"'
  ${EndIf}
FunctionEnd

!macro RemoveLegacyRegistration ROOT
  ReadRegStr $0 ${ROOT} "${UNINSTALL_KEY}" "MainBinaryName"
  ReadRegStr $1 ${ROOT} "${UNINSTALL_KEY}" "DisplayVersion"
  ${If} $0 == "lobbylocker.exe"
    ${If} $1 == "0.0.5"
    ${OrIf} $1 == "0.0.6"
      DeleteRegKey ${ROOT} "${UNINSTALL_KEY}"
    ${EndIf}
  ${EndIf}
!macroend

!macro UpdateLegacyShortcuts CONTEXT
  SetShellVarContext ${CONTEXT}
  ${If} ${FileExists} "$SMPROGRAMS\LobbyLocker.lnk"
    CreateShortcut "$SMPROGRAMS\LobbyLocker.lnk" "$INSTDIR\lobbylocker-app.exe"
  ${EndIf}
  ${If} ${FileExists} "$SMPROGRAMS\LobbyLocker\LobbyLocker.lnk"
    CreateShortcut "$SMPROGRAMS\LobbyLocker\LobbyLocker.lnk" "$INSTDIR\lobbylocker-app.exe"
  ${EndIf}
  ${If} ${FileExists} "$DESKTOP\LobbyLocker.lnk"
    CreateShortcut "$DESKTOP\LobbyLocker.lnk" "$INSTDIR\lobbylocker-app.exe"
  ${EndIf}
!macroend

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\payload\lobbylocker-app.exe"
  File "..\..\payload\lobbylocker.exe"
  RMDir /r "$INSTDIR\data"
  SetOutPath "$INSTDIR\data"
  File /r "..\..\payload\data\*.*"
  SetOutPath "$INSTDIR"
  CreateDirectory "$SMPROGRAMS\LobbyLocker"
  CreateShortcut "$SMPROGRAMS\LobbyLocker\LobbyLocker.lnk" "$INSTDIR\lobbylocker-app.exe"
  CreateShortcut "$SMPROGRAMS\LobbyLocker\LobbyLocker Cleanup (reset firewall).lnk" "$INSTDIR\lobbylocker-app.exe" "--reset-firewall"
  ${If} $LegacyUpdate == 1
    !insertmacro UpdateLegacyShortcuts all
    !insertmacro UpdateLegacyShortcuts current
    !insertmacro RemoveLegacyRegistration HKCU
    SetRegView 64
    !insertmacro RemoveLegacyRegistration HKCU
    !insertmacro RemoveLegacyRegistration HKLM
    SetRegView 32
    !insertmacro RemoveLegacyRegistration HKLM
  ${EndIf}
  WriteRegStr HKLM "Software\LobbyLocker" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker" "DisplayName" "LobbyLocker"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\lobbylocker-app.exe",0'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LobbyLocker" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  DeleteRegValue HKLM "${UNINSTALL_KEY}" "MainBinaryName"
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
