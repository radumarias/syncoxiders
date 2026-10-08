#!/usr/bin/env node
// Scripted sender and receiver for Oxfer (https://oxfer.app), for automation and
// AI agents. The app's UI is an egui canvas, so there is no DOM to drive: this
// script locates the primary (blue) button in screenshots, answers the browser
// file chooser, and reads the share link from the clipboard hook it installs.
//
//   node oxfer-agent.mjs send <file> [--once] [--chat <room-link>] [--link-file path]
//   node oxfer-agent.mjs recv <share-link | room-link> [out-dir]
//   node oxfer-agent.mjs chat <room-link> [--name N] [--say text] [--until regex]
//
// A room link (`#chat&…&cap=…`) is a chat room hosted by another open page. `chat` joins
// it and prints one JSON object per event; stdin lines are posted as messages. `send
// --chat` posts the share link into the room once the file is ready, and `recv` with a
// room link waits for a share link to be posted there, then downloads it.
//
// Requires Node >= 22, `playwright` and `pngjs`, and a browser Playwright can
// launch (Google Chrome, or `npx playwright install chromium|firefox|webkit`).
// Common flags: --browser chrome|chromium|firefox|webkit (default: chrome, then
// chromium), --headed, --timeout <seconds>, --origin <url>, --verbose.
//
// The share link fragment is a bearer capability. This script prints it once
// (`LINK …`) and never writes it anywhere else unless --link-file is given.

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { access, mkdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const DEFAULT_ORIGIN = "https://oxfer.app/";
export const PRIMARY_BLUE = { r: 22, g: 116, b: 216 };
const HEADER_HEIGHT = 70; // the top bar also has a blue "Choose File" button
const VIEWPORT = { width: 1280, height: 800 };

/** Classify a URL the way an agent sees it: "home" (send from here), "share"
 *  (receive from here), "diagnostics", or "invalid". */
export function classifyLink(input) {
    let url;
    try {
        url = new URL(String(input));
    } catch {
        return "invalid";
    }
    const local = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
    if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) return "invalid";
    if (url.pathname.replace(/\/+$/, "") === "/diags") return "diagnostics";
    // The capability belongs in the fragment, which never reaches the server.
    if (/(^|[?&])(cap|endpoint\w*)=/.test(url.search)) return "invalid";
    const fragment = url.hash.slice(1);
    if (!fragment || /^(dev|sink=\w+)(&(dev|sink=\w+))*$/.test(fragment)) return "home";
    const parts = Object.fromEntries(
        fragment.split("&").map(part => {
            const eq = part.indexOf("=");
            return eq < 0 ? [part, ""] : [part.slice(0, eq), part.slice(eq + 1)];
        }),
    );
    const endpoint = Object.keys(parts).find(key => key.startsWith("endpoint"));
    if (endpoint && /^[a-z2-7]{20,}$/.test(endpoint.slice("endpoint".length)) && /^[0-9a-f]{32}$/.test(parts.cap ?? "")) {
        return "chat" in parts ? "chat" : "share";
    }
    return "invalid";
}

/** A share link posted inside chat text, if any (trailing punctuation stripped). */
export function findShareLink(text) {
    for (const match of String(text).matchAll(/https?:\/\/\S+/g)) {
        const candidate = match[0].replace(/[\s.,;:!?)\]]+$/, "");
        if (classifyLink(candidate) === "share") return candidate;
    }
    return null;
}

/** The room link with the display name the agent wants to join as. */
export function withChatName(link, name) {
    if (!/^[A-Za-z0-9_.-]{1,32}$/.test(name ?? "")) return link;
    return `${link}&name=${name}`;
}

/** Tiny argv parser: positionals plus `--flag` and `--key value`. */
export function parseArgs(argv, booleans = []) {
    const positional = [];
    const options = {};
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (!arg.startsWith("--")) {
            positional.push(arg);
            continue;
        }
        const [key, inline] = arg.slice(2).split(/=(.*)/s);
        if (booleans.includes(key)) {
            options[key] = true;
        } else if (inline !== undefined) {
            options[key] = inline;
        } else {
            if (i + 1 >= argv.length) throw new Error(`--${key} needs a value`);
            options[key] = argv[++i];
        }
    }
    return { positional, options };
}

