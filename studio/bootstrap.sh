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

UPSTREAM_TAG="${UPSTREAM_TAG:-1.117.0}"
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

# --- container/web-target build support (learned bootstrapping F0) ----------
# Networks that block electronjs.org / GitHub release assets can still build
# the WEB target: native modules build against the local node instead of
# electron, the electron binary download is skipped, and @vscode/ripgrep is
# satisfied by seeding its download cache from the system ripgrep.
if [ "${WEB_ONLY:-0}" = "1" ]; then
  echo "== WEB_ONLY: node-runtime .npmrc (electron keys kept for gulp only)"
  # keep target/ms_build_id (build/lib/util.ts getElectronVersion reads them)
  # but drop runtime/disturl so node-gyp builds against the local node.
  sed -i '/^disturl=/d; /^runtime=/d' "$DEST/.npmrc"
  # upstream preinstall dereferences the electron target unconditionally on
  # Linux; guard it (fork patch #1). Idempotent.
  python3 - "$DEST/build/npm/preinstall.ts" <<'PYEOF'
import sys
p = sys.argv[1]
s = open(p).read()
old = "if (process.platform === 'linux') {"
new = "if (process.platform === 'linux' && local !== undefined) {"
if old in s and new not in s:
    s = s.replace(old, new, 1)
    s = s.replace("local!.target", "local.target")
    open(p, "w").write(s)
    print("   preinstall.ts guarded")
else:
    print("   preinstall.ts already guarded")
PYEOF
  if command -v rg > /dev/null; then
    RG_CACHE="/tmp/vscode-ripgrep-cache-1.17.1"
    mkdir -p "$RG_CACHE" /tmp/forte-rgpack
    cp "$(command -v rg)" /tmp/forte-rgpack/rg
    tar czf "$RG_CACHE/ripgrep-v15.0.1-x86_64-unknown-linux-musl.tar.gz" -C /tmp/forte-rgpack rg
    echo "   ripgrep cache seeded from system rg"
  fi
  export ELECTRON_SKIP_BINARY_DOWNLOAD=1 PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
  echo "   npm ci must run with: ELECTRON_SKIP_BINARY_DOWNLOAD=1 PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1"
  echo "   note: with runtime/disturl stripped, do NOT re-run npm ci after"
  echo "   restoring them; the target= key then mispoints node-gyp."
fi

cat <<EOF

Bootstrapped. Next, inside $DEST:

  npm ci                                 # upstream deps (native modules)
  npm run compile                        # or: npm run watch
  ./scripts/code-web.sh                  # WEB target — works in the container
  ./scripts/code.sh                      # desktop — needs Electron (dev Mac/CI)

Forte workbench code goes in: src/vs/workbench/contrib/forteDaw/
Milestones: docs/webdaw/09-forte-studio-fork.md (F0..F4)
EOF
