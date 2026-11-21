#!/usr/bin/env bash
# Bereitet einen Unterbau-Merge (Swoftys Upstream -> unser Port) pruefbar vor.
# Ohne --go passiert nur: git fetch --tags + neuer Zweig. Sonst nichts.
set -euo pipefail

BASE_DEFAULT=erc/1.10.0-port
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PYTHON=${PYTHON:-python}

usage() {
    cat <<'EOF'
erc-merge.sh [--go] [--branch NAME] [--base ZWEIG] <tag>

Ablauf:
  1. git fetch --tags
  2. neuen Zweig erc/<tag>-port von erc/1.10.0-port anlegen (Name mit --branch)
  3. mit --go: auf den Zweig wechseln und git merge <tag> --no-commit
  4. mit --go: python scripts/erc-audit.py   (unsere ERC-Zutaten pruefen)
  5. mit --go: cargo test -p erc-launcher

Ohne --go endet das Skript nach Schritt 2; der Merge startet nur mit --go.
Jeder scheiternde Schritt bricht mit Meldung ab.

Optionen:
  --go             Merge, Audit und Tests wirklich ausfuehren
  --branch NAME    Zweigname statt erc/<tag>-port
  --base ZWEIG     Ausgangszweig statt erc/1.10.0-port
  -h, --help       diese Hilfe

Umgebung:
  PYTHON           Python-Aufruf fuer das Audit (Vorgabe: python)

Bekannte Konflikte: Cargo.lock, docs/releases/*, native/build.sh,
native/src/core/runtime/{runtime.hpp,lifecycle.cpp}, native/tools/core_init_order.txt
(bzw. native/build/init-order.txt).
EOF
}

die() { echo "erc-merge.sh: $*" >&2; exit 1; }

RUN=0
BRANCH=""
BASE=$BASE_DEFAULT
TAG=""
while [ $# -gt 0 ]; do
    case $1 in
        -h | --help) usage; exit 0 ;;
        --go) RUN=1 ;;
        --branch) [ $# -ge 2 ] || die "--branch braucht einen Namen"; BRANCH=$2; shift ;;
        --base) [ $# -ge 2 ] || die "--base braucht einen Zweig"; BASE=$2; shift ;;
        -*) die "unbekannte Option '$1' (siehe --help)" ;;
        *)
            [ -z "$TAG" ] || die "nur ein Tag erlaubt ('$TAG' und '$1')"
            TAG=$1
            ;;
    esac
    shift
done

[ -n "$TAG" ] || { usage >&2; die "kein Tag angegeben"; }
[ -n "$BRANCH" ] || BRANCH="erc/$TAG-port"

cd "$ROOT"
git rev-parse --git-dir >/dev/null 2>&1 || die "$ROOT ist kein Git-Repository"

echo "== 1/3 git fetch --tags"
git fetch --tags || die "git fetch --tags fehlgeschlagen (Netz, Rechte?)"

git rev-parse --verify --quiet "$TAG^{commit}" >/dev/null \
    || die "Tag '$TAG' ist nach dem Fetch nicht vorhanden"
git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null \
    || die "Ausgangszweig '$BASE' ist nicht vorhanden"

echo "== 2/3 neuer Zweig $BRANCH von $BASE"
git show-ref --verify --quiet "refs/heads/$BRANCH" \
    && die "Zweig '$BRANCH' existiert bereits; bitte --branch oder Aufraeumen"
git branch "$BRANCH" "$BASE" || die "Zweig '$BRANCH' liess sich nicht anlegen"

if [ "$RUN" -eq 0 ]; then
    echo
    echo "Bereit. Der Merge wurde NICHT gestartet (ohne --go)."
    echo "Naechster Schritt: git switch $BRANCH && git merge $TAG --no-commit"
    echo "oder erneut: $0 --go $TAG"
    exit 0
fi

echo "== 3/3 Merge $TAG auf $BRANCH"
git switch "$BRANCH" || die "Wechsel auf '$BRANCH' fehlgeschlagen"
if ! git merge "$TAG" --no-commit; then
    cat >&2 <<EOF
erc-merge.sh: Merge '$TAG' ist mit Konflikten stehen geblieben.
Bekannte Konflikte: Cargo.lock, docs/releases/*, native/build.sh,
native/src/core/runtime/runtime.hpp, native/src/core/runtime/lifecycle.cpp,
native/tools/core_init_order.txt (bzw. native/build/init-order.txt).
Konflikte aufloesen (git status), dann erneut:
  $PYTHON scripts/erc-audit.py && cargo test -p erc-launcher
Abbrechen mit: git merge --abort
EOF
    exit 1
fi

echo "== Audit"
if ! "$PYTHON" scripts/erc-audit.py; then
    die "Audit rot: unsere ERC-Zutaten fehlen oder Marker sind weg. Merge pruefen, nicht committen."
fi

echo "== Tests"
cargo test -p erc-launcher || die "cargo test -p erc-launcher rot. Merge pruefen, nicht committen."

echo
echo "Grün. Merge liegt vorbereitet (noch nicht committet) auf $BRANCH."