function isPrimaryBlue(r, g, b) {
    return b >= 170 && r <= 90 && g >= 70 && g <= 170 && b - r >= 120;
}

/** Find the largest filled primary-blue box in an RGBA bitmap, ignoring the
 *  header. Returns its centre and bounds, or null. Gaps up to `gap` pixels are
 *  bridged so white button text does not split a row, and up to `rowGap` rows
 *  so a label line does not split the box. */
export function findPrimaryButton(rgba, width, height, { minY = HEADER_HEIGHT, minWidth = 90, minHeight = 24, gap = 16, rowGap = 12 } = {}) {
    const boxes = [];
    for (let y = minY; y < height; y++) {
        const runs = [];
        let start = -1;
        let last = -1;
        for (let x = 0; x <= width; x++) {
            const i = (y * width + x) * 4;
            const blue = x < width && isPrimaryBlue(rgba[i], rgba[i + 1], rgba[i + 2]);
            if (blue) {
                if (start < 0) start = x;
                last = x;
            } else if (start >= 0 && x - last > gap) {
                if (last - start + 1 >= minWidth) runs.push([start, last]);
                start = -1;
            }
        }
        for (const [x0, x1] of runs) {
            const box = boxes.find(b => y - b.y1 <= rowGap + 1 && x0 <= b.x1 && x1 >= b.x0);
            if (box) {
                box.y1 = y;
                box.x0 = Math.min(box.x0, x0);
                box.x1 = Math.max(box.x1, x1);
            } else {
                boxes.push({ x0, x1, y0: y, y1: y });
            }
        }
    }
    let best = null;
    for (const box of boxes) {
        const w = box.x1 - box.x0 + 1;
        const h = box.y1 - box.y0 + 1;
        if (h < minHeight || w < minWidth) continue;
        if (!best || w * h > best.area) best = { ...box, area: w * h, x: Math.round((box.x0 + box.x1) / 2), y: Math.round((box.y0 + box.y1) / 2) };
    }
    return best;
}

