// Ferryx Remote Service Worker - offline shell cache and phone alert (Web Push) display.
// The shell is network-first so a rebuilt client reaches an installed phone on its next load;
// cached hashed files under /assets are immutable and stay cache-first.
const CACHE_NAME = 'ferryx-remote-v2';
const ASSETS_TO_CACHE = [
  '/',
  '/index.html',
  '/manifest.webmanifest',
  '/ferryx-icon.png'
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(ASSETS_TO_CACHE))
  );
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))
      )
    )
  );
  self.clients.claim();
});

self.addEventListener('fetch', (event) => {
  if (event.request.method !== 'GET' || event.request.url.includes('/api/')) {
    return;
  }
  if (event.request.mode === 'navigate' || event.request.destination === 'document') {
    event.respondWith(
      fetch(event.request)
        .then(async (response) => {
          if (response && response.ok) {
            const cached = await caches.open(CACHE_NAME);
            await cached.put(event.request, response.clone());
          }
          return response;
        })
        .catch(async () => {
          const cached = await caches.match(event.request);
          if (cached) return cached;
          const shell = await caches.match('/index.html');
          return shell || Response.error();
        })
    );
    return;
  }
  event.respondWith(
    caches.match(event.request).then((cachedResponse) => {
      return cachedResponse || fetch(event.request);
    })
  );
});

// Phone alerts. The gateway's payload (src-tauri/src/remote/push.rs AgentStateNotification)
// carries only the agent name, worktree, state, title and body; `agent_id` is the backend
// session id, so one notification per session is kept (tag) and a tap opens that session.
const TEST_ALERT_TAG = 'ferryx-test';

function sessionUrl(sessionId) {
  return sessionId ? '/#session=' + encodeURIComponent(sessionId) : '/';
}

function notificationFromPush(payload) {
  const data = payload && typeof payload === 'object' ? payload : {};
  const sessionId = typeof data.agent_id === 'string' ? data.agent_id : '';
  return {
    title: typeof data.title === 'string' && data.title ? data.title : 'Ferryx',
    options: {
      body: typeof data.body === 'string' ? data.body : '',
      tag: sessionId || TEST_ALERT_TAG,
      renotify: true,
      icon: '/icon-192.png',
      badge: '/icon-192.png',
      data: { sessionId, state: typeof data.state === 'string' ? data.state : '', url: sessionUrl(sessionId) },
    },
  };
}

function readPushPayload(event) {
  if (!event.data) return {};
  try {
    return event.data.json();
  } catch (_) {
    return { body: event.data.text() };
  }
}

self.addEventListener('push', (event) => {
  const { title, options } = notificationFromPush(readPushPayload(event));
  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const data = event.notification.data || {};
  const sessionId = typeof data.sessionId === 'string' ? data.sessionId : '';
  const url = new URL(sessionUrl(sessionId), self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((windows) => {
      const open = windows.find((client) => new URL(client.url).origin === self.location.origin);
      if (open) {
        if (sessionId) open.postMessage({ type: 'ferryx:open-session', sessionId });
        return open.focus();
      }
      return self.clients.openWindow(url);
    })
  );
});
