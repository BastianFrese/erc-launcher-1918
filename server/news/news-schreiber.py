#!/usr/bin/env python3
"""Redaktion — schreibt Neuigkeiten in die Datei, die der Launcher liest.

Eine Seite mit vier Feldern (Titel, Kurzfassung, Text, Bild). Beim Speichern
landet der Beitrag in `/var/www/downloads/news.json`, von wo ihn der Launcher
holt (er fragt die Datei regelmäßig und zeigt neue Beiträge als Neuigkeit).

Nur im Heimnetz: der Dienst hört auf der LAN-Adresse und nimmt Anfragen nur aus
192.168.100.0/24, dem Tailscale-Netz (100.64.0.0/10) und von localhost an.

    python3 news-schreiber.py            # Vorgaben: 0.0.0.0:8096, /var/www/downloads
    NEWS_DIR=/tmp/probe PORT=8097 python3 news-schreiber.py

Felder je Beitrag (so liest sie der Launcher):

    id       "news/<datum>-<slug>"
    kind     "news"   (Versionsnotizen behalten "release" und werden hier nicht angefasst)
    title    Überschrift
    date     JJJJ-MM-TT
    summary  eine Zeile für die Liste
    body     Markdown (**fett**, `Code`, - Liste, Absätze)
    image    Adresse des Bildes, sonst ganz weggelassen
"""

from __future__ import annotations

import html
import ipaddress
import json
import os
import re
import shutil
import sys
import time
from email import policy
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, quote, urlparse

BIND = os.environ.get("BIND", "0.0.0.0")
PORT = int(os.environ.get("PORT", "8096"))
NEWS_DIR = Path(os.environ.get("NEWS_DIR", "/var/www/downloads"))
FEED = NEWS_DIR / "news.json"
BILDER = NEWS_DIR / "news"
OEFFENTLICH = "https://download.erdi-erc.de"

ERLAUBTE_NETZE = [
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("192.168.100.0/24"),
    ipaddress.ip_network("100.64.0.0/10"),  # Tailscale
]

MAX_FORMULAR = 12 * 1024 * 1024
MAX_BILD = 8 * 1024 * 1024
BILDARTEN = {
    b"\x89PNG\r\n\x1a\n": ".png",
    b"\xff\xd8\xff": ".jpg",
    b"GIF87a": ".gif",
    b"GIF89a": ".gif",
}
BACKUPS = 10