function sha256File(file) {
    return new Promise((resolve, reject) => {
        const hash = createHash("sha256");
        createReadStream(file).on("data", chunk => hash.update(chunk)).on("end", () => resolve(hash.digest("hex"))).on("error", reject);
    });
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

class Session {
    constructor(options) {
        this.options = options;
        this.timeoutMs = Number(options.timeout ?? 120) * 1000;
        this.log = [];       // recent console lines only; iroh traces are chatty
        this.waiters = [];
        this.counters = [];  // { pattern, n } tallied as lines arrive
        this.snaps = 0;
        this.chatEvents = [];  // every `oxfer:chat` DOM event, as the page emitted it
        this.chatWaiters = [];
        this.chatSeen = 0;     // how many chatEvents have been printed/handled
    }

    onChatEvent(detail) {
        this.chatEvents.push(detail);
        this.debug(`[chat] ${JSON.stringify(detail).slice(0, 200)}`);
        const index = this.chatEvents.length - 1;
        for (const waiter of this.chatWaiters.splice(0)) {
            if (waiter.predicate(detail, index)) waiter.resolve(detail);
            else this.chatWaiters.push(waiter);
        }
    }

    /** The first chat event (past or future) matching `predicate`. */
    waitForChat(predicate, timeoutMs = this.timeoutMs, what = "a chat event") {
        const seen = this.chatEvents.find((event, index) => predicate(event, index));
        if (seen) return Promise.resolve(seen);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.chatWaiters = this.chatWaiters.filter(w => w.resolve !== resolve);
                reject(new Error(`timed out after ${timeoutMs / 1000}s waiting for ${what}`));
            }, timeoutMs);
            this.chatWaiters.push({ predicate, resolve: detail => { clearTimeout(timer); resolve(detail); } });
        });
    }

    /** Open the room in `page` and wait until the host accepted us. */
    async joinChat(page, link, name) {
        await page.goto(withChatName(link, name), { waitUntil: "load", timeout: this.timeoutMs });
        const outcome = await this.waitForChat(
            e => e.type === "connected" || e.type === "failed" || e.type === "closed",
            this.timeoutMs,
            "the room to accept us",
        );
        if (outcome.type !== "connected") {
            throw new Error(`could not join the room: ${outcome.message ?? outcome.type}`);
        }
        return outcome;
    }

    /** Post `text` to the room open in `page`. With `you`, resolve only once the host has
     *  relayed it back, so the message is really in the room before the caller moves on. */
    async chatSend(page, text, you = null) {
        for (let i = 0; i < 50; i++) {
            const ready = await page.evaluate(() => Boolean(globalThis.oxfer?.chat?.send)).catch(() => false);
            if (ready) break;
            await sleep(100);
        }
        const before = this.chatEvents.length;
        await page.evaluate(t => globalThis.oxfer.chat.send(t), String(text));
        if (you === null) return;
        const wanted = String(text).trim();
        await this.waitForChat(
            (e, index) => index >= before && e.type === "message" && e.from === you && e.text === wanted,
            15_000,
            "the room to echo our message",
        );
    }

    /** Count matching console lines from now on, without keeping them. */
    counter(pattern) {
        const counter = { pattern, n: 0 };
        this.counters.push(counter);
        return counter;
    }

    say(line) {
        process.stdout.write(`${line}\n`);
    }

    debug(line) {
        if (this.options.verbose) process.stderr.write(`${line}\n`);
    }

    async launch(mode) {
        const { chromium, firefox, webkit } = await import("playwright");
        const headless = !this.options.headed;
        const chromiumArgs = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"];
        const wanted = this.options.browser ?? "auto";
        const candidates = wanted === "auto" ? ["chrome", "chromium"] : [wanted];
        let lastError;
        for (const candidate of candidates) {
            try {
                if (candidate === "firefox") {
                    this.browser = await firefox.launch({ headless });
                } else if (candidate === "webkit") {
                    this.browser = await webkit.launch({ headless });
                } else {
                    const launch = { headless, args: chromiumArgs };
                    if (candidate !== "chromium") launch.channel = candidate;
                    this.browser = await chromium.launch(launch);
                }
                this.family = candidate === "firefox" || candidate === "webkit" ? candidate : "chromium";
                this.debug(`browser: ${candidate}`);
                break;
            } catch (error) {
                lastError = error;
            }
        }
        if (!this.browser) throw new Error(`could not launch a browser (${lastError?.message ?? "unknown"}); try --browser chromium after \`npx playwright install chromium\``);

        const context = await this.browser.newContext({
            viewport: VIEWPORT,
            deviceScaleFactor: 1,
            colorScheme: "light",
            acceptDownloads: true,
        });
        if (this.family === "chromium") {
            await context.grantPermissions(["clipboard-read", "clipboard-write"]).catch(() => {});
        }
        await context.addInitScript(() => {
            // Capture the share link without depending on clipboard-read support.
            const clipboard = navigator.clipboard;
            const original = clipboard?.writeText?.bind(clipboard);
            if (clipboard) {
                Object.defineProperty(clipboard, "writeText", {
                    configurable: true,
                    value: async text => {
                        window.__oxferAgentClipboard = String(text);
                        try { if (original) await original(text); } catch { /* headless clipboards may refuse */ }
                    },
                });
            }
        });
        if (mode === "recv") {
            await context.addInitScript(() => {
                // Headless Chromium exposes the save-file picker but cannot show it, so
                // the click would hang. Without it Oxfer streams through its service
                // worker, which Playwright receives as an ordinary download.
                try {
                    delete window.showSaveFilePicker;
                    Object.defineProperty(window, "showSaveFilePicker", { value: undefined, configurable: true });
                } catch { /* ignore */ }
            });
        }
        await context.exposeFunction("__oxferAgentChatEvent", detail => this.onChatEvent(detail));
        await context.addInitScript(() => {
            window.addEventListener("oxfer:chat", event => {
                try { window.__oxferAgentChatEvent(event.detail); } catch { /* page closing */ }
            });
        });
        this.context = context;
        this.page = await this.newPage();
    }

    /** A page whose console feeds the log, counters and waiters. */
    async newPage() {
        const page = await this.context.newPage();
        page.on("console", message => {
            const text = message.text();
            this.log.push(text);
            if (this.log.length > 2000) this.log.splice(0, this.log.length - 1000);
            for (const counter of this.counters) if (counter.pattern.test(text)) counter.n++;
            this.debug(`[console] ${text.replace(/%c/g, "").slice(0, 200)}`);
            for (const waiter of this.waiters.splice(0)) {
                if (waiter.pattern.test(text)) waiter.resolve(text);
                else this.waiters.push(waiter);
            }
        });
        page.on("pageerror", error => this.debug(`[pageerror] ${error.message}`));
        return page;
    }

    /** Save every download `page` starts into `outDir`, never overwriting. */
    collectDownloads(page, outDir, downloads) {
        page.on("download", download => {
            const saving = (async () => {
                let dest = path.join(outDir, download.suggestedFilename());
                for (let n = 1; await access(dest).then(() => true, () => false); n++) {
                    const { name, ext } = path.parse(download.suggestedFilename());
                    dest = path.join(outDir, `${name} (${n})${ext}`);
                }
                await download.saveAs(dest);
                return dest;
            })();
            downloads.push(saving);
        });
    }

    waitForLog(pattern, timeoutMs = this.timeoutMs, what = pattern.source) {
        const seen = this.log.find(line => pattern.test(line));
        if (seen) return Promise.resolve(seen);
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.waiters = this.waiters.filter(w => w.resolve !== resolve);
                reject(new Error(`timed out after ${timeoutMs / 1000}s waiting for ${what}`));
            }, timeoutMs);
            this.waiters.push({ pattern, resolve: text => { clearTimeout(timer); resolve(text); } });
        });
    }

    async open(url) {
        await this.page.goto(url, { waitUntil: "load", timeout: this.timeoutMs });
        await this.waitForLog(/event handlers installed/, this.timeoutMs, "the app to start (WebAssembly + WebGL)");
        await sleep(500);
    }

    async bitmap() {
        const { PNG } = await import("pngjs");
        // egui repaints on input; nudge the pointer so the screenshot is current.
        await this.page.mouse.move(VIEWPORT.width - 10, VIEWPORT.height - 10);
        await sleep(150);
        const png = PNG.sync.read(await this.page.screenshot({ type: "png" }));
        return png;
    }

    async snap(label) {
        if (!this.options.snap) return;
        await mkdir(this.options.snap, { recursive: true });
        const file = path.join(this.options.snap, `${String(++this.snaps).padStart(2, "0")}-${label}.png`);
        await this.page.mouse.move(VIEWPORT.width - 10, VIEWPORT.height - 10);
        await sleep(150);
        await this.page.screenshot({ path: file });
        this.debug(`snapshot: ${file}`);
    }

    /** Poll until the primary button is visible, then return its centre. */
    async waitForPrimaryButton(what, timeoutMs = this.timeoutMs) {
        const deadline = Date.now() + timeoutMs;
        while (Date.now() < deadline) {
            const png = await this.bitmap();
            const button = findPrimaryButton(png.data, png.width, png.height);
            if (button) return button;
            await sleep(1000);
        }
        await this.snap(`timeout-${what.replace(/\W+/g, "-")}`);
        throw new Error(`timed out after ${timeoutMs / 1000}s waiting for the "${what}" button`);
    }

    async clickPrimaryButton(what, timeoutMs) {
        const button = await this.waitForPrimaryButton(what, timeoutMs);
        this.debug(`clicking "${what}" at (${button.x}, ${button.y})`);
        await this.page.mouse.click(button.x, button.y);
    }

    async close() {
        // A page mid-transfer can keep browser.close() waiting; never hang exit on it.
        await Promise.race([this.browser?.close().catch(() => {}), sleep(5000)]);
    }
}

