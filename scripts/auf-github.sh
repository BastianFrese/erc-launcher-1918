#!/usr/bin/env bash
# Einen gebauten Stand oeffentlich machen — und den Tag auf genau diesen Commit setzen.
#
# Ablauf:
#   1. Arbeitsbaum muss sauber sein (sonst bricht es ab).
#   2. Der oeffentliche Zweig wird ein exaktes Abbild des Arbeitszweigs (eigener
#      Commit, ohne Upstream-Historie; LICENSE bleibt AGPL).
#   3. Probe: die Baeume beider Zweige muessen gleich sein, sonst Abbruch.
#   4. Push auf <FERN> main.
#   5. Nur mit --tag: Fassung gegen apps/launcher/Cargo.toml pruefen, Tag
#      erc-<fassung> auf genau diesen Commit setzen und pushen. Ein vorhandener,
#      anderswohin zeigender Tag wird NIE verschoben.
#
#     ./scripts/auf-github.sh "Was sich geaendert hat"
#     ./scripts/auf-github.sh --tag 1.5.0 "release: 1.5.0 — ..."
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
    [ -n "$TAG_FASSUNG" ] || die "--tag braucht eine Fassung, z. B. --tag 1.5.0"
fi
text=${1:-"Stand $(date +%Y-%m-%d)"}

# 1. Keine uncommitteten Aenderungen an verfolgten Dateien: sonst spiegelt der Lauf die
#    halbe Arbeit. Unverfolgte Reste (alte Namen aus frueheren Merges) stoeren den Spiegel
#    nicht, der nur verfolgte Inhalte uebernimmt.
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "Arbeitsbaum ist nicht sauber — erst committen"

git rev-parse --verify --quiet "$ARBEIT" >/dev/null || die "Zweig $ARBEIT fehlt"
git rev-parse --verify --quiet "$OEFFENTLICH" >/dev/null || die "Zweig $OEFFENTLICH fehlt"

# 5 vorab (billig): die Fassung im Quelltext muss die sein, die getaggt werden soll.
if [ -n "$TAG_FASSUNG" ]; then
    im_text=$(git show "$ARBEIT:apps/launcher/Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -1)
    [ "$im_text" = "$TAG_FASSUNG" ] \
        || die "apps/launcher/Cargo.toml nennt $im_text, verlangt war $TAG_FASSUNG"
fi

# 2. Spiegeln: Index exakt auf den Baum des Arbeitszweigs, dann in den Arbeitsbaum schreiben.
git checkout -q "$OEFFENTLICH"
trap 'git checkout -q "$ARBEIT" 2>/dev/null || true' EXIT
git read-tree "$ARBEIT"
git checkout-index -a -f
if git diff --cached --quiet; then
    echo "nichts Neues — der oeffentliche Stand ist schon der von $ARBEIT"
else
    git commit -q -m "$text"
fi

# 3. Probe: Inhalt, nicht nur Fassungsnummer. Genau dieser Vergleich hat bei
#    beta.1..beta.4 gefehlt — die Tags zeigten auf einen aelteren Stand.
rest=$(git diff --name-only "$OEFFENTLICH" "$ARBEIT")
[ -z "$rest" ] || die "Baeume weichen ab:
$rest"
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
