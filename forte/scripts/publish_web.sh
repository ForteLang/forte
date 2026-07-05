#!/usr/bin/env bash
# Assemble the static listening site — editor + zero-install player +
# package catalog + the essentials package (albums included) — and publish
# it to the gh-pages branch. One-time setup on GitHub afterwards:
#   Settings → Pages → Deploy from a branch → gh-pages / (root)
#
#   scripts/publish_web.sh              # build + push gh-pages
#   DRY_RUN=1 scripts/publish_web.sh    # build only; prints the site dir
#
# Site layout = repo layout collapsed to the root: the web pages fetch
# ../../packages/… which the browser clamps to /packages/… at the root,
# so the SAME files work in-repo and on Pages.
set -euo pipefail
cd "$(dirname "$0")/.."    # forte/ (the core)

echo "== 1/3 wasm =="
scripts/build_web.sh

echo "== 2/3 site =="
SITE="${SITE_OUT:-$(mktemp -d)}"
cp -r web/. "$SITE"/
mkdir -p "$SITE/packages"
cp -r ../packages/essentials_0.6.0 "$SITE/packages/"
cargo run -q -p fortelang --bin forte -- web index > "$SITE/packages.json"
touch "$SITE/.nojekyll"
echo "   site: $SITE"

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "DRY_RUN=1 — push を省略しました(python3 -m http.server -d $SITE で確認できます)"
  exit 0
fi

echo "== 3/3 push gh-pages =="
URL=$(git -C .. remote get-url origin)
(
  cd "$SITE"
  git init -q
  git checkout -q -b gh-pages
  git add -A
  git commit -q -m "publish listening site"
  git push -f "$URL" gh-pages
)
echo "OK: gh-pages を更新しました → Settings → Pages で gh-pages / (root) を選ぶと公開されます"
