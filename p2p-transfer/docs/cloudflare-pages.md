# Production browser deployment

Cloudflare Pages builds `p2p-transfer` directly from GitHub. A push or merge to
`main` triggers a production deployment after the build succeeds. No deployment
token or GitHub Actions deployment workflow is needed.

## One-time Cloudflare setup

First commit and push the build script to `main`. In Cloudflare, open
**Workers & Pages → Create application → Pages → Connect to Git**, authorize
the Cloudflare GitHub application for `radumarias/syncoxiders`, and select that
repository. Use:

| Setting | Value |
| --- | --- |
| Production branch | `main` |
| Framework preset | None |
| Root directory | `p2p-transfer` |
| Build command | `bash build-pages.sh` |
| Build output directory | `dist` |
| Build system | v3 |
| Environment variable | `SKIP_DEPENDENCY_INSTALL=1` |

Set the environment variable for both production and preview builds. It skips
irrelevant automatic package installation; the script installs its own Rust
and Trunk prerequisites. The checkout still includes the parent Cargo workspace
and its lockfile even though the build root is `p2p-transfer`.

Choose a project name (it determines `<project>.pages.dev`), then **Save and
Deploy**. In the project's branch controls, leave automatic production
deployments enabled for `main`. Disable preview deployments if only `main`
should build, or leave previews enabled to test pull requests on separate URLs.
Leave build watch paths at their default so changes to the workspace
`Cargo.toml` and `Cargo.lock` also trigger builds.

An existing **Git-integrated** Pages project can use these same settings.
An existing **Direct Upload** project cannot be converted to Git integration:
create a new Git-integrated project, or use GitHub Actions with Wrangler if
keeping that existing project and its `pages.dev` hostname is required. Do not
delete a live project just to connect GitHub.

Connecting GitHub, choosing the production branch, and enabling automatic
deployments are Cloudflare dashboard settings, not settings this repository can
activate by itself.

## Build behavior

`build-pages.sh` uses the crate's `rust-toolchain` (currently rolling nightly),
downloads checksum-verified Trunk 0.21.14 for Cloudflare's Linux x86_64 build
environment, and runs:

```sh
trunk build --release --locked --public-url /
```

The script fails rather than updating the workspace lockfile, using a stale
crate-local lockfile, or publishing assets over Pages' 25 MiB per-file limit.
Trunk copies the service worker, browser assets, and `assets/_headers` into
`dist`. The headers require revalidation of the app's stable filenames and
prevent caching of `sw.js`. Keep these headers when changing hosting settings.

The app is hosted at `/`, not `/p2p-transfer/`. Pages' default SPA fallback
serves client-side routes such as `/diags`; do not add a top-level `404.html`
to this app's output. The repository's Jekyll website is not part of this build.

For a local production build on Linux x86_64:

```sh
bash p2p-transfer/build-pages.sh
```

The nightly toolchain is not date-pinned, so `--locked` fixes dependencies but
does not make builds fully reproducible across toolchain updates.

## Verify the first deployment

1. Check the Pages deployment shows branch `main` and the expected commit.
2. Open the HTTPS production URL and compare the app's displayed revision.
3. Reload normally (without `#dev`) and confirm the service worker loads.
4. Open `/diags` directly and run the paired diagnostics on two browsers.
5. Transfer a small file between the browsers and confirm it saves correctly.
6. Push the next change to `main` and confirm Pages deploys it automatically.

Git integration deploys on a successful Pages build; it does not wait for
GitHub Actions tests. Use required CI checks and branch protection on `main`
if production changes must pass CI before merging.

References: [Git integration](https://developers.cloudflare.com/pages/get-started/git-integration/),
[build image](https://developers.cloudflare.com/pages/configuration/build-image/).
