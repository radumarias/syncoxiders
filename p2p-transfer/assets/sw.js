// Service worker for p2p-transfer.
//
// Network-first for the app shell: Trunk.toml sets `filehash = false`, so
// `p2p-transfer_bg.wasm` never changes filename between builds — a
// cache-first shell would shadow every later `trunk serve`/`trunk build`
// forever. See CLAUDE.md and design §4.10.1.
const cacheName = 'p2p-transfer-v2';
const shellSuffixes = ['/', '/index.html', '/p2p-transfer.js', '/p2p-transfer_bg.wasm'];

/* Take over immediately; activate() clears stale caches before this worker
   starts controlling pages. */
self.addEventListener('install', () => {
  self.skipWaiting();
});

/* Drop every cache from a previous version and start controlling all open
   tabs right away. */
self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== cacheName).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

/* Reserved for the SW streaming download route (§4.10.2, filled in T6b).
   Matched strictly on the path prefix so a ticket fragment or query string
   can never be mistaken for it — the URL fragment never reaches the SW
   anyway. */
function isDownloadRoute(pathname) {
  return pathname === '/download' || pathname.startsWith('/download/');
}

function isShellRequest(request, pathname) {
  if (request.mode === 'navigate') return true;
  return shellSuffixes.some((suffix) => pathname.endsWith(suffix));
}

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);

  if (isDownloadRoute(url.pathname)) {
    // No body yet: T6b implements the streaming response here. Not calling
    // respondWith() falls through to the browser's normal network fetch, so
    // this route stays inert (and never cached) until then.
    return;
  }

  if (isShellRequest(e.request, url.pathname)) {
    e.respondWith(
      fetch(e.request)
        .then((response) => {
          const copy = response.clone();
          caches.open(cacheName).then((cache) => cache.put(e.request, copy));
          return response;
        })
        .catch(() => caches.match(e.request))
    );
    return;
  }

  // Everything else (icons, fonts, manifest): cache-first, as before.
  e.respondWith(
    caches.match(e.request).then((response) => response || fetch(e.request))
  );
});
