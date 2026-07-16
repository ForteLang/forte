// Service worker: precache the whole editor so it works fully offline
// (SYS-NFR-001 — sharing is packages on GitHub, never a dependency for composing).
const CACHE = 'forte-v17';
const ASSETS = [
  './',
  './index.html',
  './catalog.html',
  './player.html',
  './main.js',
  './viz.js',
  './storage.js',
  './vcs.js',
  './logo.svg',
  './worklet.js',
  './recorder.js',
  './rec-worker.js',
  './frec.js',
  './forte.wasm',
  '../../packages/essentials_0.6.0/songs/first-light.forte',
  '../../packages/essentials_0.6.0/songs/slow-circles.forte',
  '../../packages/essentials_0.6.0/songs/night-parade.forte',
  '../../packages/essentials_0.6.0/songs/handmade.forte',
  '../../packages/essentials_0.6.0/songs/devices/warm.forte',
];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(ASSETS)));
  self.skipWaiting();
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

// NETWORK-FIRST with cache fallback: the app's own files change with every
// build, and serving a stale main.js against a fresh forte.wasm (or vice
// versa) breaks boot. Fresh when online, cached when offline.
self.addEventListener('fetch', (e) => {
  if (e.request.method !== 'GET') return;
  // the project API (forte daw) is live state — never serve it from cache
  if (new URL(e.request.url).pathname.includes('/api/')) return;
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok) {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(e.request, copy));
        }
        return res;
      })
      .catch(() => caches.match(e.request))
  );
});
