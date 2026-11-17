#!/usr/bin/env bash
# Einen Stand auf GitHub bringen.
#
# Der oeffentliche Zweig `main` zeigt genau das, was auf dem Arbeitszweig liegt —
# aber als eigener Commit, ohne Swoftys Upstream-Historie. Die Lizenz bleibt die
# AGPL (LICENSE im Repo), die Herkunft steht im README.
#
#     ./auf-github.sh "Was sich geaendert hat"
#
# Andere Zweige/Remotes: ARBEIT=... OEFFENTLICH=... FERN=... voranstellen.
set -euo pipefail
ARBEIT=${ARBEIT:-erc/1.9.60}
OEFFENTLICH=${OEFFENTLICH:-erc-eigen}
FERN=${FERN:-erc1918}

text=${1:-"Stand $(date +%Y-%m-%d)"}

git checkout -q "$OEFFENTLICH"
git restore --source="$ARBEIT" --staged --worktree -- :/
if git diff --cached --quiet; then
    echo "nichts Neues — der oeffentliche Stand ist schon der von $ARBEIT"
else
    git commit -q -m "$text"
    git push "$FERN" "$OEFFENTLICH:main"
fi
git checkout -q "$ARBEIT"
