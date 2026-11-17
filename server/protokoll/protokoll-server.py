"""Nimmt Protokolle der Ligarechner entgegen und legt sie nach Tagen ab.

Nur Standardbibliothek. Aufruf: python3 protokoll-server.py

    POST /protokoll   Kopfzeile  X-ERC-Token: <Kennwort>
                      Rumpf       reiner Text, höchstens MAX_BYTES
                      Antwort     201 mit der Ablage, 401, 413 oder 400

    POST /lauf        wie /protokoll, hängt aber an dieselbe Datei an

    POST /logs/<kennwort>
                      Rumpf       multipart/form-data mit `payload_json` und
                                  `files[0]` (das Bündel des Launchers)
                      Antwort     201 mit der Ablage, 401, 413 oder 400

    GET  /            sagt, wie viele Ablagen es gibt — ohne Inhalte
"""

from __future__ import annotations

import hmac
import json
import os
import re
import time
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ABLAGE = Path(os.environ.get("ERC_ABLAGE", "/var/lib/protokoll"))
KENNWORT = os.environ.get("ERC_TOKEN", "")
LOGS_KENNWORT = os.environ.get("ERC_LOGS_TOKEN", "")
PORT = int(os.environ.get("ERC_PORT", "8080"))
MAX_BYTES = 2 * 1024 * 1024
MAX_BUNDLE = 96 * 1024 * 1024


def heute() -> Path:
    ordner = ABLAGE / datetime.now(timezone.utc).strftime("%Y-%m-%d")
    ordner.mkdir(parents=True, exist_ok=True)
    return ordner


def zielname(quelle: str, endung: str = ".log") -> Path:
    ordner = heute()
    sauber = re.sub(r"[^a-zA-Z0-9_.-]", "_", quelle)[:40] or "unbekannt"
    stempel = datetime.now(timezone.utc).strftime("%H%M%S")
    ziel = ordner / f"{stempel}-{sauber}{endung}"

    zaehler = 1
    while ziel.exists():
        ziel = ordner / f"{stempel}-{sauber}-{zaehler}{endung}"
        zaehler += 1
    return ziel


def zerlege_multipart(rumpf: bytes, grenze: str) -> dict[str, tuple[str, bytes]]:
    """Teile eines multipart/form-data-Rumpfs: Name -> (Dateiname, Daten)."""
    teile: dict[str, tuple[str, bytes]] = {}
    marke = b"--" + grenze.encode("ascii", "replace")
    for stueck in rumpf.split(marke):
        stueck = stueck.strip(b"\r\n")
        if not stueck or stueck == b"--":
            continue
        kopf, _, daten = stueck.partition(b"\r\n\r\n")
        if not _:
            continue
        text = kopf.decode("utf-8", "replace")
        treffer = re.search(r'name="([^"]*)"', text)
        if not treffer:
            continue
        name = treffer.group(1)
        datei = re.search(r'filename="([^"]*)"', text)
        teile[name] = (datei.group(1) if datei else "", daten)
    return teile


