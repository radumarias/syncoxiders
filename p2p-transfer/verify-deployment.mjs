// Compare HTTPS deployments with the local release build before a domain cutover.
// Usage: node verify-deployment.mjs https://oxfer.app/ [more origins...]
// Origins are checked concurrently, each path of an origin too.
// Run with the same P2P_RELAY_URL as the build: the expected headers of each
// path, including the Content-Security-Policy-Report-Only, are rendered from
// assets/_headers with it, the way Cloudflare applies them (headersForPath).
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import {
    CSP_REPORT_ONLY,
    headersForPath,
    isMainModule,
    renderHeaders,
    securityHeaders,
    splitDirectives,
} from "./package-cf-output.mjs";

// Headers that make browsers send reports to a third party. Cloudflare Network
// Error Logging adds NEL and Report-To (reports go to a.nel.cloudflare.com).
// The privacy notice describes no browser reporting, so none may be served.
export const REPORTING_HEADERS = ["NEL", "Report-To", "Reporting-Endpoints"];

export const NEL_REMEDY =
    'Turn off Cloudflare Network Error Logging with the zone setting "nel" (zone oxfer.app): the ' +
    "Network Error Logging toggle in the Cloudflare dashboard, or PATCH /zones/{zone_id}/settings/nel " +
    'with {"value":{"enabled":false}} (https://developers.cloudflare.com/network-error-logging/).';

const ATTEMPTS = 10;
const RETRY_DELAY_MS = 3_000;

// [path, dist file, MIME substring, served at this URL without a redirect]
const checks = [
    ["/", "index.html", "text/html"],
    ["/diags", "index.html", "text/html"],
    ["/p2p-transfer.js", "p2p-transfer.js", "javascript"],
    ["/p2p-transfer_bg.wasm", "p2p-transfer_bg.wasm", "application/wasm"],
    ["/sw.js", "sw.js", "javascript"],
    ["/theme.html", "theme.html", "text/html"],
    ["/privacy", "privacy.html", "text/html", true],
    ["/terms", "terms.html", "text/html", true],
    ["/abuse", "abuse.html", "text/html", true],
    ["/assets/legal.css", "assets/legal.css", "text/css"],
    ["/assets/boot.js", "assets/boot.js", "javascript"],
    ["/assets/app-init.js", "assets/app-init.js", "javascript"],
    ["/assets/theme-lab.js", "assets/theme-lab.js", "javascript"],
    ["/assets/favicon.js", "assets/favicon.js", "javascript"],
    ["/assets/oxfer-favicon-light.svg", "assets/oxfer-favicon-light.svg", "image/svg+xml"],
    ["/assets/oxfer-favicon-dark.svg", "assets/oxfer-favicon-dark.svg", "image/svg+xml"],
    ["/assets/oxfer-wordmark-light.svg", "assets/oxfer-wordmark-light.svg", "image/svg+xml"],
    ["/assets/oxfer-wordmark-dark.svg", "assets/oxfer-wordmark-dark.svg", "image/svg+xml"],
];

export const checkedPaths = checks.map(([path]) => path);

/**
 * A failure that waiting for the edge to update cannot fix, such as a header
 * that differs from `_headers` on a response whose bytes are this build's, or
 * Network Error Logging (a zone setting). It is reported without retrying.
 */
export class PolicyError extends Error {}

/**
 * Whether the host's zone settings are the operator's. `*.workers.dev`
 * (including preview URLs) belongs to Cloudflare, so the oxfer.app zone's
 * "nel" setting cannot turn Network Error Logging off there.
 * @param {string} hostname
 */
export function operatorControlsZone(hostname) {
    return !hostname.toLowerCase().endsWith(".workers.dev");
}

/**
 * The browser reporting headers a response carries.
 * @param {Headers} headers
 * @returns {string[]}
 */
export function reportingHeaders(headers) {
    return REPORTING_HEADERS.filter(name => headers.has(name));
}

/**
 * Check the hosting headers of one response against the headers `_headers`
 * gives its path: every header exactly, Cache-Control as a set of directives.
 * Throws a PolicyError listing every mismatch, or naming the reporting headers
 * when `enforceNoReporting` is set.
 * @param {string} path
 * @param {Headers} served the response headers
 * @param {Headers} expected `headersForPath(rendered, path)`
 * @param {{ enforceNoReporting: boolean }} options when false, reporting headers
 *   are returned instead of failing the check
 * @returns {string[]} reporting headers present and not enforced
 */
