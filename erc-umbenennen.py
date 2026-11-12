# -*- coding: utf-8 -*-
"""Benennt den Launcher-Quelltext auf ERC um.

    python -I erc-umbenennen.py [--pruefen]

Nur, was ein Fahrer zu sehen bekommt. Alles Technische bleibt:

  * Mod-Kennungen ("swofty.*"), Adressen (*.swofty.net), ABI-Namen
    (SwoftyAddonInit, SwoftyContext, SwoftyMod ...) - darauf zeigen die Mods.
  * Pfade: "Swofty" als Ordner im Spielverzeichnis und in %LOCALAPPDATA%.
  * Der Datenordner (15 GB Einrichtung) wird nicht umbenannt.

Zweiter Lauf ist folgenlos (idempotent).
"""

import re
import sys
from pathlib import Path

TEXTENDUNGEN = {".rs", ".toml", ".md", ".json", ".h", ".hpp", ".c", ".cpp", ".sh"}

# Wurzel(n) relativ zum Repo. Nur diese werden angefasst.
ZIELE = ["apps/launcher/src", "apps/launcher/build.rs", "README.md", "Cargo.toml", "apps/launcher/Cargo.toml", "docs/news"]

ERSETZUNGEN = [
    ("Swofty Launcher", "ERC Launcher"),
    ("Swofty's", "ERC"),
    ("Swofty", "ERC"),
]

GESCHUETZT = [
    r"[A-Za-z0-9._/-]*swofty\.net[A-Za-z0-9._/-]*",
    r"Swofty-Developments",
    r"net\.swofty\.[A-Za-z0-9._-]+",
    r"swofty\.[a-z0-9][a-z0-9._-]*",
    # Bezeichner, an denen die Mods haengen: SwoftyMod, SwoftyApi, SwoftyEv...
    r"Swofty[A-Za-z0-9_]+",
    r"SWOFTY_[A-Z_]+",
    r"[Ss]wofty[-_]?(?:launcher|core|cli|sdk|addon|formats|builtins)[A-Za-z_]*",
    r"swofty\.exe",
    r"swofty-[a-z0-9-]+",
    # "Swofty" als Teil eines Pfads - der Ordner heisst weiter so.
    r"[A-Za-z0-9_.-]*[/\\]Swofty(?:[\\/][A-Za-z0-9_.-]*)?",
]


def maskiere(text):
    stelle = []

    def merken(m):
        stelle.append(m.group(0))
        return "\x00%d\x00" % (len(stelle) - 1)

    muster = re.compile("|".join("(?:%s)" % g for g in GESCHUETZT))
    return muster.sub(merken, text), stelle


def demaskiere(text, stelle):
    return re.sub(r"\x00(\d+)\x00", lambda m: stelle[int(m.group(1))], text)


def umbenennen(text):
    text, stelle = maskiere(text)
    for alt, neu in ERSETZUNGEN:
        text = text.replace(alt, neu)
    return demaskiere(text, stelle)


def dateien(wurzel):
    for ziel in ZIELE:
        pfad = wurzel / ziel
        if pfad.is_file():
            yield pfad
        elif pfad.is_dir():
            for datei in pfad.rglob("*"):
                if datei.is_file() and datei.suffix.lower() in TEXTENDUNGEN:
                    yield datei


def main():
    pruefen = "--pruefen" in sys.argv
    wurzel = Path(__file__).resolve().parent
    geaendert = 0
    for datei in dateien(wurzel):
        try:
            alt = datei.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            continue
        neu = umbenennen(alt)
        if neu == alt:
            continue
        geaendert += 1
        if pruefen:
            print("waere geaendert: %s" % datei.relative_to(wurzel))
        else:
            datei.write_bytes(neu.encode("utf-8"))
    print("%d Datei(en) %s." % (geaendert, "betroffen" if pruefen else "umbenannt"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
