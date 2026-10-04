// Static pages must work under the Content-Security-Policy in assets/_headers:
// script-src 'self' 'wasm-unsafe-eval' allows no inline script, no inline event
// handler attribute, no javascript: URL and no cross-origin subresource.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import vm from "node:vm";
import { LEGAL_PAGES, OPERATOR_RELAY, checkLegalPages, trunkCopyFiles } from "../package-cf-output.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const crateFile = path => new URL(`../${path}`, import.meta.url);

// Every HTML file Trunk copies into dist and the Worker serves.
const pages = ["index.html", "theme.html", ...LEGAL_PAGES];

const ATTRIBUTE = /([^\s"'<>\/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
const TAG = /<([a-zA-Z][\w-]*)((?:\s+[^\s"'<>\/=]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?)*)\s*\/?>/g;

function attributesOf(source) {
    const attributes = new Map();
    for (const [, name, double, single, bare] of source.matchAll(ATTRIBUTE)) {
        attributes.set(name.toLowerCase(), double ?? single ?? bare ?? "");
    }
    return attributes;
}

/** Tags in document order, with the text of raw-text elements removed. */
function tagsOf(html) {
    const markup = html
        .replace(/<!--[\s\S]*?-->/g, "")
        .replace(/(<(script|style)\b[^>]*>)[\s\S]*?(<\/\2\s*>)/gi, "$1$3");
    return {
        markup,
        tags: [...markup.matchAll(TAG)].map(([source, name, attrs]) => ({
            source,
            name: name.toLowerCase(),
            attributes: attributesOf(attrs),
        })),
    };
}

function scripts(html) {
    return [...html.replace(/<!--[\s\S]*?-->/g, "").matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)].map(
        ([source, attrs, body]) => ({ source, attributes: attributesOf(attrs), body }),
    );
}

for (const page of pages) {
    test(`${page} has no inline script, inline event handler or javascript: URL`, async () => {
        const html = await read(`../${page}`);
        const found = scripts(html);
        assert.equal(
            found.length,
            (html.replace(/<!--[\s\S]*?-->/g, "").match(/<script\b/gi) ?? []).length,
            "every <script> has a matching </script>",
        );
        for (const script of found) {
            assert.ok(script.attributes.get("src"), `inline <script> is blocked by the CSP: ${script.source.slice(0, 80)}`);
            assert.equal(script.body.trim(), "", `a <script src> must not carry inline code: ${script.source.slice(0, 80)}`);
        }
        const { markup, tags } = tagsOf(html);
        for (const tag of tags) {
            for (const [name, value] of tag.attributes) {
                assert.doesNotMatch(name, /^on/, `inline event handler ${name} on <${tag.name}>`);
                assert.doesNotMatch(value.trim(), /^javascript:/i, `javascript: URL in <${tag.name} ${name}>`);
            }
        }
        // Backstop for markup the tokenizer above does not recognise as a tag.
        assert.doesNotMatch(markup, /<[^>]*\son[a-z]+\s*=/i, "inline event handler attribute");
    });

    test(`${page} loads no cross-origin subresource`, async () => {
        const html = await read(`../${page}`);
        const loadingRels = [
            "stylesheet", "icon", "apple-touch-icon", "mask-icon", "manifest", "preload",
            "modulepreload", "prefetch", "preconnect", "dns-prefetch",
        ];
        for (const tag of tagsOf(html).tags) {
            const rels = (tag.attributes.get("rel") ?? "").toLowerCase().split(/\s+/);
            const loads =
                ["script", "img", "iframe", "source", "audio", "video", "embed", "object", "track"].includes(tag.name) ||
                (tag.name === "link" && rels.some(rel => loadingRels.includes(rel)));
            if (!loads) {
                continue;
            }
            for (const name of ["src", "href", "data", "srcset"]) {
                const value = tag.attributes.get(name);
                if (value === undefined) {
                    continue;
                }
                const urls = name === "srcset" ? value.split(",").map(part => part.trim().split(/\s+/)[0]) : [value.trim()];
                for (const url of urls) {
                    const crossOrigin = url.startsWith("//") ||
                        (/^[a-z][a-z0-9+.-]*:/i.test(url) && !/^(data|blob):/i.test(url));
                    assert.equal(crossOrigin, false, `<${tag.name} ${name}="${value}"> must be same-origin`);
                }
            }
        }
    });
}

// checkDist (check.sh, build-web.sh) byte-compares exactly these entries.
test("Trunk copies the served pages and every script and stylesheet they load into dist", async () => {
    const copied = new Map(trunkCopyFiles(await read("../index.html")));
    for (const page of pages.filter(page => page !== "index.html")) {
        assert.equal(copied.get(page), page, `index.html copies ${page} to the dist root`);
    }
    assert.equal(copied.get("assets/_headers"), "_headers");
    assert.equal(copied.get("assets/sw.js"), "sw.js");
    const served = new Set(copied.values());
    for (const page of pages) {
        for (const tag of tagsOf(await read(`../${page}`)).tags) {
            const rels = (tag.attributes.get("rel") ?? "").toLowerCase().split(/\s+/);
            const url =
                tag.name === "script" ? tag.attributes.get("src")
                : tag.name === "link" && rels.includes("stylesheet") ? tag.attributes.get("href")
                : undefined;
            if (url !== undefined) {
                assert.ok(served.has(url), `${page} loads ${url}, which no Trunk copy-file entry puts in dist`);
            }
        }
    }
    await Promise.all([...copied.keys()].map(path => readFile(crateFile(path))));
});

test("Trunk injects no inline loader and keeps stable file names", async () => {
    const trunk = await read("../Trunk.toml");
    assert.match(trunk, /^\[build\]$/m);
    assert.match(trunk, /^inject_scripts = false$/m);
    assert.match(trunk, /^filehash = false$/m);
});

test("index.html runs boot.js synchronously before the app-init.js module", async () => {
    const html = await read("../index.html");
    const found = scripts(html).map(script => script.attributes);
    const boot = found.findIndex(attributes => attributes.get("src") === "assets/boot.js");
    const init = found.findIndex(attributes => attributes.get("src") === "assets/app-init.js");
    assert.ok(boot >= 0 && init >= 0);
    assert.ok(boot < init, "boot.js must come before app-init.js");
    // A classic parser-blocking script: it runs before any module script, while
    // location still holds the share fragment.
    for (const flag of ["type", "defer", "async"]) {
        assert.equal(found[boot].has(flag), false, `boot.js must not be ${flag}`);
    }
    assert.equal(found[init].get("type"), "module");
    assert.ok(html.indexOf("assets/boot.js\"></script>") > html.indexOf('id="loading_text"'));
    // Both preloads come after <base> so they resolve against the public URL,
    // and the wasm preload matches init()'s CORS-mode fetch.
    const { tags } = tagsOf(html);
    const base = tags.findIndex(tag => tag.name === "base");
    const modulePreload = tags.findIndex(tag => tag.name === "link" && tag.attributes.get("rel") === "modulepreload");
    const wasmPreload = tags.findIndex(tag => tag.name === "link" && tag.attributes.get("rel") === "preload");
    assert.ok(base >= 0 && modulePreload > base && wasmPreload > base);
    assert.equal(tags[modulePreload].attributes.get("href"), "p2p-transfer.js");
    const wasm = tags[wasmPreload].attributes;
    assert.equal(wasm.get("href"), "p2p-transfer_bg.wasm");
    assert.equal(wasm.get("as"), "fetch");
    assert.equal(wasm.get("type"), "application/wasm");
    assert.equal(wasm.get("crossorigin"), "");
});

test("theme.html loads the theme lab script at the end of <body>", async () => {
    const html = await read("../theme.html");
    assert.match(html, /<script src="assets\/theme-lab\.js"><\/script>\n<\/body>/);
    const source = await read("../assets/theme-lab.js");
    assert.doesNotMatch(source, /\.onclick\s*=/, "use addEventListener");
    // Markup built at runtime must not reintroduce inline handlers either.
    assert.doesNotMatch(source, /<[^>]*\son[a-z]+\s*=/i);
    new vm.Script(source, { filename: "theme-lab.js" });
});

// app-init.js resolves ../p2p-transfer.js and ../p2p-transfer_bg.wasm against
// its own URL; run it from a scratch dist-shaped directory with a fake module.
async function runAppInit({ initModule }) {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-app-init-"));
    await mkdir(join(dir, "assets"));
    await copyFile(crateFile("assets/app-init.js"), join(dir, "assets/app-init.js"));
    if (initModule !== undefined) {
        await writeFile(join(dir, "p2p-transfer.js"), initModule);
    }
    const loading = {
        text: undefined,
        set textContent(value) {
            this.text = value;
        },
        set innerHTML(_value) {
            throw new Error("app-init.js must not write HTML");
        },
    };
    globalThis.__oxferInitCalls = [];
    globalThis.document = {
        getElementById(id) {
            assert.equal(id, "loading_text");
            return loading;
        },
    };
    try {
        const outcome = await import(pathToFileURL(join(dir, "assets/app-init.js")).href).then(
            () => ({ ok: true }),
            error => ({ ok: false, error }),
        );
        return { dir, loading, calls: globalThis.__oxferInitCalls, ...outcome };
    } finally {
        delete globalThis.document;
        delete globalThis.__oxferInitCalls;
        await rm(dir, { recursive: true, force: true });
    }
}

test("app-init.js initialises the wasm bundle from the dist root", async () => {
    const run = await runAppInit({
        initModule: "export default async function init(options) { globalThis.__oxferInitCalls.push(options); }\n",
    });
    assert.equal(run.ok, true, run.error?.stack);
    assert.equal(run.calls.length, 1);
    const [options] = run.calls;
    assert.deepEqual(Object.keys(options), ["module_or_path"]);
    assert.equal(Object.getPrototypeOf(options), Object.prototype, "wasm-bindgen only unpacks a plain object");
    assert.ok(options.module_or_path instanceof URL);
    assert.equal(options.module_or_path.href, pathToFileURL(join(run.dir, "p2p-transfer_bg.wasm")).href);
    assert.equal(run.loading.text, undefined, "the loading text stays until the app replaces it");
});

test("app-init.js reports a failed start as plain text and rethrows", async () => {
    const failed = await runAppInit({
        initModule: "export default async function init() { throw new Error('<b>boom</b>'); }\n",
    });
    assert.equal(failed.ok, false);
    assert.equal(failed.error.message, "<b>boom</b>");
    assert.equal(failed.loading.text, "Oxfer could not start. See the developer console for details.");

    const missing = await runAppInit({ initModule: undefined });
    assert.equal(missing.ok, false, "a missing p2p-transfer.js rejects");
    assert.equal(missing.loading.text, "Oxfer could not start. See the developer console for details.");
});

function runBoot({ hash, controlled = false, serviceWorker = true }) {
    const events = [];
    const href = `https://oxfer.app/${hash}`;
    const container = {
        controller: controlled ? {} : null,
        register(url, options) {
            events.push(["register", url.href, options.scope]);
            return Promise.resolve({});
        },
        getRegistrations() {
            events.push(["getRegistrations"]);
            return Promise.resolve([{ unregister: async () => events.push(["unregister"]) }]);
        },
    };
    const context = {
        URL,
        Promise,
        console,
        location: { href, hash, reload: () => events.push(["reload"]) },
        navigator: serviceWorker ? { serviceWorker: container } : {},
        document: { baseURI: "https://oxfer.app/" },
        history: { replaceState: (state, title, url) => events.push(["replaceState", state, title, url]) },
        caches: {
            keys: async () => ["oxfer-v3"],
            delete: async key => events.push(["deleteCache", key]),
        },
    };
    context.window = context;
    vm.createContext(context);
    return read("../assets/boot.js").then(async source => {
        vm.runInContext(source, context, { filename: "boot.js" });
        await new Promise(resolve => setTimeout(resolve, 0));
        return { context, events, href };
    });
}

test("boot.js registers the service worker at the public URL", async () => {
    const { context, events } = await runBoot({ hash: "#ticket=abc&cap=def" });
    assert.deepEqual(events, [["register", "https://oxfer.app/sw.js", "https://oxfer.app/"]]);
    assert.ok(context.p2pServiceWorkerRegistration instanceof Promise);
    const none = await runBoot({ hash: "", serviceWorker: false });
    assert.deepEqual(none.events, []);
});

test("boot.js #dev clears workers and caches, then restores the captured URL once", async () => {
    const controlled = await runBoot({ hash: "#dev&ticket=abc", controlled: true });
    assert.deepEqual(controlled.events, [
        ["getRegistrations"],
        ["unregister"],
        ["deleteCache", "oxfer-v3"],
        ["replaceState", null, "", controlled.href],
        ["reload"],
    ]);
    const fresh = await runBoot({ hash: "#dev" });
    assert.deepEqual(fresh.events, [["getRegistrations"], ["unregister"], ["deleteCache", "oxfer-v3"]]);
});

test("privacy.html names the operator relay that the relay go-live guard requires", async () => {
    const privacy = await read("../privacy.html");
    assert.ok(
        privacy.replace(/<!--[\s\S]*?-->/g, "").includes(new URL(OPERATOR_RELAY).host),
        "privacy.html no longer names relay.oxfer.app in its text; change OPERATOR_RELAY in package-cf-output.mjs to the relay it describes",
    );
});

// Scratch copies of the legal pages, unfilled or with every placeholder filled.
async function withLegalPages(fn) {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-legal-"));
    try {
        const sources = await Promise.all(LEGAL_PAGES.map(async page => [page, await read(`../${page}`)]));
        const write = async filled => {
            for (const [page, html] of sources) {
                await writeFile(join(dir, page), filled ? html.replace(/\[\[[A-Z][A-Z0-9_]*\]\]/g, "Filled") : html);
            }
        };
        return await fn(dir, write);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
}

const ALLOW_HINT = /\nFor a local test build only, set OXFER_ALLOW_PLACEHOLDERS=1\.$/;
const NEEDS_OPERATOR_RELAY =
    /\nThe filled legal pages describe the relay at https:\/\/relay\.oxfer\.app, so P2P_RELAY_URL must list only it; /;
// Invisible characters fail the relay-list check first, and a value of only
// non-ASCII spaces is not mistaken for "unset".
const INVISIBLE_RELAYS = [
    ["\uFEFFhttps://relay.oxfer.app", "U\\+FEFF"],
    ["https://relay.oxfer.app\u00A0", "U\\+00A0"],
    ["https://relay\u200B.oxfer.app", "U\\+200B"],
    ["\u00A0", "U\\+00A0"],
    ["\u3000", "U\\+3000"],
];

test("checkLegalPages refuses placeholders, and filled pages unless the relay list is the operator relay alone", async () => {
    await withLegalPages(async (crate, write) => {
        const check = (relayUrl, allowPlaceholders = false) => checkLegalPages({ crate, relayUrl, allowPlaceholders });
        const rejects = (promise, patterns, label) =>
            assert.rejects(promise, error => {
                for (const pattern of patterns) {
                    assert.match(error.message, pattern, label);
                }
                return true;
            });

        // Unfilled pages never package, whatever the relay, unless explicitly allowed.
        await write(false);
        await rejects(check(OPERATOR_RELAY), [
            /^privacy\.html still contains the placeholder \[\[EFFECTIVE_DATE\]\]\n/,
            /\nterms\.html still contains the placeholder \[\[OPERATOR_NAME\]\]\n/,
            /\nReplace the placeholders in privacy\.html, terms\.html and abuse\.html before deploying\.\n/,
            ALLOW_HINT,
        ]);
        const allowed = await check(undefined, true);
        assert.ok(allowed.includes("privacy.html still contains the placeholder [[RELAY_LOCATION]]"));
        assert.equal(allowed.at(-1), "OXFER_ALLOW_PLACEHOLDERS=1: packaging anyway. Do not deploy this build.");

        // Filled pages need the relay they describe, and no other.
        await write(true);
        for (const [relay, reason] of [
            [undefined, /^P2P_RELAY_URL is not set, so the build uses n0's public relays, not https:\/\/relay\.oxfer\.app\.\n/],
            ["", /is not set/],
            [" \t\n", /is not set/],
            ["https://eu.relay.example", /^P2P_RELAY_URL does not list https:\/\/relay\.oxfer\.app\.\n/],
            ["http://relay.oxfer.app", /does not list/],
            ["https://relay.oxfer.app:8443", /does not list/],
            [" https://eu.relay.example , https://relay.oxfer.app ", /^P2P_RELAY_URL lists other relays besides https:\/\/relay\.oxfer\.app\.\n/],
        ]) {
            await rejects(check(relay), [reason, NEEDS_OPERATOR_RELAY, ALLOW_HINT], `filled, relay ${JSON.stringify(relay)}`);
        }
        for (const relay of [OPERATOR_RELAY, "https://relay.oxfer.app/", " https://relay.oxfer.app , https://relay.oxfer.app/ "]) {
            assert.deepEqual(await check(relay), [], `filled, relay ${JSON.stringify(relay)}`);
        }
        assert.deepEqual(await check("https://eu.relay.example,https://relay.oxfer.app", true), [
            "P2P_RELAY_URL lists other relays besides https://relay.oxfer.app.",
            "OXFER_ALLOW_PLACEHOLDERS=1: packaging filled legal pages that describe https://relay.oxfer.app in a " +
                "build whose P2P_RELAY_URL does not list only it. Do not deploy this build.",
        ]);

        for (const filled of [false, true]) {
            await write(filled);
            for (const [relay, code] of INVISIBLE_RELAYS) {
                for (const allow of [false, true]) {
                    await rejects(
                        check(relay, allow),
                        [new RegExp(`^P2P_RELAY_URL entry 1 contains the invisible character ${code}; retype the value$`)],
                        `${JSON.stringify(relay)} filled=${filled} allow=${allow}`,
                    );
                }
            }
        }
    });
});

// build-web.sh's own part of the guards is the one node call; run that call
// from a scratch copy of package-cf-output.mjs next to scratch legal pages.
test("build-web.sh runs the relay and legal-page guards in one node call before the Trunk build", async () => {
    const build = await read("../build-web.sh");
    const guards = build.indexOf("\nnode package-cf-output.mjs --check-relay --check-legal\n");
    const trunk = build.indexOf('\n"$tools/trunk" build ');
    const dist = build.indexOf("\nnode package-cf-output.mjs --check-dist\n");
    const packaging = build.indexOf("\nnode package-cf-output.mjs\n");
    assert.ok(guards >= 0 && trunk > guards, "the guards run before the Trunk build");
    assert.ok(dist > trunk && packaging > dist, "the dist check runs after the Trunk build, before packaging");
    assert.ok(
        !build.includes(new URL(OPERATOR_RELAY).host),
        "build-web.sh leaves the operator relay to OPERATOR_RELAY in package-cf-output.mjs",
    );
    await withLegalPages(async (dir, write) => {
        await copyFile(crateFile("package-cf-output.mjs"), join(dir, "package-cf-output.mjs"));
        const run = (relay, allow) => {
            const { P2P_RELAY_URL: _relay, OXFER_ALLOW_PLACEHOLDERS: _allow, ...env } = process.env;
            if (relay !== undefined) env.P2P_RELAY_URL = relay;
            if (allow !== undefined) env.OXFER_ALLOW_PLACEHOLDERS = allow;
            return spawnSync(process.execPath, ["package-cf-output.mjs", "--check-relay", "--check-legal"], {
                cwd: dir,
                encoding: "utf8",
                env,
            });
        };
        await write(false);
        const placeholders = run(OPERATOR_RELAY);
        assert.equal(placeholders.status, 1, placeholders.stderr);
        assert.equal(placeholders.stdout, "connect-src relay sources: https://relay.oxfer.app wss://relay.oxfer.app\n");
        assert.match(placeholders.stderr, /^package-cf-output: privacy\.html still contains the placeholder \[\[EFFECTIVE_DATE\]\]$/m);
        const allowed = run(undefined, "1");
        assert.equal(allowed.status, 0, allowed.stderr);
        assert.match(allowed.stderr, /\nOXFER_ALLOW_PLACEHOLDERS=1: packaging anyway\. Do not deploy this build\.\n$/);
        assert.equal(run(undefined, "true").status, 1, "only OXFER_ALLOW_PLACEHOLDERS=1 bypasses the guards");

        await write(true);
        const live = run(OPERATOR_RELAY);
        assert.equal(live.status, 0, live.stderr);
        assert.equal(live.stderr, "");
        const mixed = run("https://eu.relay.example,https://relay.oxfer.app");
        assert.equal(mixed.status, 1);
        assert.match(mixed.stderr, /^package-cf-output: P2P_RELAY_URL lists other relays besides https:\/\/relay\.oxfer\.app\.$/m);
        for (const [relay, code] of INVISIBLE_RELAYS) {
            const result = run(relay, "1");
            assert.equal(result.status, 1, JSON.stringify(relay));
            assert.equal(result.stdout, "", "the relay check fails before printing any source");
            assert.match(result.stderr, new RegExp(`entry 1 contains the invisible character ${code}`));
            assert.doesNotMatch(result.stderr, /placeholder|filled legal pages/);
        }
    });
});
