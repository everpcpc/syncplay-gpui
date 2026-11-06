; Per-user Syncplay installer (no admin required).
; Expects syncplay.exe next to this script; pass /DVERSION=x.y.z and
; /DOUTFILE=path on the makensis command line.

Unicode true
!include "MUI2.nsh"
!define MUI_ICON "syncplay.ico"
!define MUI_UNICON "syncplay.ico"

!ifndef VERSION
!define VERSION "0.0.0"
!endif
!ifndef OUTFILE
!define OUTFILE "syncplay-setup.exe"
!endif

Name "Syncplay"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Syncplay"
InstallDirRegKey HKCU "Software\Syncplay" "InstallDir"
RequestExecutionLevel user

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
	SetOutPath "$INSTDIR"
	File "syncplay.exe"
	WriteUninstaller "$INSTDIR\Uninstall.exe"

	CreateDirectory "$SMPROGRAMS\Syncplay"
	CreateShortcut "$SMPROGRAMS\Syncplay\Syncplay.lnk" "$INSTDIR\syncplay.exe"
	CreateShortcut "$SMPROGRAMS\Syncplay\Uninstall.lnk" "$INSTDIR\Uninstall.exe"

	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "DisplayName" "Syncplay"
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "DisplayIcon" '"$INSTDIR\syncplay.exe",0'
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "UninstallString" '"$INSTDIR\Uninstall.exe"'
	WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "DisplayVersion" "${VERSION}"
	WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "NoModify" 1
	WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay" "NoRepair" 1
	; ani-cli discovers syncplay.exe through PATH. EnVar changes only the
	; current user's environment, so installation still needs no elevation.
	EnVar::AddValue "PATH" "$INSTDIR"
	Pop $0
	System::Call 'user32::SendMessageTimeout(i 0xFFFF, i 0x001A, i 0, t "Environment", i 0x0002, i 5000, *i .r1)'
SectionEnd

Section "Uninstall"
	Delete "$INSTDIR\syncplay.exe"
	Delete "$INSTDIR\Uninstall.exe"
	EnVar::DeleteValue "PATH" "$INSTDIR"
	Pop $0
	System::Call 'user32::SendMessageTimeout(i 0xFFFF, i 0x001A, i 0, t "Environment", i 0x0002, i 5000, *i .r1)'
	RMDir "$INSTDIR"
	Delete "$SMPROGRAMS\Syncplay\Syncplay.lnk"
	Delete "$SMPROGRAMS\Syncplay\Uninstall.lnk"
	RMDir "$SMPROGRAMS\Syncplay"
	DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Syncplay"
	DeleteRegKey HKCU "Software\Syncplay"
SectionEnd
