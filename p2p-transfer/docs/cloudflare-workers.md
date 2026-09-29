# Production browser deployment

The browser app is an assets-only Cloudflare Worker. Trunk still builds the
Rust/WASM client; Cloudflare only hosts the static files. File transfers stay
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
| Server entrypoint | none |

No paid Workers features are required. Static asset requests are free.

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
   `/diags`, WASM, the service worker, and `_headers`.
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
