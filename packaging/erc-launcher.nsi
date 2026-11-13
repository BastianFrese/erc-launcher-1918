; Installationsprogramm des ERC Launchers.
;
; Baut aus dem Ordner `dist-1918` ein Setup, das jeder Fahrer doppelklicken kann:
; Programm nach %LOCALAPPDATA%\Programs\ERCL, Startmenue-Eintrag, Eintrag unter
; "Apps", und ein Deinstallationsprogramm.
;
;     makensis /INPUTCHARSET UTF8 packaging\erc-launcher.nsi

Unicode true
SetCompressor /SOLID lzma

!define NAME "ERC Launcher"
!define VERSION "0.7.8"
!define HERSTELLER "Erdi's Racing Community"
!define QUELLE "C:\Users\basti\source\repos\swofty-erc-1918\dist-1918"
!define ZIEL "$LOCALAPPDATA\Programs\ERCL"
!define SCHLUESSEL "Software\Microsoft\Windows\CurrentVersion\Uninstall\ERCL"

Name "${NAME} ${VERSION}"
OutFile "C:\Users\basti\source\repos\swofty-erc-1918\ERC-Launcher-${VERSION}-setup.exe"
InstallDir "${ZIEL}"
InstallDirRegKey HKCU "${SCHLUESSEL}" "InstallLocation"
RequestExecutionLevel user
BrandingText "${HERSTELLER}"
Icon "C:\Users\basti\source\repos\erc-launcher\src-tauri\icons\icon.ico"
UninstallIcon "C:\Users\basti\source\repos\erc-launcher\src-tauri\icons\icon.ico"

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "C:\Users\basti\source\repos\erc-launcher\src-tauri\icons\icon.ico"
!define MUI_UNICON "C:\Users\basti\source\repos\erc-launcher\src-tauri\icons\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\erc-launcher.exe"
!define MUI_FINISHPAGE_RUN_TEXT "ERC Launcher jetzt starten"

!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_PRE VerzeichnisUeberspringen
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "German"

; Ist schon eine Fassung eingerichtet, wird sie ersetzt statt danebengelegt:
; das Verzeichnis steht fest, die Auswahlseite faellt weg.
Function VerzeichnisUeberspringen
  ReadRegStr $0 HKCU "${SCHLUESSEL}" "InstallLocation"
  StrCmp $0 "" fertig
    StrCpy $INSTDIR $0
    Abort
  fertig:
FunctionEnd

Section "ERC Launcher" SecMain
  SetOutPath "$INSTDIR"

  ; Erst pruefen, ob das Programm noch laeuft: eine laufende .exe laesst sich
  ; nicht ersetzen, und ein halb ueberschriebener Ordner startet nicht mehr.
  ; Im stillen Lauf fragt niemand — dort wird gewartet, bis die alte .exe weg ist.
  ${IfNot} ${Silent}
    Sleep 1500
    IfFileExists "$INSTDIR\erc-launcher.exe" 0 weiter
      FindWindow $0 "" "ERC Launcher for F1 25"
      StrCmp $0 0 weiter
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
          "Der ERC Launcher laeuft noch. Bitte schliesse ihn, dann geht es weiter." \
          IDRETRY weiter
        Abort
    weiter:
  ${EndIf}

  ; Die Altdatei wegnehmen, bevor kopiert wird. Der Launcher hat sich fuer seine
  ; eigene Aktualisierung gerade beendet; bis Windows ihn freigibt, dauert es
  ; einen Augenblick. Bis zu zehn Sekunden warten.
  StrCpy $1 0
  warten:
    Delete "$INSTDIR\erc-launcher.exe"
    IfFileExists "$INSTDIR\erc-launcher.exe" 0 frei
    Sleep 500
    IntOp $1 $1 + 1
    IntCmp $1 20 frei warten warten
  frei:

  File /r "${QUELLE}\*.*"

  CreateDirectory "$SMPROGRAMS\ERC Launcher"
  CreateShortCut "$SMPROGRAMS\ERC Launcher\ERC Launcher.lnk" "$INSTDIR\erc-launcher.exe"
  CreateShortCut "$DESKTOP\ERC Launcher.lnk" "$INSTDIR\erc-launcher.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "${SCHLUESSEL}" "DisplayName" "${NAME}"
  WriteRegStr HKCU "${SCHLUESSEL}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${SCHLUESSEL}" "Publisher" "${HERSTELLER}"
  WriteRegStr HKCU "${SCHLUESSEL}" "DisplayIcon" "$INSTDIR\erc-launcher.exe"
  WriteRegStr HKCU "${SCHLUESSEL}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${SCHLUESSEL}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${SCHLUESSEL}" "NoModify" 1
  WriteRegDWORD HKCU "${SCHLUESSEL}" "NoRepair" 1
SectionEnd

; Nach dem stillen Lauf startet der Installer den Launcher selbst wieder —
; der Fahrer hat nur einmal geklickt und sieht kein Fenster dazwischen.
Function .onInstSuccess
  ${If} ${Silent}
    Exec '"$INSTDIR\erc-launcher.exe"'
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\ERC Launcher\ERC Launcher.lnk"
  RMDir "$SMPROGRAMS\ERC Launcher"
  Delete "$DESKTOP\ERC Launcher.lnk"
  DeleteRegKey HKCU "${SCHLUESSEL}"

  ; Das Programm selbst zuerst, damit danach der Ordner leer ist.
  Delete "$INSTDIR\erc-launcher.exe"
  RMDir /r "$INSTDIR"
SectionEnd
