# p2p-transfer contributor guide

## Project

`p2p-transfer` ("Oxfer") is an `eframe`/`egui` application for native and
browser file transfer. Upstream iroh 1.1 supplies authenticated endpoints, share
tickets, signalling, and the relay transport. Browser-to-browser WebRTC data
transfer is implemented as a direct `RTCDataChannel`, with offer/answer/ICE
travelling over iroh's encrypted control stream and the iroh stream retained as
fallback.

The browser build is served from `https://oxfer.app` by an assets-only
Cloudflare Worker, together with static legal pages at `/privacy`, `/terms` and
`/abuse`. The compliance plan behind the legal pages, headers and relay kit is
[`docs/compliance-plan.md`](docs/compliance-plan.md); its status and records
are in [`docs/compliance/`](docs/compliance/README.md).

The parent Cargo workspace has `p2p-transfer` as its default member.
`README.md` is inherited `eframe_template` material, not project documentation.
`docs/webrtc-transfer-plan.md` is the original design document; where it
differs from the code (for example the relay list or the STUN list), the code
is current.

## Toolchain and commands

`rust-toolchain` is in this directory and pins nightly with `rustfmt`, `clippy`,
and `wasm32-unknown-unknown`. Run commands from `p2p-transfer/`; running Cargo
from the workspace root may select the user's stable toolchain instead.

- `cargo run --release`: native desktop app.
- `trunk serve`: debug browser build at `http://127.0.0.1:8080`.
- `trunk build`: static `dist/`.
- `bash build-web.sh` (`npm run build`): release Trunk build, checks, and cf
  Build Output packaging. Before the build it rejects a `P2P_RELAY_URL` that
  the relay-list parser refuses, including any character outside printable
  ASCII. After it, it refuses to package while the legal pages contain
  placeholders, and, once they are filled, unless `P2P_RELAY_URL` lists
  `https://relay.oxfer.app` (relay go-live guard).
  `OXFER_ALLOW_PLACEHOLDERS=1 bash build-web.sh` bypasses those two page
  guards, not the relay-list check, for local test builds only and must never
  be deployed.
- `P2P_RELAY_URL=https://relay.example bash build-web.sh`: build for
  self-hosted relays instead of n0's (see Relay selection below).
- `npx cf deploy --prebuilt` (`npm run deploy`): upload the packaged Worker
  (`cf` from `package.json`).
