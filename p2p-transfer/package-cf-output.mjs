// Package Trunk `dist/` as a cf Build Output Specification tree.
// `cf deploy --prebuilt` uploads this; it does not evaluate cloudflare.config.ts.
//
// The packaged `_headers` is rendered from `dist/_headers` (a byte copy of
// `assets/_headers`): the `{{RELAY_CONNECT_SRC}}` token in the
// Content-Security-Policy becomes the relay origins the wasm build talks to,
// taken from the same compile-time `P2P_RELAY_URL` that `src/node.rs` reads.
import { realpathSync } from "node:fs";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));

export const workerConfig = {
    name: "oxfer",
    compatibilityDate: "2026-09-29",
    workersDev: true,
    previewUrls: true,
    assets: {
        notFoundHandling: "single-page-application",
    },
};

export const productionDomains = ["oxfer.app", "www.oxfer.app"];

export const RELAY_CONNECT_SRC_TOKEN = "{{RELAY_CONNECT_SRC}}";
// n0's public relays (*.relay.n0.iroh.link, HTTPS latency probes) and pkarr
// discovery at https://dns.iroh.link/pkarr, used while P2P_RELAY_URL is unset.
export const N0_CONNECT_SRC = "https://*.iroh.link wss://*.iroh.link";
// The `/*` Cache-Control that `verify-deployment.mjs` expects on every response.
// Other rules add to it: Cloudflare joins the values of every matching rule with
// a comma, so `/sw.js` gets its no-store rule appended.
export const CACHE_CONTROL = "public, max-age=0, must-revalidate";
// The legal pages add `no-transform`, which stops Cloudflare features that
// rewrite responses, such as Email Address Obfuscation of their contact
// addresses, so they are served byte for byte as built. It is set nowhere else
// because it also stops Cloudflare compressing responses, and the wasm and
// JavaScript should stay compressed. Only the legal pages contain addresses.
// https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/
// https://developers.cloudflare.com/speed/optimization/content/compression/
export const NO_TRANSFORM_PATHS = ["/privacy", "/terms", "/abuse"];
// The operator's relay, which the filled legal pages describe.
export const OPERATOR_RELAY = "https://relay.oxfer.app";
export const PERMISSIONS_POLICY =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), midi=(), screen-wake-lock=(self)";
export const X_FRAME_OPTIONS = "SAMEORIGIN";
export const CSP_REPORT_ONLY = "Content-Security-Policy-Report-Only";
// Cloudflare static assets: at most 100 rules and 2,000 characters per line.
const HEADERS_MAX_LINE = 2000;
const HEADERS_MAX_RULES = 100;
// Only names a CSP host-source can express: no IPv6 literal, no trailing dot.
const CSP_HOST = /^[a-z0-9-]+(\.[a-z0-9-]+)*$/;
// A value of only ASCII whitespace means unset, as in build-web.sh; src/node.rs
// trims it too.
const UNSET_RELAY = /^[\t\n\v\f\r ]*$/;
// Any other character outside printable ASCII fails the build. JS `trim` and
// Rust `str::trim` strip different characters (JS strips U+FEFF, Rust keeps
// it and then falls back to n0's relays at run time), so this parser must not
// trim anything the Rust one keeps.
const NOT_PRINTABLE_ASCII = /[^\x20-\x7E]/u;
// Characters that cannot be seen; the error names them by code point.
const INVISIBLE = /^[\p{Cc}\p{Cf}\p{Z}]$/u;

// Name the entry by position only: a mistyped value may hold a credential, and
// CI logs are not the place to repeat it (src/node.rs does the same).
function relayError(index, reason) {
    return new Error(`P2P_RELAY_URL entry ${index + 1} ${reason}`);
}

function nonAsciiError(value, offset) {
    const index = value.slice(0, offset).split(",").length - 1;
    const code = value.codePointAt(offset);
    // A visible character could be part of a mistyped credential: do not repeat it.
    const reason = INVISIBLE.test(String.fromCodePoint(code))
        ? `contains the invisible character U+${code.toString(16).toUpperCase().padStart(4, "0")}; retype the value`
        : "contains a character outside printable ASCII";
    return relayError(index, reason);
}

function parseRelayEntry(entry, index) {
    if (entry === "") {
        throw relayError(index, "is empty; separate relay URLs with single commas");
    }
    if (/\s/.test(entry)) {
        throw relayError(index, "contains whitespace");
    }
    let url;
    try {
        url = new URL(entry);
    } catch {
        throw relayError(index, "is not an absolute URL such as https://relay.example.org");
    }
    if (url.protocol !== "https:" && url.protocol !== "http:") {
        throw relayError(index, `uses ${url.protocol} but must use https: (or http: for local tests)`);
    }
    if (url.username || url.password || entry.includes("@")) {
        throw relayError(index, "must not contain credentials");
    }
    if (url.search || entry.includes("?")) {
        throw relayError(index, "must not contain a query");
    }
    if (url.hash || entry.includes("#")) {
        throw relayError(index, "must not contain a fragment");
    }
    if (url.pathname !== "/") {
        throw relayError(index, "must not have a path");
    }
    if (!CSP_HOST.test(url.hostname)) {
        throw relayError(index, "has a host that a Content-Security-Policy source cannot express");
    }
    return url;
}

