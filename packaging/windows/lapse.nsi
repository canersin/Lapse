; Lapse Windows Setup Script (NSIS 3 Modern UI 2)
; Built on Linux with makensis

Unicode true
SetCompressor /SOLID lzma

; Definitions with defaults if not supplied via command line
!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef STAGE
  !define STAGE "."
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif

; Application metadata
Name "Lapse"
OutFile "${OUTDIR}/Lapse-${VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\Lapse"
InstallDirRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "InstallLocation"
RequestExecutionLevel admin

; Includes
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

; Insert FileFunc macros
!insertmacro GetSize

; Icons
!define MUI_ICON "${STAGE}/lapse.ico"
!define MUI_UNICON "${STAGE}/lapse.ico"

; Language Selection Dialog Settings (persisted for uninstaller)
!define MUI_LANGDLL_REGISTRY_ROOT "HKLM"
!define MUI_LANGDLL_REGISTRY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse"
!define MUI_LANGDLL_REGISTRY_VALUENAME "InstallerLanguage"

; UI Configuration
!define MUI_ABORTWARNING

; Installer Pages
!insertmacro MUI_PAGE_WELCOME

; License Page (repo's LICENSE file)
!insertmacro MUI_PAGE_LICENSE "${STAGE}/LICENSE"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES

; Finish Page with Run Lapse checkbox
!define MUI_FINISHPAGE_RUN "$INSTDIR\lapse.exe"
!define MUI_FINISHPAGE_RUN_TEXT $(RUN_LAPSE_TEXT)
!insertmacro MUI_PAGE_FINISH

; Uninstaller Pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

; Supported Languages
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Turkish"

; Language Strings - English
LangString DESC_SecMain ${LANG_ENGLISH} "Lapse (required)"
LangString DESC_SecDesktopShortcut ${LANG_ENGLISH} "Desktop shortcut"
LangString DESC_SecAutostart ${LANG_ENGLISH} "Start Lapse when Windows starts"
LangString RUN_LAPSE_TEXT ${LANG_ENGLISH} "Run Lapse"
LangString UNINSTALL_DATA_PROMPT ${LANG_ENGLISH} "Do you also want to delete settings (%APPDATA%\lapse) and temporary buffer (%LOCALAPPDATA%\lapse)?$\r$\n$\r$\nNote: Saved clips will never be deleted."
LangString DESC_SecMain_Tooltip ${LANG_ENGLISH} "Installs core Lapse executable, FFmpeg dependencies, and application assets."
LangString DESC_SecDesktopShortcut_Tooltip ${LANG_ENGLISH} "Creates a shortcut to Lapse on your desktop."
LangString DESC_SecAutostart_Tooltip ${LANG_ENGLISH} "Starts Lapse in daemon mode automatically when Windows starts."

; Language Strings - Turkish
LangString DESC_SecMain ${LANG_TURKISH} "Lapse (gerekli)"
LangString DESC_SecDesktopShortcut ${LANG_TURKISH} "Masaüstü kısayolu"
LangString DESC_SecAutostart ${LANG_TURKISH} "Windows başladığında Lapse'i başlat"
LangString RUN_LAPSE_TEXT ${LANG_TURKISH} "Lapse uygulamasını çalıştır"
LangString UNINSTALL_DATA_PROMPT ${LANG_TURKISH} "Ayarları (%APPDATA%\lapse) ve geçici arabelleği (%LOCALAPPDATA%\lapse) de silmek istiyor musunuz?$\r$\n$\r$\nNot: Kaydedilen klipler hiçbir zaman silinmez."
LangString DESC_SecMain_Tooltip ${LANG_TURKISH} "Temel Lapse çalıştırılabilir dosyasını, FFmpeg bağımlılıklarını ve uygulama varlıklarını yükler."
LangString DESC_SecDesktopShortcut_Tooltip ${LANG_TURKISH} "Masaüstünde Lapse için bir kısayol oluşturur."
LangString DESC_SecAutostart_Tooltip ${LANG_TURKISH} "Windows başladığında Lapse'i arka planda otomatik olarak başlatır."

; Initialization functions
Function .onInit
  SetRegView 64
  !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Function un.onInit
  SetRegView 64
  !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

; Installer Sections
Section "$(DESC_SecMain)" SecMain
  SectionIn RO
  SetRegView 64

  ; Kill any running lapse.exe before installing over an existing install
  nsExec::Exec 'taskkill /f /im lapse.exe'
  Pop $0
  Sleep 500

  SetOutPath "$INSTDIR"
  File "${STAGE}/lapse.exe"
  File "${STAGE}/ffmpeg.exe"
  File "${STAGE}/ffprobe.exe"
  File "${STAGE}/FFMPEG-LICENSE.txt"
  File "${STAGE}/lapse.ico"

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start Menu shortcut
  CreateDirectory "$SMPROGRAMS\Lapse"
  CreateShortcut "$SMPROGRAMS\Lapse\Lapse.lnk" "$INSTDIR\lapse.exe" "" "$INSTDIR\lapse.ico"
  CreateShortcut "$SMPROGRAMS\Lapse.lnk" "$INSTDIR\lapse.exe" "" "$INSTDIR\lapse.ico"

  ; Calculate estimated size in KB for ARP
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0

  ; Register uninstaller under HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "DisplayName" "Lapse"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "Publisher" "Lapse"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "DisplayIcon" "$INSTDIR\lapse.ico"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "EstimatedSize" $0
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse" "NoRepair" 1
SectionEnd

Section "$(DESC_SecDesktopShortcut)" SecDesktopShortcut
  SetOutPath "$INSTDIR"
  CreateShortcut "$DESKTOP\Lapse.lnk" "$INSTDIR\lapse.exe" "" "$INSTDIR\lapse.ico"
SectionEnd

Section "$(DESC_SecAutostart)" SecAutostart
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Lapse" '"$INSTDIR\lapse.exe" --daemon'
SectionEnd

; Section descriptions
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_SecMain_Tooltip)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktopShortcut} $(DESC_SecDesktopShortcut_Tooltip)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} $(DESC_SecAutostart_Tooltip)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; Uninstaller Section
Section "Uninstall"
  SetRegView 64

  ; 1. Kill running lapse.exe first
  nsExec::Exec 'taskkill /f /im lapse.exe'
  Pop $0
  Sleep 500

  ; 2. Remove shortcuts
  Delete "$DESKTOP\Lapse.lnk"
  Delete "$SMPROGRAMS\Lapse.lnk"
  Delete "$SMPROGRAMS\Lapse\Lapse.lnk"
  RMDir "$SMPROGRAMS\Lapse"

  ; 3. Remove autostart Run value
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Lapse"

  ; 4. Remove uninstaller registry entry
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lapse"

  ; 5. Remove installed files
  Delete "$INSTDIR\lapse.exe"
  Delete "$INSTDIR\ffmpeg.exe"
  Delete "$INSTDIR\ffprobe.exe"
  Delete "$INSTDIR\FFMPEG-LICENSE.txt"
  Delete "$INSTDIR\lapse.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  ; 6. Ask whether to also delete settings and buffer (default No; skipped if silent)
  IfSilent skip_data
  MessageBox MB_YESNO|MB_DEFBUTTON2|MB_ICONQUESTION $(UNINSTALL_DATA_PROMPT) IDNO skip_data
    RMDir /r "$APPDATA\lapse"
    RMDir /r "$LOCALAPPDATA\lapse"
  skip_data:
SectionEnd
