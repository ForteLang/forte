#!/usr/bin/env bash
# Assemble the static listening site — package catalog (landing page) +
# zero-install player + the essentials package (albums included) — and
# publish it to the gh-pages branch. The editor is a local development
# tool (`forte browser`) and is NOT published. One-time setup on GitHub:
#   Settings → Pages → Deploy from a branch → gh-pages / (root)
#
#   scripts/publish_web.sh              # build + push gh-pages
#   DRY_RUN=1 scripts/publish_web.sh    # build only; prints the site dir
#
# The site collapses the repo layout to its root, and the copies are
# REWRITTEN for it: in-repo the pages sit at /forte/web/ and fetch
# ../../packages/…, but a GitHub *project* page serves under /<repo>/ —
# climbing two levels would escape the site. At assembly time every
# ../../packages/ reference in the copies becomes packages/ (same dir).
set -euo pipefail
cd "$(dirname "$0")/.."    # forte/ (the core)

echo "== 1/3 wasm =="
scripts/build_web.sh

echo "== 2/3 site =="
SITE="${SITE_OUT:-$(mktemp -d)}"
cp -r web/. "$SITE"/
# strip the editor and its dev-only assets from the public site — the
# catalog IS the landing page (published songs are listened to, not edited)
rm -f "$SITE"/main.js "$SITE"/storage.js "$SITE"/vcs.js "$SITE"/recorder.js \
      "$SITE"/rec-worker.js "$SITE"/frec.js
cp "$SITE"/catalog.html "$SITE"/index.html
# kill-switch service worker: anyone who opened the previously published
# editor carries its offline cache — clear it and unregister
cat > "$SITE"/sw.js <<'EOF'
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil((async () => {
  for (const k of await caches.keys()) await caches.delete(k);
  await self.registration.unregister();
  for (const c of await self.clients.matchAll({ type: 'window' })) c.navigate(c.url);
})()));
EOF
mkdir -p "$SITE/packages"
cp -r ../packages/essentials_0.6.0 "$SITE/packages/"
cargo run -q -p fortelang --bin forte -- web index > "$SITE/packages.json"
# rewrite the repo-relative package paths for the collapsed layout
sed -i.bak -e 's|\.\./\.\./packages/|packages/|g' -e 's|`\.\./\.\./${rel}`|`${rel}`|g' \
  "$SITE"/*.html "$SITE"/*.js
rm -f "$SITE"/*.bak
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
