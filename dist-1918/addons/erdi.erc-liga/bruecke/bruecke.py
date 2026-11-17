"""ERC-Bruecke: verbindet das LAN-Netzwerkspiel von F1 25 ueber das Internet.

Das Spiel bekommt SWOFTY_NETPLAY="self=<N>;base=<PORT>" und schickt dann alles, was
fuer Mitglied j bestimmt ist, an 127.0.0.1:(PORT+j). Was von 127.0.0.1:(PORT+j) kommt,
haelt es fuer ein Paket von Mitglied j. Diese Bruecke ist deshalb nur eine Vermittlung:

    Spiel  --(base+j)-->  Bruecke  --(Internet)-->  Bruecke  --(base+i)-->  Spiel

Jedes Paket zwischen zwei Bruecken traegt ein Byte vorne: die Nummer des Absenders.

    python bruecke.py --selbst 0 --basis 30000 --port 35500 --mitglied 1=203.0.113.5:35500
    python bruecke.py --selbst 1 --basis 30000 --port 35500 --mitglied 0=198.51.100.7:35500

Nur Standardbibliothek. Selbsttest: python -I bruecke.py --selbsttest
"""

from __future__ import annotations

import argparse
import json
import select
import socket
import sys
import threading
from pathlib import Path

SPIELPORT_VORGABE = 21414
STATUSDATEI = Path.home() / "AppData" / "Local" / "Swofty" / "swofty" / "netplay.json"


def spielport() -> int:
    """Der Spielport steht in swofty/netplay.json, sobald das Spiel seinen Socket gebunden hat."""
    kandidaten = [
        STATUSDATEI,
        Path("E:/SteamLibrary/steamapps/common/F1 25/swofty/netplay.json"),
    ]
    for pfad in kandidaten:
        try:
            daten = json.loads(pfad.read_text(encoding="utf-8"))
            if int(daten.get("game_port", 0)) > 0:
                return int(daten["game_port"])
        except (OSError, ValueError, json.JSONDecodeError):
            continue
    return SPIELPORT_VORGABE


def mitglieder_lesen(texte: list[str]) -> dict[int, tuple[str, int]]:
    ergebnis: dict[int, tuple[str, int]] = {}
    for text in texte:
        links, _, rechts = text.partition("=")
        host, _, port = rechts.rpartition(":")
        if not links.isdigit() or not host or not port.isdigit():
            raise ValueError(f"Mitglied unlesbar: {text} (erwartet 1=203.0.113.5:35500)")
        kennung = int(links)
        if not 1 <= kennung <= 254:
            raise ValueError(f"Mitgliedsnummer ausserhalb 1..254: {kennung}")
        ergebnis[kennung] = (host, int(port))
    return ergebnis


def betreiben(selbst: int, basis: int, port: int, mitglieder: dict[int, tuple[str, int]],
              ziel_spiel: int | None = None) -> None:
    ziel_spiel = ziel_spiel or spielport()

    # Ein Socket je Mitglied: darueber sprechen wir mit dem Spiel "als" dieses Mitglied,
    # und darauf hoeren wir, was das Spiel an dieses Mitglied schicken will.
    zum_spiel: dict[int, socket.socket] = {}
    for kennung in mitglieder:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.bind(("127.0.0.1", basis + kennung))
        s.setblocking(False)
        zum_spiel[kennung] = s

    nach_draussen = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    nach_draussen.bind(("0.0.0.0", port))
    nach_draussen.setblocking(False)

    print(f"Bruecke: selbst={selbst} basis={basis} port={port} spielport={ziel_spiel}")
    for kennung, adresse in mitglieder.items():
        print(f"  Mitglied {kennung} -> {adresse[0]}:{adresse[1]} (lokal auf 127.0.0.1:{basis + kennung})")

    gesehen = {"spiel_raus": 0, "spiel_rein": 0, "fremd_verworfen": 0}

    while True:
        lesen = [nach_draussen, *zum_spiel.values()]
        bereit, _, _ = select.select(lesen, [], [], 1.0)
        for sock in bereit:
            try:
                daten, quelle = sock.recvfrom(65535)
            except OSError:
                continue

            if sock is nach_draussen:
                # Von einem anderen Rechner: erstes Byte sagt, von wem es kommt.
                if len(daten) < 2:
                    continue
                absender, nutzlast = daten[0], daten[1:]
                ziel = zum_spiel.get(absender)
                if ziel is None:
                    gesehen["fremd_verworfen"] += 1
                    continue
                ziel.sendto(nutzlast, ("127.0.0.1", ziel_spiel))
                gesehen["spiel_rein"] += 1
                continue

            # Aus dem Spiel: nur Pakete seines eigenen Sockets weiterreichen.
            if quelle[0] != "127.0.0.1":
                continue
            kennung = next((k for k, v in zum_spiel.items() if v is sock), None)
            if kennung is None:
                continue
            adresse = mitglieder[kennung]
            nach_draussen.sendto(bytes([selbst]) + daten, adresse)
            gesehen["spiel_raus"] += 1


