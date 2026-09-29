// Compare an HTTPS deployment with the local release build before a domain cutover.
// Usage: node verify-deployment.mjs https://oxfer.app
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

const origin = new URL(process.argv[2]);
assert.equal(origin.protocol, "https:", "deployment must use HTTPS");
assert.equal(origin.href, `${origin.origin}/`, "pass an origin, not a path or share link");

const checks = [
    ["/", "index.html", "text/html"],
    ["/diags", "index.html", "text/html"],
    ["/p2p-transfer.js", "p2p-transfer.js", "javascript"],
    ["/p2p-transfer_bg.wasm", "p2p-transfer_bg.wasm", "application/wasm"],
    ["/sw.js", "sw.js", "javascript"],
    ["/theme.html", "theme.html", "text/html"],
    ["/assets/favicon.js", "assets/favicon.js", "javascript"],
    ["/assets/oxfer-favicon-light.svg", "assets/oxfer-favicon-light.svg", "image/svg+xml"],
    ["/assets/oxfer-favicon-dark.svg", "assets/oxfer-favicon-dark.svg", "image/svg+xml"],
    ["/assets/oxfer-wordmark-light.svg", "assets/oxfer-wordmark-light.svg", "image/svg+xml"],
    ["/assets/oxfer-wordmark-dark.svg", "assets/oxfer-wordmark-dark.svg", "image/svg+xml"],
];
const digest = bytes => createHash("sha256").update(bytes).digest("hex");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function verifyOnce() {
    for (const [path, file, mime] of checks) {
        const response = await fetch(new URL(path, origin), {
            headers: {
                "Cache-Control": "no-cache",
                "User-Agent": "Mozilla/5.0 Oxfer-deployment-check",
            },
            signal: AbortSignal.timeout(30_000),
        });
        assert.equal(response.status, 200, `${path}: HTTP status`);
        assert.ok(response.headers.get("content-type")?.includes(mime), `${path}: MIME type`);
        assert.match(response.headers.get("cache-control") ?? "", /must-revalidate/, `${path}: cache policy`);
        assert.equal(response.headers.get("referrer-policy"), "no-referrer", `${path}: referrer policy`);
        assert.equal(response.headers.get("x-content-type-options"), "nosniff", `${path}: nosniff`);
        if (path === "/sw.js") {
            assert.match(response.headers.get("cache-control"), /no-store/, "service worker must not be cached");
        }
        const local = await readFile(new URL(`./dist/${file}`, import.meta.url));
        const remote = new Uint8Array(await response.arrayBuffer());
        assert.equal(digest(remote), digest(local), `${path}: deployed bytes differ from release build`);
        console.log(`${origin.origin}${path}: matches release build and hosting headers`);
    }
}

let lastError;
for (let attempt = 1; attempt <= 10; attempt += 1) {
    try {
        await verifyOnce();
        lastError = undefined;
        break;
    } catch (error) {
        lastError = error;
        console.error(`attempt ${attempt}/10 failed: ${error.message}`);
        await sleep(3_000);
    }
}
if (lastError) {
    throw lastError;
}
