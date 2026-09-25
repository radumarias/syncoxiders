import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../assets/wake-lock.js", import.meta.url), "utf8");
let sequence = 0;

async function fixture(request) {
    const listeners = new Map();
    let visibility = "visible";
    globalThis.isSecureContext = true;
    globalThis.document = {
        get visibilityState() { return visibility; },
        addEventListener(type, callback) { listeners.set(type, callback); },
    };
    Object.defineProperty(globalThis, "navigator", {
        configurable: true,
        value: { wakeLock: { request } },
    });
    const api = await import(
        `data:text/javascript;base64,${Buffer.from(source).toString("base64")}#${sequence++}`
    );
    return {
        api,
        visibility(state) {
            visibility = state;
            listeners.get("visibilitychange")();
        },
    };
}

const settled = () => new Promise(resolve => setImmediate(resolve));

function sentinel() {
    let released = false;
    let releaseCalls = 0;
    let notify;
    return {
        get released() { return released; },
        get releaseCalls() { return releaseCalls; },
        addEventListener(type, callback) {
            assert.equal(type, "release");
            notify = callback;
        },
        async release() {
            releaseCalls += 1;
            if (!released) {
                released = true;
                notify?.();
            }
        },
        revoke() {
            released = true;
            notify?.();
        },
    };
}

test("one request per active visible session, release on hide/stop, reacquire on return", async () => {
    const locks = [];
    const { api, visibility } = await fixture(async type => {
        assert.equal(type, "screen");
        const lock = sentinel();
        locks.push(lock);
        return lock;
    });
    api.setTransferWakeLock(true);
    await settled();
    assert.equal(api.transferWakeLockStatus(), "active");
    for (let i = 0; i < 100; i++) api.setTransferWakeLock(true);
    assert.equal(locks.length, 1);

    visibility("hidden");
    await settled();
    assert.equal(locks[0].releaseCalls, 1);
    assert.equal(api.transferWakeLockStatus(), "suspended");
    visibility("visible");
    await settled();
    assert.equal(locks.length, 2);
    assert.equal(api.transferWakeLockStatus(), "active");

    api.setTransferWakeLock(false);
    await settled();
    assert.equal(locks[1].releaseCalls, 1);
    assert.equal(api.transferWakeLockStatus(), "off");
    visibility("hidden");
    visibility("visible");
    assert.equal(locks.length, 2);
});

test("denial and OS revocation do not loop; a user action can retry", async () => {
    let calls = 0;
    const lock = sentinel();
    const { api } = await fixture(async () => {
        if (++calls === 1) throw new Error("browser policy");
        return lock;
    });
    api.setTransferWakeLock(true);
    await settled();
    assert.equal(api.transferWakeLockStatus(), "denied");
    for (let i = 0; i < 100; i++) api.setTransferWakeLock(true);
    assert.equal(calls, 1);
    api.retryTransferWakeLock();
    await settled();
    assert.equal(api.transferWakeLockStatus(), "active");
    lock.revoke();
    assert.equal(api.transferWakeLockStatus(), "released");
    assert.equal(calls, 2);
    api.setTransferWakeLock(false);
});

test("an in-flight request cannot retain a lock after transfer cancellation", async () => {
    let finish;
    const lock = sentinel();
    const { api } = await fixture(() => new Promise(resolve => { finish = resolve; }));
    api.setTransferWakeLock(true);
    assert.equal(api.transferWakeLockStatus(), "requesting");
    api.setTransferWakeLock(false);
    finish(lock);
    await settled();
    assert.equal(lock.releaseCalls, 1);
    assert.equal(api.transferWakeLockStatus(), "off");
});

test("a new transfer retries after the canceled request finishes its delayed release", async () => {
    let finishFirst;
    let finishRelease;
    let calls = 0;
    const stale = sentinel();
    stale.release = () => new Promise(resolve => {
        finishRelease = async () => {
            stale.revoke();
            resolve();
        };
    });
    const fresh = sentinel();
    const { api } = await fixture(() => {
        calls += 1;
        return calls === 1
            ? new Promise(resolve => { finishFirst = resolve; })
            : Promise.resolve(fresh);
    });
    api.setTransferWakeLock(true);
    api.setTransferWakeLock(false);
    finishFirst(stale);
    await settled();
    assert.equal(typeof finishRelease, "function");
    api.setTransferWakeLock(true);
    assert.equal(calls, 1, "old request still owns the pending slot");
    await finishRelease();
    await settled();
    assert.equal(calls, 2, "new session gets a request when the old one settles");
    assert.equal(api.transferWakeLockStatus(), "active");
    api.setTransferWakeLock(false);
    await settled();
    assert.equal(fresh.releaseCalls, 1);
});

test("unsupported wake lock reports the fallback without breaking transfer", async () => {
    const { api } = await fixture(undefined);
    api.setTransferWakeLock(true);
    assert.equal(api.transferWakeLockStatus(), "unsupported");
    api.setTransferWakeLock(false);
});

test("a new user gesture requests immediately even while an older request is pending", async () => {
    let resolveFirst;
    let calls = 0;
    const first = sentinel();
    const second = sentinel();
    const { api } = await fixture(() => {
        calls += 1;
        return calls === 1 ? new Promise(resolve => { resolveFirst = resolve; }) : Promise.resolve(second);
    });
    api.setTransferWakeLock(true);
    assert.equal(calls, 1);
    api.retryTransferWakeLock();
    assert.equal(calls, 2, "the click must not wait for the old request");
    await settled();
    assert.equal(api.transferWakeLockStatus(), "active");
    resolveFirst(first);
    await settled();
    assert.equal(first.releaseCalls, 1, "a late duplicate lock must be released");
    api.setTransferWakeLock(false);
    await settled();
    assert.equal(second.releaseCalls, 1);
});

test("an already-released sentinel is never reported as active", async () => {
    const stale = sentinel();
    stale.revoke();
    const fresh = sentinel();
    let calls = 0;
    const { api } = await fixture(async () => (++calls === 1 ? stale : fresh));
    api.setTransferWakeLock(true);
    await settled();
    assert.equal(api.transferWakeLockStatus(), "released");
    api.retryTransferWakeLock();
    await settled();
    assert.equal(api.transferWakeLockStatus(), "active");
    api.setTransferWakeLock(false);
});
