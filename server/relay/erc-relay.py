#!/usr/bin/env python3
"""ERC-Relay — vermittelt Lobbys zwischen Rechnern, die keinen Port freigeben können.

Jeder Teilnehmer schickt seine Pakete an diesen Dienst, der sie an die übrigen
Teilnehmer derselben Lobby weiterreicht. Damit braucht nur noch der Server einen
offenen Port.

Paketformat von einem Teilnehmer:

    b"ERC-RELAY" + raum (4 Zeichen, ASCII) + eigene Kennung (1 Byte) + Nutzlast

Der Dienst merkt sich, von welcher Adresse welche Kennung in welchem Raum kommt,
und schickt jedes Paket an alle anderen Teilnehmer dieses Raums — unverändert,
mit der Kennung des Absenders davor wie sie ankommt.

Warum so einfach: Der Launcher setzt die Kennung ohnehin vor jedes Paket, um
Mitglieder zu unterscheiden. Der Relay muss sie nur weiterreichen.
"""

from __future__ import annotations

import json
import os
import signal
import socket
import time

PORT = 35501
STILLE_SEK = 90  # so lange bleibt ein Teilnehmer ohne Lebenszeichen im Raum
KOPF = b"ERC-RELAY"
RAUM_LEN = 4

# Wer die Lobbys auflisten will, schickt das hier — ohne Raum, ohne Kennung.
LISTE = b"ERC-LOBBYS"

# Der Host beschreibt seine Lobby: ERC-RAUM name=…;kat=…
BESCHREIBUNG = b"ERC-RAUM "


def zerlegen(paket: bytes):
    """Raum, Kennung und Nutzlast aus einem Paket holen — oder None."""
    if len(paket) < len(KOPF) + RAUM_LEN + 2 or not paket.startswith(KOPF):
        return None
    raum = paket[len(KOPF) : len(KOPF) + RAUM_LEN].decode("ascii", "ignore")
    if not raum.isalnum():
        return None
    kennung = paket[len(KOPF) + RAUM_LEN]
    nutzlast = paket[len(KOPF) + RAUM_LEN + 1 :]
    return raum, kennung, nutzlast


def zustand_datei() -> str:
    """Wo der Stand liegt. `DynamicUser=yes` macht `/opt` schreibgeschuetzt,
    deshalb gibt systemd ueber `StateDirectory` einen Ordner vor."""
    ordner = (os.environ.get("STATE_DIRECTORY") or "").split(":")[0]
    if not ordner or not os.path.isdir(ordner):
        ordner = os.path.dirname(os.path.abspath(__file__))
    return os.path.join(ordner, "rooms.json")


def zustand_laden():
    """Raeume und Beschreibungen von der letzten Runde — ein Neustart soll eine
    laufende Lobby nicht aus der Liste werfen."""
    raeume: dict[str, dict[int, tuple[tuple[str, int], float]]] = {}
    beschreibung: dict[str, dict[str, object]] = {}
    try:
        with open(zustand_datei(), encoding="utf-8") as f:
            stand = json.load(f)
    except (OSError, ValueError):
        return raeume, beschreibung
    jetzt = time.time()
    for raum, mitglieder in (stand.get("raeume") or {}).items():
        for kennung, (adresse, wann) in (mitglieder or {}).items():
            if jetzt - float(wann) <= STILLE_SEK:
                raeume.setdefault(raum, {})[int(kennung)] = (tuple(adresse), float(wann))
    for raum, info in (stand.get("beschreibung") or {}).items():
        if raum in raeume and isinstance(info, dict):
            beschreibung[raum] = info
    if raeume:
        print(f"Stand geladen: {len(raeume)} Raum/Raeume", flush=True)
    return raeume, beschreibung


def zustand_sichern(raeume, beschreibung) -> None:
    datei = zustand_datei()
    stand = {
        "raeume": {
            raum: {str(k): [list(a), w] for k, (a, w) in mitglieder.items()} for raum, mitglieder in raeume.items()
        },
        "beschreibung": beschreibung,
    }
    try:
        with open(datei + ".neu", "w", encoding="utf-8") as f:
            json.dump(stand, f)
        os.replace(datei + ".neu", datei)
    except OSError as e:
        print(f"Stand nicht gesichert: {e}", flush=True)


