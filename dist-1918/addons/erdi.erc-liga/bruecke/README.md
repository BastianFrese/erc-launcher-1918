# Brücke — LAN-Sitzung über das Internet

Verbindet das Netzwerkspiel von F1 25 über das Internet, ohne VPN, ohne Swoftys
Zugangssperre. Grundlage ist der Netzwerkweg, der im installierten Kern bereits steckt
(`SWOFTY_NETPLAY`).

## Wie es zusammenhängt

```
Spiel --(127.0.0.1:base+j)--> Brücke --(Internet:--port)--> Brücke --(127.0.0.1:base+j)--> Spiel
```

Das Spiel bekommt beim Start:

```powershell
$env:SWOFTY_NETPLAY = "self=0;base=30000"
swofty run
```

Dann schickt es alles, was für Mitglied *j* bestimmt ist, an `127.0.0.1:30000+j`, und
hält alles, was von dort kommt, für Pakete von Mitglied *j*. Der Kern schreibt bei
gebundenem Spielsocket `swofty/netplay.json` mit `game_port` — daraus liest die Brücke
den Zielport.

Zwischen zwei Brücken trägt jedes Paket vorne ein Byte: die Nummer des Absenders.

## Starten

Auf **jedem** Rechner, mit eigener Nummer und den Adressen der anderen:

```powershell
python bruecke.py --selbst 0 --basis 30000 --port 35500 --mitglied 1=203.0.113.5:35500
python bruecke.py --selbst 1 --basis 30000 --port 35500 --mitglied 0=198.51.100.7:35500
```

Der Host braucht eine Portweiterleitung für UDP `35500` auf seinen Rechner; die anderen
nur eine erreichbare Adresse.

## Prüfen

```powershell
python -I bruecke.py --selbsttest     # zwei Brücken über Loopback, beide Richtungen
```

Im Spiel-Log muss stehen:

```
[netplay] bridge on, self=0 base=30000 game_port=21414
```

## Grenzen

- **Unverschlüsselt.** Wer den Port kennt, kann Pakete einspeisen. Für die eigene Liga
  im ersten Test gedacht; vor dem Ligabetrieb gehört ein gemeinsamer Schlüssel davor.
- **Ein Host ist Autorität.** Wechselt er, bricht die Sitzung ab.
- **Mod-Abgleich nicht eingebaut.** Alle müssen denselben Modstand haben — das prüft
  derzeit niemand.
