import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
    CACHE_CONTROL,
    CSP_REPORT_ONLY,
    N0_CONNECT_SRC,
    PERMISSIONS_POLICY,
    RELAY_CONNECT_SRC_TOKEN,
    X_FRAME_OPTIONS,
    packageCfOutput,
    parseHeaders,
    parseRelayList,
    productionDomains,
    relayConnectSources,
    renderHeaders,
    securityHeaders,
    workerConfig,
} from "../package-cf-output.mjs";
import { NEL_REMEDY, checkHeaders, operatorControlsZone, reportingHeaders } from "../verify-deployment.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");

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

test("an unset, empty or whitespace-only P2P_RELAY_URL keeps n0's relays", () => {
    for (const value of [undefined, null, "", " ", "\t\n "]) {
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

test("assets/_headers carries the report-only CSP, Permissions-Policy and X-Frame-Options", async () => {
    const source = await read("../assets/_headers");
    assert.equal(source.split(RELAY_CONNECT_SRC_TOKEN).length, 2, "exactly one relay token");
    const rules = parseHeaders(source);
    assert.deepEqual(rules.map(([pattern]) => pattern), ["/*", "/sw.js"]);
    const all = Object.fromEntries(rules[0][1]);
    assert.equal(all["Cache-Control"], CACHE_CONTROL);
    assert.equal(CACHE_CONTROL, "public, max-age=0, must-revalidate, no-transform");
    assert.equal(all["Referrer-Policy"], "no-referrer");
    assert.equal(all["X-Content-Type-Options"], "nosniff");
    assert.equal(all[CSP_REPORT_ONLY], expectedCsp(RELAY_CONNECT_SRC_TOKEN));
    assert.equal(all["Permissions-Policy"], PERMISSIONS_POLICY);
    assert.equal(
        PERMISSIONS_POLICY,
        "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), midi=(), screen-wake-lock=(self)",
    );
    assert.equal(all["X-Frame-Options"], X_FRAME_OPTIONS);
    assert.equal(X_FRAME_OPTIONS, "SAMEORIGIN");
    // Enforcement waits for the manual cross-browser matrix (plan B1).
    assert.equal("Content-Security-Policy" in all, false);
    assert.deepEqual(rules[1][1], [["Cache-Control", "no-cache, no-store, must-revalidate"]]);
    // /sw.js also matches /*, and Cloudflare comma-joins a header that several
    // matching rules set, so the service worker gets no-transform and no-store.
    const sw = `${all["Cache-Control"]}, ${rules[1][1][0][1]}`.split(", ");
    for (const directive of ["no-transform", "no-store", "no-cache"]) {
        assert.ok(sw.includes(directive), directive);
    }
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
            "X-Frame-Options": X_FRAME_OPTIONS,
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

// The headers package-cf-output.mjs renders for the n0 default, as served.
async function servedHeaders(path, extra = {}) {
    const expected = securityHeaders(renderHeaders(await read("../assets/_headers"), ""));
    const headers = new Headers({
        "Cache-Control": path === "/sw.js" ? `${CACHE_CONTROL}, no-cache, no-store, must-revalidate` : CACHE_CONTROL,
        "Referrer-Policy": "no-referrer",
        "X-Content-Type-Options": "nosniff",
        "Permissions-Policy": PERMISSIONS_POLICY,
        "X-Frame-Options": X_FRAME_OPTIONS,
        [CSP_REPORT_ONLY]: expected[CSP_REPORT_ONLY],
        ...extra,
    });
    return { expected, headers };
}

// As served by oxfer.app and oxfer.42dev.workers.dev on 30 September 2026.
const nelHeaders = {
    NEL: '{"report_to":"cf-nel","success_fraction":0.0,"max_age":604800}',
    "Report-To": '{"group":"cf-nel","max_age":604800,"endpoints":[{"url":"https://a.nel.cloudflare.com/report/v4?s=x"}]}',
};

test("verify-deployment accepts the headers assets/_headers produces", async () => {
    for (const path of ["/", "/privacy", "/sw.js"]) {
        const { expected, headers } = await servedHeaders(path);
        assert.deepEqual(checkHeaders(path, headers, expected, { enforceNoReporting: true }), []);
    }
});

test("verify-deployment fails while Cloudflare Network Error Logging is on", async () => {
    const { expected, headers } = await servedHeaders("/privacy", nelHeaders);
    assert.deepEqual(reportingHeaders(headers), ["NEL", "Report-To"]);
    assert.throws(
        () => checkHeaders("/privacy", headers, expected, { enforceNoReporting: true }),
        error =>
            error.message.startsWith("/privacy: the response carries NEL and Report-To headers") &&
            error.message.includes("privacy notice does not describe") &&
            error.message.includes(NEL_REMEDY),
    );
    assert.match(NEL_REMEDY, /zone setting "nel"/);
    assert.match(NEL_REMEDY, /PATCH \/zones\/\{zone_id\}\/settings\/nel with \{"value":\{"enabled":false\}\}/);
    for (const [name, value] of Object.entries({ nel: nelHeaders.NEL, "report-to": "{}", "Reporting-Endpoints": 'default="https://r.example"' })) {
        const single = (await servedHeaders("/", { [name]: value })).headers;
        assert.throws(() => checkHeaders("/", single, expected, { enforceNoReporting: true }), /Network Error Logging/, name);
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

test("verify-deployment requires no-transform on every path and no-store on /sw.js", async () => {
    for (const path of ["/", "/sw.js"]) {
        const { expected } = await servedHeaders(path);
        const old = (await servedHeaders(path, {
            "Cache-Control": path === "/sw.js"
                ? "public, max-age=0, must-revalidate, no-cache, no-store, must-revalidate"
                : "public, max-age=0, must-revalidate",
        })).headers;
        assert.throws(
            () => checkHeaders(path, old, expected, { enforceNoReporting: true }),
            /lacks no-transform; without it Cloudflare features such as Email Address Obfuscation/,
        );
    }
    const { expected, headers } = await servedHeaders("/sw.js", { "Cache-Control": CACHE_CONTROL });
    assert.throws(() => checkHeaders("/sw.js", headers, expected, { enforceNoReporting: true }), /no-store/);
    const relay = (await servedHeaders("/", { [CSP_REPORT_ONLY]: expectedCsp("https://relay.oxfer.app wss://relay.oxfer.app") })).headers;
    assert.throws(() => checkHeaders("/", relay, expected, { enforceNoReporting: true }), /check P2P_RELAY_URL/);
});

test("CI runs every node and browser suite that check.sh runs", async () => {
    const check = await read("../check.sh");
    const ci = await read("../../.github/workflows/ci.yml");
    const suites = (await readdir(new URL(".", import.meta.url))).filter(name => name.endsWith(".test.mjs")).sort();
    for (const suite of suites) {
        assert.match(check, new RegExp(`^node --test tests/${suite.replaceAll(".", "\\.")}$`, "m"), `check.sh runs ${suite}`);
    }
    const commands = check.split("\n").filter(line => /^(node --test |wasm-pack test )/.test(line));
    assert.ok(commands.length >= suites.length + 3);
    const start = ci.indexOf("\n  p2p-browser-integration:\n");
    assert.ok(start >= 0, "ci.yml has the p2p-browser-integration job");
    const next = ci.slice(start + 1).search(/\n  [\w-]+:\n/);
    const job = next < 0 ? ci.slice(start) : ci.slice(start, start + 1 + next);
    for (const command of commands) {
        assert.match(job, new RegExp(`^\\s+(run: )?${command.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m"), `ci.yml runs ${command}`);
    }
});
