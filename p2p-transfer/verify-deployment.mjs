// Compare an HTTPS deployment with the local release build before a domain cutover.
// Usage: node verify-deployment.mjs https://oxfer.app/
// Run with the same P2P_RELAY_URL as the build: the expected
// Content-Security-Policy-Report-Only is rendered from assets/_headers with it.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
    CACHE_CONTROL,
    CSP_REPORT_ONLY,
    PERMISSIONS_POLICY,
    X_FRAME_OPTIONS,
    renderHeaders,
    securityHeaders,
} from "./package-cf-output.mjs";

// Headers that make browsers send reports to a third party. Cloudflare Network
// Error Logging adds NEL and Report-To (reports go to a.nel.cloudflare.com).
// The privacy notice describes no browser reporting, so none may be served.
export const REPORTING_HEADERS = ["NEL", "Report-To", "Reporting-Endpoints"];

export const NEL_REMEDY =
    'Turn off Cloudflare Network Error Logging with the zone setting "nel" (zone oxfer.app): the ' +
    "Network Error Logging toggle in the Cloudflare dashboard, or PATCH /zones/{zone_id}/settings/nel " +
    'with {"value":{"enabled":false}} (https://developers.cloudflare.com/network-error-logging/).';

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
 * Assert the hosting headers of one response.
 * @param {string} path
 * @param {Headers} headers
 * @param {Record<string, string>} expected `securityHeaders()` of the rendered `_headers`
 * @param {{ enforceNoReporting: boolean }} options when false, reporting headers
 *   are returned instead of failing the check
 * @returns {string[]} reporting headers present and not enforced
 */
export function checkHeaders(path, headers, expected, { enforceNoReporting }) {
    const reporting = reportingHeaders(headers);
    if (reporting.length > 0 && enforceNoReporting) {
        assert.fail(
            `${path}: the response carries ${reporting.join(" and ")} headers (Cloudflare Network Error ` +
                "Logging), so browsers would report connection details to Cloudflare. The privacy notice " +
                `does not describe this. ${NEL_REMEDY}`,
        );
    }
    const cacheControl = headers.get("cache-control") ?? "";
    const directives = cacheControl.split(",").map(directive => directive.trim().toLowerCase());
    for (const directive of CACHE_CONTROL.split(",").map(part => part.trim())) {
        assert.ok(
            directives.includes(directive),
            `${path}: Cache-Control "${cacheControl}" lacks ${directive}` +
                (directive === "no-transform"
                    ? "; without it Cloudflare features such as Email Address Obfuscation may rewrite the response"
                    : ""),
        );
    }
    if (path === "/sw.js") {
        assert.ok(directives.includes("no-store"), "/sw.js: the service worker must not be cached (no-store)");
    }
    assert.equal(headers.get("referrer-policy"), "no-referrer", `${path}: referrer policy`);
    assert.equal(headers.get("x-content-type-options"), "nosniff", `${path}: nosniff`);
    assert.equal(headers.get("permissions-policy"), PERMISSIONS_POLICY, `${path}: Permissions-Policy`);
    assert.equal(headers.get("x-frame-options"), X_FRAME_OPTIONS, `${path}: X-Frame-Options`);
    assert.equal(
        headers.get(CSP_REPORT_ONLY),
        expected[CSP_REPORT_ONLY],
        `${path}: ${CSP_REPORT_ONLY} (check P2P_RELAY_URL matches the build)`,
    );
    return enforceNoReporting ? [] : reporting;
}

const digest = bytes => createHash("sha256").update(bytes).digest("hex");
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function verifyOnce(origin, expected, enforceNoReporting) {
    const unenforced = new Set();
    for (const [path, file, mime, canonical = false] of checks) {
        const response = await fetch(new URL(path, origin), {
            headers: {
                "Cache-Control": "no-cache",
                "User-Agent": "Mozilla/5.0 Oxfer-deployment-check",
            },
            signal: AbortSignal.timeout(30_000),
        });
        assert.equal(response.status, 200, `${path}: HTTP status`);
        if (canonical) {
            assert.equal(response.redirected, false, `${path}: served without a redirect (got ${response.url})`);
        }
        assert.ok(response.headers.get("content-type")?.includes(mime), `${path}: MIME type`);
        for (const name of checkHeaders(path, response.headers, expected, { enforceNoReporting })) {
            unenforced.add(name);
        }
        // Also catches any Cloudflare feature that rewrites HTML.
        const local = await readFile(new URL(`./dist/${file}`, import.meta.url));
        const remote = new Uint8Array(await response.arrayBuffer());
        assert.equal(
            digest(remote),
            digest(local),
            `${path}: deployed bytes differ from release build (not yet deployed, or rewritten by a Cloudflare feature)`,
        );
        console.log(`${origin.origin}${path}: matches release build and hosting headers`);
    }
    return [...unenforced];
}

async function main(argument) {
    if (!argument) {
        throw new Error("usage: node verify-deployment.mjs https://oxfer.app/");
    }
    const origin = new URL(argument);
    assert.equal(origin.protocol, "https:", "deployment must use HTTPS");
    assert.equal(origin.href, `${origin.origin}/`, "pass an origin, not a path or share link");

    const expected = securityHeaders(
        renderHeaders(await readFile(new URL("./assets/_headers", import.meta.url), "utf8"), process.env.P2P_RELAY_URL),
    );
    assert.equal(expected["Permissions-Policy"], PERMISSIONS_POLICY, "assets/_headers: Permissions-Policy");
    assert.equal(expected["X-Frame-Options"], X_FRAME_OPTIONS, "assets/_headers: X-Frame-Options");
    const enforceNoReporting = operatorControlsZone(origin.hostname);

    let lastError;
    let unenforced = [];
    for (let attempt = 1; attempt <= 10; attempt += 1) {
        try {
            unenforced = await verifyOnce(origin, expected, enforceNoReporting);
            lastError = undefined;
            break;
        } catch (error) {
            lastError = error;
            console.error(`attempt ${attempt}/10 failed: ${error.message}`);
            await sleep(3_000);
        }
    }
    if (lastError) {
        throw lastError;
    }
    if (unenforced.length > 0) {
        console.warn(
            `warning: ${origin.origin} serves ${unenforced.join(" and ")} headers (Cloudflare Network Error ` +
                `Logging). ${origin.hostname} is a workers.dev hostname outside the oxfer.app zone, so that ` +
                'zone\'s "nel" setting does not apply to it, and the privacy notice does not describe it. To ' +
                "stop serving the app there, set workersDev: false and previewUrls: false " +
                "(docs/cloudflare-workers.md).",
        );
    }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    await main(process.argv[2]);
}
