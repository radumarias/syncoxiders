# Production browser deployment

The browser app is a Cloudflare Worker with static assets. Trunk builds the
Rust/WASM client and Cloudflare hosts the static files. The only server code is
[`worker/index.mjs`](../worker/index.mjs), which answers AI agents with Markdown
(see [Markdown for agents](#markdown-for-agents)). File transfers stay
peer-to-peer in the browser.

Use the **`cf` CLI** for this crate. There is no `wrangler.jsonc` /
`wrangler.toml`. Do not introduce one, and do not fall back to Wrangler if a
`cf` command fails: run `cf --help` or `cf cli search <what you want to do>`.

`cf deploy` without `--prebuilt` expects a Vite or Wrangler bundle. This app is
built with Trunk, so the production path is always: package Trunk `dist/` into
cf's Build Output, then `cf deploy --prebuilt`.

## Local production deploy

From `p2p-transfer/`:

```sh
npm ci
npm run build
npm run deploy
node verify-deployment.mjs https://oxfer.42dev.workers.dev/
```

`npm run build` is `bash build-web.sh`: Trunk, then `package-cf-output.mjs`
copies `dist/` into `.cloudflare/output/v0/workers/default/assets/`. That tree
is gitignored. `npm run deploy` is `cf deploy --prebuilt` using the `cf`
version in `package.json`.

`package-cf-output.mjs` includes `oxfer.app` and `www.oxfer.app` by default so a
later `cf deploy --prebuilt` cannot drop those custom domains. Use
`--no-domains` only for an isolated `workers.dev` upload.

## Worker settings

[`cloudflare.config.ts`](../cloudflare.config.ts) is the committed Worker
identity. Keep it aligned with `package-cf-output.mjs`:

| Setting | Value |
| --- | --- |
| Worker name | `oxfer` |
| Compatibility date | `2026-09-29` |
| `workers.dev` | enabled |
| Preview URLs | enabled |
| Not-found handling | `single-page-application` (`/diags` serves `index.html`) |
| Server entrypoint | `worker/index.mjs`, packaged as `bundle/index.mjs` |
| Run Worker first | `WORKER_FIRST` in `worker/index.mjs`: `/`, `/diags` and their `.md` views |
| Bindings | `ASSETS` (the static assets) |

No paid Workers features are required. Static asset requests are free. The
paths above invoke the Worker, and so does any non-navigation request that
matches no asset (with single-page-application handling those reach the Worker,
which hands them back to the assets), so they count against the Workers request
quota (100,000 a day on the Free plan); the WASM, scripts, images and every
other file stay free asset requests. If the account is on the Free plan and the
home page ever nears that quota, move to Workers Paid or drop `/` and `/diags`
from `runWorkerFirst` (agents can still fetch `/index.md`).

## Markdown for agents

The HTML at `/` and `/diags` is an app shell drawn by WebAssembly on a canvas, so
an agent that fetches it sees "Loading…". The Worker serves a Markdown version
instead:

| Request | Response |
| --- | --- |
| `GET /index.md`, `GET /diags.md` | Markdown, always |
| `GET /` or `/diags` with `Accept: text/markdown` ranked at least as high as HTML | Markdown |
| `GET /` or `/diags` from a self-declared AI agent user agent (`ClaudeBot`, `Claude-User`, `GPTBot`, `ChatGPT-User`, `PerplexityBot`, …) that is not a browser navigation (`Sec-Fetch-Mode: navigate`) | Markdown, unless it sends `text/markdown;q=0` |
| Anything else, including browsers and search crawlers | The HTML shell |

The Markdown is front matter (`title`, `url`) plus a source file: `/` uses
[`assets/llms.txt`](../assets/llms.txt) and `/diags` uses
[`assets/diags.md`](../assets/diags.md), so `/llms.txt` and `/index.md` never
drift apart. Negotiated responses carry `Vary: Accept, User-Agent`; HTML
responses also carry `Link: </index.md>; rel="alternate"; type="text/markdown"`,
and `index.html` has the same `<link rel="alternate">`.

In the page, [`assets/view-toggle.js`](../assets/view-toggle.js) adds a
**Human / Machine** switch. **Machine** covers the canvas with exactly the
Markdown the Worker serves for the current page; **Human** (or Escape) returns to
the app. It never changes the URL, so a share link's fragment is untouched. It is
hidden when `navigator.webdriver` is set, so `oxfer-agent.mjs` screenshots and
clicks are unaffected. Under `trunk serve` there is no Worker and it shows the
source files directly.

Check it from a shell:

```sh
curl -sH 'Accept: text/markdown' https://oxfer.app/ | head
curl -sI https://oxfer.app/ | grep -i -e vary -e link
```

Cloudflare's zone-level "Markdown for Agents" setting converts HTML to Markdown
at the edge. It does not help here: it would convert the shell, not the app.

## Custom domains

Those hostnames are Worker custom domains. The previous Pages project remains as
rollback at `oxfer.pages.dev` and is no longer bound to `oxfer.app`.

```sh
node verify-deployment.mjs https://oxfer.app/
node verify-deployment.mjs https://www.oxfer.app/
```

## Verify a deployment

1. The Worker version matches the intended Git commit (shown in the app).
2. `node verify-deployment.mjs <origin>/` matches the local `dist` bytes,
   `/diags`, WASM, the service worker, `_headers`, and the Markdown views.
3. Reload without `#dev` and confirm the service worker controls the page.
4. Transfer a small file between two browsers.

## Automatic deploys from `main`

[`.github/workflows/oxfer-web.yml`](../../.github/workflows/oxfer-web.yml) runs
`npm ci`, `npm run build`, and `npx cf deploy --prebuilt` on each push to `main`
that touches this crate, the workspace lockfile, or the workflow itself.
Cloudflare Workers Builds cannot watch this repository yet: the account is not
connected to GitHub (`This project is disconnected from your Git account`).

One-time GitHub Actions secrets:

1. Create a Cloudflare **Account API token** with the **Edit Cloudflare Workers**
   template: [Account API tokens](https://dash.cloudflare.com/profile/api-tokens).
2. In the GitHub repo: **Settings → Secrets and variables → Actions**:
   - `CLOUDFLARE_API_TOKEN` — the token value
   - `CLOUDFLARE_ACCOUNT_ID` — `3c0c25f9a4a8f887acef630c72f55e6e`

After the secrets exist, **Actions → oxfer-web → Run workflow** deploys the
current `main` without waiting for another commit.

`cf` is currently open beta. Pin the version in `package.json` (and the npm
lockfile) until its Build Output format stabilizes.

Account operations use the same CLI, for example `cf workers get oxfer` or
`cf cli search "custom domains"`.

References: [cf CLI](https://blog.cloudflare.com/cloudflare-cf-cli-launch/),
[static-asset billing](https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/).
