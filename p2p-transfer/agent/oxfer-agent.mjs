#!/usr/bin/env node
// Scripted sender and receiver for Oxfer (https://oxfer.app), for automation and
// AI agents. The app's UI is an egui canvas, so there is no DOM to drive: this
// script locates the primary (blue) button in screenshots, answers the browser
// file chooser, and reads the share link from the clipboard hook it installs.
//
//   node oxfer-agent.mjs send <file> [--once] [--link-file path] [--snap dir]
//   node oxfer-agent.mjs recv <share-link> [out-dir] [--snap dir]
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
import { access, mkdir, mkdtemp, readdir, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
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
        return "share";
    }
    return "invalid";
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

/**
 * Wait for every download to settle. Fails when `isGone()` reports the page closed or
 * crashed, or when `bytesSoFar()` (if given) stays the same for `stallMs`, so a sender
 * that vanished mid-stream cannot keep the receiver waiting forever.
 */
export async function awaitDownloads(downloads, { isGone, bytesSoFar, stallMs, pollMs = 500 }) {
    const all = Promise.all(downloads);
    let settled = false;
    all.then(() => { settled = true; }, () => { settled = true; });
    let last = -1;
    let lastChange = Date.now();
    while (!settled) {
        if (isGone()) throw new Error("the app page closed or crashed during the download");
        if (bytesSoFar) {
            const bytes = await bytesSoFar();
            if (bytes !== last) {
                last = bytes;
                lastChange = Date.now();
            } else if (Date.now() - lastChange > stallMs) {
                throw new Error(`the download made no progress for ${stallMs / 1000}s; is the sender's browser still open?`);
            }
        }
        await Promise.race([all.catch(() => {}), sleep(pollMs)]);
    }
    return all;
}

class Session {
    constructor(options) {
        this.options = options;
        this.timeoutMs = Number(options.timeout ?? 120) * 1000;
        this.log = [];       // recent console lines only; iroh traces are chatty
        this.waiters = [];
        this.counters = [];  // { pattern, n } tallied as lines arrive
        this.snaps = 0;
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
        // In-progress downloads land here, so the receiver can tell a stalled stream
        // from a slow one.
        this.downloadsDir = await mkdtemp(path.join(os.tmpdir(), "oxfer-agent-"));
        const downloadsPath = this.downloadsDir;
        const wanted = this.options.browser ?? "auto";
        const candidates = wanted === "auto" ? ["chrome", "chromium"] : [wanted];
        let lastError;
        for (const candidate of candidates) {
            try {
                if (candidate === "firefox") {
                    this.browser = await firefox.launch({ headless, downloadsPath });
                } else if (candidate === "webkit") {
                    this.browser = await webkit.launch({ headless, downloadsPath });
                } else {
                    const launch = { headless, args: chromiumArgs, downloadsPath };
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
        this.context = context;
        this.page = await context.newPage();
        this.pageGone = false;
        this.page.on("close", () => { this.pageGone = true; });
        this.page.on("crash", () => { this.pageGone = true; });
        this.page.on("console", message => {
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
        this.page.on("pageerror", error => this.debug(`[pageerror] ${error.message}`));
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
        if (this.downloadsDir) await rm(this.downloadsDir, { recursive: true, force: true }).catch(() => {});
    }

    /** Bytes written so far to in-progress and finished downloads. */
    async downloadedBytes() {
        let total = 0;
        for (const name of await readdir(this.downloadsDir).catch(() => [])) {
            total += (await stat(path.join(this.downloadsDir, name)).catch(() => null))?.size ?? 0;
        }
        return total;
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

        const wanted = options.once ? 1 : Number(options.count ?? Infinity);
        // "serve complete" only means the sender finished writing; the receiver's
        // verified receipt ("receiver verified …") is the success signal.
        const servedCount = session.counter(/serve complete/);
        const verifiedCount = session.counter(/receiver verified/);
        let served = 0;
        let completed = 0;
        let lastServed = Date.now();
        while (completed < wanted) {
            await sleep(500);
            while (served < servedCount.n) { session.say(`SERVED ${++served}`); lastServed = Date.now(); }
            while (completed < verifiedCount.n) session.say(`COMPLETE ${++completed}`);
            if (session.pageGone) throw new Error("the app page closed or crashed while sharing");
            // A bounded run must not wait forever for a receipt that is not coming: the
            // receiver failed, or the build predates the "receiver verified" line.
            if (Number.isFinite(wanted) && served > completed && Date.now() - lastServed > session.timeoutMs) {
                throw new Error(`served ${served} but no verification receipt arrived within ${session.timeoutMs / 1000}s`);
            }
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
    if (kind !== "share") {
        throw new Error(kind === "home"
            ? "that is the Oxfer home page, not a share link; a share link has a #endpoint…&cap=… fragment"
            : `not an Oxfer share link (${kind})`);
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
        session.page.on("download", download => {
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
            await awaitDownloads(downloads, {
                isGone: () => session.pageGone,
                // Only Chromium is known to grow the file in `downloadsPath` as bytes
                // arrive; elsewhere a long download would look stalled.
                bytesSoFar: session.family === "chromium" ? () => session.downloadedBytes() : null,
                stallMs: session.timeoutMs,
            });
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
        session.say(verified() >= downloads.length ? "VERIFIED" : "UNVERIFIED (no verification receipt seen; compare hashes with the sender)");
        await stop(verified() >= downloads.length ? 0 : 3);
    } catch (error) {
        await session.snap("error");
        process.stderr.write(`error: ${error.message}\n`);
        await stop(2);
    }
}

export function usage() {
    return `Usage:
  oxfer-agent.mjs send <file> [--once | --count N] [--link-file path]
  oxfer-agent.mjs recv <share-link> [out-dir]
Options: --browser chrome|chromium|firefox|webkit  --headed  --timeout seconds
         --origin https://oxfer.app/  --snap dir  --verbose
Output lines: LINK <url>, SHARING, COMPLETE n (send); SAVED <path> <bytes> <sha256>, VERIFIED (recv).
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
        if (rest.length === 0) throw new Error("recv needs a share link");
        await recv(rest[0], rest[1] ?? ".", options);
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