class Abnehmer(BaseHTTPRequestHandler):
    server_version = "ERC-Protokoll/2"

    def _antwort(self, code: int, text: str) -> None:
        daten = text.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(daten)))
        self.end_headers()
        self.wfile.write(daten)

    def do_GET(self) -> None:  # noqa: N802 — Vorgabe der Bibliothek
        if self.path != "/":
            self._antwort(404, "nicht da\n")
            return
        tage = sorted(p.name for p in ABLAGE.iterdir() if p.is_dir()) if ABLAGE.is_dir() else []
        self._antwort(200, f"ERC-Protokoll\nTage: {len(tage)}\n")

    def _kennwort_ok(self) -> bool:
        mitgegeben = self.headers.get("X-ERC-Token", "")
        return bool(KENNWORT) and hmac.compare_digest(mitgegeben, KENNWORT)

    def do_POST(self) -> None:  # noqa: N802
        if self.path.startswith("/logs/"):
            self._buendel()
            return
        if self.path == "/lauf":
            self._lauf()
            return
        if self.path != "/protokoll":
            self._antwort(404, "nicht da\n")
            return
        self._text()

    def _text(self) -> None:
        if not KENNWORT:
            self._antwort(500, "kein Kennwort gesetzt\n")
            return

        mitgegeben = self.headers.get("X-ERC-Token", "")
        if not hmac.compare_digest(mitgegeben, KENNWORT):
            self._antwort(401, "Kennwort stimmt nicht\n")
            return

        laenge = int(self.headers.get("Content-Length", "0") or 0)
        if laenge <= 0 or laenge > MAX_BYTES:
            self._antwort(413, "Rumpf fehlt oder ist zu groß\n")
            return

        rumpf = self.rfile.read(laenge)
        if not rumpf:
            self._antwort(400, "leer\n")
            return

        quelle = self.headers.get("X-ERC-Rechner", "unbekannt")
        ziel = zielname(quelle)

        kopf = (
            f"# empfangen {datetime.now(timezone.utc).isoformat()}\n"
            f"# von {quelle}\n"
            f"# {laenge} Bytes\n\n"
        )
        ziel.write_bytes(kopf.encode("utf-8") + rumpf)
        self._antwort(201, f"abgelegt: {ziel.relative_to(ABLAGE)}\n")

    def _lauf(self) -> None:
        """Laufender Testbericht: hängt an, statt eine neue Datei anzulegen."""
        if not KENNWORT:
            self._antwort(500, "kein Kennwort gesetzt\n")
            return
        if not self._kennwort_ok():
            self._antwort(401, "Kennwort stimmt nicht\n")
            return

        laenge = int(self.headers.get("Content-Length", "0") or 0)
        if laenge <= 0 or laenge > MAX_BYTES:
            self._antwort(413, "Rumpf fehlt oder ist zu groß\n")
            return

        rumpf = self.rfile.read(laenge)
        kennung = re.sub(r"[^a-zA-Z0-9_.-]", "_", self.headers.get("X-ERC-Kennung", "unbekannt"))[:40]
        heute_ordner = heute()
        ziel = heute_ordner / f"lauf-{kennung or 'unbekannt'}.log"
        with ziel.open("ab") as datei:
            datei.write(rumpf)
        self._antwort(201, f"angehängt: {ziel.relative_to(ABLAGE)}\n")

    def _buendel(self) -> None:
        """Das Bündel des Launchers: multipart mit payload_json und files[0]."""
        if not LOGS_KENNWORT:
            self._antwort(500, "kein Kennwort für Bündel gesetzt\n")
            return
        mitgegeben = self.path[len("/logs/"):].strip("/")
        if not hmac.compare_digest(mitgegeben, LOGS_KENNWORT):
            self._antwort(401, "Kennwort stimmt nicht\n")
            return

        laenge = int(self.headers.get("Content-Length", "0") or 0)
        if laenge <= 0 or laenge > MAX_BUNDLE:
            self._antwort(413, "Rumpf fehlt oder ist zu groß\n")
            return

        art = self.headers.get("Content-Type", "")
        treffer = re.search(r"boundary=([^;]+)", art)
        if "multipart/form-data" not in art or not treffer:
            self._antwort(400, "kein multipart\n")
            return

        rumpf = self.rfile.read(laenge)
        teile = zerlege_multipart(rumpf, treffer.group(1).strip('"'))

        inhalt = ""
        if "payload_json" in teile:
            try:
                inhalt = str(json.loads(teile["payload_json"][1].decode("utf-8", "replace")).get("content", ""))
            except (ValueError, AttributeError):
                inhalt = ""

        datei = teile.get("files[0]")
        if datei is None or not datei[1]:
            self._antwort(400, "kein Bündel dabei\n")
            return

        name = re.sub(r"[^a-zA-Z0-9_.-]", "_", datei[0] or "buendel.zip")
        ziel = zielname(name.rsplit(".", 1)[0], ".zip")
        ziel.write_bytes(datei[1])

        bericht = ziel.with_suffix(".txt")
        bericht.write_text(
            f"# empfangen {datetime.now(timezone.utc).isoformat()}\n"
            f"# von {self.headers.get('User-Agent', 'unbekannt')}\n"
            f"# {len(datei[1])} Bytes\n\n{inhalt}\n",
            encoding="utf-8",
        )
        self._antwort(201, f"abgelegt: {ziel.relative_to(ABLAGE)}\n")

    def log_message(self, muster: str, *args) -> None:
        # Kein Zugriffsprotokoll auf der Platte — nur auf der Konsole.
        print(f"[{time.strftime('%H:%M:%S')}] {muster % args}", flush=True)


def main() -> None:
    ABLAGE.mkdir(parents=True, exist_ok=True)
    print(f"ERC-Protokoll auf Port {PORT}, Ablage {ABLAGE}", flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Abnehmer).serve_forever()


if __name__ == "__main__":
    main()