async function send(files, options) {
    for (const file of files) {
        const info = await stat(file).catch(() => null);
        if (!info?.isFile()) throw new Error(`not a readable file: ${file}`);
    }
    const origin = options.origin ?? DEFAULT_ORIGIN;
    if (classifyLink(origin) !== "home") throw new Error(`--origin must be the app origin, got ${origin}`);
    const session = new Session(options);
    const stop = async code => { await session.close(); process.exit(code); };
    process.on("SIGINT", () => stop(130));
    process.on("SIGTERM", () => stop(143));
    try {
        await session.launch("send");
        await session.open(origin);
        await session.snap("home");

        const chooser = session.page.waitForEvent("filechooser", { timeout: 15_000 });
        await session.clickPrimaryButton("Send files");
        await (await chooser).setFiles(files.map(f => path.resolve(f)));
        await session.waitForLog(/ready to share/, session.timeoutMs, "file hashing to finish (\"ready to share\")");
        await session.snap("ready");

        await session.clickPrimaryButton("Copy link", 30_000);
        let link = null;
        for (let i = 0; i < 50 && !link; i++) {
            link = await session.page.evaluate(() => window.__oxferAgentClipboard ?? null);
            if (!link && session.family === "chromium") {
                link = await session.page.evaluate(() => navigator.clipboard.readText()).catch(() => null);
            }
            if (!link) await sleep(100);
        }
        if (classifyLink(link) !== "share") throw new Error(`did not get a share link from the app (got ${link ? "something else" : "nothing"})`);
        await session.snap("sharing");
        if (options["link-file"]) await writeFile(options["link-file"], `${link}\n`, { mode: 0o600 });
        session.say(`LINK ${link}`);
        session.say(`SHARING ${files.map(f => path.basename(f)).join(" ")}`);
        let chatPage = null;
        let chatYou = null;
        if (options.chat) {
            if (classifyLink(options.chat) !== "chat") throw new Error("--chat needs a room link (#chat&…&cap=…)");
            chatPage = await session.newPage();
            const joined = await session.joinChat(chatPage, options.chat, options.name ?? "sender-agent");
            session.say(`CHAT joined as ${joined.you}; members: ${joined.members.join(", ")}`);
            const sizes = await Promise.all(files.map(async f => (await stat(f)).size));
            const names = files.map((f, i) => `${path.basename(f)} (${sizes[i]} bytes)`).join(", ");
            await session.chatSend(chatPage, `Sharing ${names}. Open this link to receive it: ${link}`, joined.you);
            session.say("CHAT posted the share link");
            chatYou = joined.you;
        }

        const wanted = options.once ? 1 : Number(options.count ?? Infinity);
        // "serve complete" only means the sender finished writing; the receiver's
        // verified receipt ("receiver verified …") is the success signal.
        const servedCount = session.counter(/serve complete/);
        const verifiedCount = session.counter(/receiver verified/);
        let served = 0;
        let completed = 0;
        while (completed < wanted) {
            await sleep(500);
            while (served < servedCount.n) session.say(`SERVED ${++served}`);
            while (completed < verifiedCount.n) session.say(`COMPLETE ${++completed}`);
            while (session.chatSeen < session.chatEvents.length) {
                session.say(`CHAT ${JSON.stringify(session.chatEvents[session.chatSeen++])}`);
            }
        }
        if (chatPage && completed > 0) {
            await session.chatSend(chatPage, `Transfer verified (${completed} receiver${completed === 1 ? "" : "s"}).`, chatYou).catch(() => {});
        }
        await stop(0);
    } catch (error) {
        await session.snap("error");
        process.stderr.write(`error: ${error.message}\n`);
        await stop(2);
    }
}

