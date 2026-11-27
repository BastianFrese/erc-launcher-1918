# Redaktion — Neuigkeiten für den Launcher

Eine Seite im Browser, drei Felder, ein Knopf: der Beitrag landet in
`/var/www/downloads/news.json`, und der Launcher holt ihn dort ab (er fragt die
Datei und zeigt neue Beiträge unter „Neuigkeiten"; er hat die Liste bis zu
**30 Minuten** im Zwischenspeicher).

## Wo er läuft

| Was | Wert |
|---|---|
| Rechner | `erdi-ws` (192.168.100.7) |
| Adresse | `http://192.168.100.7:8096/` — **nur** Heimnetz und Tailscale |
| Dienst | `redaktion.service` (systemd, Nutzer `www-data`) |
| Quelle hier | `server/news/news-schreiber.py` → `/opt/news/news-schreiber.py` |
| Datei | `/var/www/downloads/news.json` (Sicherungen `news.json.bak-<stempel>`, die letzten 10) |
| Bilder | `/var/www/downloads/news/`, ausgeliefert als `https://download.erdi-erc.de/news/<datei>` |

Kein nginx davor: der Dienst hört selbst auf der LAN-Adresse, damit er über
Cloudflare gar nicht erreichbar ist. Er nimmt nur Anfragen aus
`192.168.100.0/24`, `100.64.0.0/10` (Tailscale) und von localhost an.

## Einrichten (falls der Rechner neu aufgesetzt wird)

```bash
mkdir -p /opt/news /var/www/downloads/news
scp server/news/news-schreiber.py erdi-ws:/opt/news/
# /etc/systemd/system/redaktion.service — siehe unten
systemctl daemon-reload && systemctl enable --now redaktion
```

Die Unit setzt `BIND=192.168.100.7`, `PORT=8096`, `NEWS_DIR=/var/www/downloads`,
läuft als `www-data` und schreibt nur unter `/var/www/downloads`.

> [!warning] Zwei Rechte sind nötig, sonst bricht das Speichern ab:
> `chown www-data:www-data /var/www/downloads/news.json` (die Datei selbst) und
> `chgrp www-data /var/www/downloads && chmod 775 /var/www/downloads` (damit die
> Sicherung und die neue Datei daneben angelegt werden können). Das erste
> Speichern scheiterte genau daran (`PermissionError` auf `news.json.bak-…`).

## Was der Launcher von einem Beitrag liest

```json
{
  "id": "news/2026-11-25-zweites-rennen",
  "kind": "news",
  "title": "Zweites Rennen am Sonntag",
  "date": "2026-11-25",
  "summary": "Start 19:00, Bahrain, 25 % Distanz",
  "body": "**Wichtig:** …",
  "image": "https://download.erdi-erc.de/news/bild-20261125-0926.png"
}
```

`kind: "release"` gehört den Versionsnotizen — die Redaktion fasst sie nicht an.
Der Text ist Markdown: `**fett**`, `*kursiv*`, `` `Code` ``, `- Liste`, Tabellen,
`[Link](https://…)`, Leerzeile = neuer Absatz. Nur `https:`-, `mailto:`- und
seitenrelative Links werden angezeigt.

## Probefahrt neben dem echten Dienst

`NEWS_DIR=/tmp/probe PORT=8097 python3 news-schreiber.py` läuft neben dem echten Dienst,
ohne die ausgelieferte Datei anzufassen. Damit eine Probeseite nicht mit der echten
verwechselt wird, schreibt jede Seite unten ihre Adresse und ihre Datei hin
(„Diese Seite: 127.0.0.1:8097 · Datei: /tmp/probe/news.json").

> [!warning] Genau daran ist am 25.11. ein echter Schreibversuch gescheitert: ein alter
> Tab zeigte auf `127.0.0.1:8097`, der Probedienst war beendet — „Verbindung abgelehnt"
> bei `/speichern`, im Dienstjournal der echten Seite stand kein einziger POST.

## Grenzen

- Höchstens 8 MB je Bild (PNG, JPG, GIF, WebP — geprüft am Dateikopf, nicht an der Endung).
- Ein Beitrag mit gleichem Datum und gleichem Titel ersetzt den vorherigen (gleiche `id`).
- Löschen entfernt nur eigene Neuigkeiten; Versionsnotizen lehnt der Dienst ab.
