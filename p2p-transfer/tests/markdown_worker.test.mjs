import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import worker, { PAGES, WORKER_FIRST, wantsMarkdown } from "../worker/index.mjs";
import { workerConfig } from "../package-cf-output.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const CHROME = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8";
const HTML = "<!doctype html><title>Oxfer</title>\n";

// Stand-in for the assets binding: real sources for the Markdown files, the shell
// for everything else (single-page-application fallback), and the _headers policy.
async function fakeAssets() {
    const files = {
        "/llms.txt": [await read("../assets/llms.txt"), "text/plain; charset=utf-8"],
        "/assets/diags.md": [await read("../assets/diags.md"), "text/markdown; charset=utf-8"],
    };
    const seen = [];
    return {
        seen,
        async fetch(input) {
            const request = input instanceof Request ? input : new Request(input);
            const { pathname } = new URL(request.url);
            seen.push(pathname);
            const [body, type] = files[pathname] ?? [HTML, "text/html; charset=utf-8"];
            return new Response(body, {
                headers: {
                    "Content-Type": type,
                    "Cache-Control": "public, max-age=0, must-revalidate",
                    "Referrer-Policy": "no-referrer",
                    "X-Content-Type-Options": "nosniff",
                    ETag: '"source"',
                },
            });
        },
    };
}

async function get(path, headers = {}, method = "GET") {
    const ASSETS = await fakeAssets();
    const response = await worker.fetch(new Request(`https://oxfer.app${path}`, { method, headers }), { ASSETS });
    return { response, body: await response.text(), seen: ASSETS.seen };
}

test("browsers get the HTML shell, with Vary and a Markdown alternate", async () => {
    for (const path of ["/", "/diags"]) {
        const { response, body } = await get(path, { Accept: CHROME, "User-Agent": "Mozilla/5.0 Chrome/141" });
        assert.equal(body, HTML);
        assert.match(response.headers.get("content-type"), /^text\/html/);
        assert.equal(response.headers.get("vary"), "Accept, User-Agent");
        const md = PAGES.find(page => page.html.includes(path)).md;
        assert.equal(response.headers.get("link"), `<${md}>; rel="alternate"; type="text/markdown"`);
        assert.equal(response.headers.get("referrer-policy"), "no-referrer");
    }
});

test("Accept: text/markdown gets the Markdown page with front matter", async () => {
    const llms = await read("../assets/llms.txt");
    const { response, body } = await get("/", { Accept: "text/markdown, text/html, */*" });
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("content-type"), "text/markdown; charset=utf-8");
    assert.equal(response.headers.get("vary"), "Accept, User-Agent");
    assert.equal(response.headers.get("etag"), null, "the source file's validator must not leak");
    assert.equal(response.headers.get("link"), '<https://oxfer.app/>; rel="canonical"');
    assert.equal(response.headers.get("x-content-type-options"), "nosniff");
    assert.equal(body, `---\ntitle: "Oxfer"\nurl: "https://oxfer.app/"\n---\n\n${llms}`);

    for (const topic of ["## Chat rooms for agents", "--chat \"<room-link>\"", "recv \"<room-link>\"",
        "oxfer-agent.mjs room", "For agents", "window.oxfer.chat.send", "Choose where to save"]) {
        assert.ok(body.includes(topic), `the home page Markdown covers ${topic}`);
    }

    const diags = await get("/diags", { Accept: "text/markdown" });
    assert.match(diags.body, /^---\ntitle: "Oxfer diagnostics"\nurl: "https:\/\/oxfer.app\/diags"\n---\n\n# Oxfer diagnostics\n/);
});

test("/index.md and /diags.md are Markdown whatever the client asks for", async () => {
    for (const [path, heading] of [["/index.md", "# Oxfer\n"], ["/diags.md", "# Oxfer diagnostics\n"]]) {
        const { response, body } = await get(path, { Accept: CHROME });
        assert.equal(response.headers.get("content-type"), "text/markdown; charset=utf-8");
        assert.ok(body.includes(`---\n\n${heading}`), path);
    }
    const head = await get("/index.md", {}, "HEAD");
    assert.equal(head.response.status, 200);
    assert.equal(head.body, "");
});

