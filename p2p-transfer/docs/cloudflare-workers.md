# Production browser deployment

The browser app is an assets-only Cloudflare Worker served at
`https://oxfer.app`. Trunk still builds the Rust/WASM client; Cloudflare only
hosts the static files. File transfers stay peer-to-peer in the browser.

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

`npm run build` is `bash build-web.sh`. In order, it:

1. Fails if a stale `p2p-transfer/Cargo.lock` exists; builds use the
   workspace's `../Cargo.lock`.
2. Before the long build, runs one call on the source pages,
   `node package-cf-output.mjs --check-relay --check-legal`:
   - validates `P2P_RELAY_URL` ([relay selection](#relay-selection-p2p_relay_url)).
     An empty or ASCII-whitespace-only value is unset first; any other value
     with a character outside printable ASCII fails;
   - fails while the legal pages still contain placeholders
     ([legal pages](#legal-pages-and-the-placeholder-guard));
   - fails when the legal pages are filled but `P2P_RELAY_URL` does not list
     only `https://relay.oxfer.app`: the published privacy notice would not
     match the relays the build uses
     ([relay go-live guard](#relay-go-live-guard)).
3. Runs a Trunk 0.21.14 release build. The Trunk binary is downloaded into
   `target/pages-tools/` and checked against a pinned SHA-256.
4. Fails if any file is over Cloudflare's 25 MiB asset limit.
5. Runs `node package-cf-output.mjs --check-dist` (`checkDist`, which
   `check.sh` runs too). It fails unless every Trunk `copy-file` entry of
   `index.html` (the page scripts and stylesheet, `theme.html`, the three
   legal pages, `_headers`, `sw.js`, the manifest, the icons and the images)
   is in `dist/`, at its `data-target-path` or the root, byte for byte. It
   also fails if any HTML file in `dist/` has a `<script>` without `src`, or
   if `dist/index.html` is missing or empty. It reports every problem at
   once.
6. Runs `package-cf-output.mjs`. It copies `dist/` into
   `.cloudflare/output/v0/workers/default/assets/` and writes a rendered
   `_headers` there ([security headers](#security-headers)).

The `.cloudflare/` tree is gitignored. `npm run deploy` is
`cf deploy --prebuilt` using the `cf` version pinned in `package.json`.

Run `verify-deployment.mjs` with the same `P2P_RELAY_URL` as the build. It
renders the expected headers, Content-Security-Policy included, from that
value.

`package-cf-output.mjs` includes `oxfer.app` and `www.oxfer.app` by default so a
later `cf deploy --prebuilt` cannot drop those custom domains. Use
`--no-domains` only for an isolated `workers.dev` upload.

## Worker settings

[`cloudflare.config.ts`](../cloudflare.config.ts) is the committed Worker
identity. Keep it aligned with `workerConfig` in `package-cf-output.mjs`;
`tests/package-cf-output.test.mjs` compares them.

| Setting | Value |
| --- | --- |
| Worker name | `oxfer` |
| Compatibility date | `2026-09-29` |
| `workers.dev` | enabled |
| Preview URLs | enabled |
| Not-found handling | `single-page-application` (`/diags` serves `index.html`) |
| HTML handling | default `auto-trailing-slash`: `/privacy` serves `privacy.html`, and `/privacy.html` redirects (307) to `/privacy` |
| Server entrypoint | none |

No `_redirects` file is used. `verify-deployment.mjs` checks that `/privacy`,
`/terms` and `/abuse` are served without a redirect.

No paid Workers features are required. Static asset requests are free.

## Custom domains

`oxfer.app` and `www.oxfer.app` are Worker custom domains. `oxfer.app` is the
production host.

The earlier Pages project at `oxfer.pages.dev` is no longer bound to
`oxfer.app`, but on 30 September 2026 it still served an older build. That
build has no legal pages (every path falls back to the app page) and none of
the [security headers](#security-headers). Delete the Pages project, or
redeploy it from the same build as production. Until then it is a second,
stale copy of the app.

```sh
node verify-deployment.mjs https://oxfer.app/ https://www.oxfer.app/
```

## Verify a deployment

1. The Worker version matches the intended Git commit (shown in the app).
2. `node verify-deployment.mjs <origin>/ [<origin>/ ...]` checks every
   origin given, concurrently. For each it fetches `/`, `/diags`, the JS and
   WASM bundle, the service worker, `theme.html`, `/privacy`, `/terms`,
   `/abuse` and the static scripts, stylesheet and images, all at once. For
   each path it checks the status and MIME type, that the bytes equal the
   local `dist/`, and then every header `assets/_headers` gives that path:
   rendered with `P2P_RELAY_URL`, with the values of every matching rule
   joined as Cloudflare joins them (`headersForPath` in
   `package-cf-output.mjs`). Each header must match exactly, and
   `Cache-Control` as a set of directives (order, case and repeats do not
   matter). So `/privacy`, `/terms` and `/abuse` need `no-transform` and no
   other path may have it, and `/sw.js` needs `no-store`. All mismatches of a
   response are reported together. It also fails if a response carries
   `NEL`, `Report-To` or `Reporting-Endpoints`
   ([Network Error Logging](#network-error-logging-must-be-off)); on a
   `*.workers.dev` origin it only warns. While the edge updates, a path whose
   request, status, redirect, MIME type or bytes fail is retried on its own,
   up to 10 attempts 3 seconds apart. The headers are checked only once the
   bytes are this build's, so a header mismatch or NEL fails at once, without
   retrying. A deploy that changes only `assets/_headers` and is checked
   while the edge still serves the old headers therefore fails; run the check
   again a little later.
3. Reload without `#dev` and confirm the service worker controls the page.
4. Transfer a small file between two browsers.
5. Open `/diags` and check the relay lines ([docs/diagnostics.md](diagnostics.md)).

## Relay selection: `P2P_RELAY_URL`

Production uses n0's public relays until the GitHub Actions **repository
variable** `P2P_RELAY_URL` is set. It is a variable, not a secret: its value is
compiled into the public WASM bundle and appears in a response header. Never
put a credential or token in it.

One value is used in five places:

| Where | What happens |
| --- | --- |
| `.github/workflows/oxfer-web.yml` | The deploy job sets `P2P_RELAY_URL: ${{ vars.P2P_RELAY_URL }}`. An unset variable arrives as an empty string. |
| `build-web.sh` | An empty or ASCII-whitespace-only value is unset, which means n0's relays. Any other value is exported and checked with `node package-cf-output.mjs --check-relay --check-legal` before the long build. Once the legal pages are filled, that call also requires the value to list only `https://relay.oxfer.app` ([relay go-live guard](#relay-go-live-guard)). |
| `src/node.rs` | `RelayChoice::from_env()` reads `option_env!("P2P_RELAY_URL")` at compile time. A valid list selects `RelayChoice::Custom`, built on iroh's `presets::Minimal`: only the listed relays, with no n0 relay map and no pkarr publishing or lookup at `dns.iroh.link`. |
| `package-cf-output.mjs` | Replaces `{{RELAY_CONNECT_SRC}}` in `_headers` with the matching CSP `connect-src` sources. |
| `verify-deployment.mjs` | Renders the same policy and asserts that every origin it checks serves it. |

Format: a comma-separated list of `https://host[:port]` URLs, with `http://`
only for a local test relay. Spaces around commas are trimmed, order is kept
and duplicates are dropped. The value must be printable ASCII: the build
fails for any other character, visible or not, such as a byte-order mark
(U+FEFF) or a no-break space pasted in with the value. It also fails for an
empty entry (for example a trailing comma), another scheme such as `wss://`,
credentials, a path, query or fragment, an IPv6 literal, or a host with a
trailing dot. Error messages give the entry's position, never its text; for
an invisible character they also give its code point, for example
`P2P_RELAY_URL entry 1 contains the invisible character U+FEFF; retype the value`.

| `P2P_RELAY_URL` | Relay sources in `connect-src` |
| --- | --- |
| unset, empty or blank | `https://*.iroh.link wss://*.iroh.link` (n0 relays, their HTTPS probes, and pkarr at `dns.iroh.link`) |
| `https://relay.oxfer.app` | `https://relay.oxfer.app wss://relay.oxfer.app` |
| `https://a.example, https://b.example:8443` | `https://a.example wss://a.example https://b.example:8443 wss://b.example:8443` |

The Rust parser in `src/node.rs` accepts some values the build check
rejects, such as a path or a trailing-dot host. The reverse must never
happen, because the app would then fall back to n0 at run time while the
CSP and the privacy notice name the configured relay. Trimming is where the
two differ: Rust's `str::trim` keeps U+FEFF, which JavaScript's `trim`
strips, so a check that trimmed would accept a value starting with a
byte-order mark that the app then rejects. The build check therefore
rejects every character outside printable ASCII instead of trimming it. A
build that skips `build-web.sh` (plain `trunk build`, `trunk serve` or
`cargo`) with an invalid value logs
`P2P_RELAY_URL is not a valid relay list (...); using the default` in the
browser console and uses n0's relays.

Switching production to the self-hosted relay (runbook:
[`deploy/relay/README.md`](../deploy/relay/README.md#switching-production-to-the-relay)):

```sh
gh variable set P2P_RELAY_URL --repo radumarias/syncoxiders --body https://relay.oxfer.app
gh workflow run oxfer-web.yml --repo radumarias/syncoxiders --ref main
```

Then open `https://oxfer.app/diags`. It should list
`Relay wss://relay.oxfer.app/relay` and no `*.relay.n0.iroh.link` probes.
To roll back before the legal pages are filled, run
`gh variable delete P2P_RELAY_URL --repo radumarias/syncoxiders` and re-run
the workflow. Once they are filled, that build fails at the
[relay go-live guard](#relay-go-live-guard), which requires the list to be
`https://relay.oxfer.app` alone whatever the pages say. A rollback to n0 is
then one commit that edits the privacy notice (and any page or record that
names `relay.oxfer.app`) to describe n0's relays, and changes the guard
(`OPERATOR_RELAY` and `checkLegalPages` in `package-cf-output.mjs`) and its
tests in `tests/web-pages.test.mjs` to match; delete the variable when that
commit lands ([relay runbook](../deploy/relay/README.md#rollback)).

The same value works locally:

```sh
P2P_RELAY_URL=https://relay.oxfer.app npm run build
P2P_RELAY_URL=https://relay.oxfer.app node verify-deployment.mjs https://oxfer.app/
```

## Security headers

[`assets/_headers`](../assets/_headers) sets these on every path (`/*`).
Cloudflare comma-joins the values of every rule that matches a request path,
so a path with its own `Cache-Control` rule gets both:

- `/sw.js` adds `no-cache, no-store, must-revalidate`, so the service worker
  is served with `no-store`.
- `/privacy`, `/terms` and `/abuse` add `no-transform`
  ([why only these](#why-cache-control-no-transform)). `/privacy.html` and
  the other `.html` spellings only redirect (307) to these paths.

| Header | Value |
| --- | --- |
| `Cache-Control` | `public, max-age=0, must-revalidate` |
| `Referrer-Policy` | `no-referrer` |
| `X-Content-Type-Options` | `nosniff` |
| `Content-Security-Policy-Report-Only` | the policy below |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), midi=(), screen-wake-lock=(self)` |
| `X-Frame-Options` | `SAMEORIGIN` |

No script keeps its own copy of these headers: `verify-deployment.mjs`
derives each path's expected headers from `assets/_headers`, and
`tests/package-cf-output.test.mjs` pins the values in the file.

The Content-Security-Policy, with the relay token rendered at packaging
time:

```text
default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline';
img-src 'self' data: blob:; font-src 'self'; connect-src 'self' {{RELAY_CONNECT_SRC}};
worker-src 'self' blob:; frame-src 'self' blob:; manifest-src 'self'; object-src 'none';
base-uri 'self'; form-action 'none'; frame-ancestors 'self'
```

`assets/_headers` holds it on one line. `dist/_headers` stays a byte copy of
the source with the token still in it. Only the packaged copy under
`.cloudflare/output/` is rendered. `package-cf-output.mjs` also refuses to
package when:

- the token is missing, or another `{{...}}` token is left after rendering;
- a line is longer than 2,000 characters, or there are more than 100 rules
  (Cloudflare's `_headers` limits).

Before it fetches anything, `verify-deployment.mjs` also refuses a rendered
file that sets the CSP, `Permissions-Policy` or `X-Frame-Options` outside
`/*`, or sets one twice (`securityHeaders`), because Cloudflare would then
comma-join the values.

`script-src 'self'` allows no inline script. `Trunk.toml` therefore sets
`inject_scripts = false`, and `index.html` loads `assets/boot.js` (service
worker registration, `#dev` reset) and then the `assets/app-init.js` module,
which starts the WASM. Every served HTML page must avoid inline `<script>`,
`on*` attributes, `javascript:` URLs and cross-origin subresources;
`tests/web-pages.test.mjs` checks this.

No `report-uri` or `report-to` directive is set, so violations of the
report-only policy are not sent anywhere. They appear only in the browser
console; Chromium prefixes them with `[Report Only]`. On n0 builds the Diags
page always causes one: its dotted-hostname probe of
`wss://euc1-1.relay.n0.iroh.link./relay`, because a hostname ending in a dot
does not match `*.iroh.link`. If the page's first relay registration fails,
the legacy dotted-DNS check adds more: its HTTPS latency probes of
`https://<relay>.relay.n0.iroh.link./ping` for each of the four n0 relays,
repeated while it runs, and a `wss://<relay>.relay.n0.iroh.link./relay`
connection if one answers
([docs/diagnostics.md](diagnostics.md#n0-builds-trailing-dns-dots)).
Custom-relay builds cause neither.

### Enforcing the policy

Run the manual matrix first. It is listed in
[`docs/compliance/README.md`](compliance/README.md#8-enforce-the-content-security-policy):
current Chrome, Firefox and Safari on desktop, Safari on iOS and Chrome on
Android. On each, cover a direct and a relayed transfer, every download route
(`sink=fsa`, `sink=sw`, `sink=mem`), resume, Diags, the theme page, the legal
pages and keep-screen-on. The policy has only been exercised in Chromium so
far. Keep the console open and fix or account for every `[Report Only]`
message. Then:

1. In `assets/_headers`, rename `Content-Security-Policy-Report-Only:` to
   `Content-Security-Policy:`. Leave the policy text unchanged.
2. In `package-cf-output.mjs`, set the `CSP_REPORT_ONLY` constant to
   `"Content-Security-Policy"`. Renaming the constant is optional; if you do,
   update its imports in `verify-deployment.mjs` and
   `tests/package-cf-output.test.mjs`.
3. In `tests/package-cf-output.test.mjs`, the test "assets/_headers carries the
   report-only CSP, Permissions-Policy and X-Frame-Options" asserts that no
   enforcing `Content-Security-Policy` header exists. Change it to assert that
   `Content-Security-Policy-Report-Only` is gone.
4. Run `node --test tests/package-cf-output.test.mjs tests/web-pages.test.mjs`
   and `bash build-web.sh`.
5. Push to `main`. The deploy job's `verify-deployment.mjs` now asserts the
   enforcing header. Load `/`, `/diags`, `/theme.html` and the legal pages with
   the console open, and transfer a file.

To roll back, revert that commit and redeploy.

### Why `Cache-Control: no-transform`

Only `/privacy`, `/terms` and `/abuse` send `no-transform`.

Cloudflare documents `no-transform` as a way to stop features that edit a
response body, among them
[Email Address Obfuscation](#zone-settings-that-rewrite-responses), which
would rewrite the `mailto:` links on the legal pages and add a script. With
it, the legal pages are served byte for byte as built, which the byte
comparison in `verify-deployment.mjs` and the published
[bundle hashes](#bundle-hashes) rely on. They are the only served pages with
email addresses.

Cloudflare also documents that `no-transform` stops it from compressing
responses with Brotli or gzip. On 30 September 2026 `oxfer.app` served
`p2p-transfer_bg.wasm` with `content-encoding: br`: about 3.6 MB transferred
instead of 9.3 MB. So the app shell, the JavaScript and the WASM do not get
`no-transform`, and keep that compression. Check it after a deploy:

```sh
curl -sS -o /dev/null -D - -H 'Accept-Encoding: br, gzip' \
  https://oxfer.app/p2p-transfer_bg.wasm | grep -i '^content-encoding:'
```

It should print `content-encoding: br` (or `gzip`). The byte comparison in
`verify-deployment.mjs` still catches any rewrite of the other paths.

`verify-deployment.mjs` keeps no list of these paths: it expects each
checked path's `Cache-Control` directives from `assets/_headers`
(`headersForPath` in `package-cf-output.mjs`). It names the directive when a
legal page lacks `no-transform`, and fails when any other checked path has
it. `tests/package-cf-output.test.mjs` pins the rule: its
`NO_TRANSFORM_PATHS` must be exactly the paths `assets/_headers` gives
`no-transform`, every checked path must get the expected directives, and
`verify-deployment.mjs` must check each of the three. To add a page with an
email address, give its clean path a `Cache-Control: no-transform` rule in
`assets/_headers`, add it to `checks` in `verify-deployment.mjs`, and add it
to `NO_TRANSFORM_PATHS` in the test, in the same commit.

## Legal pages and the placeholder guard

`privacy.html`, `terms.html` and `abuse.html` sit at the crate root and share
`assets/legal.css`. Trunk copies them into `dist/`, and the Worker serves them
at `/privacy`, `/terms` and `/abuse`. They are static pages with no scripts.
The app's bottom bar links to them on every screen.

Until the operator's details are filled in, the pages contain these tokens:

| Token | Pages |
| --- | --- |
| `[[OPERATOR_NAME]]`, `[[OPERATOR_ADDRESS]]`, `[[EFFECTIVE_DATE]]` | all three |
| `[[OPERATOR_REGISTRATION]]` | all three; delete the line marked "delete for a natural person" if the operator is not a company |
| `[[RELAY_HOSTING_PROVIDER]]`, `[[RELAY_LOCATION]]` | `privacy.html` |

A comment at the top of each page lists them without brackets.

`build-web.sh` refuses to build while any `[[UPPER_CASE]]` token remains in
`privacy.html`, `terms.html` or `abuse.html`, and names each file and token.
It checks the source pages before the Trunk build (`checkLegalPages` in
`package-cf-output.mjs`); after the build, `--check-dist` proves that `dist/`
holds byte copies of them. Until the tokens are replaced, the `oxfer-web`
workflow fails at its **Build browser app** step on every push to `main`,
before the Trunk build. This is intentional.

For a local test build only:

```sh
OXFER_ALLOW_PLACEHOLDERS=1 npm run build
```

It warns and packages anyway. It also bypasses the
[relay go-live guard](#relay-go-live-guard), but not the relay-list check.
Only the value `1` bypasses them. Never set it in CI and never deploy its
output.

### Relay go-live guard

The filled pages describe the operator's relay at `relay.oxfer.app`, run
from the VPS kit in [`deploy/relay/`](../deploy/relay/README.md). Once no
placeholder is left, `build-web.sh` also refuses to build unless
`P2P_RELAY_URL` lists only `https://relay.oxfer.app`. An unset, empty or
ASCII-whitespace-only value (n0's public relays) fails, and so do a list of
only other relays and a list with `https://relay.oxfer.app` among others:
the filled privacy notice describes one relay, and a build that also used
others would send users' addresses to relays it does not name. Repeating
`https://relay.oxfer.app` passes. The error says which case applies and
never repeats the other entries. The guard is `operatorRelayProblem`, called
by `checkLegalPages` in `package-cf-output.mjs`, which `build-web.sh` runs
before the Trunk build with
`node package-cf-output.mjs --check-relay --check-legal`. The relay is
`OPERATOR_RELAY` in `package-cf-output.mjs`; `build-web.sh` does not name
it. `verify-deployment.mjs` then checks that the deployed CSP matches the
value. The app's "Technical details" likewise credits Oxfer only when every
listed relay has that host (`OPERATOR_RELAY_HOST` in `src/node.rs`), so
change both with the pages.

The guard does not read the page text. Instead, a test in
`tests/web-pages.test.mjs` fails when `privacy.html` no longer names
`relay.oxfer.app`, so the guard changes in the same commit as the pages. The
same file calls `checkLegalPages` directly, and runs the
`node package-cf-output.mjs --check-relay --check-legal` call of
`build-web.sh` against scratch copies of the pages. `OXFER_ALLOW_PLACEHOLDERS=1` bypasses this
guard too, with a warning, for local test builds only. It never bypasses the
relay-list check, which runs first
([relay selection](#relay-selection-p2p_relay_url)).

The pages and the compliance records are written for the VPS kit. The kit's
Fly.io variant would make statements about IP blocking, log retention, the
relay's processor and international transfers false; the relay runbook's
[Fly.io variant](../deploy/relay/README.md#flyio-variant) section lists what
must change first.

So set the repository variable `P2P_RELAY_URL`
([relay selection](#relay-selection-p2p_relay_url)) no later than the commit
that fills the pages. The owner actions in
[`docs/compliance/README.md`](compliance/README.md#owner-actions) give the
full order, including turning off
[Network Error Logging](#network-error-logging-must-be-off) first.

## Zone settings that rewrite responses

`verify-deployment.mjs` compares the served bytes with `dist/`, so any
Cloudflare zone feature that rewrites HTML makes it fail. Email Address
Obfuscation is one such feature. Cloudflare turns it on for new zones. It
rewrites email addresses, including `mailto:` links, and injects an
`email-decode.min.js` script. The legal pages link to `privacy@oxfer.app` and
`abuse@oxfer.app`. Cloudflare documents that it skips responses with
`Cache-Control: no-transform`, which the legal pages send
([why only they do](#why-cache-control-no-transform)). Turn it off in the
zone as well:
**Security** → **Settings** → filter **Client-side abuse** → **Email Address
Obfuscation** → **Off**. If `/privacy`, `/terms` or `/abuse` still differ
from the release build, check this setting and any other feature that edits
HTML.

### Network Error Logging must be off

Cloudflare Network Error Logging (NEL) adds `nel` and `report-to` headers that
ask browsers to send reports about failed requests to `a.nel.cloudflare.com`.
Both were on every `oxfer.app` response on 30 September 2026. The privacy
notice describes no browser reporting, so `verify-deployment.mjs` fails on
`oxfer.app` and `www.oxfer.app` while a response carries `NEL`, `Report-To` or
`Reporting-Endpoints`, and with it the deploy workflow's **Verify production**
step. `*.workers.dev` hostnames are outside the `oxfer.app` zone, whose
setting cannot reach them, so there the script only prints a warning.

Turn it off with the zone setting `nel`: the zone's **Network Error Logging**
toggle in the dashboard, or with `cf` from `p2p-transfer/` (after `npm ci` and
`npx cf auth login`; the zone ID is on the zone's **Overview** page):

```sh
npx cf zones settings get nel --zone <zone-id>
npx cf zones settings edit nel --zone <zone-id> --value-enabled=false
curl -sI https://oxfer.app/ | grep -iE '^(nel|report-to):'   # expect no output
```

The edit sends `PATCH /zones/{zone_id}/settings/nel` with
`{"value":{"enabled":false}}`; add `--dry-run` to print the request without
sending it. It needs permission to edit the zone's settings, which the
Workers deploy token from the **Edit Cloudflare Workers** template does not
have. Any zone administrator can turn NEL back on, which is why every deploy
checks it. Cloudflare's documentation:
[Network Error Logging](https://developers.cloudflare.com/network-error-logging/).

## Bundle hashes

The deploy job's **Record bundle hashes** step runs after the build. It
writes the SHA-256 of every file in `dist/` except `_headers`, sorted by path,
to `p2p-transfer/oxfer-web-sha256.txt`. `_headers` is left out because
Cloudflare applies it instead of serving it. The list is published twice:

- in the run's job summary, with the commit SHA;
- as the artifact `oxfer-web-sha256-<commit>`, which **Upload bundle hashes**
  uploads.

GitHub keeps artifacts and logs for 90 days by default, and at most 90 days in
a public repository. For a permanent record, attach each production list to a
GitHub release, or copy it to private records:

```sh
gh run download <run-id> --repo radumarias/syncoxiders --name oxfer-web-sha256-<commit>
gh release create web-<commit> --repo radumarias/syncoxiders --target <commit> \
  --title "oxfer.app web bundle <commit>" --notes "SHA-256 of the deployed files" \
  oxfer-web-sha256.txt
```

Anyone can compare what `oxfer.app` serves with the list, using only `curl`
and `sha256sum`:

```sh
while read -r hash path; do
  served=$(curl -fsSL "https://oxfer.app/${path}" | sha256sum | cut -d' ' -f1)
  if [ "$served" = "$hash" ]; then echo "ok   $path"; else echo "DIFF $path"; fi
done < oxfer-web-sha256.txt
```

`index.html`, `theme.html` and the legal pages redirect to their clean paths
(`/`, `/theme`, `/privacy`), and `curl -L` follows the redirects. A match shows
that the served files are the ones CI built from that commit; review the
commit itself for what the code does. The response headers are not in the
list; `verify-deployment.mjs` checks them. The incident procedure is in
[`docs/compliance/incident-runbook.md`](compliance/incident-runbook.md#8-verifying-what-oxferapp-serves).

## Supply-chain checklist (plan B4)

- [ ] Two-factor authentication on the GitHub account and the Cloudflare
      account.
- [ ] Protect `main` with a branch protection rule or ruleset that requires the
      `ci` workflow's checks (`ci (ubuntu-latest)`, `ci (macos-latest)`,
      `ci (windows-latest)`, `p2p-web-checks`, `p2p-browser-integration`).
      Turn on "Do not allow bypassing" if the rule should bind administrators
      too.
- [ ] `CLOUDFLARE_API_TOKEN` is an Account API token made from the **Edit
      Cloudflare Workers** template for this account only. Roll it every year,
      update the secret, and re-run `oxfer-web` to confirm.
- [ ] Keep the **Verify production** step (`verify-deployment.mjs` against
      `workers.dev`, `www.oxfer.app` and `oxfer.app`) in the deploy job.
- [ ] Keep build tools pinned: Trunk by version and SHA-256 in `build-web.sh`,
      and `cf` by exact version in `package.json` and `package-lock.json`
      (installed with `npm ci`).
- [ ] Keep each production bundle-hash list beyond GitHub's 90 days
      ([bundle hashes](#bundle-hashes)).
- [ ] Optional: pin the third-party actions in `.github/workflows/oxfer-web.yml`
      by commit SHA. They are pinned by major tag today (`@v4`).

## Geoblocking lever (plan E4)

The lever is **off by default**. The owner's decision is to comply with the UK
Online Safety Act; blocking a country is a fallback, not the plan. If it is
ever needed, one WAF custom rule on the `oxfer.app` zone blocks the web app for
visitors whose IP address Cloudflare places in that country.

1. In the Cloudflare dashboard, open the `oxfer.app` zone and go to
   **Security rules**
   ([direct link](https://dash.cloudflare.com/?to=/:account/:zone/security/security-rules)).
   Older dashboard navigation: **Security** → **WAF** → **Custom rules**.
2. Select **Create rule** → **Custom rules**.
3. **Rule name**: for example `Geoblock GB (OSA fallback)`.
4. Under **When incoming requests match**, set **Field** `Country`,
   **Operator** `equals` and **Value** `United Kingdom`. In the expression
   editor this is:

   ```text
   (ip.src.country eq "GB")
   ```

   For several countries, use a set: `(ip.src.country in {"GB" "AU"})`.
5. Under **Then take action**, choose **Block**.
6. Select **Deploy**, or **Save as Draft** to prepare the rule without turning
   it on.

Plan availability: custom rules are available on every plan, and the Free plan
allows 5 of them. Block and the `eq` and `in` operators are available on Free.
A custom block response (your own HTML) needs Pro or higher, so on Free,
blocked visitors get Cloudflare's default block page (HTTP 403). The
`matches` (regex) operator needs Business or higher; the rules here do not
use it.

To keep the legal pages reachable from the blocked country, for example so
people there can still read the terms or send a report, exclude those pages
and their assets:

```text
(ip.src.country eq "GB" and not http.request.uri.path in {"/privacy" "/terms" "/abuse" "/assets/legal.css" "/assets/oxfer-favicon-light.svg" "/assets/oxfer-wordmark-light.svg" "/assets/oxfer-wordmark-dark.svg"})
```

What the rule does not cover:

- **Other hostnames.** The rule belongs to the `oxfer.app` zone, so it covers
  `oxfer.app` and `www.oxfer.app`. `oxfer.42dev.workers.dev` and the Worker's
  preview URLs are not hostnames in that zone. To close them, set
  `workersDev: false` and `previewUrls: false` in both `cloudflare.config.ts`
  and `workerConfig` in `package-cf-output.mjs`. Then remove the `workers.dev`
  origin from the workflow's **Verify production** step. `oxfer.pages.dev` is
  also outside the zone ([custom domains](#custom-domains)).
- **The relay and native apps.** `relay.oxfer.app` is DNS-only, so Cloudflare
  does not see its traffic. A native app that is already installed keeps
  working. Blocking a country at the relay would need a firewall rule on the
  VPS with a GeoIP list, which `deploy/relay/` does not include.
- **Location accuracy.** Location comes from the client's IP address, so VPN
  users are placed wherever their exit is.

Returning visitors are blocked too. `assets/sw.js` fetches navigations from
the network first and falls back to its cache only when the request fails, and
a 403 response is not a failure.

To check the rule, open **Security** → **Analytics** and select the **Events**
tab. On Free this shows sampled events from the last 24 hours. Also load
`https://oxfer.app/` from a network in the blocked country, for example a VPN
exit there. To turn the lever off, disable or delete the rule. Record when it
was turned on or off and why in the private records
([`docs/compliance/README.md`](compliance/README.md#private-records)).

## CI checks

The `ci` workflow ([`.github/workflows/ci.yml`](../../.github/workflows/ci.yml))
runs on pushes and pull requests to `main` and `release`. Two of its jobs run
the same commands as `check.sh`:

- `p2p-web-checks` needs no Rust toolchain, so it does not wait for the
  `wasm-pack` install. It runs `node --test tests/*.test.mjs`, every node
  suite (including `package-cf-output.test.mjs` and `web-pages.test.mjs`),
  and `sh deploy/relay/render.sh --check`.
- `p2p-browser-integration` installs nightly Rust and `wasm-pack` and runs
  the three Firefox tests `webrtc_wasm`, `relay_wasm` and `resume_wasm`.

The test "CI runs the node suites, the relay kit check and every browser
suite that check.sh runs" in `tests/package-cf-output.test.mjs` fails when
`check.sh` or `p2p-web-checks` lacks the `node --test` or `render.sh --check`
line, when `p2p-web-checks` uses a Rust toolchain, or when `check.sh` or
`p2p-browser-integration` lacks the `wasm-pack test` line of a
`tests/*_wasm.rs` suite. A new node suite needs no new line.

CI does not run `check.sh`'s fmt, clippy, wasm32 check, Trunk build or
`--check-dist`. The deploy job's `build-web.sh` runs the release Trunk build
and `--check-dist`, but only once the legal pages are filled: until then it
stops at the placeholder guard, before the build. Run `./check.sh` locally.

## Automatic deploys from `main`

[`.github/workflows/oxfer-web.yml`](../../.github/workflows/oxfer-web.yml) runs
on each push to `main` that touches this crate, the workspace manifest or
lockfile, or the workflow itself. Its steps:

1. `npm ci`
2. `npm run build`, with `P2P_RELAY_URL` from the repository variable; it
   fails before the Trunk build when the
   [placeholder](#legal-pages-and-the-placeholder-guard) or
   [relay go-live](#relay-go-live-guard) guard trips
3. record and upload the bundle hashes
4. `npx cf deploy --prebuilt`
5. one `verify-deployment.mjs` run that checks `oxfer.42dev.workers.dev`,
   `www.oxfer.app` and `oxfer.app` concurrently: bytes, headers, and Network
   Error Logging off on the two `oxfer.app` hostnames

Cloudflare Workers Builds cannot watch this repository yet: the account is not
connected to GitHub (`This project is disconnected from your Git account`).

One-time GitHub Actions settings, under **Settings → Secrets and variables →
Actions**:

1. Create a Cloudflare **Account API token** with the **Edit Cloudflare Workers**
   template: [Account API tokens](https://dash.cloudflare.com/profile/api-tokens).
2. **Secrets**:
   - `CLOUDFLARE_API_TOKEN`: the token value
   - `CLOUDFLARE_ACCOUNT_ID`: `3c0c25f9a4a8f887acef630c72f55e6e`
3. **Variables** (optional): `P2P_RELAY_URL`, once the self-hosted relay is
   live ([relay selection](#relay-selection-p2p_relay_url)).

After the secrets exist, **Actions → oxfer-web → Run workflow** deploys the
current `main` without waiting for another commit.

`cf` is currently open beta. Pin the version in `package.json` (and the npm
lockfile) until its Build Output format stabilizes.

Account operations use the same CLI, for example `cf workers get oxfer` or
`cf cli search "custom domains"`.

References: [cf CLI](https://blog.cloudflare.com/cloudflare-cf-cli-launch/),
[static-asset billing](https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/),
[HTML handling](https://developers.cloudflare.com/workers/static-assets/routing/advanced/html-handling/),
[WAF custom rules](https://developers.cloudflare.com/waf/custom-rules/),
[create a custom rule in the dashboard](https://developers.cloudflare.com/waf/custom-rules/create-dashboard/),
[`ip.src.country`](https://developers.cloudflare.com/ruleset-engine/rules-language/fields/reference/ip.src.country/),
[rules language operators](https://developers.cloudflare.com/ruleset-engine/rules-language/operators/),
[Security Events](https://developers.cloudflare.com/waf/analytics/security-events/),
[Email Address Obfuscation](https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/),
[GitHub artifact and log retention](https://docs.github.com/en/github/administering-a-repository/configuring-the-retention-period-for-github-actions-artifacts-and-logs-in-your-repository).
