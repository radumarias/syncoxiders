// Static pages must work under the Content-Security-Policy in assets/_headers:
// script-src 'self' 'wasm-unsafe-eval' allows no inline script, no inline event
// handler attribute, no javascript: URL and no cross-origin subresource.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import vm from "node:vm";
import { OPERATOR_RELAY } from "../package-cf-output.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const crateFile = path => new URL(`../${path}`, import.meta.url);

// Every HTML file Trunk copies into dist and the Worker serves.
const pages = ["index.html", "theme.html", "privacy.html", "terms.html", "abuse.html"];
const legalPages = ["privacy.html", "terms.html", "abuse.html"];

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

test("the legal pages are copied into dist by Trunk", async () => {
    const html = await read("../index.html");
    for (const page of legalPages) {
        assert.match(html, new RegExp(`<link data-trunk rel="copy-file" href="${page.replace(".", "\\.")}">`));
    }
    for (const asset of ["boot.js", "app-init.js", "theme-lab.js", "legal.css"]) {
        assert.match(
            html,
            new RegExp(`<link data-trunk rel="copy-file" href="assets/${asset.replace(".", "\\.")}" data-target-path="assets">`),
        );
    }
    await Promise.all(["assets/legal.css", ...legalPages].map(path => readFile(crateFile(path))));
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

/** The lines of build-web.sh between `# BEGIN name` and `# END name`. */
function block(build, name) {
    const begin = build.indexOf(`\n# BEGIN ${name}`);
    const end = build.indexOf(`\n# END ${name}\n`);
    assert.ok(begin >= 0 && end > begin, `build-web.sh has a "${name}" block`);
    return build.slice(begin + 1, end + 1);
}

// Runs build-web.sh's relay-list check and legal-page guards (not the Trunk
// build) in a scratch directory holding dist/ copies of the legal pages.
test("build-web.sh packages filled legal pages only with a relay list that includes relay.oxfer.app", async () => {
    const build = await read("../build-web.sh");
    assert.match(build, new RegExp(`^operator_relay=${OPERATOR_RELAY.replaceAll(".", "\\.")}$`, "m"));
    const script = `set -euo pipefail\n${block(build, "relay-list check")}${block(build, "legal-page guards")}echo "guards passed"\n`;
    const privacy = await read("../privacy.html");
    assert.ok(
        privacy.replace(/<!--[\s\S]*?-->/g, "").includes("relay.oxfer.app"),
        "privacy.html no longer names relay.oxfer.app in its text; update the relay guard in build-web.sh to match the relay it describes",
    );
    const dir = await mkdtemp(join(tmpdir(), "oxfer-build-guards-"));
    try {
        await symlink(fileURLToPath(crateFile("package-cf-output.mjs")), join(dir, "package-cf-output.mjs"));
        await writeFile(join(dir, "guards.sh"), script);
        const pages = Object.fromEntries(await Promise.all(legalPages.map(async page => [page, await read(`../${page}`)])));
        const run = async ({ filled, relay, allow = false }) => {
            await rm(join(dir, "dist"), { recursive: true, force: true });
            await mkdir(join(dir, "dist"));
            for (const [page, html] of Object.entries(pages)) {
                await writeFile(join(dir, "dist", page), filled ? html.replace(/\[\[[A-Z][A-Z0-9_]*\]\]/g, "Filled") : html);
            }
            const { P2P_RELAY_URL: _relay, OXFER_ALLOW_PLACEHOLDERS: _allow, ...env } = process.env;
            if (relay !== undefined) env.P2P_RELAY_URL = relay;
            if (allow) env.OXFER_ALLOW_PLACEHOLDERS = "1";
            const result = spawnSync("bash", ["guards.sh"], { cwd: dir, encoding: "utf8", env });
            return { ...result, output: `${result.stdout}${result.stderr}` };
        };
        const passes = (result, label) => {
            assert.equal(result.status, 0, `${label}: ${result.output}`);
            assert.match(result.stdout, /guards passed/, label);
        };
        const fails = (result, pattern, label) => {
            assert.equal(result.status, 1, `${label}: ${result.output}`);
            assert.match(result.stderr, pattern, label);
            assert.doesNotMatch(result.stdout, /guards passed/, label);
        };

        // Unfilled pages never package, whatever the relay, unless explicitly allowed.
        fails(await run({ filled: false, relay: OPERATOR_RELAY }), /still contains the placeholder \[\[OPERATOR_NAME\]\]/, "placeholders");
        const allowed = await run({ filled: false, relay: undefined, allow: true });
        passes(allowed, "placeholders allowed");
        assert.match(allowed.stderr, /OXFER_ALLOW_PLACEHOLDERS=1: packaging anyway\. Do not deploy this build\./);

        // Filled pages need the relay they describe.
        const missing = /The filled legal pages describe the relay at https:\/\/relay\.oxfer\.app, so P2P_RELAY_URL must list it/;
        for (const relay of [undefined, "", " \t\n", "https://eu.relay.example", "http://relay.oxfer.app", "https://relay.oxfer.app:8443"]) {
            fails(await run({ filled: true, relay }), missing, `filled, relay ${JSON.stringify(relay)}`);
        }
        for (const relay of [OPERATOR_RELAY, "https://relay.oxfer.app/", " https://eu.relay.example , https://relay.oxfer.app "]) {
            passes(await run({ filled: true, relay }), `filled, relay ${JSON.stringify(relay)}`);
        }
        const override = await run({ filled: true, relay: undefined, allow: true });
        passes(override, "filled, n0, allowed");
        assert.match(override.stderr, /does not list it\. Do not deploy this build\./);

        // Invisible characters fail the relay-list check before the build, and a
        // value of only non-ASCII spaces is not mistaken for "unset".
        for (const [relay, code] of [
            ["\uFEFFhttps://relay.oxfer.app", "U\\+FEFF"],
            ["https://relay.oxfer.app\u00A0", "U\\+00A0"],
            ["https://relay\u200B.oxfer.app", "U\\+200B"],
            ["\u00A0", "U\\+00A0"],
            ["\u3000", "U\\+3000"],
        ]) {
            for (const allow of [false, true]) {
                const result = await run({ filled: true, relay, allow });
                fails(result, new RegExp(`entry 1 contains the invisible character ${code}`), `${JSON.stringify(relay)} allow=${allow}`);
                assert.doesNotMatch(result.output, /placeholder|filled legal pages/);
            }
        }
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
