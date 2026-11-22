#!/usr/bin/env python3
"""Prueft, ob unsere ERC-Zutaten nach einem Unterbau-Merge noch im Arbeitsbaum stehen.

Jeder Punkt ist eine Zeile: `ok` oder `FEHLT`. Exitcode 1, sobald ein Punkt fehlt.
Nur lesend; aendert nichts.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# (Beschriftung, Art, Pfad, Muster)
#   exists        - Datei muss da sein
#   match         - Datei muss da sein und das Muster enthalten
#   optional      - wie match, aber eine fehlende Datei ist ok (erzeugter Stand)
#   dir_match     - irgendeine *.rs-Datei unter dem Ordner muss passen
CHECKS = [
    # Der eigene Mehrspieler-Weg: die Einheiten selbst.
    ("netplay/netplay.cpp", "exists", "native/src/core/netplay/netplay.cpp", None),
    ("netplay/netplay_bridge.cpp", "exists", "native/src/core/netplay/netplay_bridge.cpp", None),
    ("netplay/netplay_drive.cpp", "exists", "native/src/core/netplay/netplay_drive.cpp", None),
    ("netplay/netplay_room.cpp", "exists", "native/src/core/netplay/netplay_room.cpp", None),

    # Deklarationen im fremden Header (Netplay muss dort auftauchen).
    ("runtime.hpp: netplay_setup()", "match", "native/src/core/runtime/runtime.hpp",
     r"\bvoid\s+netplay_setup\s*\("),
    ("runtime.hpp: netplay_bridge_setup()", "match", "native/src/core/runtime/runtime.hpp",
     r"\bvoid\s+netplay_bridge_setup\s*\("),
    ("runtime.hpp: netplay_room_setup()", "match", "native/src/core/runtime/runtime.hpp",
     r"\bvoid\s+netplay_room_setup\s*\("),
    ("runtime.hpp: netplay_drive_setup()", "match", "native/src/core/runtime/runtime.hpp",
     r"\bvoid\s+netplay_drive_setup\s*\("),

    # Unsere Mixins beim Anhaengen.
    ("lifecycle.cpp: netplay_icons_mixins()", "match", "native/src/core/runtime/lifecycle.cpp",
     r"add_core_sets\s*\(\s*runtime::netplay_icons_mixins\s*\(\s*\)\s*\)"),
    ("lifecycle.cpp: netplay_keys_mixins()", "match", "native/src/core/runtime/lifecycle.cpp",
     r"add_core_sets\s*\(\s*runtime::netplay_keys_mixins\s*\(\s*\)\s*\)"),
    ("lifecycle.cpp: netplay_privilege_mixins()", "match", "native/src/core/runtime/lifecycle.cpp",
     r"add_core_sets\s*\(\s*runtime::netplay_privilege_mixins\s*\(\s*\)\s*\)"),

    # Deckblatt der Raum-/Sitzungsanzeige.
    ("build.sh: kopiert assets/cover.png ins Addon", "match", "native/build.sh",
     r'cp\s+assets/cover\.png\s+"\$DIST/swofty/addons/core/cover\.png"'),
    ("build.sh: native/assets/cover.png vorhanden", "exists", "native/assets/cover.png", None),
    ("room_cover.cpp: laedt swofty:addons/core/cover.png", "match",
     "native/src/core/hud/room_cover.cpp", r'"swofty:addons/core/cover\.png"'),
    ("room_cover.cpp: fragt netplay_room::cover_signal()", "match",
     "native/src/core/hud/room_cover.cpp", r"netplay_room::cover_signal\s*\(\s*\)"),

    # Reihenfolge-Dateien fuehren die Netplay-Einheiten.
    ("core_link_order.txt: netplay.cpp", "match", "native/tools/core_link_order.txt",
     r"^src/core/netplay/netplay\.cpp$"),
    ("core_link_order.txt: netplay_bridge.cpp", "match", "native/tools/core_link_order.txt",
     r"^src/core/netplay/netplay_bridge\.cpp$"),
    ("core_link_order.txt: netplay_drive.cpp", "match", "native/tools/core_link_order.txt",
     r"^src/core/netplay/netplay_drive\.cpp$"),
    ("core_link_order.txt: netplay_room.cpp", "match", "native/tools/core_link_order.txt",
     r"^src/core/netplay/netplay_room\.cpp$"),
    ("build/init-order.txt: netplay-Einheiten", "optional", "native/build/init-order.txt",
     r"src/core/netplay/netplay\.cpp[\s\S]*src/core/netplay/netplay_bridge\.cpp"
     r"[\s\S]*src/core/netplay/netplay_drive\.cpp[\s\S]*src/core/netplay/netplay_room\.cpp"),

    # Launcher: Online-Reiter, Palette, Hilfe-Seite, Anzeigenamen, Relay, Anmeldung.
    ("tabs.rs: Online fuehrt zu Page::Multiplayer", "match", "apps/launcher/src/shell/tabs.rs",
     r"Self::Online\s*=>\s*Page::Multiplayer"),
    ("theme.rs: ERC-Palette 0xe10600", "match", "apps/launcher/src/ui/theme.rs",
     r"accent:\s*hex\s*\(\s*0xe10600\s*\)"),
    ("store/: Hilfe-Seite Goal/Supporter", "dir_match", "apps/launcher/src/features/store",
     r"support::\{Goal,\s*Supporter\}"),
    ("library.rs: display_name-Mapping", "match", "apps/launcher/src/services/library.rs",
     r"fn\s+display_name\s*\("),
    ("ampel.rs: eigenes Relay ueber default_relay()", "match", "apps/launcher/src/features/ampel.rs",
     r"default_relay\s*\(\s*\)"),
    ("relay.rs: f1.erdi-erc.de", "match", "packages/core/src/online/multiplayer/relay.rs",
     r'"f1\.erdi-erc\.de'),
    ("sign_in.rs: AUTH_URL raum.erdi-erc.de", "match", "packages/core/src/online/server/sign_in.rs",
     r'AUTH_URL\s*:\s*&str\s*=\s*"https://raum\.erdi-erc\.de"'),

    # Der Online-Reiter fuehrt in die Lobby, nicht in fremde Bestenlisten (22.11.2026:
    # der 1.10.24-Merge brachte Zeitfahren als Startseite und einen Helfer
    # `online_pages` zurueck, obwohl beides bei uns entfernt ist).
    ("tabs.rs: Online-Reiter startet in der Lobby", "match", "apps/launcher/src/shell/tabs.rs",
     r"Tab::Online\.home\(\)\s*==\s*Page::Multiplayer"),
    ("tabs.rs: kein fremdes online_pages", "absent", "apps/launcher/src/shell/tabs.rs",
     r"\bfn\s+online_pages\s*\("),
]

# Unsere Markierungen in fremden Dateien: Datei -> Kurzname.
MARKERS = [
    ("native/src/core/runtime/runtime.hpp", "netplay-decls"),
    ("native/src/core/runtime/lifecycle.cpp", "netplay-mixins"),
    ("native/build.sh", "cover-copy"),
    ("native/src/core/hud/room_cover.cpp", "netplay-cover"),
    ("native/src/core/hud/room_cover.cpp", "netplay-cover-signal"),
]

def read(path):
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def has(path, pattern):
    text = read(path)
    return text is not None and re.search(pattern, text, re.MULTILINE) is not None


def check(kind, rel, pattern):
    path = ROOT / rel
    if kind == "exists":
        return path.is_file()
    if kind == "match":
        return has(path, pattern)
    if kind == "absent":
        return not has(path, pattern)
    if kind == "optional":
        return not path.exists() or has(path, pattern)
    if kind == "dir_match":
        if not path.is_dir():
            return False
        return any(has(f, pattern) for f in sorted(path.rglob("*.rs")))
    raise ValueError(f"unbekannte Art: {kind}")


def main():
    missing = []
    total = 0
    for label, kind, rel, pattern in CHECKS:
        total += 1
        if check(kind, rel, pattern):
            print(f"ok     {label}")
        else:
            missing.append(label)
            hint = f"{rel}: {pattern}" if pattern else rel
            print(f"FEHLT  {label}  ->  {hint}")

    for rel, name in MARKERS:
        total += 1
        label = f"Marker ERC-BEGIN/ERC-END {name} in {rel}"
        begin = has(ROOT / rel, rf"(?://|#)\s*ERC-BEGIN\s+{re.escape(name)}\b")
        end = has(ROOT / rel, rf"(?://|#)\s*ERC-END\s+{re.escape(name)}\b")
        if begin and end:
            print(f"ok     {label}")
        else:
            missing.append(label)
            print(f"FEHLT  {label}")

    print()
    print(f"ERGEBNIS: {total - len(missing)} von {total} in Ordnung")
    if missing:
        print("Fehlend: " + ", ".join(missing))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
