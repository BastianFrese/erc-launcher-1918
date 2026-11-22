# ERC-Merge-Playbook (Unterbau 1.10.x)

Kurzablauf für einen Swoftys-Upstream-Merge auf unseren Port `erc/1.10.0-port`.
Unsere Haken sind mit `// ERC-BEGIN <name>` … `// ERC-END <name>` markiert (in
`build.sh` mit `#`). `scripts/erc-audit.py` prüft, ob sie und die übrigen
ERC-Zutaten noch da sind — grün heißt: gefunden, rot heißt: laut gescheitert.

1. **Zweig.** `bash scripts/erc-merge.sh <tag>` holt Tags und legt
   `erc/<tag>-port` von `erc/1.10.0-port` an (Name über `--branch`, Basis über
   `--base`). Ohne `--go` passiert nichts weiter.

2. **Merge.** Mit `--go` wechselt das Skript auf den Zweig und führt
   `git merge <tag> --no-commit` aus. Bekannte Konflikte: `Cargo.lock`,
   `docs/releases/*`, `native/build.sh`, `native/src/core/runtime/runtime.hpp`,
   `native/src/core/runtime/lifecycle.cpp`, `native/tools/core_init_order.txt`
   (bzw. das erzeugte `native/build/init-order.txt`). Konflikte von Hand
   auflösen, unsere Blöcke zwischen den Markern zurückholen, `git status` lesen;
   Abbrechen mit `git merge --abort`.

3. **Audit.** `python scripts/erc-audit.py` — je Punkt eine Zeile `ok`/`FEHLT`,
   Exitcode 1 bei Lücken. Fehlt etwas: Haken nachziehen, nicht committen. Das
   Audit liest nur den Arbeitsbaum.

4. **Tests.** `cargo test -p erc-launcher` muss grün sein (das Skript macht das
   mit `--go` selbst).

5. **Kern bauen.** Im Ordner `native`:
   `PATH=/c/Users/basti/tools/zig-x86_64-windows-0.16.0:$PATH ./build.sh`.
   Der Bau muss grün sein; fehlende `native/assets/cover.png` oder eine
   verlorene `cp`-Zeile fallen bereits im Audit auf.

6. **Installieren.** Gebautes Ergebnis nach `dist-110\loader` und
   `erc-test-110\loader` kopieren (beide Ziele, damit Test- und normale
   Installation denselben Stand haben).

7. **Abnahme im Spiel.** Raum erstellen → Deckblatt (`cover.png`) erscheint →
   LAN-Lobby erreichbar. Erst danach gilt der Merge als abgenommen.

8. **Release.** Reihenfolge: bauen → committen und pushen → GitHub-Release
   `erc-<fassung>` → erst dann Download-Seite und Feed aktualisieren.
