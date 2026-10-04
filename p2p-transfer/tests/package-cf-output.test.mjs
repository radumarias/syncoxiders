import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
    CSP_REPORT_ONLY,
    N0_CONNECT_SRC,
    OPERATOR_RELAY,
    RELAY_CONNECT_SRC_TOKEN,
    checkDist,
    headerRuleMatches,
    headersForPath,
    operatorRelayProblem,
    packageCfOutput,
    parseHeaders,
    parseRelayList,
    productionDomains,
    relayConnectSources,
    renderHeaders,
    securityHeaders,
    splitDirectives,
    trunkCopyFiles,
    workerConfig,
} from "../package-cf-output.mjs";
import {
    NEL_REMEDY,
    PolicyError,
    checkHeaders,
    checkedPaths,
    operatorControlsZone,
    parseOrigins,
    reportingHeaders,
    verifyOrigin,
} from "../verify-deployment.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const crate = fileURLToPath(new URL("..", import.meta.url));

// The hosting policy assets/_headers carries; verify-deployment.mjs compares
// deployments with whatever that file says, so these pin the file itself.
const CACHE_CONTROL = "public, max-age=0, must-revalidate";
const PERMISSIONS_POLICY =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), midi=(), screen-wake-lock=(self)";
// The legal pages add no-transform, which stops Cloudflare features that
// rewrite responses, such as Email Address Obfuscation of their contact
// addresses, so they are served byte for byte as built. It is set nowhere else
// because it also stops Cloudflare compressing responses, and the wasm and
// JavaScript should stay compressed. Only the legal pages contain addresses.
// https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/
// https://developers.cloudflare.com/speed/optimization/content/compression/
const NO_TRANSFORM_PATHS = ["/privacy", "/terms", "/abuse"];

/** A path's Cache-Control directives as Cloudflare serves them, without repeats. */
const directives = (rendered, path) => [...new Set(splitDirectives(headersForPath(rendered, path).get("cache-control") ?? ""))];

// The policy from the compliance plan (B1), with the relay sources left open.
const expectedCsp = relay =>
    "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; " +
    "img-src 'self' data: blob:; font-src 'self'; " +
    `connect-src 'self' ${relay}; ` +
    "worker-src 'self' blob:; frame-src 'self' blob:; manifest-src 'self'; object-src 'none'; " +
    "base-uri 'self'; form-action 'none'; frame-ancestors 'self'";

const minimalHeaders = `/*\n  Referrer-Policy: no-referrer\n  ${CSP_REPORT_ONLY}: connect-src 'self' ${RELAY_CONNECT_SRC_TOKEN}\n`;

test("cloudflare.config.ts matches the packaged Worker identity", async () => {
    const source = await read("../cloudflare.config.ts");
    assert.match(source, /from "cf\/config"/);
    assert.match(source, new RegExp(`name: "${workerConfig.name}"`));
    assert.match(source, new RegExp(`compatibilityDate: "${workerConfig.compatibilityDate}"`));
    assert.match(source, /notFoundHandling: "single-page-application"/);
    assert.match(source, /domains: \["oxfer\.app", "www\.oxfer\.app"\]/);
    assert.doesNotMatch(source, /\bentrypoint\s*:/);
    assert.doesNotMatch(source, /wrangler\.(jsonc?|toml)/);
});

test("the crate has no Wrangler config; Cloudflare hosting is cf-only", async () => {
    const names = await readdir(new URL("..", import.meta.url));
    assert.equal(names.some(name => /^wrangler\.(jsonc?|toml)$/.test(name)), false);
    const pkg = JSON.parse(await read("../package.json"));
    assert.equal(pkg.devDependencies.cf, "1.0.0-beta.5");
    assert.equal(pkg.scripts.deploy, "cf deploy --prebuilt");
    assert.equal("wrangler" in (pkg.dependencies ?? {}), false);
    assert.equal("wrangler" in (pkg.devDependencies ?? {}), false);
});

test("an unset, empty or ASCII-whitespace-only P2P_RELAY_URL keeps n0's relays", () => {
    for (const value of [undefined, null, "", " ", "\t\n ", "\r\n\v\f"]) {
        assert.deepEqual(parseRelayList(value), [], JSON.stringify(value));
        assert.equal(relayConnectSources(value), N0_CONNECT_SRC);
    }
    assert.equal(N0_CONNECT_SRC, "https://*.iroh.link wss://*.iroh.link");
});

test("relay URLs become matching HTTP(S) and WebSocket connect-src sources", () => {
    assert.equal(relayConnectSources("https://relay.oxfer.app"), "https://relay.oxfer.app wss://relay.oxfer.app");
    assert.equal(relayConnectSources("https://relay.oxfer.app/"), "https://relay.oxfer.app wss://relay.oxfer.app");
    // Scheme and host are case-normalised; the default port is dropped.
    assert.equal(relayConnectSources("HTTPS://Relay.Oxfer.App:443"), "https://relay.oxfer.app wss://relay.oxfer.app");
    assert.equal(
        relayConnectSources(" https://eu.relay.example:8443 , http://127.0.0.1:3340/ "),
        "https://eu.relay.example:8443 wss://eu.relay.example:8443 http://127.0.0.1:3340 ws://127.0.0.1:3340",
    );
    assert.equal(
        relayConnectSources("https://relay.oxfer.app,https://relay.oxfer.app/"),
        "https://relay.oxfer.app wss://relay.oxfer.app",
        "duplicates are listed once",
    );
    assert.deepEqual(
        parseRelayList("https://a.example,https://b.example:444").map(url => url.href),
        ["https://a.example/", "https://b.example:444/"],
    );
});

