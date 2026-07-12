; Craftmjne installer (NSIS / Modern UI 2).
;
; Installs per-user to %LOCALAPPDATA%\Craftmjne — no admin rights required to
; install *or* to update. This matters because the in-game auto-updater
; (src/updater.rs) rewrites the installed .exe in place on every launch; a
; Program Files install would need a UAC-elevated updater to do that, which
; is a much bigger (and worse) piece of software than the game itself.
;
; Build (from repo root, after `cargo build --release`):
;   makensis -DAPP_VERSION=0.2.0 -DSRC_EXE=target\release\craftmjne.exe installer\craftmjne.nsi
; Produces CraftmjneSetup.exe in the repo root.

!include "MUI2.nsh"

!ifndef APP_VERSION
  !define APP_VERSION "0.0.0"
!endif
!ifndef SRC_EXE
  !define SRC_EXE "..\target\release\craftmjne.exe"
!endif

Name "Craftmjne"
OutFile "..\CraftmjneSetup.exe"
InstallDir "$LOCALAPPDATA\Craftmjne"
InstallDirRegKey HKCU "Software\Craftmjne" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "Craftmjne"
VIAddVersionKey "FileDescription" "Craftmjne voxel game installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "MIT license"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\craftmjne.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Craftmjne now"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Craftmjne" SEC_APP
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=craftmjne.exe" "${SRC_EXE}"

  ; Block definitions (one *.json per block - see src/blocks.rs). The game
  ; looks for this folder next to its own exe at startup and won't run
  ; without it. Relative to this script's own directory (installer/), same
  ; as everything else here except SRC_EXE.
  SetOutPath "$INSTDIR\blocks"
  File /r "..\blocks\*.json"
  SetOutPath "$INSTDIR"

  ; Optional custom-texture folder (see src/atlas.rs and textures/blocks/
  ; README.md) - just the README, so it exists and is discoverable right
  ; next to the exe; the game runs fine with nothing else in it.
  SetOutPath "$INSTDIR\textures\blocks"
  File "..\textures\blocks\README.md"
  SetOutPath "$INSTDIR"

  WriteRegStr HKCU "Software\Craftmjne" "InstallDir" "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Craftmjne"
  CreateShortcut "$SMPROGRAMS\Craftmjne\Craftmjne.lnk" "$INSTDIR\craftmjne.exe"
  CreateShortcut "$SMPROGRAMS\Craftmjne\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\Craftmjne.lnk" "$INSTDIR\craftmjne.exe"

  ; Add/Remove Programs entry (per-user, HKCU — no admin rights needed).
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "DisplayName" "Craftmjne"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne" \
    "DisplayIcon" "$INSTDIR\craftmjne.exe"
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
  Delete "$INSTDIR\craftmjne.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\blocks"
  RMDir /r "$INSTDIR\textures"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Craftmjne\Craftmjne.lnk"
  Delete "$SMPROGRAMS\Craftmjne\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Craftmjne"
  Delete "$DESKTOP\Craftmjne.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftmjne"
  DeleteRegKey HKCU "Software\Craftmjne"
SectionEnd
