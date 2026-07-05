// Service worker: precache the whole editor so it works fully offline
// (SYS-NFR-001 — the Hub is for sharing, never a dependency for composing).
const CACHE = 'forte-v10';
const ASSETS = [
  './',
  './index.html',
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
  '../packages/essentials_0.6.0/songs/first-light.forte',
  '../packages/essentials_0.6.0/songs/slow-circles.forte',
  '../packages/essentials_0.6.0/songs/night-parade.forte',
  '../packages/essentials_0.6.0/songs/handmade.forte',
  '../packages/essentials_0.6.0/songs/devices/warm.forte',
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

// cache-first with network refresh: instant + offline, updates in background
self.addEventListener('fetch', (e) => {
  if (e.request.method !== 'GET') return;
  e.respondWith(
    caches.match(e.request).then((hit) => {
      const refresh = fetch(e.request)
        .then((res) => {
          if (res.ok) {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put(e.request, copy));
          }
          return res;
        })
        .catch(() => hit);
      return hit || refresh;
    })
  );
});
