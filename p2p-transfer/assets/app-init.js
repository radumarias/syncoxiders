// Module entry point for the wasm app, loaded by index.html after the classic
// assets/boot.js. Trunk.toml sets inject_scripts = false, so this file replaces
// Trunk's inline loader and index.html carries no inline script; the
// Content-Security-Policy can then use script-src 'self' 'wasm-unsafe-eval'.
// index.html preloads both files below; paths resolve against this module's
// URL (/assets/app-init.js), so they reach the dist root.
const loadFailed = 'Oxfer could not start. See the developer console for details.';

try {
    // A dynamic import keeps a failed script fetch inside this try block.
    const { default: init } = await import('../p2p-transfer.js');
    await init({ module_or_path: new URL('../p2p-transfer_bg.wasm', import.meta.url) });
} catch (error) {
    const loading = document.getElementById('loading_text');
    if (loading) {
        // Plain text only: never interpret an error message as HTML.
        loading.textContent = loadFailed;
    }
    throw error;
}
