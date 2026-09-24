// Reachability checks only: a WebSocket opening does not prove that a peer can be
// dialled or that the relay accepted an iroh session. Never send a transfer link here.
export function probeRelay(url, timeoutMs = 6000) {
    return new Promise((resolve) => {
        let socket;
        let timer;
        let finished = false;
        const finish = (status) => {
            if (finished) return;
            finished = true;
            clearTimeout(timer);
            if (socket) {
                socket.onopen = socket.onerror = socket.onclose = null;
                try { socket.close(); } catch (_) { /* The result is already settled. */ }
            }
            resolve(status);
        };
        try {
            // Match the iroh 1.1 relay client's WebSocket subprotocol offer.
            // An unversioned WebSocket probe is rejected even on a healthy relay.
            socket = new WebSocket(url, ["iroh-relay-v1", "iroh-relay-v2"]);
            socket.onopen = () => finish("WebSocket opened");
            socket.onerror = () => finish("WebSocket failed (browser hides the reason)");
            socket.onclose = () => finish("WebSocket closed before opening");
            timer = setTimeout(() => finish("WebSocket timed out"), timeoutMs);
        } catch (_) {
            finish("WebSocket could not be created");
        }
    });
}

export async function browserDiagnostics(relays) {
    const lines = [
        `Time: ${new Date().toISOString()}`,
        `Browser: ${navigator.userAgent}`,
        `Secure context: ${window.isSecureContext ? "yes" : "no"}`,
        `Browser reports online: ${navigator.onLine ? "yes" : "no"}`,
        `WebRTC available: ${typeof RTCPeerConnection === "function" ? "yes" : "no"}`,
        `Origin-private storage available: ${typeof navigator.storage?.getDirectory === "function" ? "yes" : "no"}`,
    ];
    const urls = relays.split(",").filter(Boolean);
    const results = await Promise.all(urls.map((relay) => probeRelay(relay)));
    urls.forEach((relay, index) => lines.push(`Relay ${relay}: ${results[index]}`));
    return lines.join("\n");
}
