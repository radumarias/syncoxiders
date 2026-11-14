# Oxfer (`p2p-transfer`)

Oxfer sends files directly between two devices, encrypted in transit, with no
copy on any server. It is one Rust codebase built with
[`eframe`](https://github.com/emilk/egui/tree/master/crates/eframe)/[`egui`](https://github.com/emilk/egui):
a native desktop app and a WebAssembly browser app, live at
<https://oxfer.app>.

The sender picks or drops files and gets a share link. The receiver opens the
link and saves the files straight from the sender's device. The sender's app or
tab must stay open until every receiver has finished.

## Features

- **Peer-to-peer transfer.** Peers find each other through
  [iroh](https://github.com/n0-computer/iroh) 1.1: the link carries an
  `EndpointTicket`, and the receiver dials it over iroh's authenticated,
  TLS 1.3 encrypted QUIC transport.
- **Direct browser-to-browser path.** Between two browsers, the WebRTC offer,
  answer and ICE candidates travel over the iroh control stream and the bytes
  flow over a direct `RTCDataChannel` (DTLS). If ICE fails, or the data channel
  breaks mid-transfer, the same protocol resumes over the iroh relay, so no
  TURN server is needed.
- **Capability links.** The share link holds a bearer capability in the URL
  fragment, which the browser never sends to a server.
- **Verified files.** The sender hashes every file with BLAKE3 before sharing;
  the receiver checks each file against the authenticated manifest and only
  then sends a `Verified` receipt, which is what marks the transfer complete.
  Multiple files per share (up to 2,000 in one manifest).
- **Bounded memory for large files.** The sender reads in slices, and the
  receiver grants flow-control credit only after its sink has accepted bytes.
  Browser downloads stream to disk through the File System Access API
  (Chromium) or a service-worker streaming download; an in-memory fallback is
  capped at 256 MiB. Native downloads are staged in the destination folder and
  published only after size and hash checks, never overwriting an existing file.
- **Reconnect and resume.** The receiver retries transient failures with
  backoff and keeps acknowledged offsets, hash state and finished files across
  reconnects. An opt-in resumable browser copy (OPFS) survives a closed tab.
  See [`docs/resumable-transfers.md`](docs/resumable-transfers.md).
- **Screen wake lock** while sharing or receiving in the browser.
- **Drag and drop** of files onto the window, natively and in the browser.
- **Chat rooms.** `https://oxfer.app/#chat` opens a text room hosted by the
  open page, on its own ALPN (`oxfer/chat/1`). People and agents can talk and
  swap share links there; "Copy for agent" copies a link with a short brief.
- **Scripted sender and receiver** for automation and AI agents:
  `agent/oxfer-agent.mjs` drives the real app in a Playwright browser, and the
  site serves it at `/oxfer-agent.mjs` with a summary at `/llms.txt`. See
  [`docs/agent-transfer.md`](docs/agent-transfer.md).
- **Diagnostics.** `/diags` (or **Diags** in the bottom bar) runs local
  checks and a paired peer test and produces a shareable report without
  secrets. See [`docs/diagnostics.md`](docs/diagnostics.md).
- **Themes.** Clean (default), Rusty and Phosphor, each with light and dark
  variants, plus a theme lab at `/theme.html` for editing and exporting
  palettes.

## Layout

| Path | What it is |
| --- | --- |
| `src/app.rs` | UI state machine (home, send, receive, chat, diagnostics). Holds handles and metadata, never file bytes. |
| `src/node.rs` | iroh node, share-link tickets, capability checks, relay choice, sender protocol handler. |
| `src/protocol.rs` | Bounded, tagged postcard frames (1 MiB max): handshake, manifest, credit, signalling, chunks, completion. |
| `src/transfer.rs` | Transport-independent sender and receiver state machines. |
| `src/webrtc.rs`, `assets/webrtc-channel.js` | Browser data channel: SDP/ICE, backpressure, path stats, fallback. |
| `src/file_io/` | Source and sink contracts, native staging, browser sources and sinks, OPFS resume. |
| `src/chat.rs`, `assets/chat-bridge.js` | Chat rooms and the page-script bridge the agent script uses. |
| `src/diagnostics.rs`, `assets/diagnostics.js` | The diagnostics page. |
| `assets/sw.js`, `assets/download-sinks.js` | App cache and streaming download responses. |
| `agent/oxfer-agent.mjs` | Playwright sender, receiver and chat client. |
| `tests/` | wasm-bindgen browser tests (`webrtc_wasm`, `relay_wasm`, `resume_wasm`) and Node tests for the JS assets and tooling. |
| `docs/` | Design notes and operating guides. |

Contributor notes, invariants and architecture details are in
[`CLAUDE.md`](CLAUDE.md).

## Building and running

Run everything from this directory. `rust-toolchain` pins nightly with
`rustfmt`, `clippy` and the `wasm32-unknown-unknown` target; running Cargo from
the workspace root may pick a different toolchain. The workspace keeps a single
`../Cargo.lock`.

### Native

```sh
cargo run --release
```

On Debian or Ubuntu, first install:

```sh
sudo apt-get install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

### Browser

You need [Trunk](https://trunkrs.dev/) and clang with `llvm-ar`, because every
wasm32 build compiles ring's C code, which gcc cannot target.

```sh
cargo install --locked trunk
trunk serve          # http://127.0.0.1:8080, rebuilds on change
trunk build          # static files in dist/
```

`assets/sw.js` caches the app shell. Open `http://127.0.0.1:8080/#dev` to
unregister service workers and clear caches; this also disables the
service-worker download sink, so test that route without `#dev`. The fragment
flags `sink=fsa`, `sink=sw` and `sink=mem` force a download route.

## Testing

`./check.sh` is the required gate (CI runs the Rust build plus the resume
worker and Firefox WebRTC and resume tests):

- `cargo check`, `cargo fmt --check` and `cargo clippy -D warnings` on native
  and `wasm32-unknown-unknown`;
- `cargo test` (unit, loopback endpoint and doc tests);
- Node tests for the resume worker, diagnostics, WebRTC stats, wake lock,
  themes, theme lab, favicon, Cloudflare packaging and the agent CLI;
- real Firefox tests through `wasm-pack test --headless --firefox` for WebRTC,
  the relay and OPFS resume;
- `trunk build`, then checks that the built assets match the sources.

It needs Node.js 22+, Firefox, `wasm-pack`, Trunk and clang. For a single test,
use `cargo test <substring>`. `local_test_*` tests are offline and
deterministic; `online_test_*` tests may use the public relay. The WebRTC
browser test needs a default network route (not Internet access).

## Deploying

The browser app is an assets-only Cloudflare Worker, deployed with the `cf`
CLI (there is no Wrangler config):

```sh
npm ci
npm run build        # build-web.sh: release Trunk build, then package for cf
npm run deploy       # cf deploy --prebuilt
npm run verify -- https://oxfer.app/
```

Pushes to `main` that touch this crate deploy automatically through
`.github/workflows/oxfer-web.yml`, which then verifies the workers.dev host,
`www.oxfer.app` and `oxfer.app`. Setup and secrets are in
[`docs/cloudflare-workers.md`](docs/cloudflare-workers.md). For another static
host, upload the `dist/` from `trunk build --release --locked`.

## In progress

A Dioxus-based web UI and shared sessions are being built on the
`feature/mobile-web` branch and are not yet on `main`.

## License

MIT OR Apache-2.0, see [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE).
