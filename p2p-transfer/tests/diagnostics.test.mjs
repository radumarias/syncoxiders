import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../assets/diagnostics.js", import.meta.url), "utf8");
const { probeRelay, browserDiagnostics } = await import(
    `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);

test("a relay connection opening reports success and closes the probe socket", async () => {
    let closed = false;
    globalThis.WebSocket = class {
        constructor(url, protocols) {
            assert.deepEqual(protocols, ["iroh-relay-v1", "iroh-relay-v2"]);
            queueMicrotask(() => this.onopen());
        }
        close() { closed = true; }
    };
    assert.equal(await probeRelay("wss://example.test/relay"), "WebSocket opened");
    assert.equal(closed, true);
});

test("an unreachable relay settles without waiting forever", async () => {
    let closed = false;
    globalThis.WebSocket = class {
        close() { closed = true; }
    };
    assert.equal(await probeRelay("wss://example.test/relay", 1), "WebSocket timed out");
    assert.equal(closed, true);
});

test("report includes only browser facts and selected relay checks", async () => {
    Object.defineProperty(globalThis, "navigator", {
        configurable: true,
        value: { userAgent: "TestBrowser", onLine: true, storage: {} },
    });
    globalThis.window = { isSecureContext: true };
    globalThis.WebSocket = class {
        constructor(url) {
            assert.equal(url, "wss://example.test/relay");
            queueMicrotask(() => this.onerror());
        }
        close() {}
    };
    const report = await browserDiagnostics("wss://example.test/relay");
    assert.match(report, /Browser: TestBrowser/);
    assert.match(report, /Relay wss:\/\/example.test\/relay: WebSocket failed/);
    assert.doesNotMatch(report, /endpoint|capability|filename/i);
});
