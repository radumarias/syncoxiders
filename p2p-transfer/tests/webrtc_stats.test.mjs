import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../assets/webrtc-channel.js", import.meta.url), "utf8");
const { closePeer, createPeer, maxMessageSize, pathKind, performanceSample, sendFrame } = await import(
    `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);

function connection() {
    const stats = new Map([
        ["transport", { type: "transport", selectedCandidatePairId: "pair" }],
        ["pair", {
            id: "pair", type: "candidate-pair", state: "succeeded",
            localCandidateId: "local", remoteCandidateId: "remote",
            currentRoundTripTime: 0.12, availableOutgoingBitrate: 200_000,
            bytesSent: 900_000, bytesReceived: 400_000,
        }],
        ["local", { candidateType: "srflx", address: "SECRET_LOCAL_IP" }],
        ["remote", { candidateType: "host", address: "SECRET_REMOTE_IP" }],
    ]);
    return {
        pc: { getStats: async () => stats },
        channel: { bufferedAmount: 1024 },
        role: "offerer",
        sentBytes: 800_000,
        receivedBytes: 300_000,
        maxSentFrame: 262_000,
        maxReceivedFrame: 0,
        drainWaitMs: 250,
        lastStats: null,
    };
}

test("selected candidate type and rates appear without exposing ICE addresses", async () => {
    const oldPerformance = globalThis.performance;
    let now = 0;
    globalThis.performance = { now: () => now };
    const peer = connection();
    try {
        const reports = await peer.pc.getStats();
        peer.pc.getStats = async () => reports;
        assert.equal(await pathKind(peer), "direct");
        assert.match(await performanceSample(peer), /candidates=srflx\/host rtt=120ms/);
        now = 5000;
        reports.get("pair").bytesSent += 500_000;
        reports.get("pair").bytesReceived += 100_000;
        peer.sentBytes += 200_000;
        peer.receivedBytes += 50_000;
        const line = await performanceSample(peer);
        assert.match(line, /pairTx=100000B\/s pairRx=20000B\/s/);
        assert.match(line, /queuedTx=40000B\/s deliveredRx=10000B\/s/);
        assert.match(line, /maxFrameTx=262000B/);
        assert.doesNotMatch(line, /SECRET|address|candidateId|capability/i);

        now = 10000;
        reports.get("remote").candidateType = "relay";
        reports.get("transport").selectedCandidatePairId = "pair2";
        reports.set("pair2", { ...reports.get("pair"), id: "pair2" });
        assert.equal(await pathKind(peer), "relayed");
        assert.match(await performanceSample(peer), /pairTx=-B\/s/);
        now = 15000;
        reports.get("pair2").bytesSent = 0; // A browser counter reset is not negative speed.
        assert.match(await performanceSample(peer), /pairTx=-B\/s/);
    } finally {
        globalThis.performance = oldPerformance;
    }
});

test("unavailable or ambiguous stats are not mislabelled direct", async () => {
    const peer = connection();
    peer.pc.getStats = async () => new Map();
    assert.equal(await pathKind(peer), "unknown");
    assert.match(await performanceSample(peer), /path=unknown.*pairTx=-B\/s/);
    peer.pc.getStats = async () => { throw new Error("Safari hid statistics"); };
    assert.equal(await pathKind(peer), "unknown");
});

test("explicit sender frame cap never exceeds the negotiated SCTP limit", () => {
    const peer = { pc: { sctp: { maxMessageSize: 262_144 } } };
    globalThis.location = { search: "" };
    assert.equal(maxMessageSize(peer), 262_144);
    globalThis.location.search = "?dcframe=64";
    assert.equal(maxMessageSize(peer), 64 * 1024);
    globalThis.location.search = "?dcframe=16";
    assert.equal(maxMessageSize(peer), 16 * 1024);
    peer.pc.sctp.maxMessageSize = 32 * 1024;
    globalThis.location.search = "?dcframe=64";
    assert.equal(maxMessageSize(peer), 32 * 1024);
    globalThis.location.search = "?dcframe=9999";
    assert.equal(maxMessageSize(peer), 32 * 1024);
    delete globalThis.location;
});

test("performance sampling stops with the peer and never stalls delivery", async () => {
    const oldInterval = globalThis.setInterval;
    const oldClear = globalThis.clearInterval;
    const oldPeer = globalThis.RTCPeerConnection;
    let tick;
    let cleared = false;
    globalThis.setInterval = (callback, interval) => {
        assert.equal(interval, 5000);
        tick = callback;
        return 42;
    };
    globalThis.clearInterval = id => {
        assert.equal(id, 42);
        cleared = true;
    };
    globalThis.RTCPeerConnection = class {
        connectionState = "connected";
        createDataChannel() { return { readyState: "open", bufferedAmount: 0, close() {} }; }
        async getStats() { return await connection().pc.getStats(); }
        close() {}
    };
    try {
        const lines = [];
        const received = [];
        const peer = createPeer("offerer", [], frame => received.push(frame), () => {}, () => {},
            () => {}, line => lines.push(line));
        await tick();
        assert.equal(lines.length, 1);
        assert.match(lines[0], /WebRTC perf role=offerer/);
        let finishStats;
        peer.pc.getStats = () => new Promise(resolve => { finishStats = resolve; });
        const pending = tick();
        await tick();
        peer.channel.onmessage({ data: new Uint8Array([1, 2, 3]).buffer });
        assert.deepEqual([...received[0]], [1, 2, 3]);
        assert.equal(peer.receivedBytes, 3);
        assert.equal(lines.length, 1);
        closePeer(peer);
        assert.equal(cleared, true);
        finishStats(await connection().pc.getStats());
        await pending;
        await tick();
        assert.equal(lines.length, 1);
    } finally {
        globalThis.setInterval = oldInterval;
        globalThis.clearInterval = oldClear;
        globalThis.RTCPeerConnection = oldPeer;
    }
});

test("drain time is accounted during and after a stalled or failed send", async () => {
    const oldPerformance = globalThis.performance;
    let now = 0;
    globalThis.performance = { now: () => now };
    try {
        const peer = connection();
        const listeners = new Map();
        peer.channel = {
            readyState: "open",
            bufferedAmount: 5 * 1024 * 1024,
            addEventListener(type, callback) { listeners.set(type, callback); },
            removeEventListener(type) { listeners.delete(type); },
            send() {},
        };
        peer.drainWaitMs = 0;
        await performanceSample(peer);
        const sent = sendFrame(peer, new Uint8Array([1, 2]));
        now = 5000;
        assert.match(await performanceSample(peer), /drainWait=5000ms\/interval/);
        now = 7000;
        peer.channel.bufferedAmount = 0;
        listeners.get("bufferedamountlow")();
        await sent;
        now = 10000;
        assert.match(await performanceSample(peer), /drainWait=2000ms\/interval/);
        assert.equal(peer.drainWaitMs, 7000);

        peer.channel.bufferedAmount = 5 * 1024 * 1024;
        const failed = sendFrame(peer, new Uint8Array([3]));
        now = 12000;
        listeners.get("close")();
        await assert.rejects(failed, /closed/);
        assert.equal(peer.drainWaitMs, 9000);
        assert.equal(peer.drainStartedAt, null);
        assert.match(await performanceSample(peer), /drainWait=2000ms\/interval/);
    } finally {
        globalThis.performance = oldPerformance;
    }
});