/**
 * Parse a comma-separated `P2P_RELAY_URL` value. `undefined`, `null`, an empty
 * string and strings of only ASCII whitespace mean "unset" (n0's default
 * relays) and return `[]`. Otherwise the value must be printable ASCII, and
 * every entry `http(s)://host[:port][/]`, optionally surrounded by spaces.
 * @param {string | undefined | null} value
 * @returns {URL[]}
 */
export function parseRelayList(value) {
    if (value === undefined || value === null) {
        return [];
    }
    if (typeof value !== "string") {
        throw new TypeError("P2P_RELAY_URL must be a string");
    }
    if (UNSET_RELAY.test(value)) {
        return [];
    }
    const offset = value.search(NOT_PRINTABLE_ASCII);
    if (offset >= 0) {
        throw nonAsciiError(value, offset);
    }
    return value.split(",").map((entry, index) => parseRelayEntry(entry.trim(), index));
}

/**
 * Whether a `P2P_RELAY_URL` value lists the relay at `origin`.
 * @param {string | undefined | null} value
 * @param {string} origin such as `https://relay.oxfer.app`
 */
export function listsRelay(value, origin) {
    const wanted = new URL(origin).origin;
    return parseRelayList(value).some(url => url.origin === wanted);
}

/**
 * CSP `connect-src` sources for the relays a build uses: the HTTP(S) origin for
 * probes plus the matching WebSocket origin for the relay connection.
 * @param {string | undefined | null} value a `P2P_RELAY_URL` value
 * @returns {string}
 */
export function relayConnectSources(value) {
    const relays = parseRelayList(value);
    if (relays.length === 0) {
        return N0_CONNECT_SRC;
    }
    const sources = [];
    for (const url of relays) {
        const socket = url.protocol === "https:" ? "wss:" : "ws:";
        for (const source of [`${url.protocol}//${url.host}`, `${socket}//${url.host}`]) {
            if (!sources.includes(source)) {
                sources.push(source);
            }
        }
    }
    return sources.join(" ");
}

/**
 * Parse a Cloudflare `_headers` file into `[pattern, [[name, value], ...]]` rules.
 * @param {string} text
 * @returns {Array<[string, Array<[string, string]>]>}
 */
export function parseHeaders(text) {
    const rules = [];
    for (const [number, line] of text.split("\n").entries()) {
        if (line.trim() === "" || line.trimStart().startsWith("#")) {
            continue;
        }
        if (!/^\s/.test(line)) {
            rules.push([line.trim(), []]);
            continue;
        }
        const match = line.trim().match(/^([A-Za-z0-9-]+):\s*(.*)$/);
        if (!match || rules.length === 0) {
            throw new Error(`_headers line ${number + 1} is not a "Name: value" header under a path rule`);
        }
        rules.at(-1)[1].push([match[1], match[2]]);
    }
    return rules;
}

/**
 * Whether a `_headers` URL pattern matches a request path. Cloudflare matches
 * rules against the request URL: `*` (one per pattern) matches anything, and a
 * `:name` placeholder matches one path segment.
 * @param {string} pattern
 * @param {string} path
 */
export function headerRuleMatches(pattern, path) {
    if (!pattern.startsWith("/")) {
        throw new Error(`_headers rule ${pattern}: only path patterns are supported here`);
    }
    const source = pattern
        .replace(/[.+?^${}()|[\]\\]/g, "\\$&")
        .replace(/\*/g, ".*")
        .replace(/:[A-Za-z]\w*/g, "[^/]+");
    return new RegExp(`^${source}$`).test(path);
}

/**
 * The Cache-Control directives Cloudflare serves for `path`: the values of
 * every matching rule, joined in file order, lower-cased and without repeats.
 * @param {string} rendered a rendered `_headers` file
 * @param {string} path
 * @returns {string[]}
 */
export function cacheControlDirectives(rendered, path) {
    const directives = [];
    for (const [pattern, headers] of parseHeaders(rendered)) {
        if (!headerRuleMatches(pattern, path)) {
            continue;
        }
        for (const [name, value] of headers) {
            if (name.toLowerCase() !== "cache-control") {
                continue;
            }
            for (const directive of splitDirectives(value)) {
                if (!directives.includes(directive)) {
                    directives.push(directive);
                }
            }
        }
    }
    return directives;
}

/**
 * @param {string} value a Cache-Control header value
 * @returns {string[]} its directives, trimmed and lower-cased
 */
export function splitDirectives(value) {
    return value
        .split(",")
        .map(directive => directive.trim().toLowerCase())
        .filter(directive => directive !== "");
}

/**
 * Render the `{{RELAY_CONNECT_SRC}}` token in a `_headers` file.
 * @param {string} text the `_headers` source
 * @param {string | undefined | null} relayValue a `P2P_RELAY_URL` value
 * @returns {string}
 */
