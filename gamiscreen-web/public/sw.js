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
    const title = data.title || 'Gamiscreen';
    let body = data.body || '';
    let url = '#status';

    if (data.type === 'pending_count' && typeof data.count === 'number') {
      body =
        data.count > 0
          ? `${data.count} notification${data.count === 1 ? '' : 's'} pending.`
          : 'All notifications resolved.';
      url = '#notifications';
    } else if (data.type === 'remaining_updated') {
      const minutes = data.remaining_minutes;
      const child = data.child_id;
      if (typeof minutes === 'number') {
        body =
          minutes >= 0
            ? `Remaining time for ${child || 'child'}: ${minutes} minute${minutes === 1 ? '' : 's'}.`
            : `${child || 'Child'} is out of time.`;
      }
      if (child) {
        url = `#child/${encodeURIComponent(child)}`;
      }
    }

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
  const targetUrl = event.notification?.data?.url || '#status';

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
