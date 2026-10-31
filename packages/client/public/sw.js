// Service worker: makes the app start offline and fast. It caches the static app shell only. It never caches
// /api/*, the installer, OAuth callbacks, cross-origin requests or anything with a query string, and it holds no
// tokens: the GitHub token and device keys live in localStorage/IndexedDB, not here.
const VERSION = 'v2';
const SHELL = `termenbox-shell-${VERSION}`;
const ASSETS = `termenbox-assets-${VERSION}`;
const STATIC = /^\/(favicon\.svg|manifest\.webmanifest|icons\/[\w.-]+)$/;

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL).then((c) => c.add('/')).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== SHELL && k !== ASSETS).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

const withTimeout = (promise, ms) =>
  Promise.race([promise, new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), ms))]);

// Web Push (decision 0095, U11). The payload is {session, kind, at, tag, deep_link} and nothing
// an agent wrote; the words are ours. A newer notification for the same session replaces the older by `tag`.
const WORDS = { needs_input: 'needs you', finished: 'finished', failed: 'failed', attention: 'wants you', update: 'has an update' };
self.addEventListener('push', (event) => {
  let data = {};
  try { data = event.data ? event.data.json() : {}; } catch { /* Show the generic text. */ }
  const name = typeof data.session === 'string' ? data.session.slice(0, 64) : 'A workspace';
  const word = WORDS[data.kind] ?? WORDS.attention;
  // Only a path inside this app is followed, whatever the payload says.
  const deepLink = typeof data.deep_link === 'string' && data.deep_link.startsWith('/#/') ? data.deep_link.slice(0, 512) : '/';
  event.waitUntil(
    self.registration.showNotification(`${name} ${word}`, {
      tag: typeof data.tag === 'string' ? data.tag.slice(0, 64) : name,
      renotify: true,
      icon: '/icons/icon-192.png',
      data: {
        session: typeof data.session === 'string' ? data.session : '',
        deep_link: deepLink,
      },
    }),
  );
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const deepLink = event.notification.data?.deep_link ?? '/';
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((list) => {
      for (const c of list) {
        if ('focus' in c) {
          // Only the fragment changes, so the running app hears a hashchange and keeps its connections.
          return c.navigate(deepLink).then(() => c.focus(), () => c.focus());
        }
      }
      return self.clients.openWindow(deepLink);
    }),
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  const url = new URL(req.url);
  if (req.method !== 'GET' || url.origin !== self.location.origin) return;

  // Page loads: the network first (so a new deploy shows at once), the cached shell when offline or slow.
  if (req.mode === 'navigate') {
    event.respondWith(
      (async () => {
        try {
          const res = await withTimeout(fetch(req), 4000);
          if (res.ok && url.pathname === '/' && !url.search) {
            const copy = res.clone();
            event.waitUntil(caches.open(SHELL).then((c) => c.put('/', copy)));
          }
          return res;
        } catch {
          return (await caches.match('/', { cacheName: SHELL })) ?? Response.error();
        }
      })(),
    );
    return;
  }

  // Hashed build output never changes under the same name: cache first.
  if (url.pathname.startsWith('/assets/') && !url.search) {
    event.respondWith(
      caches.open(ASSETS).then(async (cache) => {
        const hit = await cache.match(req);
        if (hit) return hit;
        const res = await fetch(req);
        if (res.ok) cache.put(req, res.clone());
        return res;
      }),
    );
    return;
  }

  // Icons and the manifest: show the cached copy, refresh it in the background.
  if (STATIC.test(url.pathname) && !url.search) {
    event.respondWith(
      caches.open(ASSETS).then(async (cache) => {
        const hit = await cache.match(req);
        const refresh = fetch(req)
          .then((res) => {
            if (res.ok) cache.put(req, res.clone());
            return res;
          })
          .catch(() => hit);
        return hit ?? refresh;
      }),
    );
  }
});
