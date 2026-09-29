# Production browser deployment

The browser app is an assets-only Cloudflare Worker. Trunk still builds the
Rust/WASM client; Cloudflare only hosts the static files. File transfers stay
peer-to-peer in the browser.

`cf pages deploy` cannot upload this project: that command is for legacy Pages
and refuses Direct Upload projects such as the previous `oxfer` Pages site.
Use `cf deploy --prebuilt` instead.

## Local production deploy

From `p2p-transfer/`:

```sh
bash build-web.sh
npx --yes cf@1.0.0-beta.5 deploy --prebuilt
node verify-deployment.mjs https://oxfer.<account>.workers.dev/
```

`build-web.sh` runs Trunk, then `package-cf-output.mjs` copies `dist/` into the
cf Build Output tree at `.cloudflare/output/v0/workers/default/assets/`. That
tree is gitignored. `--prebuilt` uploads it without invoking Vite or Wrangler.

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
`bash build-web.sh` and `npx cf@1.0.0-beta.5 deploy --prebuilt` on each push to
`main` that touches this crate, the workspace lockfile, or the workflow itself.
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

`cf` is currently open beta; pin `cf@1.0.0-beta.5` until its Build Output format
stabilizes.

References: [Migrate from Pages to Workers](https://developers.cloudflare.com/workers/static-assets/migration-guides/migrate-from-pages/),
[static-asset billing](https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/),
[cf CLI](https://blog.cloudflare.com/cloudflare-cf-cli-launch/).
