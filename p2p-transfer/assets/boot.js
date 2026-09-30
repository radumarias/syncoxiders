// Classic synchronous boot script, loaded from index.html before the wasm
// module (assets/app-init.js). Kept out of index.html so the
// Content-Security-Policy needs no 'unsafe-inline' script source.
// Capture the full href before the wasm app can scrub the fragment
// (it removes the ticket/cap once it has read them, §4.7.1).
const orig = location.href;
const flags = location.hash.slice(1).split('&');

if ('serviceWorker' in navigator) {
    if (flags.includes('dev')) {
        // #dev is a hard reset: unregister every worker and clear
        // every cache so a stale cache-first build can never shadow
        // this load (Trunk.toml's filehash = false means the wasm
        // bundle's filename never changes between builds).
        const wasControlled = !!navigator.serviceWorker.controller;
        Promise.all([
            navigator.serviceWorker.getRegistrations()
                .then(rs => Promise.all(rs.map(r => r.unregister()))),
            caches.keys()
                .then(ks => Promise.all(ks.map(k => caches.delete(k)))),
        ]).then(() => {
            if (wasControlled) {
                // Reloads at most once: after unregister, there is no
                // controller left, so this branch is not re-entered.
                // Restore the original href immediately before
                // reloading — replaceState + reload run in one
                // synchronous task, so the wasm's fragment scrub can
                // never interleave between them.
                history.replaceState(null, '', orig);
                location.reload();
            }
        });
    } else {
        // Start before Save can be clicked. The streaming sink waits for
        // this exact registration to control the page, including first visit.
        window.p2pServiceWorkerRegistration = navigator.serviceWorker.register(
            new URL('sw.js', document.baseURI), { scope: new URL('.', document.baseURI).href }
        );
        // Keep the rejected promise for actionable sink errors, without
        // an unhandled rejection when the user never starts a download.
        window.p2pServiceWorkerRegistration.catch(error => console.warn('Service worker unavailable:', error));
    }
}
