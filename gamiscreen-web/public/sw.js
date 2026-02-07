// Basic service worker for offline caching
const CACHE_NAME = 'gamiscreen-cache-v2';
const OFFLINE_URL = 'index.html';

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await cache.addAll([
        'index.html',
        'manifest.webmanifest',
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
  const url = new URL(request.url);
  const isApiRequest = url.pathname.includes('/api/');
  const isSseRequest =
    request.headers.get('accept') === 'text/event-stream' || url.pathname.endsWith('/sse');

  event.respondWith(
    (async () => {
      try {
        if (isApiRequest || isSseRequest) {
          // Always go to network for API calls (no caching) to avoid stale data
          return await fetch(request);
        }
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

try {
  importScripts('notification-format.js');
} catch (err) {
  console.error('Service worker failed to load notification formatter', err);
}

self.addEventListener('push', (event) => {
  const data = (() => {
    try {
      if (!event.data) return null;
      return event.data.json();
    } catch {
      try {
        const text = event.data?.text?.();
        return text ? { title: 'Gamiscreen', body: text } : null;
      } catch {
        return null;
      }
    }
  })();

  if (!data) return;

  const promise = (async () => {
    const formatter = self.__gamiscreenFormatNotification;
    const formatted =
      typeof formatter === 'function' ? formatter(data) : null;

    const title = formatted?.title || data.title || 'Gamiscreen';
    const body = formatted?.body || data.body || '';
    const url = formatted?.url || data.url || '#status';

    if (!body) return;

    await self.registration.showNotification(title, {
      body,
      icon: '/icons/icon-192x192.png',
      badge: '/icons/icon-192x192.png',
      tag: data.type ? `gamiscreen-${data.type}` : undefined,
      renotify: false,
      data: { url },
    });
  })();

  event.waitUntil(promise);
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const fallbackUrl = (() => {
    try {
      return new URL('/#status', self.registration.scope).href;
    } catch {
      return '#status';
    }
  })();

  const targetUrl = (() => {
    const rawTarget = event.notification?.data?.url;
    if (!rawTarget || typeof rawTarget !== 'string') return fallbackUrl;
    // Absolute URLs (https:, http:, mailto:, etc.) should be used as-is.
    if (/^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(rawTarget)) return rawTarget;
    try {
      if (rawTarget.startsWith('#')) {
        return new URL(`/${rawTarget}`, self.registration.scope).href;
      }
      if (rawTarget.startsWith('/')) {
        return new URL(rawTarget, self.registration.scope).href;
      }
      return fallbackUrl;
    } catch {
      return fallbackUrl;
    }
  })();

  const openClient = async () => {
    const allClients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
    if (allClients.length > 0) {
      const client = allClients.find((c) => 'focus' in c) || allClients[0];
      await client.focus();
      if (targetUrl && 'navigate' in client) {
        try {
          await client.navigate(targetUrl);
        } catch {
          // ignore navigation failures
        }
      }
      return;
    }
    if (targetUrl) {
      await self.clients.openWindow(targetUrl);
    }
  };

  event.waitUntil(openClient());
});