async function recv(link, outDir, options) {
    const kind = classifyLink(link);
    if (kind !== "share" && kind !== "chat") {
        throw new Error(kind === "home"
            ? "that is the Oxfer home page, not a share link; a share link has a #endpoint…&cap=… fragment"
            : `not an Oxfer share or room link (${kind})`);
    }
    await mkdir(outDir, { recursive: true });
    const session = new Session(options);
    const stop = async code => { await session.close(); process.exit(code); };
    process.on("SIGINT", () => stop(130));
    process.on("SIGTERM", () => stop(143));
    try {
        await session.launch("recv");
        const verifiedCount = session.counter(/verified and saved/);
        const downloads = [];
        session.collectDownloads(session.page, outDir, downloads);
        let chatPage = null;
        let chatYou = null;
        if (kind === "chat") {
            // Join the room, wait for someone else to post a share link, then receive it
            // in a second page so the room stays open for the receipt message.
            chatPage = session.page;
            const joined = await session.joinChat(chatPage, link, options.name ?? "receiver-agent");
            session.say(`CHAT joined as ${joined.you}; members: ${joined.members.join(", ")}`);
            const posted = await session.waitForChat(
                e => e.type === "message" && e.from !== joined.you && findShareLink(e.text),
                Number(options["wait"] ?? 600) * 1000,
                "a share link to be posted in the room (--wait seconds)",
            );
            link = findShareLink(posted.text);
            session.say(`CHAT ${posted.from} posted a share link`);
            chatYou = joined.you;
            session.page = await session.newPage();
            session.collectDownloads(session.page, outDir, downloads);
        }
        await session.open(link);
        // No log line marks readiness; the "Choose where to save" button appears
        // once the manifest arrived from the sender.
        await session.clickPrimaryButton("Choose where to save");
        await session.snap("saving");

        const deadline = Date.now() + session.timeoutMs;
        while (downloads.length === 0) {
            if (Date.now() > deadline) throw new Error("the sender never started a download; is its browser still open?");
            await sleep(250);
        }
        let settled = 0;
        while (settled < downloads.length) {
            settled = downloads.length;
            await Promise.all(downloads);
            await sleep(1500); // more files of the same manifest may start late
        }
        // Oxfer verifies BLAKE3 after the sink closes; wait for every file's receipt.
        const verified = () => verifiedCount.n;
        const verifyDeadline = Date.now() + 60_000;
        while (verified() < downloads.length && Date.now() < verifyDeadline) await sleep(250);
        await session.snap("done");
        for (const dest of await Promise.all(downloads)) {
            const info = await stat(dest);
            session.say(`SAVED ${dest} ${info.size} ${await sha256File(dest)}`);
        }
        const ok = verified() >= downloads.length;
        session.say(ok ? "VERIFIED" : "UNVERIFIED (no verification receipt seen; compare hashes with the sender)");
        if (chatPage) {
            const names = (await Promise.all(downloads)).map(d => path.basename(d)).join(", ");
            await session.chatSend(chatPage, ok ? `Received and verified ${names}.` : `Saved ${names}, but could not verify it.`, chatYou).catch(() => {});
        }
        await stop(ok ? 0 : 3);
    } catch (error) {
        await session.snap("error");
        process.stderr.write(`error: ${error.message}\n`);
        await stop(2);
    }
}

