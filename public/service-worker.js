const CACHE_NAME = 'individuate-public-shell-v1';
const PUBLIC_ASSETS = [
  '/manifest.webmanifest',
  '/icons/icon.svg',
  '/icons/apple-touch-icon.png',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/icon-maskable-512.png',
  '/pkg/individuateai.css?v=20260714-ios-pwa',
  '/passkey.js?v=20260714-largeblob'
];

self.addEventListener('install', event => {
  event.waitUntil(
    caches.open(CACHE_NAME)
      .then(cache => cache.addAll(PUBLIC_ASSETS))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener('activate', event => {
  event.waitUntil(
    caches.keys()
      .then(keys => Promise.all(keys.filter(key => key !== CACHE_NAME).map(key => caches.delete(key))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', event => {
  if (event.request.method !== 'GET') return;
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;

  const isPublicAsset = PUBLIC_ASSETS.some(asset => {
    const cachedUrl = new URL(asset, self.location.origin);
    return cachedUrl.pathname === url.pathname && cachedUrl.search === url.search;
  });
  if (!isPublicAsset) return;

  event.respondWith(
    caches.match(event.request).then(cached => cached || fetch(event.request))
  );
});