export function checkHeaders(path, served, expected, { enforceNoReporting }) {
    const reporting = reportingHeaders(served);
    if (reporting.length > 0 && enforceNoReporting) {
        throw new PolicyError(
            `${path}: the response carries ${reporting.join(" and ")} headers (Cloudflare Network Error ` +
                "Logging), so browsers would report connection details to Cloudflare. The privacy notice " +
                `does not describe this. ${NEL_REMEDY}`,
        );
    }
    const mismatches = [];
    for (const [name, value] of expected) {
        const actual = served.get(name);
        if (name === "cache-control") {
            const want = new Set(splitDirectives(value));
            const have = new Set(splitDirectives(actual ?? ""));
            const missing = [...want].filter(directive => !have.has(directive));
            const extra = [...have].filter(directive => !want.has(directive));
            if (missing.length > 0 || extra.length > 0) {
                mismatches.push(
                    `Cache-Control "${actual ?? ""}" differs from assets/_headers "${value}"` +
                        (missing.length > 0 ? `; missing ${missing.join(", ")}` : "") +
                        (extra.length > 0 ? `; unexpected ${extra.join(", ")}` : ""),
                );
            }
        } else if (actual !== value) {
            mismatches.push(
                `${name} is ${actual === null ? "missing" : `"${actual}"`}, assets/_headers sets "${value}"` +
                    (name === CSP_REPORT_ONLY.toLowerCase() ? " (check P2P_RELAY_URL matches the build)" : ""),
            );
        }
    }
    if (mismatches.length > 0) {
        throw new PolicyError(`${path}: ${mismatches.join("; and ")}`);
    }
    return enforceNoReporting ? [] : reporting;
}

const digest = bytes => createHash("sha256").update(bytes).digest("hex");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

/**
 * What every origin must serve, computed once per run: for each checked path
 * the SHA-256 of the local `dist/` file and the headers `_headers` gives it.
 * @param {string} rendered the rendered `_headers` file
 * @param {URL} [dist] the release build's `dist/` directory
 */
export async function expectedResponses(rendered, dist = new URL("./dist/", import.meta.url)) {
    return Promise.all(
        checks.map(async ([path, file, mime, canonical = false]) => ({
            path,
            mime,
            canonical,
            sha256: digest(await readFile(new URL(file, dist))),
            headers: headersForPath(rendered, path),
        })),
    );
}

async function fetchPath(origin, path, fetchImpl) {
    try {
        return await fetchImpl(new URL(path, origin), {
            headers: {
                "Cache-Control": "no-cache",
                "User-Agent": "Mozilla/5.0 Oxfer-deployment-check",
            },
            signal: AbortSignal.timeout(30_000),
        });
    } catch (error) {
        throw new Error(`${path}: request failed: ${error.cause?.message ?? error.message}`);
    }
}

// Status, redirect, MIME type and bytes can still be the previous deployment's
// while the edge updates, so their failures are retried. Once the bytes are
// this build's, its headers are final and checkHeaders' failures are not.
async function verifyPath(origin, expected, { enforceNoReporting, fetchImpl }) {
    const { path, mime, canonical, sha256, headers } = expected;
    const response = await fetchPath(origin, path, fetchImpl);
    const type = response.headers.get("content-type") ?? "";
    const problem =
        response.status !== 200
            ? `HTTP status ${response.status}`
            : canonical && response.redirected
              ? "redirected; it must be served at this URL without a redirect"
              : !type.includes(mime)
                ? `MIME type "${type}" lacks ${mime}`
                : null;
    if (problem) {
        await response.body?.cancel();
        throw new Error(`${path}: ${problem}`);
    }
    // Also catches any Cloudflare feature that rewrites HTML.
    if (digest(new Uint8Array(await response.arrayBuffer())) !== sha256) {
        throw new Error(
            `${path}: deployed bytes differ from release build (not yet deployed, or rewritten by a Cloudflare feature)`,
        );
    }
    return checkHeaders(path, response.headers, headers, { enforceNoReporting });
}