/** Host a room from this process: prints the links, then relays events until stopped. */
async function room(options) {
    const origin = options.origin ?? DEFAULT_ORIGIN;
    if (classifyLink(origin) !== "home") throw new Error(`--origin must be the app origin, got ${origin}`);
    const session = new Session(options);
    const stop = async code => { await session.close(); process.exit(code); };
    process.on("SIGINT", () => stop(130));
    process.on("SIGTERM", () => stop(143));
    try {
        await session.launch("room");
        await session.page.goto(`${origin.replace(/\/?$/, "/")}#chat&agent`, { waitUntil: "load", timeout: session.timeoutMs });
        const ready = await session.waitForChat(
            e => e.type === "ready" || e.type === "failed",
            session.timeoutMs,
            "the room to open",
        );
        if (ready.type !== "ready") throw new Error(`could not open a room: ${ready.message}`);
        session.say(`ROOM ${ready.link}`);
        session.say(`AGENT ${ready.agentLink}`);
        if (options.say) await session.chatSend(session.page, options.say, ready.you);
        const { createInterface } = await import("node:readline");
        createInterface({ input: process.stdin }).on("line", line => {
            if (line.trim()) session.chatSend(session.page, line, ready.you).catch(e => process.stderr.write(`send failed: ${e.message}\n`));
        });
        for (;;) {
            while (session.chatSeen < session.chatEvents.length) {
                const event = session.chatEvents[session.chatSeen++];
                if (event.type !== "ready") session.say(JSON.stringify(event));
                if (event.type === "failed") await stop(2);
                if (event.type === "closed") await stop(0);
            }
            await sleep(200);
        }
    } catch (error) {
        await session.snap("error");
        process.stderr.write(`error: ${error.message}\n`);
        await stop(2);
    }
}