# --------------------------------------------------------------------------- Daten
def feed_lesen() -> dict:
    try:
        return json.loads(FEED.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return {"posts": []}


def feed_schreiben(daten: dict) -> None:
    """Erst daneben schreiben, dann umbenennen — nie eine halbe Datei im Netz."""
    NEWS_DIR.mkdir(parents=True, exist_ok=True)
    if FEED.exists():
        stempel = time.strftime("%Y%m%d-%H%M%S")
        shutil.copy2(FEED, FEED.with_name(f"news.json.bak-{stempel}"))
        alte = sorted(NEWS_DIR.glob("news.json.bak-*"))
        for datei in alte[:-BACKUPS]:
            datei.unlink(missing_ok=True)
    zwischen = FEED.with_name("news.json.neu")
    zwischen.write_text(json.dumps(daten, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(zwischen, FEED)


def slug(text: str) -> str:
    text = text.lower()
    text = text.replace("ä", "ae").replace("ö", "oe").replace("ü", "ue").replace("ß", "ss")
    text = re.sub(r"[^a-z0-9]+", "-", text).strip("-")
    return text[:60] or "beitrag"


def eigene(daten: dict) -> list[dict]:
    beitraege = [p for p in daten.get("posts", []) if p.get("kind") != "release"]
    return sorted(beitraege, key=lambda p: p.get("date", ""), reverse=True)


# --------------------------------------------------------------------------- Seite
def seite(daten: dict, meldung: str = "", fehler: str = "", adresse: str = "") -> bytes:
    heute = time.strftime("%Y-%m-%d")
    karten = []
    for p in eigene(daten):
        bild = f'<img src="{html.escape(p.get("image") or "")}" alt="" loading="lazy">' if p.get("image") else ""
        karten.append(
            f'<li><div class="karte">{bild}'
            f'<div><p class="titel">{html.escape(p.get("title", ""))}</p>'
            f'<p class="klein">{html.escape(p.get("date", ""))} · {html.escape(p.get("summary", ""))}</p></div>'
            f'<form method="post" action="/loeschen" data-bestaetigen="Diesen Beitrag wirklich löschen?">'
            f'<input type="hidden" name="id" value="{html.escape(p.get("id", ""))}">'
            f'<button class="leise" type="submit">Löschen</button></form></div></li>'
        )
    liste = "\n".join(karten) or '<li class="leer">Noch keine eigene Neuigkeit — die Versionsnotizen zählen nicht mit.</li>'

    hinweis = ""
    if meldung:
        hinweis = f'<p class="gut" role="status">{html.escape(meldung)}</p>'
    if fehler:
        hinweis = f'<p class="schlecht" role="alert">{html.escape(fehler)}</p>'

    text = f"""<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Redaktion — ERC Launcher</title>
<style>
:root {{
  --grund: #101013; --fläche: #17171b; --rand: #2a2a31;
  --text: #ececf1; --leise: #a6a6b2; --rot: #e10600; --gruen: #4ade80;
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0; padding: 32px 20px 64px; background: var(--grund); color: var(--text);
  font: 16px/1.6 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
}}
main {{ max-width: 720px; margin: 0 auto; }}
h1 {{ margin: 0 0 4px; font-size: 1.6rem; line-height: 1.2; }}
h2 {{ margin: 40px 0 12px; font-size: 1.05rem; letter-spacing: .01em; }}
p.unter {{ margin: 0 0 28px; color: var(--leise); }}
label {{ display: block; margin: 18px 0 6px; font-weight: 600; font-size: .95rem; }}
label .frei {{ font-weight: 400; color: var(--leise); }}
input[type=text], input[type=date], textarea {{
  width: 100%; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--rand);
  background: var(--fläche); color: var(--text); font: inherit;
}}
input[type=file] {{ color: var(--leise); }}
input[type=file]::file-selector-button {{
  font: inherit; font-size: .9rem; font-weight: 600; margin-right: 10px; padding: 8px 14px;
  border-radius: 8px; border: 1px solid var(--rand); background: var(--fläche); color: var(--text); cursor: pointer;
}}
textarea {{ min-height: 220px; resize: vertical; font-family: ui-monospace, "Cascadia Mono", Consolas, monospace; font-size: .9rem; }}
input:focus-visible, textarea:focus-visible, button:focus-visible {{
  outline: 2px solid var(--rot); outline-offset: 2px;
}}
.hilfe {{ margin: 6px 0 0; font-size: .85rem; color: var(--leise); }}
button {{
  font: inherit; font-weight: 600; border-radius: 8px; cursor: pointer;
  border: 1px solid var(--rand); background: var(--fläche); color: var(--text); padding: 10px 16px;
}}
button.haupt {{ margin-top: 26px; background: var(--rot); border-color: var(--rot); color: #fff; padding: 12px 22px; }}
button.leise {{ padding: 6px 12px; font-size: .85rem; color: var(--leise); }}
button:hover {{ filter: brightness(1.15); }}
.gut, .schlecht {{ padding: 10px 14px; border-radius: 8px; border: 1px solid var(--rand); background: var(--fläche); }}
.gut {{ border-color: var(--gruen); }}
.schlecht {{ border-color: var(--rot); }}
ul {{ list-style: none; margin: 0; padding: 0; }}
.karte {{ display: flex; gap: 14px; align-items: center; padding: 12px 0; border-top: 1px solid var(--rand); }}
.karte img {{ width: 72px; height: 48px; object-fit: cover; border-radius: 6px; flex: none; }}
.karte > div {{ flex: 1; min-width: 0; }}
.titel {{ margin: 0; font-weight: 600; }}
.klein {{ margin: 2px 0 0; font-size: .85rem; color: var(--leise); }}
.fuss {{ margin-top: 40px; padding-top: 14px; border-top: 1px solid var(--rand); }}
.leer {{ padding: 14px 0; color: var(--leise); border-top: 1px solid var(--rand); }}
</style>
</head>
<body>
<main>
  <h1>Redaktion</h1>
  <p class="unter">Schreibt eine Neuigkeit in den Launcher. Er holt die Liste selbst —
  der Beitrag steht in spätestens 30 Minuten dort, bei „Neuigkeiten" ganz oben.</p>
  {hinweis}
  <form method="post" action="/speichern" enctype="multipart/form-data">
    <label for="titel">Titel</label>
    <input id="titel" name="titel" type="text" required maxlength="120" autofocus
           placeholder="Zweites Rennen am Sonntag">
    <label for="kurz">Kurzfassung <span class="frei">— eine Zeile, erscheint in der Liste</span></label>
    <input id="kurz" name="kurz" type="text" required maxlength="160"
           placeholder="Start 19:00, Bahrain, 25 % Distanz">
    <label for="text">Text <span class="frei">— Markdown</span></label>
    <textarea id="text" name="text" required
      placeholder="**Wichtig:** alle brauchen die Fassung 1.5.22.

- Strecke: Bahrain
- Distanz: 25 %
- Qualifying: 18 Minuten"></textarea>
    <p class="hilfe">**fett** · *kursiv* · `Code` · - Liste · [Link](https://…) · Leerzeile = neuer Absatz</p>
    <label for="bild">Bild <span class="frei">— freiwillig, höchstens 8 MB (PNG, JPG, GIF, WebP)</span></label>
    <input id="bild" name="bild" type="file" accept="image/png,image/jpeg,image/gif,image/webp">
    <label for="datum">Datum</label>
    <input id="datum" name="datum" type="date" value="{heute}" required>
    <button class="haupt" type="submit">Veröffentlichen</button>
  </form>
  <h2>Was gerade in dieser Liste steht</h2>
  <ul>
{liste}
  </ul>
  <p class="klein fuss">Diese Seite: {html.escape(adresse)} · Datei: {html.escape(str(FEED))}</p>
</main>
<script src="/redaktion.js" defer></script>
</body>
</html>
"""
    return text.encode("utf-8")


# Kein Inline-Skript: die Seitenrichtlinie (CSP) erlaubt nur eigene Dateien. Ein
# Inline-Handler am Formular wurde vom Browser stillschweigend verworfen — der
# Löschen-Knopf tat deshalb nichts.
SKRIPT = """// Fragt vor dem Löschen nach. Ohne diese Datei löscht der Knopf ohne Rückfrage —
// die Server-Seite prüft weiterhin, dass nur eigene Neuigkeiten verschwinden.
document.addEventListener('submit', function (ereignis) {
  var formular = ereignis.target;
  var frage = formular.getAttribute('data-bestaetigen');
  if (frage && !window.confirm(frage)) ereignis.preventDefault();
});
""".encode("utf-8")


# --------------------------------------------------------------------------- Anfragen
def darf(adresse: str) -> bool:
    try:
        ip = ipaddress.ip_address(adresse)
    except ValueError:
        return False
    if getattr(ip, "ipv4_mapped", None):
        ip = ip.ipv4_mapped
    return any(ip in netz for netz in ERLAUBTE_NETZE)


def formular_lesen(kopfzeilen, koerper: bytes) -> tuple[dict, dict]:
    """Felder und Dateien aus einem multipart/form-data- oder urlencoded-Rumpf."""
    art = kopfzeilen.get("Content-Type", "")
    if art.startswith("application/x-www-form-urlencoded"):
        felder = {k: v[0] for k, v in parse_qs(koerper.decode("utf-8", "replace")).items()}
        return felder, {}
    roh = f"Content-Type: {art}\r\nMIME-Version: 1.0\r\n\r\n".encode() + koerper
    nachricht = BytesParser(policy=policy.default).parsebytes(roh)
    felder, dateien = {}, {}
    if nachricht.is_multipart():
        for teil in nachricht.iter_parts():
            name = teil.get_param("name", header="content-disposition")
            dateiname = teil.get_filename()
            daten = teil.get_payload(decode=True) or b""
            if dateiname:
                dateien[name] = (dateiname, daten)
            else:
                felder[name] = daten.decode(teil.get_content_charset() or "utf-8", "replace")
    return felder, dateien


def bild_ablegen(name: str, daten: bytes) -> str:
    endung = next((e for kopf, e in BILDARTEN.items() if daten.startswith(kopf)), None)
    if daten[:4] == b"RIFF" and daten[8:12] == b"WEBP":
        endung = ".webp"
    if endung is None:
        raise ValueError("Das ist kein PNG-, JPG-, GIF- oder WebP-Bild.")
    if len(daten) > MAX_BILD:
        raise ValueError("Das Bild ist größer als 8 MB.")
    BILDER.mkdir(parents=True, exist_ok=True)
    ziel = f"{slug(Path(name).stem)}-{time.strftime('%Y%m%d-%H%M%S')}{endung}"
    (BILDER / ziel).write_bytes(daten)
    return f"{OEFFENTLICH}/news/{quote(ziel)}"


class Bearbeiter(BaseHTTPRequestHandler):
    server_version = "Redaktion/1.0"

    def log_message(self, fmt, *args):  # kürzer ins journal
        sys.stdout.write("%s %s\n" % (self.address_string(), fmt % args))
        sys.stdout.flush()

    def antwort(self, code: int, koerper: bytes, art: str = "text/html; charset=utf-8") -> None:
        self.send_response(code)
        self.send_header("Content-Type", art)
        self.send_header("Content-Length", str(len(koerper)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy",
                         "default-src 'none'; style-src 'unsafe-inline'; script-src 'self'; "
                         "img-src https: data:; form-action 'self'; base-uri 'none'")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(koerper)

    def umleiten(self, ziel: str) -> None:
        self.send_response(303)
        self.send_header("Location", ziel)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def pruefen(self) -> bool:
        if not darf(self.client_address[0]):
            self.antwort(403, "Nur aus dem Heimnetz.".encode())
            return False
        return True

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        if not self.pruefen():
            return
        pfad = urlparse(self.path)
        if pfad.path == "/redaktion.js":
            self.antwort(200, SKRIPT, "application/javascript; charset=utf-8")
            return
        if pfad.path != "/":
            self.antwort(404, "Nur / gibt es hier.".encode())
            return
        meldung = {"1": "Gespeichert — steht in spätestens 30 Minuten im Launcher.",
                   "2": "Beitrag gelöscht."}.get(parse_qs(pfad.query).get("ok", [""])[0], "")
        self.antwort(200, seite(feed_lesen(), meldung=meldung, adresse=self.headers.get("Host", "")))

    def do_POST(self):
        if not self.pruefen():
            return
        laenge = int(self.headers.get("Content-Length") or 0)
        if laenge > MAX_FORMULAR:
            self.antwort(413, seite(feed_lesen(),
                                    fehler="Die Anfrage ist zu groß — Bilder höchstens 8 MB.",
                                    adresse=self.headers.get("Host", "")))
            return
        if laenge <= 0:
            self.antwort(411, "Es kam kein Inhalt mit.".encode())
            return
        koerper = self.rfile.read(laenge)
        weg = urlparse(self.path).path
        if weg == "/speichern":
            self.speichern(koerper)
        elif weg == "/loeschen":
            self.loeschen(koerper)
        else:
            self.antwort(404, "Unbekannter Weg.".encode())

    def speichern(self, koerper: bytes) -> None:
        try:
            felder, dateien = formular_lesen(self.headers, koerper)
            titel = (felder.get("titel") or "").strip()
            kurz = (felder.get("kurz") or "").strip()
            text = (felder.get("text") or "").strip()
            datum = (felder.get("datum") or "").strip()
            faenger, bild = dateien.get("bild", ("", b""))
            if not titel or not kurz or not text:
                raise ValueError("Titel, Kurzfassung und Text werden gebraucht.")
            if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", datum):
                raise ValueError("Das Datum fehlt oder ist unlesbar.")
            bild_adresse = bild_ablegen(faenger, bild) if bild else ""
            daten = feed_lesen()
            beitrag = {
                "id": f"news/{datum}-{slug(titel)}",
                "kind": "news",
                "title": titel,
                "date": datum,
                "summary": kurz,
                "body": text,
            }
            if bild_adresse:
                beitrag["image"] = bild_adresse
            alt = [p for p in daten.get("posts", []) if p.get("id") != beitrag["id"]]
            daten["posts"] = [beitrag] + alt
            feed_schreiben(daten)
        except ValueError as fehler:
            self.antwort(400, seite(feed_lesen(), fehler=str(fehler), adresse=self.headers.get("Host", "")))
            return
        self.umleiten("/?ok=1")

    def loeschen(self, koerper: bytes) -> None:
        felder, _ = formular_lesen(self.headers, koerper)
        kennung = (felder.get("id") or "").strip()
        daten = feed_lesen()
        vorher = len(daten.get("posts", []))
        daten["posts"] = [p for p in daten.get("posts", [])
                          if p.get("kind") == "release" or p.get("id") != kennung]
        if len(daten["posts"]) != vorher:
            feed_schreiben(daten)
        self.umleiten("/?ok=2")


def main() -> int:
    if not NEWS_DIR.is_dir():
        print(f"{NEWS_DIR} fehlt — falsches Verzeichnis?", file=sys.stderr)
        return 2
    diener = ThreadingHTTPServer((BIND, PORT), Bearbeiter)
    print(f"Redaktion auf http://{BIND}:{PORT}/ — Datei: {FEED}", flush=True)
    try:
        diener.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
