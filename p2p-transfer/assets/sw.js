// Service worker for Oxfer.
//
// Network-first for the app shell: Trunk.toml sets `filehash = false`, so
// `p2p-transfer_bg.wasm` never changes filename between builds — a
// cache-first shell would shadow every later `trunk serve`/`trunk build`
// forever. See CLAUDE.md and design §4.10.1.
const cacheName = 'oxfer-v3';
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

// Download URLs are unguessable, scope-relative, single-use capabilities. They
// are handled before the app-shell cache (including missing/expired routes).
const downloadPrefix = new URL('download/', self.registration.scope);
const protocol = 'p2p-download-v1';
const maxChunk = 256 * 1024;
const sessions = new Map();

function isDownloadRoute(url) {
  return url.origin === downloadPrefix.origin && url.pathname.startsWith(downloadPrefix.pathname);
}

function contentDisposition(name) {
  // RFC 6266 / RFC 8187: ASCII quoted fallback plus UTF-8 filename*. Strip
  // controls, paths, quotes and percent escapes from the fallback; never splice
  // untrusted input directly into an HTTP header.
  const safe = name.replace(/[\u0000-\u001f\u007f/\\]/g, '_').slice(0, 255) || 'file.bin';
  const ascii = safe.replace(/[^\x20-\x7e]|["%]/g, '_');
  const encoded = encodeURIComponent(safe.toWellFormed())
    .replace(/['()*]/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
  return `attachment; filename="${ascii}"; filename*=UTF-8''${encoded}`;
}

function openDownload(event, message) {
  const port = event.ports[0];
  if (!port) return;
  const reject = text => { port.postMessage({ op: 'error', message: text }); port.close(); };
  const size = Number(message.size);
  if (!/^[0-9a-f]{64}$/.test(message.token) || sessions.has(message.token) ||
      typeof message.name !== 'string' || message.name.length > 1024 ||
      typeof message.size !== 'string' || !/^(0|[1-9][0-9]*)$/.test(message.size) ||
      !Number.isSafeInteger(size) || size < 0 || sessions.size >= 64) {
    reject('Invalid download metadata or too many active downloads');
    return;
  }
  let controller;
  let pullDone;
  let credit = false;
  let fetched = false;
  let ended = false;
  let received = 0;
  let sequence = 0;
  let lastPing = Date.now();
  let releaseLifetime;
  const lifetime = new Promise(resolve => { releaseLifetime = resolve; });
  const send = message => port.postMessage(message);
  const cleanup = () => {
    ended = true;
    sessions.delete(message.token);
    clearInterval(watchdog);
    port.onmessage = null;
    port.onmessageerror = null;
    port.close();
    pullDone?.();
    pullDone = null;
    releaseLifetime();
  };
  const fail = reason => {
    if (ended) return;
    send({ op: 'error', message: reason });
    controller?.error(new Error(reason));
    cleanup();
  };
  const watchdog = setInterval(() => {
    if (Date.now() - lastPing > 45_000) fail('The receiving page stopped responding');
  }, 15_000);
  let stream;
  try {
    stream = new ReadableStream({
      start(value) { controller = value; },
      pull() {
        if (ended) return;
        // HWM=0: pull means an actual consumer is waiting, not spare capacity
        // in an unbounded queue. Only one credit, one <=256KiB chunk, one ack.
        credit = true;
        send({ op: 'demand' });
        return new Promise(resolve => { pullDone = resolve; });
      },
      cancel() { fail('The browser cancelled the download'); },
    }, { highWaterMark: 0, size: bytes => bytes.byteLength });
  } catch (error) {
    fail(`Response streaming unavailable: ${error.message}`);
    return;
  }
  port.onmessageerror = () => fail('Unreadable download message');
  port.onmessage = event => {
    if (ended) return;
    const data = event.data;
    if (data?.op === 'abort') { fail('The receiving page cancelled the download'); return; }
    if (data?.op === 'end') {
      if (!fetched || received !== size) { fail('Download length did not match the manifest'); return; }
      controller.close();
      send({ op: 'ended' });
      cleanup();
      return;
    }
    if (data?.op !== 'chunk' || !fetched || !credit ||
        data.sequence !== sequence + 1 || !(data.bytes instanceof Uint8Array) ||
        !data.bytes.byteLength || data.bytes.byteLength > maxChunk ||
        data.bytes.byteLength > size - received) {
      fail('Unexpected or oversized download chunk');
      return;
    }
    credit = false;
    try {
      controller.enqueue(data.bytes);
      received += data.bytes.byteLength;
      sequence = data.sequence;
      // Never acknowledge merely receiving a port message.
      send({ op: 'ack', sequence });
      const resolve = pullDone;
      pullDone = null;
      resolve?.();
    } catch (error) {
      fail(`Download stream failed: ${error.message}`);
    }
  };
  const url = new URL(message.token, downloadPrefix).href;
  sessions.set(message.token, {
    clientId: event.source?.id,
    ping() { lastPing = Date.now(); send({ op: 'pong' }); },
    response() {
      if (fetched || ended) return new Response('Download already used', { status: 410 });
      fetched = true;
      send({ op: 'started' });
      return new Response(stream, { headers: {
        'Content-Type': 'application/octet-stream',
        'Content-Length': message.size,
        'Content-Disposition': contentDisposition(message.name),
        'Cache-Control': 'no-store',
        'X-Content-Type-Options': 'nosniff',
      } });
    },
    lifetime,
  });
  send({ op: 'ready', url });
  // Browsers may still terminate long-running workers. The page has deadlines
  // and controller-change detection; it must never replay uncertain writes.
  event.waitUntil(lifetime);
}

self.addEventListener('message', event => {
  const message = event.data;
  if (message?.type !== protocol) return;
  if (message.op === 'open') openDownload(event, message);
  if (message.op === 'ping') {
    const session = sessions.get(message.token);
    if (session && session.clientId === event.source?.id) session.ping();
  }
});

function isShellRequest(request, pathname) {
  if (request.mode === 'navigate') return true;
  return shellSuffixes.some((suffix) => pathname.endsWith(suffix));
}

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);

  if (isDownloadRoute(url)) {
    const token = url.pathname.slice(downloadPrefix.pathname.length);
    const session = !url.search && e.request.method === 'GET' && sessions.get(token);
    e.respondWith(session ? session.response() : new Response('Download expired or unknown', { status: 410 }));
    if (session) e.waitUntil(session.lifetime);
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