async function chat(link, options) {
    if (classifyLink(link) !== "chat") throw new Error("chat needs a room link (#chat&…&cap=…)");
    const session = new Session(options);
    const stop = async code => { await session.close(); process.exit(code); };
    process.on("SIGINT", () => stop(130));
    process.on("SIGTERM", () => stop(143));
    try {
        await session.launch("chat");
        const page = session.page;
        const joined = await session.joinChat(page, link, options.name ?? "agent");
        const until = options.until ? new RegExp(options.until) : null;
        let you = joined.you;
        // Print every event as one JSON line, in order, including the ones before "connected".
        const flush = () => {
            while (session.chatSeen < session.chatEvents.length) {
                const event = session.chatEvents[session.chatSeen++];
                session.say(JSON.stringify(event));
                if (event.type === "failed") return 2;
                if (event.type === "closed") return 0;
                if (until && event.type === "message" && event.from !== you && until.test(event.text)) return 0;
            }
            return null;
        };
        if (options.say) await session.chatSend(page, options.say, you);
        const { createInterface } = await import("node:readline");
        createInterface({ input: process.stdin }).on("line", line => {
            if (line.trim()) session.chatSend(page, line, you).catch(e => process.stderr.write(`send failed: ${e.message}\n`));
        });
        for (;;) {
            const code = flush();
            if (code !== null) await stop(code);
            await sleep(200);
        }
    } catch (error) {
        await session.snap("error");
        process.stderr.write(`error: ${error.message}\n`);
        await stop(2);
    }
}

export function usage() {
    return `Usage:
  oxfer-agent.mjs send <file> [--once | --count N] [--chat <room-link>] [--link-file path]
  oxfer-agent.mjs recv <share-link | room-link> [out-dir] [--wait seconds]
  oxfer-agent.mjs chat <room-link> [--name N] [--say text] [--until regex]
  oxfer-agent.mjs room [--say text]            host a room here; prints ROOM and AGENT links
Options: --browser chrome|chromium|firefox|webkit  --headed  --timeout seconds  --name N
         --origin https://oxfer.app/  --snap dir  --verbose
Output lines: LINK <url>, SHARING, SERVED n, COMPLETE n, CHAT … (send);
              SAVED <path> <bytes> <sha256>, VERIFIED (recv); one JSON object per event (chat).
Exit codes: 0 ok, 1 usage, 2 failure, 3 saved but no verification receipt.`;
}

export async function main(argv) {
    const { positional, options } = parseArgs(argv, ["once", "headed", "verbose", "help"]);
    const [mode, ...rest] = positional;
    if (options.help || !mode) {
        process.stdout.write(`${usage()}\n`);
        return mode ? 0 : 1;
    }
    if (mode === "send") {
        if (rest.length === 0) throw new Error("send needs a file");
        await send(rest, options);
    } else if (mode === "recv" || mode === "receive") {
        if (rest.length === 0) throw new Error("recv needs a share link or a room link");
        await recv(rest[0], rest[1] ?? ".", options);
    } else if (mode === "chat") {
        if (rest.length === 0) throw new Error("chat needs a room link");
        await chat(rest[0], options);
    } else if (mode === "room") {
        await room(options);
    } else {
        throw new Error(`unknown mode ${mode}\n${usage()}`);
    }
    return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    main(process.argv.slice(2)).then(code => process.exit(code), error => {
        process.stderr.write(`error: ${error.message}\n`);
        process.exit(1);
    });
}
