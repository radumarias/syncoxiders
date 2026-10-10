# Threat model

Scope: the `p2p-transfer` crate only (the Oxfer app, deployed at https://oxfer.app). The rest of this repository
is out of scope; see the last section.

## What this project does and where untrusted input enters
`p2p-transfer` is an `eframe`/`egui` file-transfer app that runs natively and in the browser (wasm32). A sender
shares files through a link of the form `https://oxfer.app/#<iroh EndpointTicket>&cap=<32 hex>`; the receiver
dials the sender's iroh endpoint, proves the capability, receives a manifest, grants credit and receives chunks.
Browser peers try a direct WebRTC `RTCDataChannel`, signalled over the encrypted iroh control stream, and fall
back to the iroh stream. `p2p-transfer/CLAUDE.md` describes the architecture and the invariants below.

Untrusted:
- Everything a peer sends: the handshake and capability (`src/node.rs`, `src/transfer.rs`), every postcard frame
  (`src/protocol.rs`): manifest, names, sizes, hashes, credit, chunks, offsets, errors, completion and the
  `Verified` receipt, and the WebRTC signalling (SDP, ICE) carried in those frames (`src/webrtc.rs`,
  `assets/webrtc-channel.js`). Either side may be the attacker: a malicious sender against a receiver, and a
  receiver with or without the capability against a sender (anyone who learns a sender's endpoint ID, or reaches
  it through a relay, can connect without one).
- iroh relays (the public n0 relays or a custom `P2P_RELAY_URL`) and the network path: they must not learn file
  contents or capabilities, or alter data undetected.
- Share links opened by the receiver: a crafted fragment (`src/node.rs` link parsing, `cap_from_hex` in
  `src/protocol.rs` and the `hex` codec in `src/blob_store.rs`) is attacker controlled.
- Diagnostics peer-test links (`/diags` handling in `src/app.rs`) and the diagnostics endpoint they name
  (`DiagnosticNode` in `src/node.rs`), which anyone holding such a link can dial.
- Theme files imported into the theme lab (`theme.html`, deployed on the app's origin): they are meant to be
  passed between people.
- Other origins and pages in the browser: requests to the service worker's `download/` routes (`assets/sw.js`),
  `postMessage` traffic between the page, the service worker and the OPFS resume worker
  (`assets/download-sinks.js`, `assets/resume-worker.js`, `assets/resume-store.js`).
- Resume state in OPFS and IndexedDB, and native staging files left after a crash: treat them as possibly
  corrupt or stale. They must fail safely, never publish unverified bytes.

Trusted: the local user operating the app, the files they choose to share, the destination they pick, the
browser and OS, and the deployed static files themselves.

## Security invariants
- The capability is a bearer secret. It must never be logged, put in a query string or `Referer`, shown in an
  error, included in diagnostics reports, or sent anywhere except to the sender it was derived for. It is
  compared in constant time.
- No manifest, file name or byte is sent before the capability is verified, and only the files in that share
  are ever readable through it.
- Peer frame lengths and manifest counts are bounded (`MAX_FRAME`, `MAX_MANIFEST_FILES`) before any
  allocation. Credit bounds how much a peer may send, and is granted only after a sink accepted bytes.
- Peer file names go through `sanitize_name` and are never joined as paths. Native output is staged in the
  chosen directory and published, never overwriting an existing file, only after its size and BLAKE3 hash
  match the authenticated manifest.
- Service-worker download URLs are unguessable, scope-relative and single-use.
- A dropped or cancelled sink write is never replayed.

## Components that matter most / least
Most: `src/protocol.rs`, `src/transfer.rs`, `src/node.rs` (capability, links, authorization, relays),
`src/file_io/` (sinks, staging, names, resume), `src/webrtc.rs`, `assets/sw.js`, `assets/download-sinks.js`,
`assets/webrtc-channel.js`, `assets/resume-worker.js`, `assets/resume-store.js`, `index.html` and
`assets/_headers` (security headers), and in `src/blob_store.rs` the `hex` codec (it decodes link capabilities)
and `BlobHash` (the manifest hash that verification compares).

Less: `src/app.rs` (UI state; still in scope where it handles links, capabilities or peer-supplied text),
`src/diagnostics.rs` and `assets/diagnostics.js` (must not leak capabilities or tickets), `src/logging.rs`
(the in-app terminal buffer users copy into reports: no capability may reach it), `theme.html`,
`assets/theme.js`, `assets/favicon.js`, `assets/wake-lock.js`. `EchoNode`/`Echo` in `src/node.rs` and the
blob and Bao stores in `src/blob_store.rs` are older code that the transfer path does not use.

## How to exercise it
The image is built in `/src`; run everything from `/src/p2p-transfer`, which pins the nightly toolchain and,
in `.cargo/config.toml`, the wasm32 rustflags.
Every dependency is already in the cargo cache, and `CARGO_NET_OFFLINE=true` and `TRUNK_OFFLINE=true` are set.
- `cargo test`: the native unit, loopback and doc tests. Two in-process iroh endpoints with `RelayChoice::None`
  exercise the real handshake, manifest, credit, resume and staging paths. Harnesses in `src/tests.rs` such as
  `run_gate` script one side of a session frame by frame: the easiest way to play a malicious peer.
  `online_test_*` are `#[ignore]`d: they need the public relay.
- `cargo check --target wasm32-unknown-unknown --lib` for the browser build.
- `node --test tests/*.test.mjs`: deterministic tests of the resume worker, WebRTC stats, diagnostics, wake
  lock, theme, favicon and Cloudflare packaging scripts, mostly run in `node:vm` with fakes. `assets/sw.js` and
  `assets/download-sinks.js` have no Node harness yet.
- `wasm-pack test --headless --firefox -- --test webrtc_wasm` (also `resume_wasm`, `relay_wasm`): real
  Firefox runs of the data channel and the OPFS resume store. Without camera or microphone permission, Firefox
  gathers ICE candidates only on the default-route interface, so `webrtc_wasm` needs a default route (no Internet
  is needed). Without one, as under `docker run --network none`, it times out with "Couldn't gather ICE
  candidates": an environment limit, not a finding.
- `trunk build` writes the browser app to `dist/` (debug; `--release` needs a wasm-opt download).
- The native GUI needs a display and will not start in this image; drive the library instead.

## How you rate severity
- Critical: reading files that were not shared, or any shared file without the capability; a peer achieving
  code execution; a malicious sender writing outside the chosen destination or overwriting an existing file;
  the capability leaking to a third party (logs, relays, `Referer`, another origin, diagnostics).
- High: a receiver publishing or reporting as verified a file whose bytes do not match the authenticated
  manifest; another origin or page reading from or reusing a service-worker download route; signalling
  injection that redirects the data channel to a third party; persistent client-side script injection.
- Medium: remote crash, panic, or unbounded memory or disk use triggered by peer input (High if a peer
  without the capability can trigger it); a credit-window bypass; resume state that loses or duplicates
  data without being detected.
- Low: hangs or stalls a peer can cause that the documented timeouts eventually clear, misleading progress or
  status text, information exposure without a security impact.

## Anything to leave alone
- Other workspace members (`file-change-consumer`, `file-change-router`, `file-tree-merge`, `file-watcher`),
  `website/` and the Jekyll site at the repository root.
- Denial of service against the public iroh relays, and failures that need the network the scan does not have.
- Cloudflare deployment tooling (`build-web.sh`, `package-cf-output.mjs`, `cloudflare.config.ts`,
  `verify-deployment.mjs`, `.github/workflows/`), `p2p-transfer/README.md` (eframe template text), and bugs in
  iroh, egui/eframe or other dependencies unless p2p-transfer uses them unsafely.
- The memory download route's 256 MiB cap (`p2p-transfer/CLAUDE.md`), and the documented limits of browser wake
  locks and background tabs (`docs/resumable-transfers.md`).
- Attacks that need a compromised OS, browser, browser extension or the local user's cooperation.
