; Craftmjne installer (NSIS / Modern UI 2).
;
; This installs the **launcher**, not the game. The launcher downloads game
; versions itself into %LOCALAPPDATA%\Craftmjne\versions\<version>\, so a
; version is an ordinary folder that can be added, removed or switched
; between freely - nothing ever rewrites a running executable in place, which
; is what the old in-game updater did and what kept going wrong.
;
; Installs per-user to %LOCALAPPDATA%\Craftmjne - no admin rights needed, and
; the same directory the launcher already keeps saves/ and versions/ in, so
; everything Craftmjne owns lives in one place.
;
; Build (from repo root, after `cargo build --release -p craftmjne-launcher`):
;   makensis -DAPP_VERSION=1.0.0 -DSRC_EXE=target\release\craftmjne-launcher.exe installer\craftmjne.nsi
; Produces CraftmjneSetup.exe in the repo root.

!include "MUI2.nsh"

!ifndef APP_VERSION
  !define APP_VERSION "0.0.0"
!endif
!ifndef SRC_EXE
  !define SRC_EXE "..\target\release\craftmjne-launcher.exe"
!endif

Name "Craftmjne"
OutFile "..\CraftmjneSetup.exe"
InstallDir "$LOCALAPPDATA\Craftmjne"
InstallDirRegKey HKCU "Software\Craftmjne" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "Craftmjne Launcher"
VIAddVersionKey "FileDescription" "Craftmjne launcher installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "MIT license"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\craftmjne-launcher.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open the Craftmjne launcher"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Craftmjne Launcher" SEC_APP
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=craftmjne-launcher.exe" "${SRC_EXE}"

  ; No blocks/ or textures/ here on purpose: those ship inside each game
  ; version's own download, so every installed version carries the block
  ; definitions it was actually built against instead of sharing one copy
  ; that only matches whichever version was installed last.

  WriteRegStr HKCU "Software\Craftmjne" "InstallDir" "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Craftmjne"
  CreateShortcut "$SMPROGRAMS\Craftmjne\Craftmjne.lnk" "$INSTDIR\craftmjne-launcher.exe"
  CreateShortcut "$SMPROGRAMS\Craftmjne\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\Craftmjne.lnk" "$INSTDIR\craftmjne-launcher.exe"

  ; Add/Remove Programs entry (per-user, HKCU — no admin rights needed).
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "DisplayName" "Craftmjne"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "DisplayIcon" "$INSTDIR\craftmjne-launcher.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "Publisher" "Craftmjne"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "DisplayVersion" "${APP_VERSION}"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "NoRepair" 1
SectionEnd

Section "Uninstall"
  ; Deliberately file-by-file, never `RMDir /r "$INSTDIR"`: the install
  ; directory is also where the launcher keeps saves\ and versions\, and
  ; uninstalling the launcher must not delete anyone's worlds. The plain
  ; RMDir at the end only succeeds if the folder is genuinely empty, so a
  ; user with no saved worlds gets a clean removal and everyone else keeps
  ; their data.
  Delete "$INSTDIR\craftmjne-launcher.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Craftmjne\Craftmjne.lnk"
  Delete "$SMPROGRAMS\Craftmjne\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Craftmjne"
  Delete "$DESKTOP\Craftmjne.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne"
  DeleteRegKey HKCU "Software\Craftmjne"
SectionEnd