export function renderHeaders(text, relayValue) {
    if (!text.includes(RELAY_CONNECT_SRC_TOKEN)) {
        throw new Error(`_headers has no ${RELAY_CONNECT_SRC_TOKEN} token in its Content-Security-Policy`);
    }
    const rendered = text.replaceAll(RELAY_CONNECT_SRC_TOKEN, relayConnectSources(relayValue));
    const leftover = rendered.match(/\{\{[A-Z0-9_]+\}\}/);
    if (leftover) {
        throw new Error(`_headers still contains ${leftover[0]} after rendering`);
    }
    for (const [number, line] of rendered.split("\n").entries()) {
        if (line.length > HEADERS_MAX_LINE) {
            throw new Error(`_headers line ${number + 1} is ${line.length} characters; Cloudflare allows ${HEADERS_MAX_LINE}`);
        }
    }
    if (parseHeaders(rendered).length > HEADERS_MAX_RULES) {
        throw new Error(`_headers has more than ${HEADERS_MAX_RULES} rules`);
    }
    return rendered;
}

/**
 * The security headers every response carries, from the `/*` rule of a
 * rendered `_headers` file. Throws if another rule also sets one of them,
 * because Cloudflare would then comma-join the values.
 * @param {string} rendered
 */
export function securityHeaders(rendered) {
    const names = [CSP_REPORT_ONLY, "Permissions-Policy", "X-Frame-Options"];
    const result = {};
    for (const [pattern, headers] of parseHeaders(rendered)) {
        for (const [name, value] of headers) {
            const known = names.find(candidate => candidate.toLowerCase() === name.toLowerCase());
            if (!known) {
                continue;
            }
            if (pattern !== "/*") {
                throw new Error(`_headers sets ${known} for ${pattern}; set it only under /*`);
            }
            if (known in result) {
                throw new Error(`_headers sets ${known} twice under /*`);
            }
            result[known] = value;
        }
    }
    for (const name of names) {
        if (!(name in result)) {
            throw new Error(`_headers has no ${name} under /*`);
        }
    }
    return result;
}

/**
 * @param {object} [options]
 * @param {string | undefined} [options.relayUrl] the `P2P_RELAY_URL` value the
 *   wasm was built with; when the key is absent, `process.env.P2P_RELAY_URL`.
 */
export async function packageCfOutput(options = {}) {
    const {
        includeDomains = true,
        dist = join(root, "dist"),
        outputRoot = join(root, ".cloudflare/output"),
    } = options;
    const relayUrl = "relayUrl" in options ? options.relayUrl : process.env.P2P_RELAY_URL;
    // Render before touching the output so a bad relay value leaves nothing behind.
    const headers = renderHeaders(await readFile(join(dist, "_headers"), "utf8"), relayUrl);
    const bos = join(outputRoot, "v0");
    const assets = join(bos, "workers/default/assets");
    await rm(outputRoot, { recursive: true, force: true });
    await mkdir(assets, { recursive: true });
    await cp(dist, assets, { recursive: true });
    await writeFile(join(assets, "_headers"), headers);
    const config = includeDomains
        ? { ...workerConfig, domains: [...productionDomains] }
        : { ...workerConfig };
    await writeFile(
        join(bos, "config.json"),
        `${JSON.stringify({ buildContext: { isPreview: false } })}\n`,
    );
    await writeFile(
        join(bos, "workers/default/worker.config.json"),
        `${JSON.stringify(config, null, 4)}\n`,
    );
    return { assets, config, headers };
}

/**
 * Whether the module at `moduleUrl` is the script Node was started with.
 * Compares real paths: Node resolves symlinks in `import.meta.url` but not in
 * `process.argv[1]`, so a symlinked checkout would otherwise skip the CLI.
 * @param {string} moduleUrl `import.meta.url` of the module
 */
export function isMainModule(moduleUrl) {
    if (!process.argv[1]) {
        return false;
    }
    try {
        return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(moduleUrl));
    } catch {
        return false;
    }
}

if (isMainModule(import.meta.url)) {
    try {
        if (process.argv.includes("--check-relay")) {
            // build-web.sh validates P2P_RELAY_URL before the Trunk build, and
            // with --require-relay=ORIGIN checks that the list includes ORIGIN.
            const value = process.env.P2P_RELAY_URL;
            console.log(`connect-src relay sources: ${relayConnectSources(value)}`);
            const required = process.argv.find(argument => argument.startsWith("--require-relay="));
            if (required) {
                const origin = required.slice("--require-relay=".length);
                if (!listsRelay(value, origin)) {
                    throw new Error(
                        parseRelayList(value).length === 0
                            ? `P2P_RELAY_URL is not set, so the build uses n0's public relays, not ${origin}`
                            : `P2P_RELAY_URL does not list ${origin}`,
                    );
                }
            }
        } else {
            await packageCfOutput({
                includeDomains: !process.argv.includes("--no-domains"),
            });
        }
    } catch (error) {
        console.error(`package-cf-output: ${error.message}`);
        process.exit(1);
    }
}
