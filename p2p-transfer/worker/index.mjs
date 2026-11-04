// Oxfer's only server code: Markdown content negotiation in front of the static assets.
//
// The HTML at `/` and `/diags` is an app shell whose interface is a WebAssembly
// program drawn on a canvas, so an agent that fetches it learns nothing. This Worker
// runs first for the paths in `assets.runWorkerFirst` (see package-cf-output.mjs).
// Other requests are served by the asset layer; only a non-navigation request that
// matches no asset also reaches it (single-page-application handling) and is passed
// straight back to the assets.
//
// - `/index.md` and `/diags.md` always return the Markdown version of those pages.
// - `/` and `/diags` return Markdown when the client asks for it (`Accept: text/markdown`
//   ranked at least as high as HTML) or identifies as a known AI agent outside a
//   browser navigation, and the HTML shell otherwise. Both answers carry `Vary: Accept, User-Agent`.
//
// The fragment of a share link never reaches the server, so this code cannot see,
// log or leak a capability: it only ever sees the path.

/** Markdown pages: virtual path, page path, source asset, and title. */
export const PAGES = [
    { md: "/index.md", html: ["/", "/index.html"], source: "/llms.txt", title: "Oxfer" },
    { md: "/diags.md", html: ["/diags", "/diags/"], source: "/assets/diags.md", title: "Oxfer diagnostics" },
];

/** Paths the Worker must see before the asset layer answers. */
export const WORKER_FIRST = PAGES.flatMap(page => [page.md, ...page.html]);

/** Path → page, and whether the path is the page's `.md` view. */
const ROUTES = new Map(PAGES.flatMap(page => [
    [page.md, { page, direct: true }],
    ...page.html.map(path => [path, { page, direct: false }]),
]));

// Self-declared AI crawlers and user-triggered agent fetchers. These cannot run the
// WebAssembly app, so the shell is never useful to them. Search-engine crawlers are
// deliberately absent: they index the HTML page.
const AGENT_UA = /\b(ClaudeBot|Claude-User|Claude-SearchBot|anthropic-ai|GPTBot|ChatGPT-User|OAI-SearchBot|PerplexityBot|Perplexity-User|meta-externalagent|meta-externalfetcher|MistralAI-User|DuckAssistBot|cohere-ai|CCBot|Amazonbot|Bytespider)\b/i;

/** Parse an Accept header into `[type, q]` pairs, ignoring malformed entries. */
function mediaRanges(accept) {
    const ranges = [];
    for (const part of (accept ?? "").split(",")) {
        const [type, ...params] = part.trim().toLowerCase().split(";");
        if (!type || !type.includes("/")) continue;
        let q = 1;
        for (const param of params) {
            const [key, value] = param.trim().split("=");
            if (key === "q") {
                const n = Number(value);
                q = Number.isFinite(n) ? Math.min(Math.max(n, 0), 1) : 0;
            }
        }
        ranges.push([type.trim(), q]);
    }
    return ranges;
}

/** Whether a request for an HTML page should get its Markdown version instead. */
export function wantsMarkdown(headers) {
    const ranges = mediaRanges(headers.get("accept"));
    // Only an explicit text/markdown counts for Markdown; wildcards only count for HTML.
    const markdown = ranges.find(([type]) => type === "text/markdown");
    if (markdown && markdown[1] === 0) return false;
    // A browser navigation renders the app, whatever its user agent claims: an agent
    // that drives a real browser is told (llms.txt) to use the page itself.
    const navigation = headers.get("sec-fetch-mode") === "navigate";
    if (!navigation && AGENT_UA.test(headers.get("user-agent") ?? "")) return true;
    if (!markdown) return false;
    // HTML's q-value is that of its most specific matching range (RFC 9110 §12.5.1).
    const qOf = wanted => ranges.find(([type]) => type === wanted)?.[1];
    const html = qOf("text/html") ?? qOf("text/*") ?? qOf("*/*") ?? 0;
    return markdown[1] >= html;
}

function frontMatter(page, url) {
    return `---\ntitle: ${JSON.stringify(page.title)}\nurl: ${JSON.stringify(url)}\n---\n\n`;
}

function withVary(headers) {
    const vary = new Set((headers.get("vary") ?? "").split(",").map(v => v.trim()).filter(Boolean));
    vary.add("Accept");
    vary.add("User-Agent");
    headers.set("Vary", [...vary].join(", "));
}

async function markdownResponse(request, env, page) {
    const { origin } = new URL(request.url);
    const canonical = new URL(page.html[0], origin).href;
    // A fresh GET: the browser's validators belong to the page, not to the source file.
    const source = await env.ASSETS.fetch(new URL(page.source, origin));
    if (!source.ok) {
        const headers = new Headers(source.headers);
        withVary(headers);
        const body = request.method === "HEAD" ? null : source.body;
        return new Response(body, { status: source.status, statusText: source.statusText, headers });
    }
    const body = request.method === "HEAD" ? null : frontMatter(page, canonical) + await source.text();
    const headers = new Headers(source.headers);
    for (const name of ["content-length", "etag", "last-modified", "content-encoding"]) {
        headers.delete(name);
    }
    headers.set("Content-Type", "text/markdown; charset=utf-8");
    headers.set("Link", `<${canonical}>; rel="canonical"`);
    withVary(headers);
    return new Response(body, { status: 200, headers });
}

export default {
    async fetch(request, env) {
        const { pathname } = new URL(request.url);
        if (request.method !== "GET" && request.method !== "HEAD") {
            return env.ASSETS.fetch(request);
        }
        const route = ROUTES.get(pathname);
        if (route && (route.direct || wantsMarkdown(request.headers))) {
            return markdownResponse(request, env, route.page);
        }
        const response = await env.ASSETS.fetch(request);
        if (!route) return response;
        const { page } = route;
        const headers = new Headers(response.headers);
        withVary(headers);
        headers.append("Link", `<${page.md}>; rel="alternate"; type="text/markdown"`);
        return new Response(response.body, {
            status: response.status,
            statusText: response.statusText,
            headers,
        });
    },
};
