// A screen wake lock is best effort. It can prevent *automatic* display sleep
// while the transfer tab is visible; it cannot run WebRTC or WASM behind a
// manually locked screen, a hidden tab, or an OS-suspended browser.
let wanted = false;
let sentinel = null;
let pending = null;
let status = 'off';

function supported() {
  return globalThis.isSecureContext && typeof navigator?.wakeLock?.request === 'function';
}

async function release() {
  const previous = sentinel;
  sentinel = null;
  if (previous) {
    try { await previous.release(); } catch (_) { /* already released */ }
  }
}

async function acquire(fromUserGesture = false) {
  if (!wanted || sentinel || document.visibilityState !== 'visible') return;
  if (!supported()) { status = 'unsupported'; return; }
  // A new user gesture must be used *now*, not after an older pending request
  // settles (which could be after WebKit's transient activation expires).
  if (pending && !fromUserGesture) return;
  status = 'requesting';
  let request;
  try {
    request = navigator.wakeLock.request('screen');
  } catch (_) {
    status = 'denied';
    return;
  }
  pending = request;
  try {
    const lock = await request;
    if (!wanted || document.visibilityState !== 'visible') {
      try { await lock.release(); } catch (_) { /* already released */ }
      return;
    }
    if (sentinel) {
      try { await lock.release(); } catch (_) { /* already released */ }
      return;
    }
    sentinel = lock;
    lock.addEventListener('release', () => {
      if (sentinel !== lock) return;
      sentinel = null;
      status = wanted ? 'released' : 'off';
      // Do not spin on a browser/OS revocation. A new user gesture or
      // visibilitychange can try again.
    }, { once: true });
    // A sentinel can be auto-released before its request promise resumes.
    // The release event might already have fired before the listener above.
    if (lock.released) {
      sentinel = null;
      status = 'released';
    } else {
      status = 'active';
    }
  } catch (_) {
    if (pending === request && !sentinel) status = wanted ? 'denied' : 'off';
  } finally {
    if (pending === request) pending = null;
  }
}

export function setTransferWakeLock(active) {
  if (wanted === Boolean(active)) return;
  wanted = Boolean(active);
  if (wanted) {
    void acquire();
  } else {
    status = 'off';
    void release();
  }
}

// Call directly from a click/change event. WebKit may require a transient user
// activation for the first request, even if the app already wants a lock.
export function retryTransferWakeLock() {
  wanted = true;
  void acquire(true);
}

export function transferWakeLockStatus() {
  if (!wanted) return 'off';
  if (document.visibilityState !== 'visible') return 'suspended';
  return status;
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    if (wanted) void acquire(true);
  } else {
    status = wanted ? 'suspended' : 'off';
    void release();
  }
});