def selbsttest() -> None:
    """Zwei Bruecken ueber Loopback, mit nachgebauten Spiel-Sockets."""
    print("Selbsttest: zwei Bruecken, zwei Spielsockets")

    # Spiel A schickt an Mitglied 1; Spiel B schickt an Mitglied 0.
    spiel_a = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    spiel_a.bind(("127.0.0.1", 0))
    spiel_b = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    spiel_b.bind(("127.0.0.1", 0))

    a = threading.Thread(target=betreiben, args=(0, 40000, 45000, {1: ("127.0.0.1", 45001)}, spiel_a.getsockname()[1]),
                         daemon=True)
    b = threading.Thread(target=betreiben, args=(1, 41000, 45001, {0: ("127.0.0.1", 45000)}, spiel_b.getsockname()[1]),
                         daemon=True)
    a.start()
    b.start()

    import time

    time.sleep(0.4)

    # A -> Bruecke A (als Ziel Mitglied 1) -> Bruecke B -> Spiel B (als Absender 0)
    spiel_a.sendto(b"hallo", ("127.0.0.1", 40001))
    spiel_b.settimeout(3.0)
    try:
        daten, quelle = spiel_b.recvfrom(1024)
    except socket.timeout:
        raise AssertionError("Paket kam nicht an: A -> B")
    assert daten == b"hallo", f"Nutzlast veraendert: {daten!r}"
    assert quelle == ("127.0.0.1", 41000), f"falscher Absendersocket: {quelle}"

    # B -> Bruecke B (als Ziel Mitglied 0) -> Bruecke A -> Spiel A (als Absender 1)
    spiel_b.sendto(b"zurueck", ("127.0.0.1", 41000))
    spiel_a.settimeout(3.0)
    daten, quelle = spiel_a.recvfrom(1024)
    assert daten == b"zurueck", f"Nutzlast veraendert: {daten!r}"
    assert quelle == ("127.0.0.1", 40001), f"falscher Absendersocket: {quelle}"

    print("Selbsttest bestanden: beide Richtungen kommen an, Absender ist richtig")


def main() -> int:
    bote = argparse.ArgumentParser()
    bote.add_argument("--selbst", type=int, help="eigene Mitgliedsnummer, 1..254")
    bote.add_argument("--basis", type=int, default=30000, help="Basisport, wie in SWOFTY_NETPLAY")
    bote.add_argument("--port", type=int, default=35500, help="UDP-Port, auf dem die Bruecken reden")
    bote.add_argument("--mitglied", action="append", default=[], metavar="N=HOST:PORT")
    bote.add_argument("--selbsttest", action="store_true")
    args = bote.parse_args()

    if args.selbsttest:
        selbsttest()
        return 0
    if args.selbst is None or not args.mitglied:
        bote.print_help()
        return 2
    betreiben(args.selbst, args.basis, args.port, mitglieder_lesen(args.mitglied))
    return 0


if __name__ == "__main__":
    sys.exit(main())