def main() -> None:
    raeume, beschreibung = zustand_laden()
    schmutzig = False
    gesichert = 0.0

    def bei_stopp(*_):
        zustand_sichern(raeume, beschreibung)
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, bei_stopp)
    signal.signal(signal.SIGINT, bei_stopp)

    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("0.0.0.0", PORT))
    print(f"ERC-Relay auf UDP {PORT}", flush=True)

    while True:
        try:
            paket, quelle = s.recvfrom(65535)
        except OSError:
            continue

        # Lobbyliste anfragen.
        if paket == LISTE:
            jetzt = time.time()
            zeilen = ["ERC-LOBBYS"]
            for raum, mitglieder in list(raeume.items()):
                if not mitglieder:
                    continue
                letzte = max(wann for _, wann in mitglieder.values())
                if jetzt - letzte > STILLE_SEK:
                    raeume.pop(raum, None)
                    beschreibung.pop(raum, None)
                    continue
                info = beschreibung.get(raum, {})
                name = str(info.get("name", "Lobby"))
                kat = str(info.get("kat", ""))
                anzahl = len(mitglieder)
                zeilen.append(f"{raum}|{name}|{anzahl}|{kat}")
            s.sendto("\n".join(zeilen).encode(), quelle)
            print(f"[{time.strftime('%H:%M:%S')}] Lobbyliste an {quelle}: {len(zeilen) - 1} Raum/Räume", flush=True)
            continue

        teile = zerlegen(paket)
        if teile is None:
            continue
        raum, kennung, nutzlast = teile

        jetzt = time.time()
        mitglieder = raeume.setdefault(raum, {})
        neu = kennung not in mitglieder
        mitglieder[kennung] = (quelle, jetzt)
        if neu:
            schmutzig = True
            print(f"[{time.strftime('%H:%M:%S')}] Raum {raum}: Mitglied {kennung} von {quelle}", flush=True)

        # Beschreibung der Lobby (Name, Fahrzeugklasse) merken — sie kommt vom Host.
        if nutzlast.startswith(BESCHREIBUNG) and kennung == 1:
            angaben = nutzlast[len(BESCHREIBUNG) :].decode("utf-8", "replace")
            felder = {}
            for stueck in angaben.split(";"):
                if "=" in stueck:
                    k, v = stueck.split("=", 1)
                    felder[k.strip()] = v.strip()
            beschreibung[raum] = {"name": felder.get("name", "Lobby"), "kat": felder.get("kat", "")}
            schmutzig = True

        # Alte Teilnehmer vergessen.
        for k, (_, wann) in list(mitglieder.items()):
            if jetzt - wann > STILLE_SEK:
                mitglieder.pop(k, None)
                schmutzig = True
                print(f"[{time.strftime('%H:%M:%S')}] Raum {raum}: Mitglied {k} ist gegangen", flush=True)

        # Prüfrufe und Antworten mitschreiben — daran sieht man, ob ein Host antwortet.
        if nutzlast.startswith(b"ERC-PING"):
            ziele = [k for k in mitglieder if k != kennung]
            print(
                f"[{time.strftime('%H:%M:%S')}] Raum {raum}: Prüfruf von {kennung} an {ziele}",
                flush=True,
            )
        elif nutzlast.startswith(b"ERC-PONG"):
            print(
                f"[{time.strftime('%H:%M:%S')}] Raum {raum}: Antwort von {kennung}",
                flush=True,
            )

        # Kopf wieder davor, denn die Kennung des Absenders braucht der Empfänger.
        weiter = KOPF + raum.encode() + bytes([kennung]) + nutzlast
        for andere, (adresse, _) in list(mitglieder.items()):
            if andere == kennung:
                continue
            try:
                s.sendto(weiter, adresse)
            except OSError:
                pass

        # Den Stand hoechstens alle zwei Sekunden auf die Platte.
        if schmutzig and jetzt - gesichert >= 2:
            zustand_sichern(raeume, beschreibung)
            gesichert, schmutzig = jetzt, False


if __name__ == "__main__":
    main()
