// Basic service worker for offline caching
const CACHE_NAME = 'gamiscreen-cache-v1';
const OFFLINE_URL = '/';

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await cache.addAll([
        '/',
        '/index.html',
        '/manifest.webmanifest',
      ]);
      self.skipWaiting();
    })()
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(
        keys.map((key) => key !== CACHE_NAME && caches.delete(key))
      );
      self.clients.claim();
    })()
  );
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  // Only handle GET
  if (request.method !== 'GET') return;

  event.respondWith(
    (async () => {
      try {
        // Network-first for navigation requests
        if (request.mode === 'navigate') {
          const fresh = await fetch(request);
          const cache = await caches.open(CACHE_NAME);
          cache.put(request, fresh.clone());
          return fresh;
        }
        // Try cache first for others
        const cached = await caches.match(request);
        return (
          cached ||
          fetch(request).then(async (resp) => {
            const cache = await caches.open(CACHE_NAME);
            cache.put(request, resp.clone());
            return resp;
          })
        );
      } catch (e) {
        // Offline fallback
        if (request.mode === 'navigate') {
          const cache = await caches.open(CACHE_NAME);
          const cached = await cache.match(OFFLINE_URL);
          if (cached) return cached;
        }
        throw e;
      }
    })()
  );
});

