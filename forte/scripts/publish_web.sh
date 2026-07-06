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
# every package ships — the catalog lists them all
for pkg in ../packages/*/; do cp -r "$pkg" "$SITE/packages/"; done
cargo run -q -p fortelang --bin forte -- web index > "$SITE/packages.json"
# rewrite the repo-relative package paths for the collapsed layout
sed -i.bak -e 's|\.\./\.\./packages/|packages/|g' -e 's|`\.\./\.\./${rel}`|`${rel}`|g' \
  "$SITE"/*.html "$SITE"/*.js
rm -f "$SITE"/*.bak
touch "$SITE/.nojekyll"

# share pages: one static page per album with OGP tags (crawlers read the
# meta and the cover; humans are redirected straight into the player).
# These URLs are what you paste into Slack/social — the album unfurls.
# owner/repo = the remote URL's last two path segments (robust to proxies
# and scp-style remotes); override with PAGES_ORIGIN if hosting elsewhere
OWNER_REPO=$(git -C .. remote get-url origin | sed -E 's#\.git$##; s#:#/#g' | awk -F/ '{print $(NF-1)"/"$NF}' | tr 'A-Z' 'a-z')
ORIGIN="${PAGES_ORIGIN:-https://${OWNER_REPO%%/*}.github.io/${OWNER_REPO##*/}}"
python3 - "$SITE" "$ORIGIN" <<'PY'
import json, html, sys, os, urllib.parse
site, origin = sys.argv[1], sys.argv[2].rstrip('/')
data = json.load(open(os.path.join(site, 'packages.json')))
os.makedirs(os.path.join(site, 'share'), exist_ok=True)
for pkg in data['packages']:
    for a in pkg.get('albums', []):
        base = f"packages/{pkg['dir']}/albums/{a['dir']}"
        q = '&'.join('src=' + urllib.parse.quote(f'{base}/{t}', safe='') for t in a['tracks'])
        if a.get('cover'):
            q += '&cover=' + urllib.parse.quote(f"{base}/{a['cover']}", safe='')
        player = f'../player.html?{q}'
        title = html.escape(f"{a['title']} — {a.get('artist') or pkg['name']}")
        desc = html.escape(a.get('desc', ''))
        cover = f"{origin}/{base}/{a['cover']}" if a.get('cover') else ''
        page = f"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8">
<title>{title}</title>
<meta property="og:type" content="music.album">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{desc}">
<meta property="og:url" content="{origin}/share/{a['dir']}.html">
{f'<meta property="og:image" content="{cover}">' if cover else ''}
<meta name="twitter:card" content="summary_large_image">
<meta http-equiv="refresh" content="0; url={player}">
<script>location.replace({json.dumps(player)});</script>
</head><body>
<p><a href="{player}">▶ Play {title} in your browser</a> — what arrives is source code, not audio; your browser performs the deterministic render.</p>
</body></html>"""
        open(os.path.join(site, 'share', f"{a['dir']}.html"), 'w').write(page)
        print(f"   share: share/{a['dir']}.html")
PY
echo "   site: $SITE"

if [ "${DRY_RUN:-0}" = "1" ]; then
  echo "DRY_RUN=1 — push を省略しました(python3 -m http.server -d $SITE で確認できます)"
  exit 0
fi

echo "== 3/3 push gh-pages =="
URL=$(git -C .. remote get-url origin)
# author the site commit as the repo's configured user (the temp repo has
# no config of its own and would fall back to the machine default)
NAME=$(git -C .. config user.name)
EMAIL=$(git -C .. config user.email)
(
  cd "$SITE"
  git init -q
  git checkout -q -b gh-pages
  git add -A
  git -c user.name="$NAME" -c user.email="$EMAIL" commit -q -m "publish listening site"
  git push -f "$URL" gh-pages
)
echo "OK: gh-pages を更新しました → Settings → Pages で gh-pages / (root) を選ぶと公開されます"
