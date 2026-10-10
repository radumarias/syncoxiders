import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
    awaitDownloads,
    classifyLink,
    cleanChatText,
    findPrimaryButton,
    findShareLink,
    parseArgs,
    withChatName,
    PRIMARY_BLUE,
} from "../agent/oxfer-agent.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const share = "https://oxfer.app/#endpointadgxn5zl2zfsyzz7clou244grpcaza4t4e62rxsl3agmhvdmo4c7gaiaejuhi5dqom5c6l3fovrtcljrfzzgk3dbpexg4mbonfzg62bonruw42zp&cap=b12d9525bb44971d1375bf8f2bcf4e9e";

test("classifyLink tells the home page, share links and diagnostics apart", () => {
    assert.equal(classifyLink("https://oxfer.app/"), "home");
    assert.equal(classifyLink("https://oxfer.app/#dev"), "home");
    assert.equal(classifyLink("https://oxfer.app/#sink=sw&dev"), "home");
    assert.equal(classifyLink("http://127.0.0.1:8080/"), "home");
    assert.equal(classifyLink(share), "share");
    assert.equal(classifyLink("https://oxfer.app/diags"), "diagnostics");
    assert.equal(classifyLink("http://oxfer.app/"), "invalid", "plain HTTP is not a secure context");
    assert.equal(classifyLink(share.replace("&cap=b12d9525bb44971d1375bf8f2bcf4e9e", "")), "invalid", "a link without its access code");
    assert.equal(classifyLink("https://oxfer.app/?cap=b12d9525bb44971d1375bf8f2bcf4e9e"), "invalid", "the capability must stay in the fragment");
    assert.equal(classifyLink("not a url"), "invalid");
    const room = share.replace("/#", "/#chat&");
    assert.equal(classifyLink(room), "chat");
    assert.equal(classifyLink(`${room}&agent`), "chat");
    assert.equal(classifyLink(`${room}&name=agent1`), "chat");
});

test("share links are found inside chat text and names are appended safely", () => {
    assert.equal(findShareLink(`Sharing a.bin (97 bytes). Open this link to receive it: ${share}`), share);
    assert.equal(findShareLink(`here: ${share}.`), share, "trailing punctuation is not part of the link");
    assert.equal(findShareLink("no link here https://oxfer.app/"), null);
    assert.equal(findShareLink(`room ${share.replace("/#", "/#chat&")}`), null, "a room link is not a share link");
    assert.equal(withChatName("https://oxfer.app/#chat&x&cap=y", "agent-1"), "https://oxfer.app/#chat&x&cap=y&name=agent-1");
    assert.equal(withChatName("https://oxfer.app/#chat&x&cap=y", "no spaces"), "https://oxfer.app/#chat&x&cap=y");
});

test("a room only hands the receiver share links on its own origin", () => {
    const room = "https://oxfer.app/#chat&x&cap=y";
    const elsewhere = share.replace("https://oxfer.app/", "https://evil.example/");
    assert.equal(findShareLink(`take ${share}`, room), share);
    assert.equal(findShareLink(`take ${elsewhere}`, room), null, "another site is never opened");
    assert.equal(findShareLink(`take ${elsewhere} or ${share}`, room), share);
});

test("cleanChatText matches what the host relays", () => {
    assert.equal(cleanChatText("  hi\u0000there \n"), "hithere");
    assert.equal(cleanChatText("a\tb\nc"), "a\tb\nc");
    assert.equal(cleanChatText("é".repeat(3000)).length, 2048, "cut to 4096 UTF-8 bytes on a character boundary");
});

test("parseArgs separates positionals, flags and values", () => {
    const { positional, options } = parseArgs(["send", "a.bin", "--once", "--timeout", "30", "--snap=shots"], ["once"]);
    assert.deepEqual(positional, ["send", "a.bin"]);
    assert.deepEqual(options, { once: true, timeout: "30", snap: "shots" });
    assert.throws(() => parseArgs(["--timeout"]), /needs a value/);
});

function bitmap(width, height, boxes) {
    const rgba = new Uint8Array(width * height * 4).fill(240);
    for (const { x0, y0, x1, y1, color = PRIMARY_BLUE } of boxes) {
        for (let y = y0; y <= y1; y++) {
            for (let x = x0; x <= x1; x++) {
                rgba.set([color.r, color.g, color.b, 255], (y * width + x) * 4);
            }
        }
    }
    return rgba;
}

test("findPrimaryButton picks the largest primary-blue box below the header", () => {
    const width = 400, height = 300;
    const rgba = bitmap(width, height, [
        { x0: 300, y0: 10, x1: 390, y1: 50 },                       // header "Choose File" button
        { x0: 20, y0: 100, x1: 58, y1: 138 },                       // file icon, too narrow
        { x0: 100, y0: 200, x1: 300, y1: 250 },                     // the call to action
        { x0: 150, y0: 220, x1: 250, y1: 230, color: { r: 255, g: 255, b: 255 } }, // white label
        { x0: 20, y0: 160, x1: 200, y1: 190, color: { r: 220, g: 232, b: 252 } },  // pale selection
    ]);
    const button = findPrimaryButton(rgba, width, height);
    assert.ok(button);
    assert.equal(button.x, 200);
    assert.equal(button.y, 225);
    assert.equal(findPrimaryButton(bitmap(width, height, []), width, height), null);
});

test("the site tells HTTP-only clients how to drive it", async () => {
    const html = await read("../index.html");
    assert.match(html, /<noscript>[\s\S]*llms\.txt[\s\S]*<\/noscript>/);
    assert.match(html, /<meta name="description"/);
    assert.match(html, /rel="copy-file" href="assets\/llms\.txt"/);
    assert.match(html, /rel="copy-file" href="agent\/oxfer-agent\.mjs"/);
    const llms = await read("../assets/llms.txt");
    assert.match(llms, /oxfer-agent\.mjs send/);
    assert.match(llms, /oxfer-agent\.mjs recv/);
    assert.match(llms, /#endpoint…&cap=…/);
    assert.match(llms, /#chat&endpoint…&cap=…/);
    assert.match(llms, /oxfer-agent\.mjs chat/);
    assert.match(llms, /send .* --chat/);
    assert.match(html, /<noscript>[\s\S]*#chat&amp;[\s\S]*<\/noscript>/);
    const bridge = await read("../assets/chat-bridge.js");
    assert.match(bridge, /oxfer:chat/);
    assert.match(bridge, /export function installChatSender/);
    assert.doesNotMatch(llms, /cap=[0-9a-f]{32}/, "never publish a real capability");
});

test("awaitDownloads resolves when downloads finish and fails on a stall or a closed page", async () => {
    const quick = { isGone: () => false, stallMs: 50, pollMs: 5 };
    let bytes = 0;
    const growing = () => bytes++;
    const slow = new Promise(resolve => setTimeout(() => resolve("a.bin"), 120));
    assert.deepEqual(await awaitDownloads([slow], { ...quick, bytesSoFar: growing }), ["a.bin"], "slow but progressing is fine");

    const never = new Promise(() => {});
    await assert.rejects(awaitDownloads([never], { ...quick, bytesSoFar: () => 7 }), /no progress for 0.05s/);
    await assert.rejects(awaitDownloads([never], { ...quick, isGone: () => true, bytesSoFar: null }), /closed or crashed/);
    await assert.rejects(awaitDownloads([Promise.reject(new Error("canceled"))], { ...quick, bytesSoFar: null }), /canceled/);
});
