#!/usr/bin/env bash
# Bootstrap the Forte Studio fork working tree from upstream Code - OSS.
#
#   studio/bootstrap.sh [DEST]        (default: ../forte-studio)
#
# What it does:
#   1. clone microsoft/vscode at the pinned tag (or reuse DEST)
#   2. overlay studio/product.json (branding, Open VSX)
#   3. vendor the bundled vscode-forte extension into extensions/
#   4. print the build commands (web target builds in the dev container;
#      desktop needs Electron downloads → dev Mac / CI)
#
# All Forte workbench code belongs in src/vs/workbench/contrib/forteDaw/
# inside the fork — one folder, one rebase junction (ADR D-14).
set -euo pipefail

UPSTREAM_TAG="${UPSTREAM_TAG:-1.102.0}"
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
DEST="${1:-"$REPO_ROOT/../forte-studio"}"

if [ ! -d "$DEST/.git" ]; then
  echo "== cloning microsoft/vscode @ $UPSTREAM_TAG → $DEST"
  git clone --depth 1 --branch "$UPSTREAM_TAG" https://github.com/microsoft/vscode "$DEST"
else
  echo "== reusing existing clone: $DEST"
fi

echo "== overlaying product.json (Forte Studio branding + Open VSX)"
python3 - "$DEST/product.json" "$HERE/product.json" <<'EOF'
import json, sys
dest, overlay = sys.argv[1], sys.argv[2]
base = json.load(open(dest))
base.update(json.load(open(overlay)))
json.dump(base, open(dest, "w"), indent="\t", ensure_ascii=False)
print(f"   {dest}: nameShort = {base['nameShort']}")
EOF

echo "== vendoring the bundled vscode-forte extension"
mkdir -p "$DEST/extensions/forte"
cp -r "$REPO_ROOT/forte/editor/vscode-forte/." "$DEST/extensions/forte/"

cat <<EOF

Bootstrapped. Next, inside $DEST:

  npm ci                                 # upstream deps (native modules)
  npm run compile                        # or: npm run watch
  ./scripts/code-web.sh                  # WEB target — works in the container
  ./scripts/code.sh                      # desktop — needs Electron (dev Mac/CI)

Forte workbench code goes in: src/vs/workbench/contrib/forteDaw/
Milestones: docs/webdaw/09-forte-studio-fork.md (F0..F4)
EOF
