// Package Trunk `dist/` as a cf Build Output Specification tree.
// `cf deploy --prebuilt` uploads this; it does not evaluate cloudflare.config.ts.
//
// The packaged `_headers` is rendered from `dist/_headers` (a byte copy of
// `assets/_headers`): the `{{RELAY_CONNECT_SRC}}` token in the
// Content-Security-Policy becomes the relay origins the wasm build talks to,
// taken from the same compile-time `P2P_RELAY_URL` that `src/node.rs` reads.
//
// CLI (build-web.sh and check.sh):
//   node package-cf-output.mjs [--no-domains]   package dist/
//   --check-relay   validate P2P_RELAY_URL and print its connect-src sources
//   --check-legal   legal-page guards on the source pages (checkLegalPages)
//   --check-dist    dist/ holds byte copies and no inline script (checkDist)
// The --check-* flags combine and replace packaging.
import { realpathSync } from "node:fs";
import { cp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, posix } from "node:path";
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
// The operator's relay, which the filled legal pages describe. If the pages
// ever describe another relay, change this with them (tests/web-pages.test.mjs
// fails while privacy.html does not name this host).
export const OPERATOR_RELAY = "https://relay.oxfer.app";
export const LEGAL_PAGES = ["privacy.html", "terms.html", "abuse.html"];
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
// The legal pages' unfilled tokens, such as [[OPERATOR_NAME]].
const PLACEHOLDER = /\[\[[A-Z][A-Z0-9_]*\]\]/g;
// HTML scanning for checkDist: comments, <link> tags and their attributes.
const HTML_COMMENT = /<!--[\s\S]*?-->/g;
const LINK_TAG = /<link\b([^>]*)>/gi;
const ATTRIBUTE = /([^\s"'<>\/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
// Any <script> without a src attribute, whatever else it carries (a nonce,
// type or defer). Trunk.toml sets inject_scripts = false and the CSP allows no
// inline script.
const INLINE_SCRIPT = /<script\b(?![^>]*\ssrc\s*=)[^>]*>/i;

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
    // The URL parser takes credentials, a query and a fragment only from these
    // characters, so the raw entry decides, including an empty "?" or "#".
    if (entry.includes("@")) {
        throw relayError(index, "must not contain credentials");
    }
    if (entry.includes("?")) {
        throw relayError(index, "must not contain a query");
    }
    if (entry.includes("#")) {
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
 * Why a `P2P_RELAY_URL` value does not select the relay at `origin` alone, or
 * `null` when it does (the relay go-live guard). Throws if the value is invalid.
 * @param {string | undefined | null} value
 * @param {string} [origin] such as `https://relay.oxfer.app`
 * @returns {string | null}
 */
export function operatorRelayProblem(value, origin = OPERATOR_RELAY) {
    const wanted = new URL(origin).origin;
    const relays = parseRelayList(value);
    if (relays.length === 0) {
        return `P2P_RELAY_URL is not set, so the build uses n0's public relays, not ${wanted}`;
    }
    if (!relays.some(url => url.origin === wanted)) {
        return `P2P_RELAY_URL does not list ${wanted}`;
    }
    if (relays.some(url => url.origin !== wanted)) {
        return `P2P_RELAY_URL lists other relays besides ${wanted}`;
    }
    return null;
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
 * The headers Cloudflare serves for `path` under a rendered `_headers` file:
 * every matching rule applies, and a header that several rules set gets their
 * values joined with a comma in file order, so `/sw.js` gets the `/*`
 * Cache-Control followed by its own.
 * https://developers.cloudflare.com/workers/static-assets/headers/
 * @param {string} rendered
 * @param {string} path
 * @returns {Headers}
 */
export function headersForPath(rendered, path) {
    const result = new Headers();
    for (const [pattern, headers] of parseHeaders(rendered)) {
        if (headerRuleMatches(pattern, path)) {
            for (const [name, value] of headers) {
                result.append(name, value);
            }
        }
    }
    return result;
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
 * The legal-page guards build-web.sh runs before the Trunk build, on the
 * source pages (checkDist later proves dist holds byte copies of them):
 * - no `[[UPPER_CASE]]` placeholder may remain;
 * - once none remains, `P2P_RELAY_URL` must select only `OPERATOR_RELAY`, the
 *   relay the filled pages describe (the relay go-live guard).
 * `allowPlaceholders` (`OXFER_ALLOW_PLACEHOLDERS=1`, local test builds only)
 * turns both into warnings. An invalid relay list always throws first.
 * @param {object} options
 * @param {string} [options.crate] the directory holding the pages
 * @param {string | undefined | null} options.relayUrl a `P2P_RELAY_URL` value
 * @param {boolean} [options.allowPlaceholders]
 * @returns {Promise<string[]>} warnings to print; throws when a guard fails
 */
export async function checkLegalPages({ crate = root, relayUrl, allowPlaceholders = false } = {}) {
    const relayProblem = operatorRelayProblem(relayUrl);
    const placeholders = [];
    for (const page of LEGAL_PAGES) {
        const html = await readFile(join(crate, page), "utf8");
        for (const token of [...new Set(html.match(PLACEHOLDER))].sort()) {
            placeholders.push(`${page} still contains the placeholder ${token}`);
        }
    }
    let failure;
    let warnings;
    if (placeholders.length > 0) {
        failure = [...placeholders, "Replace the placeholders in privacy.html, terms.html and abuse.html before deploying."];
        warnings = [...placeholders, "OXFER_ALLOW_PLACEHOLDERS=1: packaging anyway. Do not deploy this build."];
    } else if (relayProblem) {
        failure = [
            `${relayProblem}.`,
            `The filled legal pages describe the relay at ${OPERATOR_RELAY}, so P2P_RELAY_URL must list only ` +
                "it; set P2P_RELAY_URL, or edit the pages and OPERATOR_RELAY in package-cf-output.mjs to " +
                "describe the relays in use.",
        ];
        warnings = [
            `${relayProblem}.`,
            `OXFER_ALLOW_PLACEHOLDERS=1: packaging filled legal pages that describe ${OPERATOR_RELAY} in a ` +
                "build whose P2P_RELAY_URL does not list only it. Do not deploy this build.",
        ];
    } else {
        return [];
    }
    if (allowPlaceholders) {
        return warnings;
    }
    throw new Error([...failure, "For a local test build only, set OXFER_ALLOW_PLACEHOLDERS=1."].join("\n"));
}

/**
 * The files Trunk copies verbatim: the `<link data-trunk rel="copy-file">`
 * entries of `index.html`, as `[source, dist path]` pairs. Trunk copies each
 * file under its own name into `data-target-path`, or else the dist root.
 * @param {string} html the source `index.html`
 * @returns {Array<[string, string]>}
 */
export function trunkCopyFiles(html) {
    const files = [];
    for (const [, source] of html.replace(HTML_COMMENT, "").matchAll(LINK_TAG)) {
        const attributes = new Map(
            [...source.matchAll(ATTRIBUTE)].map(([, name, double, single, bare]) => [
                name.toLowerCase(),
                double ?? single ?? bare ?? "",
            ]),
        );
        if (!attributes.has("data-trunk") || attributes.get("rel") !== "copy-file") {
            continue;
        }
        const href = attributes.get("href");
        if (!href) {
            throw new Error("index.html has a Trunk copy-file link without an href");
        }
        files.push([href, posix.join(attributes.get("data-target-path") ?? "", posix.basename(href))]);
    }
    if (files.length === 0) {
        throw new Error("index.html has no Trunk copy-file links");
    }
    return files;
}

/**
 * Check a Trunk `dist/` (check.sh and build-web.sh run this after the build):
 * every copy-file entry of `index.html` is a byte copy of its source, and no
 * HTML file in it has a `<script>` without `src`, which the CSP would block.
 * Throws with every problem found.
 * @param {object} [options]
 * @param {string} [options.crate] the directory holding `index.html` and the sources
 * @param {string} [options.dist]
 * @returns {Promise<{ copied: string[], pages: string[] }>} the dist paths checked
 */
export async function checkDist({ crate = root, dist = join(crate, "dist") } = {}) {
    const files = trunkCopyFiles(await readFile(join(crate, "index.html"), "utf8"));
    const read = path => readFile(path).catch(error => {
        if (error.code === "ENOENT") {
            return null;
        }
        throw error;
    });
    const copies = await Promise.all(
        files.map(async ([source, target]) => {
            const [want, have] = await Promise.all([read(join(crate, source)), read(join(dist, target))]);
            if (want === null) {
                return `${source} is a copy-file entry in index.html but does not exist`;
            }
            if (have === null) {
                return `dist/${target} is missing; Trunk should copy it from ${source}`;
            }
            return want.equals(have) ? null : `dist/${target} differs from ${source}`;
        }),
    );
    const problems = copies.filter(problem => problem !== null);
    const pages = (await readdir(dist, { recursive: true })).filter(name => name.endsWith(".html")).sort();
    if (!pages.includes("index.html")) {
        problems.push("dist/index.html is missing");
    }
    for (const page of pages) {
        const html = await readFile(join(dist, page), "utf8");
        if (html.trim() === "") {
            problems.push(`dist/${page} is empty`);
        }
        if (INLINE_SCRIPT.test(html)) {
            problems.push(`dist/${page} contains an inline <script>; the CSP would block it`);
        }
    }
    if (problems.length > 0) {
        throw new Error(problems.join("\n"));
    }
    return { copied: files.map(([, target]) => target), pages };
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

const CLI_FLAGS = ["--check-relay", "--check-legal", "--check-dist", "--no-domains"];

if (isMainModule(import.meta.url)) {
    try {
        const flags = process.argv.slice(2);
        const unknown = flags.find(flag => !CLI_FLAGS.includes(flag));
        if (unknown !== undefined) {
            throw new Error(`unknown argument ${unknown}; expected ${CLI_FLAGS.join(", ")}`);
        }
        const relayUrl = process.env.P2P_RELAY_URL;
        if (flags.includes("--check-relay")) {
            console.log(`connect-src relay sources: ${relayConnectSources(relayUrl)}`);
        }
        if (flags.includes("--check-legal")) {
            const allowPlaceholders = process.env.OXFER_ALLOW_PLACEHOLDERS === "1";
            for (const warning of await checkLegalPages({ relayUrl, allowPlaceholders })) {
                console.error(warning);
            }
        }
        if (flags.includes("--check-dist")) {
            const { copied, pages } = await checkDist();
            console.log(`dist: ${copied.length} copied files match their sources; ${pages.length} HTML files have no inline <script>`);
        }
        if (!flags.some(flag => flag.startsWith("--check-"))) {
            await packageCfOutput({ includeDomains: !flags.includes("--no-domains") });
        }
    } catch (error) {
        console.error(`package-cf-output: ${error.message}`);
        process.exit(1);
    }
}