- `node verify-deployment.mjs https://oxfer.app/`: compare a deployment with
  local `dist/` and assert its headers. Run it with the build's `P2P_RELAY_URL`.
  It fails while Cloudflare Network Error Logging (`NEL`/`Report-To`) is on
  for the `oxfer.app` zone and only warns on `*.workers.dev`
  ([turning NEL off](docs/cloudflare-workers.md#network-error-logging-must-be-off)).
- `cargo run -q -p p2p-transfer --example ticket-endpoint-id -- '<share link, fragment or ticket>'`:
  print a reported share's endpoint ID, the entry the relay's denylist takes
  (one line, 64 lowercase hex digits), offline. Keep `-q`: without it cargo
  echoes the argument, including any `cap=` capability, in its `Running`
  line. `-` instead of the argument reads the link from standard input and
  keeps it out of shell history. Where the ID goes:
  [`deploy/relay/README.md`](deploy/relay/README.md#abuse-blocking).
- Pushes to `main` that touch this crate deploy via
  `.github/workflows/oxfer-web.yml`.
- `./check.sh`: required gate. Native and wasm checks, fmt, clippy with
  `-D warnings` on both targets, tests, doctests, every `node --test` suite in
  `tests/`, real Firefox WebRTC, relay and persistence tests, then
  `trunk build` with byte comparisons of copied files and an inline-script
  guard on `dist/index.html`. It requires Node.js, Firefox, `wasm-pack`, and
  Trunk.
- `cargo test <substring>`: run a focused unit test.
- `sh deploy/relay/render.sh --check`: fails if the relay kit's generated
  `cloud-init.yaml` or `fly/config.toml` is stale; without `--check` it
  regenerates them. `cloud-init.yaml` embeds each kit file as `gz+b64`
  (plain text would exceed Hetzner's 32 KiB user-data limit); `--check`
  decodes and compares every payload.

Cloudflare: this crate has no Wrangler config. Use `cf` (`cf --help`,
`cf cli search …`). Do not fall back to Wrangler. `cf deploy` without
`--prebuilt` is for Vite/Wrangler bundling and is wrong here; Trunk output is
packaged first. Details, including headers, bundle hashes and the geoblocking
lever: [`docs/cloudflare-workers.md`](docs/cloudflare-workers.md).

There must be one workspace `../Cargo.lock`. A gitignored
`p2p-transfer/Cargo.lock` is stale local state and can make Trunk run a
wasm-bindgen CLI version different from the workspace dependency.

## Browser development

`Trunk.toml` sets `filehash = false` and `inject_scripts = false`. Trunk
therefore injects no inline loader; `index.html` loads two files it copies with
`copy-file`:

- `assets/boot.js`: a classic synchronous script. It captures the original
  URL, registers `sw.js` (from `assets/sw.js`), and handles the `#dev` reset.
- `assets/app-init.js`: a module that imports `p2p-transfer.js` and
  initialises `p2p-transfer_bg.wasm`. On failure it shows a plain-text message
  in `#loading_text`.

App-shell requests are network-first because filenames are not hashed.

- `#dev` unregisters service workers and clears caches. Use it to bypass stale
  builds, but it deliberately disables the service-worker streaming download
  sink. In a release build it is also the only way to get the `__p2p` console
  handle (see `src/webrtc.rs` below).
- Test Chromium's File System Access sink with or without `#dev`.
- Test the service-worker sink without `#dev`, after the page is controlled by
  the current worker. Use the `sink=fsa`, `sink=sw`, and `sink=mem` fragment
  flags to force a route.

The memory fallback is capped at 256 MiB. File System Access and service-worker
routes stream bounded chunks and are required for larger downloads.

`trunk serve` does not apply `assets/_headers`, so the Content-Security-Policy
is not active locally. The clean paths `/privacy`, `/terms` and `/abuse` come
from Cloudflare's HTML handling; locally, open the `.html` files.

### Browser storage

The web build stores only what the user asked for (ePrivacy art. 5(3)), and
sets no cookies. `privacy.html` and `docs/compliance/ropa.md` describe this
list; change them together with it:

- localStorage `oxfer.theme.v1` (`BROWSER_THEME_KEY` in `src/app.rs`), written
  only when the user picks a theme, or once at start-up to carry over a
  non-default theme the user picked in an older version (stored under
  eframe's `app` key). No other localStorage key is written.
- eframe's browser persistence is off: on wasm `App::save` writes nothing and
  `persist_egui_memory` is false, so eframe's `app` key and `egui_memory_ron`
  are never written. `restore_browser_theme` removes both legacy keys at
  start-up, after the migration.
- The service worker's app-shell cache (`oxfer-v3` in `assets/sw.js`,
  network-first).
- The opt-in "Keep a copy" IndexedDB database and OPFS directory, both named
  `oxfer-resume`. In `assets/resume-worker.js` only the saving path creates
  them: `prepare`, run when the receiver has ticked "Keep a copy" and saving
  starts, through `putGroup` and `rootDirectory(true)`. Listing, exporting
  and deleting never create them: `database(false)` checks
  `indexedDB.databases()` first and otherwise aborts the version-change
  transaction of a new database, and a missing directory makes deleting a
  no-op. Once created they stay, empty, after the last copy is deleted.
  Browsers that ran a build from before this change may still hold an empty
  `oxfer-resume` database made by the old start-up listing; the app does not
  delete it, because that could race another tab's save.

Native builds keep eframe's normal on-disk persistence (theme and save
directory). What users see and how to reset it:
[`docs/diagnostics.md`](docs/diagnostics.md#what-the-browser-stores).

## Architecture

- `src/app.rs`: UI state machine. It owns handles and metadata, never file
  bytes. Picking a sender file starts a bounded hashing preparation phase.
  Receiver Save creates sinks before sending `ReceiveCommand::Save`. The bottom
  bar on every screen links to Privacy, Terms, Abuse & safety and Source
  (`FooterLink`): inline at 760 px or wider, otherwise in a "Legal" menu. The
  web build opens `/privacy` and the other clean paths; native opens
  `https://oxfer.app/...` in the system browser through eframe's `links`
  feature (enabled in `Cargo.toml`). The receive screens (the file list before
  saving, the "Received" list and, on the web, "Saved files in this browser")
  show a "Report abuse" link (`show_report_abuse`) to the same Abuse & safety
  page; it carries nothing about the transfer. "Technical details" names the
  relay operator from `RelayChoice::from_env()` (`RelayOperator`). Its privacy
  boundary text comes from `RelayOperator::privacy_boundary_for(web)`: the
  browser text says Cloudflare serves the web app shell and answers STUN; the
  desktop text mentions neither (the desktop app uses neither) and says that
  its share links contain the device's IP addresses. The native send screen
  says the same. Browser storage: see [below](#browser-storage).
- `src/node.rs`: iroh `Node`, endpoint ticket links, capability authorization,
  relay selection, and the sender-side protocol handler. `RelayChoice::from_env()`
  reads the compile-time `P2P_RELAY_URL` through `RelayChoice::from_setting`.
  `relay_probe_urls` builds the Diags probe list. `share_endpoint_id` turns a
  reported link, fragment or bare ticket into the endpoint ID for the relay's
  `access.denylist`, through `Node::parse_fragment`; its errors are fixed
  strings that never repeat the input.
- `examples/ticket-endpoint-id.rs`: the operator's command-line wrapper around
  `share_endpoint_id` (see Toolchain and commands). It prints only the ID and
  never prints or logs the capability.
- `src/protocol.rs`: bounded tagged postcard frames: handshake, manifest,
  request/credit, signalling, chunks, completion, and errors. Never allocate
  before checking `MAX_FRAME`.
- `src/transfer.rs`: transport-independent sender and receiver state machines.
  The receiver grants credit only after a sink accepts bytes. Data-channel
  failure changes epoch and resumes over iroh.
- `src/file_io/mod.rs`: shared source/sink contracts, snapshots, filename
  policy, native per-session reads, transactional native staging, memory
  fallback, and test sinks.
- `src/file_io/web.rs` and `assets/download-sinks.js`: browser `File.slice`
  source, File System Access sink, service-worker streaming sink, and memory
  fallback.
- `assets/sw.js`: app cache plus capability-addressed, single-use streaming
  download responses with bounded demand/ack flow.
- `src/webrtc.rs` and `assets/webrtc-channel.js`: browser data-channel
  implementation: SDP/ICE, bounded inbound queue, `bufferedAmount`
  backpressure, path stats, close/failure signalling, and relay fallback.
  `ICE_SERVERS` is Cloudflare's STUN server only. The newest peer connection is
  published as `window.__p2p` only in debug builds, or when the page fragment
  has the `dev` flag when the peer is created. `debug_pc()` stays Rust-only.
- `src/diagnostics.rs` and `assets/diagnostics.js`: the `/diags` report and
  relay WebSocket probes ([`docs/diagnostics.md`](docs/diagnostics.md)).

Native and wasm share protocol and transfer logic but differ at file I/O and
endpoint transport boundaries. Any native filesystem or tokio-net-only code
must remain under `#[cfg(not(target_arch = "wasm32"))]`. Browser JS objects and
their futures are `!Send`; do not add a `Send` bound to `Source` or `Sink`.

### Relay selection (`P2P_RELAY_URL`)

`P2P_RELAY_URL` is a comma-separated list of relay URLs
(`https://host[:port]`, or `http://` for a test relay). Entries are trimmed,
order is kept and duplicates are removed. An unset or empty value, or one of
only ASCII whitespace, means n0's public relays (`N0WithoutTrailingDots` in
the browser, `N0` natively). CI passes an empty string while the repository
variable is unset. A valid list selects `RelayChoice::Custom(Vec<RelayUrl>)`
on iroh's `presets::Minimal`: only those relays, with no n0 relay map and no
pkarr or DNS lookup.

Two parsers read the same value. The JavaScript one gates the build, so it
must accept nothing the Rust one rejects:

- `src/node.rs`: trims with Rust's `str::trim` (Unicode whitespace, but not
  U+FEFF). An empty entry, an unparsable URL or a non-http(s) scheme rejects
  the whole value. The app then logs a warning naming only the entry's
  position, and falls back to n0.
- `package-cf-output.mjs` (`parseRelayList`, `relayConnectSources`,
  `listsRelay`): used by `build-web.sh --check-relay`, CSP rendering and
  `verify-deployment.mjs`. It is stricter and fails the build instead of
  falling back. A value of only ASCII whitespace is unset; any other
  character outside printable ASCII (0x20 to 0x7E) fails, because JS `trim`
  strips characters Rust keeps (a U+FEFF byte-order mark passed JS and made
  the wasm fall back to n0). The error names invisible characters by code
  point and never repeats a visible one. It also rejects credentials, paths,
  queries, fragments, IPv6 literals and trailing-dot hosts.

The gate covers only builds made by `build-web.sh`: a plain `trunk build`
with an invalid value still falls back to n0 with a console warning.

The workflow sets the value from the repository variable `vars.P2P_RELAY_URL`.

### Web pipeline and headers

- `assets/_headers` is copied verbatim to `dist/_headers`. Its `/*` rule
  carries `Content-Security-Policy-Report-Only` with one `{{RELAY_CONNECT_SRC}}`
  token, `Permissions-Policy`, `X-Frame-Options: SAMEORIGIN`, and
  `Cache-Control: public, max-age=0, must-revalidate` (`CACHE_CONTROL` in
  `package-cf-output.mjs`). Cloudflare comma-joins the values of every rule
  that matches a path: `/sw.js` adds `no-cache, no-store, must-revalidate`,
  and `/privacy`, `/terms` and `/abuse` (`NO_TRANSFORM_PATHS`) add
  `no-transform`. `no-transform` stops Cloudflare features such as Email
  Address Obfuscation from rewriting the legal pages' `mailto:` links, so
  their bytes match the build and the published hashes. It is set nowhere
  else because it also stops Cloudflare's Brotli/gzip compression, which the
  wasm and JavaScript keep
  ([why](docs/cloudflare-workers.md#why-cache-control-no-transform)).
- `package-cf-output.mjs` renders the token only in the packaged copy under
  `.cloudflare/output/`: `https://*.iroh.link wss://*.iroh.link` for n0, or each
  relay's `https://` and `wss://` origin. It refuses leftover tokens, files
  over Cloudflare's line or rule limits, and security headers set outside `/*`
  or set twice.
- `verify-deployment.mjs` asserts the rendered values on every checked path.
  It takes each path's expected `Cache-Control` directives from the rendered
  `_headers` (`cacheControlDirectives`), requires `no-transform` on the legal
  pages, fails if any other checked path has it, and requires `no-store` on
  `/sw.js`; `checkLocalCacheControl` checks the local file the same way. It
  also asserts the absence of `NEL`, `Report-To` and `Reporting-Endpoints` on
  hosts in the `oxfer.app` zone. Cloudflare Network Error Logging adds the
  first two; turn it off with the zone setting `nel`
  ([how](docs/cloudflare-workers.md#network-error-logging-must-be-off)).
- Both CLIs start through `isMainModule` (real paths on both sides), so they
  also run from a symlinked checkout instead of silently doing nothing.
- The deploy workflow publishes the SHA-256 of every `dist/` file except
  `_headers` in the job summary and the `oxfer-web-sha256-<commit>` artifact.
- Enforcing the CSP later means changing the header name in `assets/_headers`,
  the `CSP_REPORT_ONLY` value in `package-cf-output.mjs`, and
  `tests/package-cf-output.test.mjs` together
  ([steps](docs/cloudflare-workers.md#enforcing-the-policy)).

### Legal pages

`privacy.html`, `terms.html` and `abuse.html` (crate root) and
`assets/legal.css` are static, script-free pages copied by Trunk. They contain
the placeholders `[[OPERATOR_NAME]]`, `[[OPERATOR_ADDRESS]]`,
`[[OPERATOR_REGISTRATION]]`, `[[EFFECTIVE_DATE]]`, and in `privacy.html`
`[[RELAY_HOSTING_PROVIDER]]` and `[[RELAY_LOCATION]]`. The owner fills them.
`build-web.sh` refuses to package while any `[[UPPER_CASE]]` token remains, so
the deploy workflow fails until then. The pages describe the self-hosted relay
at `relay.oxfer.app`, run from the VPS kit. Once they are filled,
`build-web.sh` also refuses to package them unless `P2P_RELAY_URL` lists
`https://relay.oxfer.app` (the relay go-live guard: `operator_relay` in
`build-web.sh`, `OPERATOR_RELAY` and `--check-relay --require-relay=ORIGIN`
in `package-cf-output.mjs`). It does not read the page text; instead
`tests/web-pages.test.mjs` fails if `privacy.html` stops naming
`relay.oxfer.app`, so the guard is changed with the pages
([details](docs/cloudflare-workers.md#relay-go-live-guard)). The guards sit
between `# BEGIN` and `# END` markers in `build-web.sh`, which that test runs
on their own.

The pages and records are true for the VPS relay only. The kit's Fly.io
variant would falsify statements on IP blocking, log retention, the
processor and international transfers;
[`deploy/relay/README.md`](deploy/relay/README.md#flyio-variant) lists what
must change first.

### Relay kit and compliance records

- `deploy/relay/`: kit and runbook for a self-hosted `iroh-relay` 1.1.0 at
  `relay.oxfer.app` on an EU VPS. It contains `config.toml` (key cache off),
  a hardened systemd unit, nftables, journald, logrotate, tmpfiles, sshd and
  unattended-upgrades drop-ins, the `oxfer-relay-ban` tool with its hourly
  prune timer, and `setup.sh`, which installs the pinned release binary by
  SHA-256. `setup.sh` merges the server's `/etc/oxfer-relay/denylist.txt`
  into the relay config (`setup.sh --denylist` does only that), reloads
  nftables only when its config changed, and starts a relay that has no
  certificate yet only once public DNS points at the host. It also contains
  `render.sh`, which generates `cloud-init.yaml` and `fly/config.toml`, and a
  Fly.io variant in `fly/` (endpoint-ID blocks from the Fly secret
  `OXFER_RELAY_DENYLIST`). Start with
  [`deploy/relay/README.md`](deploy/relay/README.md). Keep the relay version in
  step with the workspace's iroh version.
- `docs/compliance/`: UK Online Safety Act assessments, the eSafety
  classification, the GDPR record of processing, the transparency statement,
  the incident runbook, and a status table for every plan item.

This repository is public. The compliance drafts and relay kit stay factual and
use the placeholders above. Operator addresses, SSH source addresses, banned
IPs, blocked endpoint IDs, abuse-log entries and signed records go in private
records or on the server (`/etc/oxfer-relay/denylist.txt`,
`/etc/nftables.d/bans.nft`), never here.

## Invariants

- Share links contain a bearer capability in the fragment. Never log or put it
  in a query string. Tools that read a link, such as `share_endpoint_id` and
  the `ticket-endpoint-id` example, output only what they extract and use
  fixed error messages.
- Peer frame lengths and manifest counts are bounded before allocation.
- Peer filenames go through the shared `sanitize_name` policy and are never
  joined as paths directly.
- Native output is staged in the destination directory and published without
  overwriting an existing file only after size and BLAKE3 verification.
- A sink write is sequential. Browser service-worker progress counts only
  acknowledged chunks; never grant credit for merely queued messages.
- Dropped/cancelled sink operations are not replayed because their commit state
  is uncertain.
- Fix warnings instead of suppressing them; the gate treats warnings as errors.
- Served HTML (`index.html`, `theme.html`, the legal pages) has no inline
  `<script>`, no `on*` event-handler attributes, no `javascript:` URLs and no
  cross-origin subresources. The CSP is `script-src 'self' 'wasm-unsafe-eval'`.
  New page script goes in an `assets/*.js` file with a Trunk `copy-file` entry
  and a `cmp` line in `check.sh` and `build-web.sh`.
  `tests/web-pages.test.mjs` enforces this.
- `P2P_RELAY_URL` is public: it is compiled into the WASM and rendered into a
  response header. Never put credentials in it. Change `src/node.rs` and
  `package-cf-output.mjs` together when the accepted format changes, and keep
  the JavaScript parser at least as strict as the Rust one.
- `ICE_SERVERS` stays one STUN server run by Cloudflare. Every STUN or TURN
  server added receives users' IP addresses, which would require updating
  `privacy.html` and the compliance records.
- Relay wording in the UI and diagnostics derives from `RelayChoice`; do not
  hard-code a relay operator.
- Keep the `__p2p` global gated; never expose it unconditionally in release.
- Keep the legal-page placeholder tokens exactly as written; the deploy guard
  relies on the `[[UPPER_CASE]]` form.
- The web build's only localStorage write is `oxfer.theme.v1`, on an explicit
  theme pick or the one-time migration ([browser storage](#browser-storage)).
  Do not re-enable eframe persistence on wasm (`App::save`,
  `persist_egui_memory`) or add a key, cookie or database without updating
  `privacy.html` and `docs/compliance/ropa.md`.
- Only saving with "Keep a copy" may create the `oxfer-resume` database and
  directory. Any new read, list, export or delete path in
  `assets/resume-worker.js` opens them with `create` false and treats a
  missing one as empty.
- Send `Cache-Control: no-transform` only for `/privacy`, `/terms` and
  `/abuse`: elsewhere it would stop Cloudflare compressing the wasm and
  JavaScript. Add no browser reporting (`report-uri`, `report-to`, NEL): the
  privacy notice describes none, and `verify-deployment.mjs` fails on the
  reporting headers.
- The relay host keeps what "What is logged" in `deploy/relay/README.md`
  lists, and the privacy notice and `docs/compliance/ropa.md` rely on it: the
  `RUST_LOG` filter in `deploy/relay/iroh-relay.service` keeps client IP
  addresses, endpoint IDs and connection events out of the relay's logs
  (`iroh_relay::server::http_server` must stay `off`: it would log the
  address and port of every refused reconnection); `key_cache_capacity = 0`
  keeps endpoint IDs in memory only while connected; the journal keeps at
  most three days; rsyslog is removed and its old files deleted; `wtmp` and
  `btmp` are rotated daily and keep at most three days (the kit's logrotate
  entry empties them at each rotation); `lastlog` (and `lastlog2` where
  present) keeps no persistent record; the firewall's rate-limit sets hold a
  source address for at most 60 seconds; blocks persist across reboots,
  reloads, upgrades and `setup.sh` reruns in `/etc/oxfer-relay/denylist.txt`
  (endpoint IDs) and `/etc/nftables.d/bans.nft` (addresses, each with an
  expiry, 30 days by default). Read that section before changing any of
  these.

## Tests

`local_test_*` must be deterministic and offline. `online_test_*` may require
the public relay and must never convert a timeout into a silent pass. Native
loopback endpoint tests use `RelayChoice::None` and should remain part of the
normal suite. Relay-list parsing, probe URLs, footer links, the Report abuse
links, relay wording (browser and desktop privacy boundary text, checked for
both targets), the native IP note, browser theme storage (written only for a
picked theme, legacy migration, native persistence kept) and
`share_endpoint_id` (every link form, round trip through iroh-relay's
`access.denylist` parse, no capability in any output, a real offline node's
link) have offline `local_test_*` coverage in `src/tests.rs` and
`src/app.rs`. The real browser data-channel integration test runs with:

```sh
wasm-pack test --headless --firefox -- --test webrtc_wasm
```

Durable OPFS checkpoint/reopen behavior runs in a real browser worker with:

```sh
wasm-pack test --headless --firefox -- --test resume_wasm
```

`./check.sh` and the `p2p-browser-integration` job in
`../.github/workflows/ci.yml` both run these, plus:

- `wasm-pack test --headless --firefox -- --test relay_wasm`: the n0 default
  relay map in the browser, including an empty `P2P_RELAY_URL`.
- Every `node --test` suite in `tests/`. `resume_worker.test.mjs` injects
  deterministic cross-worker races and failures, and its IndexedDB and OPFS
  fakes create storage only when asked, so it checks that listing,
  exporting and deleting on a fresh profile create nothing (with and without
  `indexedDB.databases()`); `package-cf-output.test.mjs` covers relay-list
  parsing (including the printable-ASCII rule and `--require-relay`), header
  rendering, packaging, the `verify-deployment.mjs` header checks
  (`no-transform` on the legal pages only, NEL) and both CLIs run through a
  symlink; `web-pages.test.mjs` covers the CSP rules for served HTML, the
  loader scripts, the legal-page copies, and runs `build-web.sh`'s relay-list
  check and legal-page guard blocks against scratch `dist/` copies.

A test in `package-cf-output.test.mjs` fails when `check.sh` misses a suite in
`tests/` or the CI job misses a `node --test` or `wasm-pack test` line of
`check.sh`, so a new suite goes into both files. CI does not run `check.sh`'s
fmt, clippy, wasm32 check or Trunk build; run `./check.sh` locally.