test("malformed relay lists are rejected with the entry position and reason", () => {
    const cases = [
        ["https://a.example,,https://b.example", /entry 2 is empty/],
        ["https://a.example,", /entry 2 is empty/],
        [",https://a.example", /entry 1 is empty/],
        ["relay.oxfer.app", /not an absolute URL/],
        ["wss://relay.oxfer.app", /uses wss:/],
        ["ftp://relay.oxfer.app", /uses ftp:/],
        ["https://user:secret@relay.oxfer.app", /credentials/],
        ["https://user@relay.oxfer.app", /credentials/],
        ["https://relay.oxfer.app/relay", /path/],
        ["https://relay.oxfer.app/?token=1", /query/],
        ["https://relay.oxfer.app/?", /query/],
        ["https://relay.oxfer.app/#frag", /fragment/],
        ["https://relay.oxfer.app#", /fragment/],
        ["https://relay .oxfer.app", /whitespace/],
        ["https://[::1]:3340", /Content-Security-Policy source cannot express/],
        ["https://relay.oxfer.app.", /Content-Security-Policy source cannot express/],
    ];
    for (const [value, reason] of cases) {
        assert.throws(() => parseRelayList(value), reason, value);
        assert.throws(
            () => parseRelayList(value),
            error => !error.message.includes("relay.oxfer.app") && !error.message.includes("secret"),
            "errors name the entry position, never its text",
        );
        assert.throws(() => relayConnectSources(value), reason, value);
        assert.throws(() => renderHeaders(minimalHeaders, value), reason, value);
    }
    assert.throws(() => parseRelayList(42), TypeError);
});

// src/node.rs trims with Rust's str::trim (Unicode White_Space), JS trim() strips
// a different set that includes U+FEFF. A value one parser accepts and the other
// rejects would make the wasm fall back to n0's relays at run time, so this
// parser rejects everything outside printable ASCII and trims only spaces.
test("relay lists with a character outside printable ASCII fail, naming invisible ones", () => {
    const cases = [
        ["\uFEFFhttps://relay.oxfer.app", 1, "U+FEFF"],
        ["https://relay.oxfer.app\uFEFF", 1, "U+FEFF"],
        ["https://a.example,\uFEFFhttps://relay.oxfer.app", 2, "U+FEFF"],
        ["\uFEFF", 1, "U+FEFF"],
        ["\u00A0https://relay.oxfer.app", 1, "U+00A0"],
        ["https://relay.oxfer.app,\u00A0", 2, "U+00A0"],
        ["https://relay\u200B.oxfer.app", 1, "U+200B"],
        ["\u200Bhttps://relay.oxfer.app", 1, "U+200B"],
        ["https://relay.oxfer.app\n", 1, "U+000A"],
        ["https://a.example,\thttps://relay.oxfer.app", 2, "U+0009"],
        ["https://a.example\u0085", 1, "U+0085"],
        ["https://a.example,https://b.example,https://c.example\u2028", 3, "U+2028"],
        ["https://relay.oxfer.app\u{E0020}", 1, "U+E0020"],
    ];
    for (const [value, entry, code] of cases) {
        const label = JSON.stringify(value);
        assert.throws(
            () => parseRelayList(value),
            new RegExp(`^Error: P2P_RELAY_URL entry ${entry} contains the invisible character ${code.replace("+", "\\+")}; retype the value$`),
            label,
        );
        assert.throws(() => relayConnectSources(value), /invisible character/, label);
        assert.throws(() => renderHeaders(minimalHeaders, value), /invisible character/, label);
        assert.throws(() => operatorRelayProblem(value), /invisible character/, label);
    }
    // A visible non-ASCII character is not repeated: it could be part of a credential.
    for (const value of ["https://relé.oxfer.app", "https://user:sécret@relay.oxfer.app", "https://relay.oxfer.app/ü"]) {
        assert.throws(
            () => parseRelayList(value),
            error => error.message === "P2P_RELAY_URL entry 1 contains a character outside printable ASCII",
            value,
        );
    }
    // Spaces around entries are still trimmed, exactly as src/node.rs does.
    assert.equal(relayConnectSources("  https://relay.oxfer.app ,https://b.example  "),
        "https://relay.oxfer.app wss://relay.oxfer.app https://b.example wss://b.example");
});

