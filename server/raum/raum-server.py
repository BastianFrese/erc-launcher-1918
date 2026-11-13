#!/usr/bin/env python3
"""ERC-Raum — der Lobby-Server des ERC Launchers.

Nur Standardbibliothek. Aufruf: python3 raum-server.py

Er haelt die Lobbys: wer drin ist, welche Mods er faehrt und ob das Spiel in
der Lobby steht. Die Spielpakete selbst gehen nicht hier durch, sondern ueber
den Relay (f1.erdi-erc.de:35501) — dieser Dienst vergibt nur Raumcode,
Raumschluessel und Mitgliedsnummern.

    GET  /v1/auth/launcher/me          wer bin ich
    GET  /v1/support/goal              Spendenstand (aus support.json)
    GET  /v1/support/supporters        die Unterstuetzer (aus support.json)
    GET  /v1/mp/rooms                  offene Lobbys
    POST /v1/mp/lobbies                Lobby aufmachen
    POST /v1/mp/lobbies/<code>/join    beitreten
    POST /v1/mp/lobbies/<code>/leave   verlassen
    GET  /v1/mp/lobbies/<code>         Stand der Lobby
    GET  /v1/mp/lobbies/<code>/ws      WebSocket: Stand und Meldungen

Keine Konten: jeder Bearer-Token gilt. Die Kennung ist der Hash des Tokens,
der Name kommt aus der Kopfzeile `X-ERC-Name` (prozent-kodiert). Der
Raumcode ist vier Zeichen — genau das, was der Relay als Raum versteht.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import re
import secrets
import struct
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote

PORT = int(os.environ.get("ERC_RAUM_PORT", "8091"))
STILLE_SEK = 600  # so lange bleibt eine Lobby ohne Verbindung stehen
HOST_FRIST = 120  # so lange darf der Host ohne Verbindung weg sein, dann schliesst der Raum
MAX_NACHRICHT = 4 * 1024 * 1024
MAX_MITGLIEDER = 254

# Der Code muss vier Zeichen haben: der Relay kennt keinen laengeren Raum.
CODE_ZEICHEN = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ"

WS_KENNUNG = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

SPERR = threading.RLock()
RAEUME: dict[str, "Raum"] = {}


def kennung_von(token: str) -> str:
    """Die feste Kennung eines Tokens: niemand meldet sich an, jeder Token gilt."""
    return hashlib.sha256(token.encode("utf-8")).hexdigest()[:12]


def schluessel() -> bytes:
    """Der Raumschluessel: 32 Bytes, die der Launcher zum Versiegeln nimmt."""
    return secrets.token_bytes(32)


def schluessel_b64(roh: bytes) -> str:
    return base64.urlsafe_b64encode(roh).decode("ascii").rstrip("=")


def support_stand() -> dict:
    """Der Spendenstand: `support.json` neben diesem Dienst, von Hand gepflegt.

    Kein Schreibweg. Wer etwas aendern will, aendert die Datei; der Dienst liest
    sie bei jeder Anfrage neu, ein Neustart ist dafuer nicht noetig.
    """
    datei = os.path.join(os.path.dirname(os.path.abspath(__file__)), "support.json")
    try:
        with open(datei, encoding="utf-8") as f:
            stand = json.load(f)
    except (OSError, ValueError):
        return {}
    return stand if isinstance(stand, dict) else {}


def support_schreiben(stand: dict) -> None:
    """Schreibt den Stand zurueck — erst daneben, dann umbenannt, damit nie halb."""
    datei = os.path.join(os.path.dirname(os.path.abspath(__file__)), "support.json")
    with open(datei + ".neu", "w", encoding="utf-8") as f:
        json.dump(stand, f, ensure_ascii=False, indent=2)
    os.replace(datei + ".neu", datei)


def kofi_einstellungen() -> dict:
    """Verification Token und Stufen-Zuordnung: `kofi.json` neben diesem Dienst."""
    datei = os.path.join(os.path.dirname(os.path.abspath(__file__)), "kofi.json")
    try:
        with open(datei, encoding="utf-8") as f:
            daten = json.load(f)
    except (OSError, ValueError):
        return {}
    return daten if isinstance(daten, dict) else {}


class Mitglied:
    def __init__(self, kennung: str, name: str, nummer: int):
        self.kennung = kennung
        self.name = name
        self.nummer = nummer
        self.bereit = False
        self.verbunden = False
        self.abdruck: str | None = None
        self.blob = None
        self.relay: str | None = None
        self.sockel: "Sockel | None" = None
        self.fort_seit: float | None = None  # wann die Verbindung abriss

    def wie(self) -> dict:
        return {
            "id": self.kennung,
            "name": self.name,
            "avatar": None,
            "ip": f"10.77.0.{self.nummer}",
            "host": False,
            "ready": self.bereit,
            "connected": self.verbunden,
            "digest": self.abdruck,
            "matches": False,
            "relay": self.relay,
        }


class Raum:
    def __init__(self, code: str, name: str, oeffentlich: bool, art: str, groesse: int, host: Mitglied):
        self.code = code
        self.name = name
        self.oeffentlich = oeffentlich
        self.art = art
        self.groesse = min(max(groesse, 2), MAX_MITGLIEDER)
        self.seit = int(time.time())
        self.host = host.kennung
        self.gestartet = False
        self.rev = 1
        self.schluessel = schluessel()
        self.naechste_nummer = 2
        self.mitglieder: dict[str, Mitglied] = {host.kennung: host}
        self.zuletzt = time.time()

    # -- was der Launcher sieht ------------------------------------------------

    def wie(self) -> dict:
        host_abdruck = self.mitglieder[self.host].abdruck if self.host in self.mitglieder else None
        mitglieder = []
        for m in self.mitglieder.values():
            eintrag = m.wie()
            eintrag["host"] = m.kennung == self.host
            eintrag["matches"] = m.abdruck is not None and m.abdruck == host_abdruck
            mitglieder.append(eintrag)
        return {
            "code": self.code,
            "host": self.host,
            "state": "started" if self.gestartet else "open",
            "rev": self.rev,
            "name": self.name,
            "public": self.oeffentlich,
            "category": self.art,
            "created_at": self.seit,
            "max_members": self.groesse,
            "members": mitglieder,
            "key": schluessel_b64(self.schluessel),
        }

    def kurz(self) -> dict:
        host = self.mitglieder.get(self.host)
        return {
            "code": self.code,
            "name": self.name,
            "host": {
                "id": self.host,
                "name": host.name if host else "?",
                "avatar": None,
            },
            "members": len(self.mitglieder),
            "max_members": self.groesse,
            "category": self.art,
            "digest": host.abdruck if host else None,
            "addons": None,
            "created_at": self.seit,
        }

    def anzeige(self, wer: str | None = None) -> dict:
        """Der Stand, wie ihn ein Mitglied bekommt: mit Schluessel."""
        stand = self.wie()
        if wer is None:
            stand.pop("key", None)
        return stand

    # ponytail: ein Schreibvorgang je Mitglied unter der globalen Sperre; bei einem
    # haengenden Teilnehmer warten alle. Fuer eine Liga-Lobby reicht das.
    def senden(self, text: str) -> None:
        for m in list(self.mitglieder.values()):
            if m.sockel is not None:
                m.sockel.sende_text(text)

    def uebertragen(self) -> None:
        """Rev hoch und allen den neuen Stand schicken."""
        self.rev += 1
        self.zuletzt = time.time()
        self.senden(json.dumps({"type": "lobby", "lobby": self.wie()}))

    def beenden(self, grund: str) -> None:
        for m in list(self.mitglieder.values()):
            if m.sockel is not None:
                m.sockel.sende_text(json.dumps({"type": "closed", "reason": grund}))
                m.sockel.schliessen(4002)


class Sockel:
    """Eine offene WebSocket-Verbindung. Schreiben von mehreren Faeden aus."""

    def __init__(self, wfile):
        self.wfile = wfile
        self.sperre = threading.Lock()
        self.zu = False

    def sende_text(self, text: str) -> None:
        self.sende(0x1, text.encode("utf-8"))

    def sende(self, opcode: int, daten: bytes) -> None:
        with self.sperre:
            if self.zu:
                return
            try:
                self.wfile.write(rahmen(opcode, daten))
                self.wfile.flush()
            except OSError:
                self.zu = True

    def schliessen(self, code: int = 1000) -> None:
        self.sende(0x8, struct.pack(">H", code))


def rahmen(opcode: int, daten: bytes) -> bytes:
    kopf = bytearray([0x80 | opcode])
    n = len(daten)
    if n < 126:
        kopf.append(n)
    elif n < 65536:
        kopf.append(126)
        kopf += struct.pack(">H", n)
    else:
        kopf.append(127)
        kopf += struct.pack(">Q", n)
    return bytes(kopf) + daten


def lese_frame(rfile):
    """(opcode, daten) oder (None, b"") wenn die Gegenstelle zugemacht hat.

    Geteilte Nachrichten (FIN=0) werden eingesammelt, bis das Ende kommt.
    """
    ganz = bytearray()
    erster = None
    while True:
        opcode, daten, ende = _lese_stueck(rfile)
        if opcode is None:
            return None, b""
        if opcode >= 0x8:  # Steuerrahmen gehen nie durch die Teilung
            return opcode, daten
        if erster is None:
            erster = opcode
        ganz += daten
        if ende:
            return erster, bytes(ganz)


def _lese_stueck(rfile):
    kopf = rfile.read(2)
    if len(kopf) < 2:
        return None, b"", True
    ende = bool(kopf[0] & 0x80)
    opcode = kopf[0] & 0x0F
    maskiert = bool(kopf[1] & 0x80)
    laenge = kopf[1] & 0x7F
    if laenge == 126:
        laenge = struct.unpack(">H", rfile.read(2))[0]
    elif laenge == 127:
        laenge = struct.unpack(">Q", rfile.read(8))[0]
    if laenge > MAX_NACHRICHT:
        raise ValueError("Nachricht zu gross")
    maske = rfile.read(4) if maskiert else b""
    daten = bytearray()
    while len(daten) < laenge:
        stueck = rfile.read(laenge - len(daten))
        if not stueck:
            return None, b"", True
        daten += stueck
    if maskiert:
        for i in range(len(daten)):
            daten[i] ^= maske[i % 4]
    return opcode, bytes(daten), ende


def neuer_code() -> str:
    while True:
        code = "".join(secrets.choice(CODE_ZEICHEN) for _ in range(4))
        if code not in RAEUME:
            return code


def raum_von(code: str) -> "Raum | None":
    return RAEUME.get((code or "").strip().upper())


def neuer_name(kennung: str) -> str:
    return f"Fahrer {kennung[:4]}"


class Abnehmer(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "ERC-Raum/1.0"

    # -- Hilfen ---------------------------------------------------------------

    def log_message(self, fmt, *args):  # noqa: A003 - Vorgabe der Basisklasse
        print(f"[{time.strftime('%H:%M:%S')}] {self.address_string()} {fmt % args}", flush=True)

    def token(self) -> str | None:
        kopf = self.headers.get("Authorization", "")
        if not kopf.startswith("Bearer "):
            return None
        return kopf[len("Bearer "):].strip() or None

    def name(self) -> str:
        roh = self.headers.get("X-ERC-Name", "")
        name = unquote(roh).strip()[:32]
        return name or neuer_name(kennung_von(self.token() or ""))

    def antwort(self, status: int, inhalt) -> None:
        text = "" if status == 204 else json.dumps(inhalt)
        roh = text.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(roh)))
        self.send_header("Connection", "close")
        self.end_headers()
        if roh:
            self.wfile.write(roh)
            self.wfile.flush()
        self.close_connection = True

    def fehler(self, status: int, grund: str, text: str, **mehr) -> None:
        antwort = {"error": text, "reason": grund}
        antwort.update(mehr)
        self.antwort(status, antwort)

    def rumpf(self) -> dict:
        laenge = int(self.headers.get("Content-Length") or 0)
        if laenge <= 0:
            return {}
        try:
            return json.loads(self.rfile.read(laenge) or b"{}")
        except (ValueError, OSError):
            return {}

    # -- Wege -----------------------------------------------------------------

    def kofi_spende(self) -> None:
        """Eine Meldung von Ko-fi: Spende, Mitgliedschaft, Shop-Bestellung.

        Ko-fi signiert nicht; es legt denselben `verification_token` in jedes
        Paket. Der steht in `kofi.json` und wird hier verglichen. Doppelte
        Meldungen erkennt `message_id`.
        """
        roh = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        felder = parse_qs(roh.decode("utf-8", "replace"))
        try:
            daten = json.loads((felder.get("data") or [""])[0])
        except ValueError:
            return self.fehler(400, "bad_payload", "data is not JSON")
        if not isinstance(daten, dict):
            return self.fehler(400, "bad_payload", "data is not an object")

        einstellung = kofi_einstellungen()
        geheim = str(einstellung.get("verification_token") or "")
        if not geheim or not secrets.compare_digest(
            str(daten.get("verification_token") or ""), geheim
        ):
            return self.fehler(403, "bad_token", "the verification token does not match")

        kennung = str(daten.get("message_id") or daten.get("kofi_transaction_id") or "")
        try:
            betrag = float(str(daten.get("amount") or "0").replace(",", "."))
        except ValueError:
            betrag = 0.0
        waehrung = str(daten.get("currency") or "").upper()
        stufe = str(daten.get("tier_name") or "")
        zuordnung = einstellung.get("tiers") or {}
        tier = str(
            zuordnung.get(stufe)
            or ("supporter_plus" if any(w in stufe.lower() for w in ("plus", "+")) else "supporter")
        )
        oeffentlich = daten.get("is_public") is True
        wer = str(daten.get("email") or daten.get("from_name") or kennung)
        person = hashlib.sha256(wer.encode("utf-8")).hexdigest()[:12]

        with SPERR:
            stand = support_stand()
            gesehen = [str(x) for x in (stand.get("kofi_seen") or [])]
            if kennung and kennung in gesehen:
                return self.antwort(200, {"ok": True, "duplicate": True})
            eintrag = {
                "_id": person,
                "_since": str(daten.get("timestamp") or "")[:10],
                "tier": tier,
                "bio": (str(daten.get("message") or "")[:140] or None),
                "display": {
                    "anonymous": not oeffentlich,
                    "name": (str(daten.get("from_name") or "") if oeffentlich else None) or None,
                    "avatar_url": None,
                },
            }
            liste = [s for s in (stand.get("supporters") or []) if isinstance(s, dict)]
            for i, alt in enumerate(liste):
                if alt.get("_id") == person:
                    # Eine Verlaengerung ist kein neuer Mensch: erster Tag und
                    # seine Nachricht bleiben stehen.
                    eintrag["_since"] = alt.get("_since") or eintrag["_since"]
                    eintrag["bio"] = eintrag["bio"] or alt.get("bio")
                    liste[i] = eintrag
                    break
            else:
                liste.insert(0, eintrag)
            stand["supporters"] = liste
            if waehrung == "EUR":
                stand["raised_cents"] = int(stand.get("raised_cents") or 0) + int(round(betrag * 100))
            if kennung:
                gesehen.append(kennung)
                stand["kofi_seen"] = gesehen[-500:]
            support_schreiben(stand)
        return self.antwort(200, {"ok": True})

    def do_GET(self) -> None:  # noqa: N802 - Vorgabe der Basisklasse
        pfad = self.path.split("?", 1)[0].rstrip("/") or "/"
        # Der Spendenstand ist oeffentlich: kein Token, keine Kennung.
        if pfad == "/v1/support/goal":
            stand = support_stand()
            return self.antwort(
                200,
                {
                    "goal_cents": int(stand.get("goal_cents") or 0),
                    "raised_cents": int(stand.get("raised_cents") or 0),
                    "since": str(stand.get("since") or ""),
                    "costs": list(stand.get("costs") or []),
                    "donate_url": str(stand.get("donate_url") or ""),
                },
            )
        if pfad == "/v1/support/supporters":
            # Felder mit Unterstrich sind Buchhaltung und gehen niemanden etwas an.
            liste = [
                {k: v for k, v in s.items() if not k.startswith("_")}
                for s in (support_stand().get("supporters") or [])
                if isinstance(s, dict)
            ]
            return self.antwort(200, {"supporters": liste})
        if not self.token():
            return self.fehler(401, "signed_out", "sign in")
        if pfad == "/":
            with SPERR:
                offen = [r.kurz() for r in RAEUME.values() if r.oeffentlich and not r.gestartet]
            return self.antwort(200, {"rooms": offen, "count": len(offen)})
        if pfad == "/v1/auth/launcher/me":
            kennung = kennung_von(self.token())
            return self.antwort(
                200,
                {
                    "user": {"id": kennung, "name": self.name(), "avatar": None},
                    "multiplayer_allowed": True,
                },
            )
        if pfad == "/v1/mp/rooms":
            with SPERR:
                offen = [r.kurz() for r in RAEUME.values() if r.oeffentlich and not r.gestartet]
            return self.antwort(200, {"rooms": offen})
        teile = pfad.strip("/").split("/")
        if teile[:3] == ["v1", "mp", "lobbies"] and len(teile) == 5 and teile[4] == "ws":
            return self.websocket(teile[3])
        if teile[:3] == ["v1", "mp", "lobbies"] and len(teile) == 4:
            mit = kennung_von(self.token())
            with SPERR:
                raum = raum_von(teile[3])
                if raum is None:
                    return self.fehler(404, "not_found", "no open lobby has that code")
                if mit not in raum.mitglieder:
                    return self.fehler(404, "not_found", "no open lobby has that code")
                return self.antwort(200, {"lobby": raum.wie()})
        return self.fehler(404, "not_found", "no route")

    def do_POST(self) -> None:  # noqa: N802 - Vorgabe der Basisklasse
        pfad = self.path.split("?", 1)[0].rstrip("/") or "/"
        # Ko-fi meldet Zahlungen hierher; kein Bearer-Token, dafuer der eigene.
        if pfad == "/v1/support/kofi":
            return self.kofi_spende()
        if not self.token():
            return self.fehler(401, "signed_out", "sign in")
        mit = kennung_von(self.token())
        name = self.name()
        if pfad == "/v1/mp/lobbies":
            return self.aufmachen(mit, name)
        teile = pfad.strip("/").split("/")
        if teile[:3] == ["v1", "mp", "lobbies"] and len(teile) == 5:
            code, was = teile[3], teile[4]
            if was == "join":
                return self.beitreten(code, mit, name)
            if was == "leave":
                return self.verlassen(code, mit)
        return self.fehler(404, "not_found", "no route")

    def aufmachen(self, mit: str, name: str) -> None:
        einstellungen = self.rumpf()
        with SPERR:
            for raum in RAEUME.values():
                if mit in raum.mitglieder:
                    return self.fehler(
                        409,
                        "in_lobby",
                        f"you are already in lobby {raum.code}; leave it first",
                        lobby=raum.code,
                    )
            code = neuer_code()
            raumname = str(einstellungen.get("name") or f"{name}'s room")[:48]
            host = Mitglied(mit, name, 1)
            raum = Raum(
                code,
                raumname,
                bool(einstellungen.get("public", True)),
                str(einstellungen.get("category") or "f1-2026")[:24],
                int(einstellungen.get("max_members") or 20),
                host,
            )
            RAEUME[code] = raum
            print(f"[{time.strftime('%H:%M:%S')}] Raum {code} aufgemacht von {name}", flush=True)
            return self.antwort(201, {"lobby": raum.wie()})

    def beitreten(self, code: str, mit: str, name: str) -> None:
        with SPERR:
            raum = raum_von(code)
            if raum is None:
                return self.fehler(404, "not_found", "no open lobby has that code")
            if mit in raum.mitglieder:
                return self.antwort(200, {"lobby": raum.wie()})
            if raum.gestartet:
                return self.fehler(409, "started", "this room is in a race; join after it ends")
            if len(raum.mitglieder) >= raum.groesse:
                return self.fehler(409, "full", "this room is full")
            andere = [r for r in RAEUME.values() if r is not raum and mit in r.mitglieder]
            for alt in andere:
                return self.fehler(
                    409,
                    "in_lobby",
                    f"you are already in lobby {alt.code}; leave it first",
                    lobby=alt.code,
                )
            nummer = raum.naechste_nummer
            raum.naechste_nummer += 1
            raum.mitglieder[mit] = Mitglied(mit, name, nummer)
            print(
                f"[{time.strftime('%H:%M:%S')}] Raum {raum.code}: {name} ist beigetreten (Nummer {nummer})",
                flush=True,
            )
            raum.uebertragen()
            return self.antwort(200, {"lobby": raum.wie()})

    def verlassen(self, code: str, mit: str) -> None:
        with SPERR:
            raum = raum_von(code)
            if raum is None or mit not in raum.mitglieder:
                return self.antwort(204, None)
            mitglied = raum.mitglieder.pop(mit)
            print(f"[{time.strftime('%H:%M:%S')}] Raum {raum.code}: {mitglied.name} ist gegangen", flush=True)
            if mit == raum.host:
                RAEUME.pop(raum.code, None)
                raum.beenden("host_left")
            else:
                if mitglied.sockel is not None:
                    mitglied.sockel.sende_text(json.dumps({"type": "left"}))
                    mitglied.sockel.schliessen(4003)
                raum.uebertragen()
            return self.antwort(204, None)

    # -- WebSocket ------------------------------------------------------------

    def websocket(self, code: str) -> None:
        mit = kennung_von(self.token())
        with SPERR:
            raum = raum_von(code)
            if raum is None or mit not in raum.mitglieder:
                return self.fehler(404, "not_found", "no open lobby has that code")
            schluessel_kopf = self.headers.get("Sec-WebSocket-Key")
        if not schluessel_kopf:
            return self.fehler(400, "bad_ws", "not a websocket handshake")
        accept = base64.b64encode(hashlib.sha1(schluessel_kopf.encode() + WS_KENNUNG).digest()).decode()
        self.send_response(101, "Switching Protocols")
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.wfile.flush()
        self.close_connection = True

        sockel = Sockel(self.wfile)
        with SPERR:
            raum = raum_von(code)
            if raum is None or mit not in raum.mitglieder:
                sockel.schliessen(4002)  # Raum weg: der Launcher soll nicht endlos neu verbinden
                return
            alt = raum.mitglieder[mit].sockel
            if alt is not None:
                # 4000 heisst beim Launcher "in einem anderen Fenster geoeffnet" — genau der Fall.
                alt.schliessen(4000)
            raum.mitglieder[mit].sockel = sockel
            raum.mitglieder[mit].verbunden = True
            raum.mitglieder[mit].fort_seit = None
            raum.uebertragen()
            for andere in raum.mitglieder.values():
                if andere.kennung == mit or andere.blob is None:
                    continue
                sockel.sende_text(
                    json.dumps(
                        {
                            "type": "fingerprint",
                            "member": andere.kennung,
                            "digest": andere.abdruck,
                            "blob": andere.blob,
                        }
                    )
                )

        # Kein Zeitlimit auf dem Socket: ein Lesen mit Zeitlimit hinterlaesst den
        # gepufferten Leser in unbrauchbarem Zustand, und die Verbindung riss alle 90
        # Sekunden ab. Der Client haelt die Verbindung selbst offen; geht er, liefert
        # read() b"".
        self.connection.settimeout(None)
        try:
            while True:
                try:
                    opcode, daten = lese_frame(self.rfile)
                except (OSError, ValueError):
                    break
                if opcode is None or opcode == 0x8:
                    break
                if opcode == 0x9:
                    sockel.sende(0xA, daten)
                    continue
                if opcode != 0x1:
                    continue
                try:
                    nachricht = json.loads(daten or b"{}")
                except ValueError:
                    continue
                self.behandeln(mit, nachricht)
        finally:
            with SPERR:
                raum = raum_von(code)
                if raum is not None:
                    aktuell = raum.mitglieder.get(mit)
                    if aktuell is not None and aktuell.sockel is sockel:
                        aktuell.sockel = None
                        aktuell.verbunden = False
                        aktuell.fort_seit = time.time()
                        raum.uebertragen()

    def behandeln(self, mit: str, nachricht: dict) -> None:
        art = nachricht.get("type")
        with SPERR:
            raum = None
            for r in RAEUME.values():
                if mit in r.mitglieder:
                    raum = r
                    break
            if raum is None:
                return
            wer = raum.mitglieder[mit]
            if art == "fingerprint":
                wer.abdruck = nachricht.get("digest")
                wer.blob = nachricht.get("blob")
                weiter = json.dumps(
                    {
                        "type": "fingerprint",
                        "member": mit,
                        "digest": wer.abdruck,
                        "blob": wer.blob,
                    }
                )
                for m in raum.mitglieder.values():
                    if m.kennung != mit and m.sockel is not None:
                        m.sockel.sende_text(weiter)
                raum.uebertragen()
            elif art == "relay":
                wer.relay = nachricht.get("relay")
                raum.uebertragen()
            elif art == "ready":
                wer.bereit = bool(nachricht.get("ready"))
                raum.uebertragen()
            elif art == "start" and mit == raum.host and not raum.gestartet:
                raum.gestartet = True
                raum.uebertragen()
                raum.senden(json.dumps({"type": "start", "key": schluessel_b64(raum.schluessel)}))
            elif art == "reopen" and mit == raum.host and raum.gestartet:
                raum.gestartet = False
                for m in raum.mitglieder.values():
                    m.bereit = False
                raum.uebertragen()
            elif art == "ping":
                if wer.sockel is not None:
                    wer.sockel.sende_text(json.dumps({"type": "pong"}))


def wachhalten() -> None:
    """Haelt jede Lobby-Verbindung offen: ohne Verkehr schliesst Cloudflare sie nach
    ein paar Minuten, und der Launcher zeigt derweil "getrennt"."""
    while True:
        time.sleep(20)
        with SPERR:
            for raum in RAEUME.values():
                for m in raum.mitglieder.values():
                    if m.sockel is not None:
                        m.sockel.sende(0x9, b"")


def aufraeumen() -> None:
    """Lobbys ohne Verbindung und ohne Leben entfernen."""
    while True:
        time.sleep(60)
        jetzt = time.time()
        with SPERR:
            for code, raum in list(RAEUME.items()):
                host = raum.mitglieder.get(raum.host)
                if host is not None and host.sockel is None and host.fort_seit is not None:
                    if jetzt - host.fort_seit > HOST_FRIST:
                        RAEUME.pop(code, None)
                        print(
                            f"[{time.strftime('%H:%M:%S')}] Raum {code}: der Host ist weg, Raum geschlossen",
                            flush=True,
                        )
                        raum.beenden("host_gone")
                        continue
                beschaeftigt = any(m.sockel is not None for m in raum.mitglieder.values())
                if not beschaeftigt and jetzt - raum.zuletzt > STILLE_SEK:
                    RAEUME.pop(code, None)
                    print(f"[{time.strftime('%H:%M:%S')}] Raum {code} ist eingeschlafen", flush=True)


def main() -> None:
    ThreadingHTTPServer.request_queue_size = 64
    server = ThreadingHTTPServer(("0.0.0.0", PORT), Abnehmer)
    server.daemon_threads = True
    threading.Thread(target=aufraeumen, daemon=True).start()
    threading.Thread(target=wachhalten, daemon=True).start()
    print(f"ERC-Raum auf TCP {PORT}", flush=True)
    server.serve_forever()


# -- Selbsttest: python3 -I raum-server.py --pruefen ---------------------------


def pruefen() -> None:
    # Ein Rahmen hin und zurueck, wie ihn der Launcher schickt (maskiert).
    roh = rahmen(0x1, b'hallo')
    assert roh[0] == 0x81 and roh[1] == 5, roh
    import io

    class R(io.BytesIO):
        pass

    nutz = b"hallo"
    maske = b"\x01\x02\x03\x04"
    kopf = bytes([0x81, 0x80 | len(nutz)]) + maske
    verdeckt = bytes(b ^ maske[i % 4] for i, b in enumerate(nutz))
    opcode, daten = lese_frame(R(kopf + verdeckt))
    assert opcode == 0x1 and daten == nutz, (opcode, daten)

    # Ein laengerer Rahmen (16-Bit-Laenge) kommt auch heil an.
    lang = b"x" * 300
    kopf = bytes([0x81, 0x80 | 126]) + struct.pack(">H", len(lang)) + maske
    verdeckt = bytes(b ^ maske[i % 4] for i, b in enumerate(lang))
    opcode, daten = lese_frame(R(kopf + verdeckt))
    assert daten == lang

    # Der Raumcode ist vier Zeichen aus dem Alphabet, der Schluessel 32 Bytes.
    code = neuer_code()
    assert len(code) == 4 and all(z in CODE_ZEICHEN for z in code)
    assert len(base64.urlsafe_b64decode(schluessel_b64(schluessel()) + "==")) == 32

    # Ein Raum vergibt Nummern, kennt den Host und gibt den Schluessel heraus.
    host = Mitglied(kennung_von("a"), "A", 1)
    raum = Raum("AB12", "Test", True, "f1-2026", 8, host)
    raum.mitglieder["b"] = Mitglied(kennung_von("b"), "B", 2)
    stand = raum.wie()
    assert stand["host"] == kennung_von("a")
    assert [m["ip"] for m in stand["members"]] == ["10.77.0.1", "10.77.0.2"]
    assert stand["members"][0]["host"] and not stand["members"][1]["host"]
    assert len(stand["key"]) == 43
    assert raum.kurz()["members"] == 2
    print("Selbsttest in Ordnung")


if __name__ == "__main__":
    import sys

    if "--pruefen" in sys.argv:
        pruefen()
    else:
        main()