/**
 * Check every path of one origin concurrently. Paths that fail are retried,
 * alone, up to `attempts` times while the edge updates; a PolicyError ends
 * the check at once.
 * @param {URL} origin
 * @param {Awaited<ReturnType<typeof expectedResponses>>} expected
 * @param {object} [options]
 * @returns {Promise<string[]>} reporting headers served where not enforced
 */
export async function verifyOrigin(
    origin,
    expected,
    { fetch: fetchImpl = fetch, attempts = ATTEMPTS, delayMs = RETRY_DELAY_MS, log = console } = {},
) {
    const enforceNoReporting = operatorControlsZone(origin.hostname);
    const unenforced = new Set();
    let pending = expected;
    for (let attempt = 1; ; attempt += 1) {
        const results = await Promise.allSettled(
            pending.map(item => verifyPath(origin, item, { enforceNoReporting, fetchImpl })),
        );
        const failed = [];
        for (const [index, result] of results.entries()) {
            if (result.status === "fulfilled") {
                result.value.forEach(name => unenforced.add(name));
                log.log(`${origin.origin}${pending[index].path}: matches release build and hosting headers`);
            } else {
                failed.push([pending[index], result.reason]);
            }
        }
        if (failed.length === 0) {
            return [...unenforced];
        }
        const final = failed.filter(([, error]) => error instanceof PolicyError);
        if (final.length > 0 || attempt >= attempts) {
            throw new Error((final.length > 0 ? final : failed).map(([, error]) => error.message).join("\n"));
        }
        for (const [, error] of failed) {
            log.error(`${origin.origin} attempt ${attempt}/${attempts} failed: ${error.message}`);
        }
        pending = failed.map(([item]) => item);
        await sleep(delayMs);
    }
}

/**
 * @param {string[]} args the origins to check
 * @returns {URL[]} the distinct origins; never repeats a rejected argument,
 *   which could be a share link with its capability
 */
export function parseOrigins(args) {
    if (args.length === 0) {
        throw new Error("usage: node verify-deployment.mjs https://oxfer.app/ [more origins...]");
    }
    const origins = new Map();
    for (const [index, argument] of args.entries()) {
        let origin;
        try {
            origin = new URL(argument);
        } catch {
            throw new Error(`argument ${index + 1} is not a URL; pass an origin such as https://oxfer.app/`);
        }
        if (origin.protocol !== "https:") {
            throw new Error(`argument ${index + 1}: deployment must use HTTPS`);
        }
        if (origin.href !== `${origin.origin}/`) {
            throw new Error(`argument ${index + 1}: pass an origin, not a path or share link`);
        }
        origins.set(origin.origin, origin);
    }
    return [...origins.values()];
}

async function main(args) {
    const origins = parseOrigins(args);
    const rendered = renderHeaders(
        await readFile(new URL("./assets/_headers", import.meta.url), "utf8"),
        process.env.P2P_RELAY_URL,
    );
    // The CSP, Permissions-Policy and X-Frame-Options must apply to every path.
    securityHeaders(rendered);
    const expected = await expectedResponses(rendered);
    const results = await Promise.allSettled(origins.map(origin => verifyOrigin(origin, expected)));
    let failures = 0;
    for (const [index, result] of results.entries()) {
        const origin = origins[index];
        if (result.status === "rejected") {
            failures += 1;
            console.error(`${origin.origin} does not match the release build:\n${result.reason.message}`);
        } else if (result.value.length > 0) {
            console.warn(
                `warning: ${origin.origin} serves ${result.value.join(" and ")} headers (Cloudflare Network ` +
                    `Error Logging). ${origin.hostname} is a workers.dev hostname outside the oxfer.app zone, ` +
                    'so that zone\'s "nel" setting does not apply to it, and the privacy notice does not ' +
                    "describe it. To stop serving the app there, set workersDev: false and previewUrls: false " +
                    "(docs/cloudflare-workers.md).",
            );
        }
    }
    if (failures > 0) {
        throw new Error(`${failures} of ${origins.length} origins failed`);
    }
}

if (isMainModule(import.meta.url)) {
    try {
        await main(process.argv.slice(2));
    } catch (error) {
        console.error(`verify-deployment: ${error.message}`);
        process.exitCode = 1;
    }
}
