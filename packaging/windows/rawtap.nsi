; RawTap per-user installer (setup.exe), built by scripts/build-windows-release.mjs:
;   makensis /DVERSION=X.Y.Z /DBINARY=... /DICON=... /DPATH_HELPER=... /DOUTFILE=... rawtap.nsi
; Installs to %LOCALAPPDATA%\rawtap without admin rights, adds that folder to
; the user's PATH, a Start menu shortcut and an Apps & features entry.
; Silent install / uninstall: /S. docs/install.ps1 runs it that way.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!define APP_NAME "RawTap"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\rawtap"
!define POWERSHELL "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\rawtap"
Icon "${ICON}"
UninstallIcon "${ICON}"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "CompanyName" "Jack Brumley"
VIAddVersionKey "LegalCopyright" "Jack Brumley, AGPL-3.0"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "/oname=rawtap.exe" "${BINARY}"
  File "/oname=rawtap.ico" "${ICON}"
  File "/oname=path-helper.ps1" "${PATH_HELPER}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\rawtap.exe" "" "$INSTDIR\rawtap.ico"

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Jack Brumley"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://brumley.info/rawtap"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\rawtap.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1

  nsExec::ExecToLog '"${POWERSHELL}" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\path-helper.ps1" -Action Add -Directory "$INSTDIR"'
  Pop $0
  StrCmp $0 "0" path_added
    DetailPrint "Could not add $INSTDIR to your PATH (exit $0). Run rawtap from that folder, or add it yourself."
    MessageBox MB_ICONEXCLAMATION|MB_OK "RawTap is installed, but its folder could not be added to your PATH.$\r$\n$\r$\n$INSTDIR" /SD IDOK
    SetErrorLevel 2
  path_added:
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog '"${POWERSHELL}" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\path-helper.ps1" -Action Remove -Directory "$INSTDIR"'
  Pop $0
  StrCmp $0 "0" path_removed
    DetailPrint "Could not remove $INSTDIR from your PATH (exit $0)."
    SetErrorLevel 2
  path_removed:

  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$INSTDIR\rawtap.exe"
  Delete "$INSTDIR\rawtap.ico"
  Delete "$INSTDIR\path-helper.ps1"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