test("known AI agents get Markdown unless they refuse it", async () => {
    for (const ua of [
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0; +claudebot@anthropic.com)",
        "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko); compatible; ChatGPT-User/1.0; +https://openai.com/bot",
        "Mozilla/5.0 (compatible; PerplexityBot/1.0; +https://perplexity.ai/perplexitybot)",
    ]) {
        const { response } = await get("/", { Accept: CHROME, "User-Agent": ua });
        assert.match(response.headers.get("content-type"), /^text\/markdown/, ua);
    }
    const refused = await get("/", { Accept: "text/html, text/markdown;q=0", "User-Agent": "ClaudeBot/1.0" });
    assert.match(refused.response.headers.get("content-type"), /^text\/html/);
});

test("content negotiation follows q-values and ignores wildcards for Markdown", () => {
    const h = (accept, ua = "") => new Headers({ accept, "user-agent": ua });
    assert.equal(wantsMarkdown(h("text/markdown")), true);
    assert.equal(wantsMarkdown(h("text/markdown, text/html")), true);
    assert.equal(wantsMarkdown(h("text/html, text/markdown;q=0.9")), false);
    assert.equal(wantsMarkdown(h("text/markdown;q=0.9, */*;q=0.1")), true);
    assert.equal(wantsMarkdown(h("TEXT/MARKDOWN ; Q=1")), true);
    assert.equal(wantsMarkdown(h("*/*")), false);
    assert.equal(wantsMarkdown(h("text/*")), false);
    assert.equal(wantsMarkdown(h(CHROME)), false);
    assert.equal(wantsMarkdown(h("text/markdown;q=bogus")), false);
    assert.equal(wantsMarkdown(new Headers()), false);
    assert.equal(wantsMarkdown(h("*/*", "Googlebot/2.1")), false, "search crawlers index the HTML");
    assert.equal(wantsMarkdown(h("*/*", "curl/8.9")), false);
});

test("other paths and methods pass straight to the assets", async () => {
    const asset = await get("/llms.txt", { Accept: "text/markdown" });
    assert.match(asset.response.headers.get("content-type"), /^text\/plain/);
    assert.equal(asset.response.headers.get("vary"), null);
    const ASSETS = await fakeAssets();
    const post = await worker.fetch(new Request("https://oxfer.app/", { method: "POST", headers: { Accept: "text/markdown" } }), { ASSETS });
    assert.match(post.headers.get("content-type"), /^text\/html/);
});

test("the Worker runs only for the negotiated pages", async () => {
    assert.deepEqual(workerConfig.assets.runWorkerFirst, WORKER_FIRST);
    assert.deepEqual([...WORKER_FIRST].sort(), ["/", "/diags", "/diags.md", "/diags/", "/index.html", "/index.md"]);
    const source = await read("../cloudflare.config.ts");
    assert.match(source, /import \{ WORKER_FIRST \} from "\.\/worker\/index\.mjs"/);
    assert.match(source, /runWorkerFirst: \[\.\.\.WORKER_FIRST\]/);
});

test("the shell links the Markdown view and loads the Human/Machine switch", async () => {
    const html = await read("../index.html");
    assert.match(html, /<link rel="alternate" type="text\/markdown" href="\/index.md"/);
    assert.match(html, /<script defer src="assets\/view-toggle.js"><\/script>/);
    assert.match(html, /copy-file" href="assets\/view-toggle.js"/);
    assert.match(html, /copy-file" href="assets\/diags.md"/);
    const toggle = await read("../assets/view-toggle.js");
    assert.match(toggle, /if \(navigator\.webdriver\) return;/, "automation must not see the floating pill");
    assert.doesNotMatch(toggle, /location\.hash\s*=|replaceState|pushState/, "the switch must not touch the URL");
    assert.doesNotMatch(toggle, /innerHTML/, "Markdown is shown as text, never parsed as HTML");
    for (const page of PAGES) {
        const pair = `["${page.md}", "${page.source}"]`;
        const entry = page.md === "/index.md" ? `const INDEX = ${pair};` : `"${page.html[0]}": ${pair}`;
        assert.ok(toggle.includes(entry), `view-toggle.js maps ${page.html[0]} like PAGES does`);
    }
});
