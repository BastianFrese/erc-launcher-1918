# ERC Liga — Swofty-Addon für die eigene Liga

Natives Plugin für F1 25 (Swofty), das Befehle der ERC-Rennleitung ins Renn-HUD bringt.
Eigenentwicklung für die eigene Liga, nicht zur Veröffentlichung bestimmt.

## Was es in dieser Fassung tut

Es liest eine kleine Textdatei und zeigt jede Änderung als Meldung im Renn-HUD:

```
%LOCALAPPDATA%\Swofty\addon-data\erdi.erc-liga\befehl.txt
Zeile 1: gruen | gelb | sc | vsc | rot | ziel
Zeile 2: optionaler eigener Text
```

Das Panel `erc-steward-panel` schreibt diese Datei bei jedem Flaggenwechsel. Damit ist
die Kette Panel → Spiel zum ersten Mal geschlossen.

**Was es noch nicht tut:** ein echtes Safety Car oder eine rote Flagge *auslösen*. Dafür
müsste das Plugin Engine-Funktionen finden und aufrufen, die das SDK nicht mitliefert
(`scan` + `resolve` + Mixins). Das ist die offene Wochenarbeit, siehe
`~\.claude\state\f1-liga-sdk-pruefung.md`.

## Bauen

```powershell
powershell -ExecutionPolicy Bypass -File native\build.ps1
```

MSVC (Visual Studio 18) statt `zig c++` aus der SDK-Vorlage — zig ist auf dem Rechner
nicht installiert. Ergebnis: `native\liga_plugin.dll` (~31 KB), Exporte
`SwoftyAddonApi` und `SwoftyAddonInit`.

**Abweichung vom SDK:** `native\include\swofty\mixin.hpp` ist lokal gepatcht. MSVC
lehnt die Selbstreferenz in `bound<…>::addr` ab (`addr: nichtdeklarierter Bezeichner`);
jetzt erst deklariert, dann definiert. Gleiche Wirkung, für MSVC übersetzbar. Beim
Hochziehen einer neuen SDK-Fassung geht der Patch verloren und muss erneut rein.

## Installieren

```powershell
swofty addons install .
swofty addons enable erdi.erc-liga
swofty registry check
```

`swofty.exe` liegt unter `%LOCALAPPDATA%\Programs\SwoftyLauncher\bin\`.

Rückbau: `swofty addons disable erdi.erc-liga` (oder `remove`).

## Testen

1. Panel starten: `python panel.py` (im Repo `erc-steward-panel`)
2. Eine Flagge schalten — die Datei `befehl.txt` ändert sich
3. F1 25 modded starten (`swofty run`) und ein Training/Rennen fahren
4. Beim Umschalten muss die Meldung im Renn-HUD erscheinen

Das Log des Plugins steht in `swofty/loader.log` (Zeilen mit `[erc-liga]`).