test("the relay go-live guard accepts a relay list of the operator relay alone", () => {
    assert.equal(OPERATOR_RELAY, "https://relay.oxfer.app");
    for (const value of [
        "https://relay.oxfer.app",
        "https://relay.oxfer.app/",
        "HTTPS://Relay.Oxfer.App:443",
        " https://relay.oxfer.app , https://relay.oxfer.app/ ",
    ]) {
        assert.equal(operatorRelayProblem(value), null, value);
    }
    for (const [value, reason] of [
        [undefined, /^P2P_RELAY_URL is not set, so the build uses n0's public relays, not https:\/\/relay\.oxfer\.app$/],
        ["", /is not set/],
        ["  ", /is not set/],
        ["https://eu.relay.example", /^P2P_RELAY_URL does not list https:\/\/relay\.oxfer\.app$/],
        ["http://relay.oxfer.app", /does not list/],
        ["https://relay.oxfer.app:8443", /does not list/],
        ["https://relay.oxfer.app.example", /does not list/],
        // The filled privacy notice describes one relay; a build that also
        // uses others would send users' addresses to relays it does not name.
        [" https://eu.relay.example , https://relay.oxfer.app ", /^P2P_RELAY_URL lists other relays besides https:\/\/relay\.oxfer\.app$/],
        ["https://relay.oxfer.app,http://127.0.0.1:3340", /lists other relays/],
    ]) {
        const problem = operatorRelayProblem(value);
        assert.match(problem ?? "", reason, String(value));
        assert.doesNotMatch(problem, /eu\.relay|127\.0\.0\.1|8443/, "the reason never repeats the other entries");
    }
    assert.throws(() => operatorRelayProblem("https://relay.oxfer.app/relay"), /path/);
    assert.equal(operatorRelayProblem("https://eu.relay.example", "https://eu.relay.example/"), null);
});

test("assets/_headers carries the report-only CSP, Permissions-Policy and X-Frame-Options", async () => {
    const source = await read("../assets/_headers");
    assert.equal(source.split(RELAY_CONNECT_SRC_TOKEN).length, 2, "exactly one relay token");
    const rules = parseHeaders(source);
    assert.deepEqual(rules.map(([pattern]) => pattern), ["/*", "/sw.js", ...NO_TRANSFORM_PATHS]);
    const all = Object.fromEntries(rules[0][1]);
    assert.equal(all["Cache-Control"], CACHE_CONTROL);
    assert.equal(all["Referrer-Policy"], "no-referrer");
    assert.equal(all["X-Content-Type-Options"], "nosniff");
    assert.equal(all[CSP_REPORT_ONLY], expectedCsp(RELAY_CONNECT_SRC_TOKEN));
    assert.equal(all["Permissions-Policy"], PERMISSIONS_POLICY);
    assert.equal(all["X-Frame-Options"], "SAMEORIGIN");
    assert.equal(rules[0][1].length, 6, "the /* rule sets these six headers only");
    // Enforcement waits for the manual cross-browser matrix (plan B1).
    assert.equal("Content-Security-Policy" in all, false);
    assert.deepEqual(rules[1][1], [["Cache-Control", "no-cache, no-store, must-revalidate"]]);
    for (const [, headers] of rules.slice(2)) {
        assert.deepEqual(headers, [["Cache-Control", "no-transform"]]);
    }
});

// Cloudflare joins the values of every matching rule with a comma
// (https://developers.cloudflare.com/workers/static-assets/headers/).
test("assets/_headers sets no-transform on the legal pages only, so the wasm and JS stay compressed", async () => {
    const rendered = renderHeaders(await read("../assets/_headers"), "");
    const base = splitDirectives(CACHE_CONTROL);
    for (const path of [...checkedPaths, "/privacy.html", "/privacy/", "/assets/oxfer-mark.svg"]) {
        const expected =
            path === "/sw.js"
                ? [...base, "no-cache", "no-store"]
                : NO_TRANSFORM_PATHS.includes(path)
                  ? [...base, "no-transform"]
                  : base;
        assert.deepEqual(directives(rendered, path), expected, path);
    }
    for (const path of NO_TRANSFORM_PATHS) {
        assert.ok(checkedPaths.includes(path), `verify-deployment.mjs checks ${path}`);
    }
});

test("headersForPath applies every matching rule and joins repeated headers as Cloudflare does", async () => {
    const rendered = renderHeaders(await read("../assets/_headers"), "");
    const root = headersForPath(rendered, "/");
    assert.deepEqual([...root.keys()], [
        "cache-control",
        CSP_REPORT_ONLY.toLowerCase(),
        "permissions-policy",
        "referrer-policy",
        "x-content-type-options",
        "x-frame-options",
    ]);
    assert.equal(root.get(CSP_REPORT_ONLY), expectedCsp(N0_CONNECT_SRC));
    assert.equal(headersForPath(rendered, "/privacy").get("cache-control"), `${CACHE_CONTROL}, no-transform`);
    assert.equal(headersForPath(rendered, "/sw.js").get("cache-control"), `${CACHE_CONTROL}, no-cache, no-store, must-revalidate`);
    const custom = headersForPath("/*\n  X-A: 1\n/files/:name\n  X-A: 2\n  X-B: b\n", "/files/report");
    assert.deepEqual([...custom], [["x-a", "1, 2"], ["x-b", "b"]]);
    assert.deepEqual([...headersForPath("/files/:name\n  X-B: b\n", "/files/a/b")], []);
});

test("headerRuleMatches follows Cloudflare's splat and placeholder rules", () => {
    for (const [pattern, path, expected] of [
        ["/*", "/", true],
        ["/*", "/assets/app-init.js", true],
        ["/sw.js", "/sw.js", true],
        ["/sw.js", "/swxjs", false],
        ["/privacy", "/privacy", true],
        ["/privacy", "/privacy.html", false],
        ["/privacy", "/privacy/", false],
        ["/assets/*", "/assets/legal.css", true],
        ["/assets/*", "/privacy", false],
        ["/files/:name/raw", "/files/a/raw", true],
        ["/files/:name/raw", "/files/a/b/raw", false],
    ]) {
        assert.equal(headerRuleMatches(pattern, path), expected, `${pattern} ${path}`);
    }
    assert.throws(() => headerRuleMatches("https://oxfer.app/*", "/"), /only path patterns/);
});

test("renderHeaders fills the relay sources for the n0 default and a custom relay", async () => {
    const source = await read("../assets/_headers");
    for (const [value, relay] of [
        [undefined, "https://*.iroh.link wss://*.iroh.link"],
        ["", "https://*.iroh.link wss://*.iroh.link"],
        ["https://relay.oxfer.app", "https://relay.oxfer.app wss://relay.oxfer.app"],
    ]) {
        const rendered = renderHeaders(source, value);
        assert.equal(rendered, source.replace(RELAY_CONNECT_SRC_TOKEN, relay));
        assert.doesNotMatch(rendered, /\{\{/);
        assert.deepEqual(securityHeaders(rendered), {
            [CSP_REPORT_ONLY]: expectedCsp(relay),
            "Permissions-Policy": PERMISSIONS_POLICY,
            "X-Frame-Options": "SAMEORIGIN",
        });
    }
});

test("renderHeaders refuses a file without the token or with another unrendered token", () => {
    assert.throws(() => renderHeaders("/*\n  X-Frame-Options: SAMEORIGIN\n", undefined), /no \{\{RELAY_CONNECT_SRC\}\} token/);
    assert.throws(
        () => renderHeaders(`${minimalHeaders}  X-Other: {{SOMETHING_ELSE}}\n`, undefined),
        /still contains \{\{SOMETHING_ELSE\}\}/,
    );
    const many = Array.from({ length: 200 }, (_, i) => `https://r${i}.relay-with-a-long-name.example`).join(",");
    assert.throws(() => renderHeaders(minimalHeaders, many), /Cloudflare allows 2000/);
});

test("securityHeaders refuses duplicated or path-specific security headers", () => {
    const base = renderHeaders(minimalHeaders, "");
    assert.throws(() => securityHeaders(base), /no Permissions-Policy/);
    const complete = `${base}  Permissions-Policy: ${PERMISSIONS_POLICY}\n  X-Frame-Options: SAMEORIGIN\n`;
    assert.equal(securityHeaders(complete)["X-Frame-Options"], "SAMEORIGIN");
    assert.throws(() => securityHeaders(`${complete}\n/sw.js\n  X-Frame-Options: DENY\n`), /only under \/\*/);
    assert.throws(() => securityHeaders(`${complete}  x-frame-options: DENY\n`), /twice/);
    assert.throws(() => parseHeaders("  Orphan: header\n"), /under a path rule/);
});

test("package-cf-output copies Trunk dist into an assets-only cf Build Output", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-"));
    try {
        const dist = join(dir, "dist");
        const headers = await read("../assets/_headers");
        await mkdir(join(dist, "assets"), { recursive: true });
        await writeFile(join(dist, "index.html"), "<!doctype html><title>oxfer</title>\n");
        await writeFile(join(dist, "sw.js"), "self.addEventListener('fetch', () => {});\n");
        await writeFile(join(dist, "_headers"), headers);
        await writeFile(join(dist, "assets/favicon.js"), "/* favicon */\n");
        const result = await packageCfOutput({
            dist,
            outputRoot: join(dir, ".cloudflare/output"),
            includeDomains: false,
            relayUrl: "https://relay.oxfer.app",
        });
        const { assets, config } = result;
        assert.deepEqual(config, workerConfig);
        assert.equal(
            await readFile(join(dir, ".cloudflare/output/v0/config.json"), "utf8"),
            `${JSON.stringify({ buildContext: { isPreview: false } })}\n`,
        );
        assert.equal(
            await readFile(join(assets, "index.html"), "utf8"),
            await readFile(join(dist, "index.html"), "utf8"),
        );
        assert.equal(
            await readFile(join(assets, "assets/favicon.js"), "utf8"),
            "/* favicon */\n",
        );
        // dist/_headers stays a byte copy of assets/_headers; only the package is rendered.
        assert.equal(await readFile(join(dist, "_headers"), "utf8"), headers);
        const packaged = await readFile(join(assets, "_headers"), "utf8");
        assert.equal(packaged, result.headers);
        assert.equal(packaged, headers.replace(RELAY_CONNECT_SRC_TOKEN, "https://relay.oxfer.app wss://relay.oxfer.app"));
        assert.equal(
            JSON.parse(await readFile(join(dir, ".cloudflare/output/v0/workers/default/worker.config.json"), "utf8")).assets.notFoundHandling,
            "single-page-application",
        );
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("package-cf-output reads P2P_RELAY_URL from the environment by default", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-env-"));
    const saved = process.env.P2P_RELAY_URL;
    try {
        const dist = join(dir, "dist");
        await mkdir(dist, { recursive: true });
        await writeFile(join(dist, "_headers"), minimalHeaders);
        process.env.P2P_RELAY_URL = "https://relay.oxfer.app";
        const custom = await packageCfOutput({ dist, outputRoot: join(dir, "out-env") });
        assert.match(custom.headers, /connect-src 'self' https:\/\/relay\.oxfer\.app wss:\/\/relay\.oxfer\.app\n/);
        process.env.P2P_RELAY_URL = "   ";
        const blank = await packageCfOutput({ dist, outputRoot: join(dir, "out-blank") });
        assert.match(blank.headers, /connect-src 'self' https:\/\/\*\.iroh\.link wss:\/\/\*\.iroh\.link\n/);
        // An explicit option wins over the environment, including an explicit "unset".
        const explicit = await packageCfOutput({ dist, outputRoot: join(dir, "out-explicit"), relayUrl: undefined });
        assert.match(explicit.headers, /\*\.iroh\.link/);
    } finally {
        if (saved === undefined) {
            delete process.env.P2P_RELAY_URL;
        } else {
            process.env.P2P_RELAY_URL = saved;
        }
        await rm(dir, { recursive: true, force: true });
    }
});

test("package-cf-output writes nothing when the relay list or _headers is invalid", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-bad-"));
    try {
        const dist = join(dir, "dist");
        const outputRoot = join(dir, "out");
        await mkdir(dist, { recursive: true });
        await writeFile(join(dist, "index.html"), "<!doctype html>\n");
        await assert.rejects(
            packageCfOutput({ dist, outputRoot, relayUrl: "" }),
            { code: "ENOENT" },
            "a dist without _headers would deploy without security headers",
        );
        await writeFile(join(dist, "_headers"), minimalHeaders);
        await assert.rejects(packageCfOutput({ dist, outputRoot, relayUrl: "https://relay.oxfer.app/relay" }), /path/);
        await writeFile(join(dist, "_headers"), "/*\n  Referrer-Policy: no-referrer\n");
        await assert.rejects(packageCfOutput({ dist, outputRoot, relayUrl: "" }), /no \{\{RELAY_CONNECT_SRC\}\} token/);
        assert.deepEqual(await readdir(dir), ["dist"]);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("production packaging keeps custom domains so cf deploy cannot drop oxfer.app", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-domains-"));
    try {
        const dist = join(dir, "dist");
        await mkdir(dist, { recursive: true });
        await writeFile(join(dist, "index.html"), "<!doctype html>\n");
        await writeFile(join(dist, "_headers"), minimalHeaders);
        const withDomains = await packageCfOutput({
            dist,
            outputRoot: join(dir, "out-domains"),
            relayUrl: "",
        });
        assert.deepEqual(withDomains.config.domains, productionDomains);
        const without = await packageCfOutput({
            dist,
            outputRoot: join(dir, "out-plain"),
            includeDomains: false,
            relayUrl: "",
        });
        assert.equal("domains" in without.config, false);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

// The headers assets/_headers gives a path for the n0 default, as Cloudflare
// serves them, plus or instead of `extra`.
async function servedHeaders(path, extra = {}) {
    const rendered = renderHeaders(await read("../assets/_headers"), "");
    const expected = headersForPath(rendered, path);
    const headers = new Headers(expected);
    for (const [name, value] of Object.entries(extra)) {
        headers.set(name, value);
    }
    return { expected, headers };
}

// As served by oxfer.app and oxfer.42dev.workers.dev on 30 September 2026.
const nelHeaders = {
    NEL: '{"report_to":"cf-nel","success_fraction":0.0,"max_age":604800}',
    "Report-To": '{"group":"cf-nel","max_age":604800,"endpoints":[{"url":"https://a.nel.cloudflare.com/report/v4?s=x"}]}',
};

test("verify-deployment accepts the headers assets/_headers produces", async () => {
    for (const path of checkedPaths) {
        const { expected, headers } = await servedHeaders(path);
        assert.deepEqual(checkHeaders(path, headers, expected, { enforceNoReporting: true }), [], path);
    }
    const legal = (await servedHeaders("/privacy")).headers.get("cache-control");
    assert.equal(legal, "public, max-age=0, must-revalidate, no-transform");
    assert.equal((await servedHeaders("/sw.js")).headers.get("cache-control"),
        "public, max-age=0, must-revalidate, no-cache, no-store, must-revalidate");
    // Cache-Control is a set of directives: order, case and repeats do not matter.
    const { expected, headers } = await servedHeaders("/sw.js", { "Cache-Control": "No-Store, no-cache, PUBLIC, max-age=0, must-revalidate" });
    assert.deepEqual(checkHeaders("/sw.js", headers, expected, { enforceNoReporting: true }), []);
});

test("verify-deployment fails while Cloudflare Network Error Logging is on", async () => {
    const { expected, headers } = await servedHeaders("/privacy", nelHeaders);
    assert.deepEqual(reportingHeaders(headers), ["NEL", "Report-To"]);
    assert.throws(
        () => checkHeaders("/privacy", headers, expected, { enforceNoReporting: true }),
        error =>
            error instanceof PolicyError &&
            error.message.startsWith("/privacy: the response carries NEL and Report-To headers") &&
            error.message.includes("privacy notice does not describe") &&
            error.message.includes(NEL_REMEDY),
    );
    assert.match(NEL_REMEDY, /zone setting "nel"/);
    assert.match(NEL_REMEDY, /PATCH \/zones\/\{zone_id\}\/settings\/nel with \{"value":\{"enabled":false\}\}/);
    for (const [name, value] of Object.entries({ nel: nelHeaders.NEL, "report-to": "{}", "Reporting-Endpoints": 'default="https://r.example"' })) {
        const single = await servedHeaders("/", { [name]: value });
        assert.throws(() => checkHeaders("/", single.headers, single.expected, { enforceNoReporting: true }), /Network Error Logging/, name);
    }
});

test("verify-deployment only reports NEL on workers.dev, outside the oxfer.app zone", async () => {
    for (const host of ["oxfer.app", "www.oxfer.app", "OXFER.APP"]) {
        assert.equal(operatorControlsZone(host), true, host);
    }
    for (const host of ["oxfer.42dev.workers.dev", "abc123-oxfer.42dev.workers.dev", "Oxfer.42dev.Workers.Dev"]) {
        assert.equal(operatorControlsZone(host), false, host);
    }
    assert.equal(operatorControlsZone("workers.dev.example"), true);
    const { expected, headers } = await servedHeaders("/", nelHeaders);
    assert.deepEqual(checkHeaders("/", headers, expected, { enforceNoReporting: false }), ["NEL", "Report-To"]);
});

test("verify-deployment requires every header assets/_headers gives a path, exactly", async () => {
    const fails = async (path, extra, pattern) => {
        const { expected, headers } = await servedHeaders(path, extra);
        assert.throws(
            () => checkHeaders(path, headers, expected, { enforceNoReporting: true }),
            error => error instanceof PolicyError && error.message.startsWith(`${path}: `) && pattern.test(error.message),
            `${path} ${JSON.stringify(extra)}`,
        );
    };
    // The legal pages without no-transform could be rewritten by Cloudflare.
    for (const path of NO_TRANSFORM_PATHS) {
        await fails(path, { "Cache-Control": CACHE_CONTROL }, /; missing no-transform$/);
    }
    // The previous deployment sent no-transform everywhere, which stops compression.
    for (const path of ["/", "/p2p-transfer_bg.wasm", "/p2p-transfer.js", "/sw.js"]) {
        const { headers } = await servedHeaders(path);
        await fails(path, { "Cache-Control": `${headers.get("cache-control")}, no-transform` }, /; unexpected no-transform$/);
    }
    await fails("/", { "Cache-Control": "max-age=0, must-revalidate, no-transform" }, /; missing public; unexpected no-transform$/);
    await fails("/sw.js", { "Cache-Control": CACHE_CONTROL }, /; missing no-cache, no-store$/);
    const relayCsp = expectedCsp("https://relay.oxfer.app wss://relay.oxfer.app");
    await fails("/", { [CSP_REPORT_ONLY]: relayCsp }, /content-security-policy-report-only is ".*", assets\/_headers sets ".*" \(check P2P_RELAY_URL matches the build\)$/);
    await fails("/terms", { "Permissions-Policy": "camera=()" }, /^\/terms: permissions-policy is "camera=\(\)", assets\/_headers sets "camera=\(\), microphone/);
    // Every mismatch of a response is reported at once.
    const { expected, headers } = await servedHeaders("/abuse", { "Referrer-Policy": "origin", "Cache-Control": CACHE_CONTROL });
    headers.delete("X-Frame-Options");
    assert.throws(
        () => checkHeaders("/abuse", headers, expected, { enforceNoReporting: true }),
        error =>
            /missing no-transform/.test(error.message) &&
            /referrer-policy is "origin", assets\/_headers sets "no-referrer"/.test(error.message) &&
            /x-frame-options is missing, assets\/_headers sets "SAMEORIGIN"/.test(error.message),
    );
});

const sha256 = text => createHash("sha256").update(text).digest("hex");
const quiet = { log() {}, error() {} };
// assert.rejects matches a RegExp against "Error: message"; this matches the message.
const rejectsWith = (promise, pattern, label) =>
    assert.rejects(promise, error => {
        assert.match(error.message, pattern, label);
        return true;
    });

// What verify-deployment expects of each path: built from assets/_headers,
// with a stand-in body instead of the dist file.
async function expectedFor(paths) {
    const rendered = renderHeaders(await read("../assets/_headers"), "");
    return paths.map(path => ({
        path,
        mime: "text/html",
        canonical: path === "/privacy",
        sha256: sha256(`body of ${path}`),
        headers: headersForPath(rendered, path),
    }));
}

function respond(expected, { body = `body of ${expected.path}`, status = 200, headers = {} } = {}) {
    const served = new Headers(expected.headers);
    served.set("Content-Type", "text/html; charset=utf-8");
    for (const [name, value] of Object.entries(headers)) {
        served.set(name, value);
    }
    return new Response(body, { status, headers: served });
}

/** A fake fetch that counts requests per path and the most in flight at once. */
function fakeFetch(expected, answer) {
    const calls = [];
    let inFlight = 0;
    const fetch = async url => {
        const path = new URL(url).pathname;
        calls.push(path);
        inFlight += 1;
        fetch.maxInFlight = Math.max(fetch.maxInFlight, inFlight);
        await new Promise(resolve => setTimeout(resolve, 5));
        inFlight -= 1;
        const item = expected.find(candidate => candidate.path === path);
        return answer(item, calls.filter(call => call === path).length);
    };
    fetch.maxInFlight = 0;
    fetch.calls = calls;
    return fetch;
}

test("verify-deployment fetches the paths concurrently and retries only the failing ones", async () => {
    const expected = await expectedFor(["/", "/privacy", "/terms"]);
    // /terms serves the previous build twice before the edge catches up.
    const fetch = fakeFetch(expected, (item, count) =>
        respond(item, item.path === "/terms" && count <= 2 ? { body: "previous build" } : {}),
    );
    const logged = [];
    const log = { log: line => logged.push(line), error: line => logged.push(line) };
    assert.deepEqual(await verifyOrigin(new URL("https://oxfer.app/"), expected, { fetch, delayMs: 0, log }), []);
    assert.equal(fetch.maxInFlight, 3);
    assert.deepEqual(fetch.calls, ["/", "/privacy", "/terms", "/terms", "/terms"]);
    assert.deepEqual(logged.filter(line => line.includes("attempt")), [
        "https://oxfer.app attempt 1/10 failed: /terms: deployed bytes differ from release build (not yet deployed, or rewritten by a Cloudflare feature)",
        "https://oxfer.app attempt 2/10 failed: /terms: deployed bytes differ from release build (not yet deployed, or rewritten by a Cloudflare feature)",
    ]);
    assert.equal(logged.filter(line => line.endsWith(": matches release build and hosting headers")).length, 3);
});

test("verify-deployment gives up after the last attempt on a failure the edge could still fix", async () => {
    const expected = await expectedFor(["/", "/privacy"]);
    const notFound = fakeFetch(expected, item => respond(item, item.path === "/" ? { status: 404 } : {}));
    await assert.rejects(
        verifyOrigin(new URL("https://oxfer.app/"), expected, { fetch: notFound, attempts: 3, delayMs: 0, log: quiet }),
        error => error.message === "/: HTTP status 404",
    );
    assert.deepEqual(notFound.calls, ["/", "/privacy", "/", "/"]);
    const redirected = fakeFetch(expected, item => {
        const response = respond(item);
        Object.defineProperty(response, "redirected", { value: true });
        return response;
    });
    await rejectsWith(
        verifyOrigin(new URL("https://oxfer.app/"), expected, { fetch: redirected, attempts: 1, log: quiet }),
        /^\/privacy: redirected; it must be served at this URL without a redirect$/,
    );
    const offline = async () => {
        throw new TypeError("fetch failed", { cause: new Error("getaddrinfo ENOTFOUND oxfer.app") });
    };
    await rejectsWith(
        verifyOrigin(new URL("https://oxfer.app/"), expected.slice(0, 1), { fetch: offline, attempts: 2, delayMs: 0, log: quiet }),
        /^\/: request failed: getaddrinfo ENOTFOUND oxfer\.app$/,
    );
    const wrongType = fakeFetch(expected, item => respond(item, { headers: { "Content-Type": "text/plain" } }));
    await rejectsWith(
        verifyOrigin(new URL("https://oxfer.app/"), expected.slice(0, 1), { fetch: wrongType, attempts: 1, log: quiet }),
        /^\/: MIME type "text\/plain" lacks text\/html$/,
    );
});

test("verify-deployment does not retry header mismatches or Network Error Logging", async () => {
    const expected = await expectedFor(["/", "/privacy"]);
    const policy = fakeFetch(expected, item => respond(item, item.path === "/privacy" ? { headers: { "X-Frame-Options": "DENY" } } : {}));
    await rejectsWith(
        verifyOrigin(new URL("https://oxfer.app/"), expected, { fetch: policy, delayMs: 0, log: quiet }),
        /^\/privacy: x-frame-options is "DENY", assets\/_headers sets "SAMEORIGIN"$/,
    );
    assert.deepEqual(policy.calls, ["/", "/privacy"]);
    const nel = fakeFetch(expected, item => respond(item, { headers: nelHeaders }));
    await assert.rejects(
        verifyOrigin(new URL("https://www.oxfer.app/"), expected, { fetch: nel, delayMs: 0, log: quiet }),
        error => error.message.split("\n").length === 2 && error.message.includes(NEL_REMEDY),
    );
    assert.equal(nel.calls.length, 2);
    // workers.dev is outside the oxfer.app zone: NEL there is returned, not fatal.
    const workers = fakeFetch(expected, item => respond(item, { headers: nelHeaders }));
    assert.deepEqual(
        await verifyOrigin(new URL("https://oxfer.42dev.workers.dev/"), expected, { fetch: workers, log: quiet }),
        ["NEL", "Report-To"],
    );
});

test("verify-deployment takes several origins and never repeats a rejected argument", () => {
    assert.throws(() => parseOrigins([]), /^Error: usage: node verify-deployment\.mjs https:\/\/oxfer\.app\/ \[more origins\.\.\.\]$/);
    assert.deepEqual(
        parseOrigins(["https://oxfer.42dev.workers.dev/", "https://www.oxfer.app/", "https://oxfer.app/", "https://OXFER.app"]).map(url => url.href),
        ["https://oxfer.42dev.workers.dev/", "https://www.oxfer.app/", "https://oxfer.app/"],
    );
    for (const [argument, reason] of [
        ["http://oxfer.app/", /^argument 2: deployment must use HTTPS$/],
        ["https://oxfer.app/#ticket=abc&cap=secret", /^argument 2: pass an origin, not a path or share link$/],
        ["https://oxfer.app/privacy", /^argument 2: pass an origin, not a path or share link$/],
        ["oxfer.app cap=secret", /^argument 2 is not a URL; pass an origin such as https:\/\/oxfer\.app\/$/],
    ]) {
        assert.throws(() => parseOrigins(["https://oxfer.app/", argument]), error => reason.test(error.message), argument);
    }
});

// Node resolves symlinks in import.meta.url but not in process.argv[1]; the CLI
// entry checks compare real paths so a symlinked checkout still runs them.
test("the CLIs run when started through a symlinked path", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-link-"));
    try {
        const link = join(dir, "p2p-transfer");
        await symlink(crate, link, "dir");
        const run = (script, args, env = {}) => {
            const { P2P_RELAY_URL: _unset, ...base } = process.env;
            return spawnSync(process.execPath, [join(link, script), ...args], {
                cwd: dir,
                encoding: "utf8",
                env: { ...base, ...env },
            });
        };
        const usage = run("verify-deployment.mjs", []);
        assert.equal(usage.status, 1, usage.stderr);
        assert.match(usage.stderr, /usage: node verify-deployment\.mjs/);
        const bad = run("package-cf-output.mjs", ["--check-relay"], { P2P_RELAY_URL: "bad url" });
        assert.equal(bad.status, 1);
        assert.match(bad.stderr, /^package-cf-output: P2P_RELAY_URL entry 1 contains whitespace$/m);
        const bom = run("package-cf-output.mjs", ["--check-relay"], { P2P_RELAY_URL: "\uFEFFhttps://relay.oxfer.app" });
        assert.equal(bom.status, 1);
        assert.match(bom.stderr, /entry 1 contains the invisible character U\+FEFF/);
        const good = run("package-cf-output.mjs", ["--check-relay"], { P2P_RELAY_URL: "https://relay.oxfer.app" });
        assert.equal(good.status, 0, good.stderr);
        assert.equal(good.stdout, "connect-src relay sources: https://relay.oxfer.app wss://relay.oxfer.app\n");
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("the package-cf-output CLI refuses unknown arguments, including the retired --require-relay", () => {
    const script = fileURLToPath(new URL("../package-cf-output.mjs", import.meta.url));
    const { P2P_RELAY_URL: _unset, ...env } = process.env;
    for (const args of [["--check-relay", `--require-relay=${OPERATOR_RELAY}`], ["--check-dsit"]]) {
        const result = spawnSync(process.execPath, [script, ...args], { encoding: "utf8", env });
        assert.equal(result.status, 1, args.join(" "));
        assert.match(result.stderr, /^package-cf-output: unknown argument --(require-relay=|check-dsit)/, args.join(" "));
        assert.equal(result.stdout, "", "nothing runs before the arguments are checked");
    }
    const unset = spawnSync(process.execPath, [script, "--check-relay"], { encoding: "utf8", env });
    assert.equal(unset.status, 0, unset.stderr);
    assert.equal(unset.stdout, `connect-src relay sources: ${N0_CONNECT_SRC}\n`);
});

/** The text of one job of a GitHub Actions workflow. */
function workflowJob(workflow, name) {
    const start = workflow.indexOf(`\n  ${name}:\n`);
    assert.ok(start >= 0, `the workflow has the ${name} job`);
    const next = workflow.slice(start + 1).search(/\n  [\w-]+:\n/);
    return next < 0 ? workflow.slice(start) : workflow.slice(start, start + 1 + next);
}

test("CI runs the node suites, the relay kit check and every browser suite that check.sh runs", async () => {
    const check = (await read("../check.sh")).split("\n");
    const ci = await read("../../.github/workflows/ci.yml");
    const runs = (job, command) =>
        new RegExp(`^\\s+run: ${command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m").test(job);
    // One node call runs every suite in tests/, so a new suite needs no new line.
    const node = "node --test tests/*.test.mjs";
    const render = "sh deploy/relay/render.sh --check";
    assert.ok(check.includes(node), "check.sh runs every node suite in tests/");
    assert.ok(check.includes(render), "check.sh checks the relay kit's generated files");
    const web = workflowJob(ci, "p2p-web-checks");
    for (const command of [node, render]) {
        assert.ok(runs(web, command), `ci.yml p2p-web-checks runs ${command}`);
    }
    assert.doesNotMatch(web, /rustup|cargo|wasm-pack/, "the node job does not wait for a Rust toolchain");
    // Every browser suite in tests/ (relay_wasm among them) runs in check.sh and in CI.
    const wasmSuites = (await readdir(new URL(".", import.meta.url)))
        .filter(name => name.endsWith("_wasm.rs"))
        .map(name => name.slice(0, -".rs".length));
    assert.ok(wasmSuites.includes("relay_wasm"));
    const browser = workflowJob(ci, "p2p-browser-integration");
    for (const suite of wasmSuites) {
        const command = `wasm-pack test --headless --firefox -- --test ${suite}`;
        assert.ok(check.includes(command), `check.sh runs ${command}`);
        assert.ok(runs(browser, command), `ci.yml p2p-browser-integration runs ${command}`);
    }
    // The release web build runs on every PR for n0 and for the Oxfer relay,
    // with placeholders allowed, and never deploys.
    const release = workflowJob(ci, "p2p-web-release-build");
    assert.ok(runs(release, "bash build-web.sh"), "ci.yml p2p-web-release-build runs bash build-web.sh");
    assert.match(release, /^\s+OXFER_ALLOW_PLACEHOLDERS: "1"$/m);
    assert.match(release, /^\s+P2P_RELAY_URL: \$\{\{ matrix\.relay \}\}$/m);
    assert.match(release, /^\s+relay: ""$/m, "one build uses n0's relays");
    assert.match(release, /^\s+relay: https:\/\/relay\.oxfer\.app$/m, "one build uses the Oxfer relay");
    assert.doesNotMatch(release, /deploy|CLOUDFLARE_|secrets\./, "the release build job never deploys");
});

test("trunkCopyFiles reads Trunk copy-file links and where Trunk puts each file", () => {
    const html = [
        '<!-- <link data-trunk rel="copy-file" href="commented-out.js"> -->',
        '<link data-trunk rel="rust" data-wasm-opt="2" />',
        '<link rel="copy-file" href="not-trunk.js">',
        '<link data-trunk rel="copy-file" href="theme.html">',
        "<link data-trunk rel='copy-file' href=assets/sw.js />",
        '<LINK DATA-TRUNK REL="copy-file" HREF="assets/boot.js" DATA-TARGET-PATH="assets">',
        '<link data-trunk rel="copy-file" href="assets/icons/a.png" data-target-path="assets/icons"/>',
    ].join("\n");
    assert.deepEqual(trunkCopyFiles(html), [
        ["theme.html", "theme.html"],
        ["assets/sw.js", "sw.js"],
        ["assets/boot.js", "assets/boot.js"],
        ["assets/icons/a.png", "assets/icons/a.png"],
    ]);
    assert.throws(() => trunkCopyFiles('<link data-trunk rel="rust">'), /no Trunk copy-file links/);
    assert.throws(() => trunkCopyFiles('<link data-trunk rel="copy-file">'), /without an href/);
});

// A scratch dist made the way Trunk makes it: every copy-file entry of the
// real index.html copied from its source, plus a generated index.html.
async function withFakeDist(fn) {
    const dist = await mkdtemp(join(tmpdir(), "oxfer-dist-"));
    try {
        const files = trunkCopyFiles(await read("../index.html"));
        for (const [source, target] of files) {
            await mkdir(dirname(join(dist, target)), { recursive: true });
            await copyFile(join(crate, source), join(dist, target));
        }
        await writeFile(join(dist, "index.html"), '<!DOCTYPE html>\n<script src="assets/boot.js"></script>\n');
        return await fn(dist, files);
    } finally {
        await rm(dist, { recursive: true, force: true });
    }
}

test("checkDist accepts a dist with byte copies of every copy-file entry and no inline script", async () => {
    await withFakeDist(async (dist, files) => {
        const result = await checkDist({ crate, dist });
        assert.deepEqual(result.copied, files.map(([, target]) => target));
        for (const target of ["_headers", "sw.js", "assets/boot.js", "assets/app-init.js", "assets/theme-lab.js", "assets/legal.css", "privacy.html"]) {
            assert.ok(result.copied.includes(target), target);
        }
        assert.deepEqual(result.pages, ["abuse.html", "index.html", "privacy.html", "terms.html", "theme.html"]);
    });
});

test("checkDist reports every changed or missing copy and every inline script", async () => {
    await withFakeDist(async dist => {
        await writeFile(join(dist, "assets/boot.js"), "// changed\n");
        await rm(join(dist, "sw.js"));
        await writeFile(join(dist, "index.html"), '<!DOCTYPE html>\n<script nonce="abc">boot()</script>\n');
        await assert.rejects(checkDist({ crate, dist }), error => {
            assert.deepEqual(error.message.split("\n"), [
                "dist/assets/boot.js differs from assets/boot.js",
                "dist/sw.js is missing; Trunk should copy it from assets/sw.js",
                "dist/index.html contains an inline <script>; the CSP would block it",
            ]);
            return true;
        });
    });
    await withFakeDist(async dist => {
        for (const tag of ["<script>", '<script type="module">', "<SCRIPT defer>", '<script data-src="x.js">']) {
            await writeFile(join(dist, "index.html"), `<!DOCTYPE html>\n${tag}</script>\n`);
            await rejectsWith(checkDist({ crate, dist }), /^dist\/index\.html contains an inline <script>/, tag);
        }
        // Every HTML file in dist counts, not only index.html.
        await writeFile(join(dist, "index.html"), '<script defer src="assets/boot.js"></script>\n');
        await mkdir(join(dist, "snippets"), { recursive: true });
        await writeFile(join(dist, "snippets/extra.html"), "<script>alert(1)</script>\n");
        await rejectsWith(checkDist({ crate, dist }), /^dist\/snippets\/extra\.html contains an inline <script>/);
        await rm(join(dist, "snippets"), { recursive: true });
        await writeFile(join(dist, "index.html"), " \n");
        await rejectsWith(checkDist({ crate, dist }), /^dist\/index\.html is empty$/);
        await rm(join(dist, "index.html"));
        await rejectsWith(checkDist({ crate, dist }), /^dist\/index\.html is missing$/);
    });
    const scratch = await mkdtemp(join(tmpdir(), "oxfer-crate-"));
    try {
        await writeFile(join(scratch, "index.html"), '<link data-trunk rel="copy-file" href="assets/gone.js" data-target-path="assets">\n');
        await mkdir(join(scratch, "dist"));
        await writeFile(join(scratch, "dist/index.html"), "<!DOCTYPE html>\n");
        await rejectsWith(checkDist({ crate: scratch }), /^assets\/gone\.js is a copy-file entry in index\.html but does not exist$/);
    } finally {
        await rm(scratch, { recursive: true, force: true });
    }
});
