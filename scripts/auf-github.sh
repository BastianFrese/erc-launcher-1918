#!/usr/bin/env bash
# Einen gebauten Stand oeffentlich machen — und den Tag auf genau diesen Commit setzen.
#
# Ablauf:
#   1. Arbeitsbaum muss sauber sein (verfolgte Dateien; unverfolgte Reste stoeren nicht).
#   2. Der oeffentliche Zweig bekommt einen Commit, dessen Baum exakt der des
#      Arbeitszweigs ist. Das passiert ueber einen eigenen Index: der Arbeitsbaum wird
#      NICHT angefasst und kein fremder Zweig ausgecheckt (das hat am 22.11.2026 die
#      alten Dateinamen der oeffentlichen Fassung im Arbeitsbaum liegen gelassen, und der
#      Kern-Bau hat sie mitkompiliert).
#   3. Probe: beide Baeume muessen gleich sein, sonst Abbruch.
#   4. Push auf <FERN> main.
#   5. Nur mit --tag: Fassung gegen apps/launcher/Cargo.toml pruefen, Tag
#      erc-<fassung> auf genau diesen Commit setzen und pushen. Ein vorhandener,
#      anderswohin zeigender Tag wird NIE verschoben.
#
#     ./scripts/auf-github.sh "Was sich geaendert hat"
#     ./scripts/auf-github.sh --tag 1.5.1 "release: 1.5.1 — ..."
#
# Umgebung: ARBEIT (Zweig), OEFFENTLICH (Spiegelzweig), FERN (Remote).
set -euo pipefail

ARBEIT=${ARBEIT:-erc/v1.10.24-port}
OEFFENTLICH=${OEFFENTLICH:-erc-eigen}
FERN=${FERN:-erc1918}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

die() { echo "auf-github.sh: $*" >&2; exit 1; }

TAG_FASSUNG=""
if [ "${1:-}" = "--tag" ]; then
    TAG_FASSUNG=${2:-}
    shift 2 || true
    [ -n "$TAG_FASSUNG" ] || die "--tag braucht eine Fassung, z. B. --tag 1.5.1"
fi
text=${1:-"Stand $(date +%Y-%m-%d)"}

# 1. Keine uncommitteten Aenderungen an verfolgten Dateien: sonst spiegelt der Lauf die
#    halbe Arbeit.
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "Arbeitsbaum ist nicht sauber — erst committen"

git rev-parse --verify --quiet "$ARBEIT" >/dev/null || die "Zweig $ARBEIT fehlt"
git rev-parse --verify --quiet "$OEFFENTLICH" >/dev/null || die "Zweig $OEFFENTLICH fehlt"

# 5 vorab (billig): die Fassung im Quelltext muss die sein, die getaggt werden soll.
if [ -n "$TAG_FASSUNG" ]; then
    im_text=$(git show "$ARBEIT:apps/launcher/Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -1)
    [ "$im_text" = "$TAG_FASSUNG" ] \
        || die "apps/launcher/Cargo.toml nennt $im_text, verlangt war $TAG_FASSUNG"
fi

# 2. Spiegel-Commit ueber einen eigenen Index.
idx="$(cd "$(git rev-parse --git-dir)" && pwd)/auf-github-index.$$"
rm -f "$idx"
GIT_INDEX_FILE=$idx git read-tree "$ARBEIT"
tree=$(GIT_INDEX_FILE=$idx git write-tree)
rm -f "$idx"
[ -n "$tree" ] || die "kein Baum aus $ARBEIT"

alt=$(git rev-parse "$OEFFENTLICH")
if [ "$(git rev-parse "$alt^{tree}")" = "$tree" ]; then
    echo "nichts Neues — der oeffentliche Stand ist schon der von $ARBEIT"
else
    neu=$(git commit-tree "$tree" -p "$alt" -m "$text")
    git update-ref "refs/heads/$OEFFENTLICH" "$neu"
    echo "Spiegel-Commit $(git rev-parse --short "$neu")"
fi

# 3. Probe: Inhalt, nicht nur Fassungsnummer.
[ "$(git rev-parse "$OEFFENTLICH^{tree}")" = "$(git rev-parse "$ARBEIT^{tree}")" ] \
    || die "Baeume weichen ab: $OEFFENTLICH != $ARBEIT"
echo "Baeume gleich: $OEFFENTLICH = $ARBEIT ($(git rev-parse --short "$OEFFENTLICH"))"

# 4. Push.
git push "$FERN" "$OEFFENTLICH:main"

# 5. Tag auf genau diesen Commit.
if [ -n "$TAG_FASSUNG" ]; then
    tag="erc-$TAG_FASSUNG"
    ziel=$(git rev-parse "$OEFFENTLICH")
    if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
        [ "$(git rev-parse "refs/tags/$tag^{commit}")" = "$ziel" ] \
            || die "Tag $tag zeigt woandershin — alten Tag nicht verschieben, von Hand entscheiden"
        echo "Tag $tag stand schon richtig auf $ziel"
    else
        git tag "$tag" "$ziel"
        git push "$FERN" "refs/tags/$tag"
    fi
    echo "Tag $tag -> $(git rev-parse --short "$ziel")"
fi

echo "fertig."
