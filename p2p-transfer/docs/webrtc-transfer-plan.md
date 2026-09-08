# Plan: browser-to-browser transfer over WebRTC with iroh signaling

Branch: `webrtc-transfer` (based on `improve-ui-38` @ e9cc2e9).
Status: approved direction; amended 2026-09-08 after an adversarial review (see §9). Nothing implemented yet.

## 1. Summary

Turn `p2p-transfer` into a browser app where the sender picks a file, gets a
link, and the receiver opens the link and downloads the file directly from the
sender's browser.

- **Bulk transfer:** the browser's own WebRTC `RTCDataChannel`, driven from
  Rust through `web-sys`. STUN for hole punching, so the path is direct
  whenever the network allows.
- **Signaling and fallback:** upstream **iroh 1.1**. The link carries an
  `EndpointTicket`. The receiver dials it over iroh's relay, and the two sides
  exchange the WebRTC offer, answer and ICE candidates over one QUIC stream.
  If ICE fails, the same file protocol runs over that iroh stream instead, so
  the relay is the fallback and no TURN server is needed.
- **Encryption:** the iroh stream is TLS 1.3 authenticated to the endpoint id
  in the link, so signaling cannot be read or tampered with by the relay. The
  data channel is DTLS between the two browsers. Files are BLAKE3-verified.
- **Large files:** sender reads the `File` in slices, receiver streams to disk,
  send loop is paced by `bufferedAmount`. Memory stays bounded at a few MiB
  regardless of file size.
- **Dependency migration:** drop the `anchalshivank/iroh` fork and its three
  `[patch]` blocks, move to iroh 1.1, egui/eframe 0.36, and current versions of
  every other dependency.

Native builds keep working: native-to-native goes over iroh's own direct QUIC
path, native-to-browser goes over the iroh relay path. Native WebRTC is out of
scope for this plan.

## 2. Analysis findings

Findings from reading `src/`, `index.html`, `assets/sw.js`, both `Cargo.toml`
files and the iroh fork.

### Transport layer (`src/node.rs`, iroh fork)

- The fork is iroh 0.91.2, last commit 2025-09-16. It tunnels QUIC through a
  **reliable, ordered** data channel (`WebRtcDeliveryMode::Reliable` in
  `magicsock/transports/webrtc.rs`), so every packet pays QUIC congestion
  control plus SCTP's, TLS plus DTLS, and head-of-line blocking. It configures
  only Google STUN, no TURN, so a failed hole punch on the WebRTC leg has no
  relay fallback.
- Upstream iroh 1.1 is relay-only in browsers by design and has no WebRTC.
  rust-libp2p has no browser-to-browser WebRTC either (issue #4389).
  `matchbox_socket` 0.14 has no `bufferedAmount` backpressure, so it cannot
  drive a multi-GB send. Driving `RTCPeerConnection` directly is the only path
  to fast browser-to-browser transfer in Rust today.
- `EchoNode` has four near-identical constructors (`spawn`, `spawn_with_files`,
  `spawn_with_bao`, `spawn_with_blob_store`) that each build an endpoint and a
  router.
- `subscribe_accept_events` creates a channel, drops the only sender, and
  returns a receiver that can never yield. Dead code.
- The sender clones the entire shared file list (`files.clone()`) per incoming
  connection, and the receiver accumulates the whole file in `all_data` before
  hashing. Neither side can handle files larger than available memory.
- The "request" written by the receiver is a dummy payload (`b"SEND_FILE"`),
  yet the protocol still frames it as a file upload.
- Per-chunk `info!` logging on both sides.

### UI layer (`src/app.rs`, 2701 lines)

- WASM `pick_file` reads the whole file through `FileReader` into
  `picked_file_data: Vec<u8>`. `add_file_to_share` copies it again into
  `shared_files_data`, and `EchoNode` holds a third copy. Large files are
  impossible in the browser build today.
- `download_file_wasm` builds a `Blob` from a full in-memory buffer and clicks
  an anchor. Same memory ceiling on the receiving side.
- `start_receiving` (363 lines) and `reconnect_for_files` (413 lines) are
  near-duplicates of the same event loop.
- `TorrentInfo`, `magnet_uri`, and related fields are leftovers with no
  transport behind them.
- `generate_shareable_url` already puts the node id in the URL fragment, which
  is the right place. `parse_node_id_from_url` reads it back but never scrubs
  it from history.
- The `eframe::App` impl uses `update()`, which eframe 0.34+ replaced with
  `logic()` and `ui()`.

### Web shell (`index.html`, `assets/sw.js`)

- The service-worker registration code sits **inside** a `<script src=".../webtorrent.min.js">`
  element. A script element with `src` ignores its inline body, so the
  service worker is never registered, the `#dev` bypass documented in
  `CLAUDE.md` has never done anything, and WebTorrent is downloaded from a CDN
  for no reason.
- `sw.js` is cache-first for every request and has no streaming download
  support.

### Build and dependencies

- Three `[patch]` blocks in `p2p-transfer/Cargo.toml` and the workspace root
  pin forks of `n0-watcher`, `netwatch`, `portmapper`, and iroh itself.
- `web-sys` and `wasm-bindgen-futures` are declared twice (general deps and
  the wasm target section) with different versions (0.3.77 vs 0.3.70).
- `web-sys` declares no features. The DOM and File APIs used in `app.rs`
  compile only because eframe happens to enable them transitively.
- `getrandom` 0.3 needs the `--cfg getrandom_backend="wasm_js"` rustflag in
  `.cargo/config.toml`; 0.4 replaced that with a plain cargo feature.
- `gloo` and `async-channel` are declared but unused outside `node.rs`
  (`async-channel`) or entirely (`gloo`).
- `tests.rs` asserts the 256 KiB chunk constant and calls `endpoint().node_id()`,
  which is `id()` in iroh 1.x.

## 3. Proposed changes

### 3.1 Dependency migration

| Crate | Now | Target | Notes |
|---|---|---|---|
| `iroh` | fork 0.91.2 | 1.1 | Native: default features. WASM target section: `default-features = false, features = ["tls-ring"]` (without a TLS provider feature `presets::N0` does not exist and `bind` fails with `InvalidCryptoProvider`). Remove all `[patch]` blocks in both manifests. |
| `iroh-tickets` | – | 1.0 | `EndpointTicket` is the link payload. |
| `egui`, `eframe` | 0.31 | 0.36.1 | `App::update` → `logic` + `ui`. Keep the `glow` feature explicit so the wgpu default does not pull WebGPU into the bundle. Edition stays 2021; MSRV 1.88 is fine on nightly. |
| `rfd` | 0.15.3 | 0.17.2 | |
| `web-sys`, `js-sys` | 0.3.77 / 0.3.70 | 0.3.104 | Single declaration, explicit feature list (see 3.3). |
| `wasm-bindgen` | 0.2.100 | 0.2.127 | |
| `wasm-bindgen-futures` | 0.4.50 (×2) | 0.4.77 | Single declaration. |
| `tokio` | 1.45 | 1.53 | Keep the reduced wasm feature set. |
| `blake3` | 1.5 | 1.8.7 | |
| `bao-tree` | 0.15 | 0.16.1 | Still native-only for now. |
| `getrandom` | 0.3 + rustflag | 0.4.3 | `features = ["wasm_js"]`, drop the cfg **only after** `cargo tree --target wasm32-unknown-unknown` shows no 0.3 left. |
| `n0-future` | 0.1.3 | 0.3.2 | Match iroh 1.1. |
| `async-channel` | 2.3.1 | remove | Replace with `n0-future`/`tokio::sync` channels. |
| `gloo` | 0.11 | remove | Unused. |
| `serde`, `log`, `env_logger`, `anyhow` | 1.0.219 / 0.4.27 / 0.11.8 / 1.0.80 | 1.0.229 / 0.4.34 / 0.11.11 / 1.0.104 | `anyhow` lives in the workspace manifest. |
| `postcard` | – | 1.1.3 | Control-stream and chunk-header encoding. |

`.cargo/config.toml` for wasm32 becomes:
`rustflags = ["-C", "target-feature=+simd128", "--cfg=web_sys_unstable_apis"]`
(BLAKE3 SIMD, which every 2026 browser supports, and the unstable-API cfg that
`Window::show_save_file_picker` needs in web-sys 0.3.104). The `getrandom_backend`
cfg goes away in step 1; only `getrandom 0.2` via `ring` remains in the wasm tree,
which does not need it.

### 3.2 Module layout

```
src/
  lib.rs          exports, unchanged shape
  main.rs         native + wasm bootstrap, unchanged
  app.rs          egui UI only; owns a TransferHandle, no bytes
  blob_store.rs   BlobHash / BlobStore / BlobCollection, unchanged
  node.rs         rewritten: iroh 1.1 Endpoint + Router, tickets, control stream
  protocol.rs     NEW: control messages + chunk framing (postcard)
  transfer.rs     NEW: transport-agnostic sender/receiver engine
  file_io.rs      NEW: FileSource / FileSink per target
  webrtc.rs       NEW, wasm32 only: RtcPeerConnection + RtcDataChannel glue
  tests.rs        updated
assets/sw.js      cache list + streaming download route
index.html        fixed script tags, no WebTorrent
```

### 3.3 `Cargo.toml`

- Native section: `iroh = "1.1"`, `tokio` full, `rfd`, `env_logger`, `bao-tree`.
- WASM section: `iroh = { version = "1.1", default-features = false, features = ["tls-ring"] }`,
  reduced `tokio`, `web-sys` with features:
  `Window Document Element HtmlInputElement HtmlAnchorElement Event
  MessageEvent File FileList Blob BlobPropertyBag Url Location History
  RtcPeerConnection RtcConfiguration RtcIceServer RtcDataChannel
  RtcDataChannelInit RtcDataChannelType RtcDataChannelEvent
  RtcSessionDescription RtcSessionDescriptionInit RtcSdpType RtcIceCandidate
  RtcIceCandidateInit RtcPeerConnectionIceEvent RtcPeerConnectionState
  RtcIceConnectionState RtcStatsReport
  FileSystemFileHandle FileSystemWritableFileStream WritableStream
  ServiceWorkerContainer ServiceWorker MessageChannel MessagePort
  HtmlIFrameElement console`.
  Trim after the code is written; every name here has a use in this plan.
  `RtcSctpTransport` is not a web-sys feature: read `pc.sctp.maxMessageSize`
  through `js_sys::Reflect` instead.
- Remove `gloo`, `async-channel`, the duplicate declarations, and all
  `[patch]` sections (also in `../Cargo.toml`).

### 3.4 `node.rs` (rewrite)

- `pub struct Node { endpoint: Endpoint, router: Router, files: SharedFiles }`.
- One constructor: `Node::bind(files, relay: RelayChoice) -> Result<Node>`,
  where `RelayChoice` is `N0Preset` for development or `Custom(RelayUrl)` for
  the self-hosted relay. It always generates a **fresh `SecretKey`** so links
  from the same person are not linkable by endpoint id.
- `Node::ticket(&self) -> EndpointTicket` after `endpoint.online().await`,
  built from `endpoint.addr()`.
- `Node::link(&self, base_url) -> String` = `{base}#{ticket}&cap={secret}`.
  Ticket strings start with `endpoint`, so `#dev` stays unambiguous. The
  `cap` value is a fresh 128-bit random capability generated with the key.
  The ticket only carries public addressing, and the relay sees endpoint ids
  in its routing protocol, so a relay operator could dial the sender without
  ever seeing the link. The sender therefore serves nothing, not even the
  manifest, until the receiver's `Hello` carries the right `cap`, compared in
  constant time. TLS authenticates the peer; `cap` authorizes it.
- ALPN becomes `b"syncoxiders/p2p-transfer/1"`.
- The `ProtocolHandler` impl is the **sender side**: accept a bi stream, run
  `transfer::serve(stream, files)`. No `BoxFuture`, no
  `#[allow(refining_impl_trait)]`; iroh 1.1 accepts `impl Future`.
- `Node::connect(ticket) -> Result<(SendStream, RecvStream)>` for the receiver.
- Drop `subscribe_accept_events`, the bao helpers move to `file_io.rs` behind
  the existing native cfg, and the four constructors collapse into one.

### 3.5 `protocol.rs` (new)

All frames are `u32 LE length + postcard bytes`, on both transports.

```rust
enum Control {
    Hello { version: u16, cap: [u8; 16], webrtc: bool }, // cap from the link fragment; wrong cap => Error + close.
                                                   // webrtc: this peer can run a data channel. Native peers say false,
                                                   // and the pair uses WebRTC only when BOTH said true, else Iroh at once.
    Manifest { files: Vec<FileMeta> },          // name, size, blake3 hash
    Offer { sdp: String },
    Answer { sdp: String },
    Ice { candidate: String, sdp_mid: Option<String>, sdp_mline_index: Option<u16> },
    Request { file: u32, offset: u64, epoch: u32 }, // resume-capable; epoch increments on every transport switch
    Credit { epoch: u32, bytes: u64 },          // receiver grants `bytes` more payload for this epoch (increments,
                                                // not cumulative); the receiver grants the initial 4 MiB right after Request
    UseRelay { committed_offset: u64 },         // receiver gave up on WebRTC; sender answers RelayReady and resumes
    RelayReady { epoch: u32 },                  // from committed_offset on the iroh stream under the new epoch
    Done { file: u32 },
    Error { message: String },
}
struct ChunkHeader { file: u32, offset: u64, len: u32, epoch: u32 }  // followed by len bytes; a stale epoch is discarded
```

`FileMeta` reuses `BlobHash`, and the manifest hash reuses
`BlobCollection::collection_hash`.

Every frame on the iroh stream carries a one-byte tag, `0` control or `1`
chunk, so the same stream carries signaling, credits, and fallback data. The
sender keeps a dedicated control reader running while a data send is blocked,
so `Credit` and `UseRelay` are always read. On `UseRelay` the sender finishes
nothing on the old transport: it stops the data channel, bumps the epoch, and
re-serves from `committed_offset`; the receiver discards any chunk whose epoch
is stale, so a switch can never duplicate or reorder bytes.

### 3.6 `transfer.rs` (new)

- `trait Transport { async fn send(&mut self, frame: &[u8]); async fn recv(&mut self) -> Option<Bytes>; fn max_frame(&self) -> usize; }`
  with two impls: `IrohStream` (both targets) and `WebRtcChannel` (wasm).
- `serve(control, files)` sender loop: on `Request` read slices from
  `FileSource` starting at `offset`, frame as `ChunkHeader + bytes`, send with
  backpressure, then `Done`.
- `fetch(control, sink)` receiver loop: write chunks to `FileSink`, feed
  `blake3::Hasher` incrementally, compare with the manifest hash at `Done`.
- Chunk payload budget: `min(transport.max_frame(), 64 KiB) - framing`, where
  framing is the encoded `ChunkHeader` plus the length prefix, so the whole
  encoded message never exceeds the negotiated limit. `IrohStream` reports
  64 KiB, `WebRtcChannel` reports the remote `sctp.maxMessageSize`. Never
  round an advertised limit upward; a peer that advertises less than 16 KiB
  gets smaller chunks, not larger ones. A unit test checks complete encoded
  frame sizes at the boundary.
- Receive window: `bufferedAmount` only bounds the sender's outgoing queue.
  On the receiver, `onmessage` keeps delivering while a slow disk sink lags,
  and the service-worker `MessagePort` path adds a second unbounded queue. The
  receiver therefore grants credits: `Control::Credit { bytes }` is sent after
  the sink has consumed data, the sender never has more than the granted
  window in flight (4 MiB initial), and the service-worker route forwards
  demand from the `ReadableStream` pull back over the port before acking.
- Progress is a `watch` channel of `TransferProgress { file, bytes_done, total,
  path: Connecting | Direct | Relayed, phase }` that `app.rs` renders. The
  engine never touches egui.

### 3.7 `file_io.rs` (new)

- `FileSource::read(offset, len) -> Vec<u8>`.
  Native: `std::fs::File` + `read_exact_at`. WASM: `web_sys::File::slice`
  then `Blob::array_buffer()` awaited through `JsFuture`. One read-ahead
  buffer so the next slice loads while the current one drains.
- `FileSink::write(bytes)`, `finish()`.
  Native: `std::fs::File` in the chosen save directory.
  WASM, tried in order:
  1. `window.show_save_file_picker()` → `FileSystemFileHandle::create_writable()`
     → `FileSystemWritableFileStream::write_with_u8_array`. Chromium only, and
     it needs transient user activation, so the UI calls it within a few
     seconds of the "Save" click.
  2. Service-worker streaming: page opens a `MessageChannel`, posts
     `{id, name, size, port}` to `sw.js`, navigates a hidden iframe to
     `download/{id}`, and the worker answers with a `Response` whose body is a
     `ReadableStream` fed from the port. Firefox and Safari. This path is
     best-effort, not assumed: the first page load may not be controlled by
     the worker yet, browsers may terminate an idle worker mid-download, and
     cancellation must close the port. The manual matrix carries explicit
     cases for each, plus recovery after a worker restart.
  3. In-memory `Blob` + anchor click (today's `download_file_wasm`), only when
     size is below 256 MiB and both above failed.
- `app.rs` stores `web_sys::File` handles (wasm) or paths (native), never
  bytes.

### 3.8 `webrtc.rs` (new, `#[cfg(target_arch = "wasm32")]`)

- `PeerChannel::offerer(ice_servers) / answerer(ice_servers)` around
  `RtcPeerConnection`; the **sender is the offerer** and creates the data
  channel (`ordered: true`, `binaryType: "arraybuffer"`).
- Trickle ICE: `onicecandidate` → `Control::Ice` over the iroh stream, and
  incoming `Ice` → `add_ice_candidate`. `Closure`s are stored on the struct,
  same pattern as `file_input_closure` in `app.rs`.
- Backpressure: `set_buffered_amount_low_threshold(1 MiB)`; `send` awaits an
  `onbufferedamountlow` oneshot whenever `buffered_amount() > 4 MiB`. That wait
  is always raced against channel `close`, `error`, cancellation, and the
  transport-switch signal, because a failed channel need not drain and the
  low-water event may never fire. A disconnect-under-backpressure test covers
  it.
- `max_frame()` reads `pc.sctp.maxMessageSize` through `js_sys::Reflect`.
- Receive: `onmessage` pushes `ArrayBuffer` payloads into an async channel.
- `path_kind()` polls `get_stats()` for the succeeded candidate pair and maps
  `relay` → `Relayed`, else `Direct`. With no TURN configured this is always
  `Direct` once connected, but keep it so a TURN server can be added later
  without UI changes.
- ICE servers: `stun:stun.cloudflare.com:3478` and `stun:stun.l.google.com:19302`.
- Failure: if `connectionState` is not `connected` within 10 s or becomes
  `failed`, the receiver sends `Control::UseRelay` and both sides switch
  `Transport` to `IrohStream`, resuming from the last written offset.

### 3.9 `app.rs`

- Migrate `impl eframe::App`: `update` → `logic` (URL parsing, theme, state
  polling) and `ui` (drawing). `save` unchanged.
- Replace `picked_file_data`, `shared_files_data`, `node`, `is_receiving`,
  `receive_status`, and the two 400-line receive loops with one
  `TransferHandle` (progress watch + cancel) plus the `SharedFiles` list of
  `FileMeta + FileSource`.
- Sender flow: pick file → `Node::bind` → show link → wait. Receiver flow: on
  load, if the fragment starts with `endpoint`, parse the ticket, call
  `history.replace_state` to scrub it, and show the receive panel with a
  "Save" button that runs `fetch`.
- Show `path: Direct | Relayed` and throughput next to the progress bar.
- Delete `TorrentInfo`, `magnet_uri`, `peers_count`, and the `value` field.
- Keep `Tc`, theme handling, `show_home_cards`, `show_receive_panel`,
  `format_size` (as a free function).

### 3.10 Web shell

- `index.html`: remove the WebTorrent tag; put the service-worker registration
  in its own `<script>`; keep the `#dev` bypass with `location.hash === "#dev"`.
- `assets/sw.js`: bump `cacheName` (evicts stale caches now that registration
  works), keep `filesToCache`, add the `download/{id}` streaming route and a
  `message` handler for the `MessageChannel` ports.
- `CLAUDE.md`: replace the wire-format and 256 KiB paragraphs with the
  `protocol.rs` description and the new module list.

### 3.11 Tests

- `local_test_*`: protocol frame round trips, chunk-size negotiation including
  framing overhead at the boundary, ticket ↔ link round trip including the
  `#dev` and `cap` cases, wrong-`cap` rejection before the manifest, receive
  window never exceeded with a slow sink, disconnect under backpressure
  unblocks the send loop, incremental hash equals whole-file hash,
  `FileSource` slicing on native, transport selection when only one side
  advertises WebRTC, a transport switch mid-file never duplicates or drops
  bytes, and a modified-after-hash file is refused.
- `online_test_*`: two native `Node`s, full transfer over `IrohStream` with
  hash verification. This exercises the entire engine on native, which is
  also the browser fallback path.
- Replace `local_test_chunk_size_calculation` (asserts the removed 256 KiB
  constant).
- Stretch: `wasm-bindgen-test` in headless Chrome with two
  `RtcPeerConnection`s in one page to cover `webrtc.rs` and backpressure.
- Manual matrix before calling it done: Chrome↔Chrome on different networks,
  Firefox and Safari as receivers, a 5 GB file, throughput on LAN and WAN.

### 3.12 Relay operations

- Development: `presets::N0` (public, rate-limited, no uptime guarantee).
- Production: self-host `iroh-relay` 1.1 on a small VPS with a domain and
  Let's Encrypt, then `RelayChoice::Custom(url)` selected by a compile-time
  env var. Browsers need `wss://` with a valid certificate.

### 3.13 Prepare phase and file snapshot

The manifest carries the final BLAKE3 hash, so the sender hashes each file
once, in a visible "Preparing" phase, before the link is shown. That pass
reads in slices with the same `FileSource`, so memory stays bounded. The
snapshot rule: the sender keeps the handle open (native `File`, wasm
`web_sys::File`) and records size and modification time at hash time; a
`Request` is refused with `Error` when either changed. Browsers already fail
the read when the underlying file changed after the `File` was picked, and the
sender maps that to the same error. The receiver still compares the final
hash with the manifest, so a change that slips past the check surfaces as a
verification failure, never as a silently corrupt file.

## 4. Refactoring opportunities (reuse before new code)

- Collapse the four `EchoNode` constructors into `Node::bind`.
- Merge `start_receiving` and `reconnect_for_files` into one `fetch` call
  driven by `TransferHandle`.
- Reuse `BlobHash`, `BlobCollection::collection_hash`, and
  `BlobCollection::to_share_string` for the manifest instead of new types.
- Reuse the `u32 LE` length-prefix convention from `node.rs` for all frames.
- Reuse the `file_input_closure` pattern for every `web-sys` callback.
- Reuse `download_file_wasm` as the last-resort `FileSink`.
- Reuse `format_size` and the `Tc` theme code as-is.
- Remove `subscribe_accept_events`, `TorrentInfo`, the dummy request upload,
  and the per-chunk logging.

## 5. New code requirements

Only what cannot come from refactoring:

- `protocol.rs`, `transfer.rs`, `file_io.rs`, `webrtc.rs` as specified.
- Service-worker streaming route in `assets/sw.js`.
- `RelayChoice` and the env-var switch in `node.rs`.
- Progress and path display in `app.rs`.

## 6. Risk assessment

| Risk | Impact | Mitigation |
|---|---|---|
| egui 0.31 → 0.36 across 2700 lines of UI code | Compile churn, subtle layout changes | Do it as its own commit first; let the compiler drive; diff screenshots of the three panels before/after. Pin `glow`. |
| iroh 1.1 renames (`NodeId`→`EndpointId`, `node_id()`→`id()`, `remote_node_id()`→`remote_id()`, `Endpoint::builder(preset)`, no `bind_transport`) | Every use site in `node.rs`, `app.rs`, `tests.rs` | Covered by the rewrite; `tests.rs` updated in the same commit. |
| `getrandom` 0.3 still in the wasm tree via a transitive dep | wasm build breaks if the cfg is removed early | Keep the rustflag until `cargo tree` is clean. |
| Wasm bundle growth (iroh 1.1 + WebRTC glue) | Slower first load | Measure `dist/*.wasm` before/after; `opt-level = "s"` for the wasm profile if needed. |
| `showSaveFilePicker` is Chromium-only and needs user activation | Firefox/Safari receivers | Service-worker streaming fallback (3.7), in-memory last resort with a size cap. |
| Service worker now actually registers | Testers may be served stale bundles | Bump `cacheName`; keep `#dev`. |
| n0 public relays are rate-limited | Slow or failing relayed transfers | Self-host before any real use (3.12). |
| Sender's tab must stay open | Product expectation | Inherent to true P2P; say so in the UI. Store-and-forward is a separate feature. |
| Safari data-channel limits | Interop | Negotiate the chunk budget from `maxMessageSize` minus framing; never round a limit up (§3.6). |
| Cargo.lock churn from the migration | Hard-to-review diff | Dependency migration is its own PR. |
| ALPN and link format change | Old builds cannot talk to new ones | Acceptable, there are no deployed users. |
| Relay operator dials the sender without the link | Unauthorized download | `cap` secret in the fragment, checked before anything is served (§3.4). |
| Slow receiver disk with a fast sender | Receiver memory grows without bound | Credit-based receive window (§3.6). |

## 7. Implementation order

Each step ends with `./check.sh` green and is a mergeable PR on its own.

1. **Dependency migration.** Upstream iroh 1.1 + `iroh-tickets`, remove all
   patches, egui/eframe 0.36 with the `App` trait split, every other bump from
   3.1, dedupe `web-sys`, explicit features, `getrandom` 0.4, fix
   `index.html`, bump `cacheName`. Keep today's relay-based transfer working
   end to end (browser is relay-only at this point). Update `tests.rs`.
   *Milestone: works via relay on upstream iroh.*
2. **Protocol and node.** `protocol.rs`, `Node::bind` with fresh keys and
   `RelayChoice`, ticket links, fragment scrub, single receive path in
   `app.rs`, delete torrent leftovers.
3. **Streaming file I/O.** `file_io.rs` and `transfer.rs` over `IrohStream`
   only. Sender slices, receiver streams to disk, incremental BLAKE3,
   bounded memory, progress channel. Native online test for the full path.
   *Milestone: multi-GB files via relay on both targets.*
4. **WebRTC data channel.** `webrtc.rs`, signaling over the control stream,
   backpressure, chunk negotiation, `Direct | Relayed` indicator.
   *Milestone: direct browser-to-browser transfer.*
5. **Fallback and resume.** ICE timeout → `UseRelay`, transport switch with
   offset resume, `Request { offset }` on reconnect.
6. **Receiver fallbacks and polish.** Service-worker streaming route for
   Firefox/Safari, in-memory last resort, multi-file manifest UI, throughput
   display, `CLAUDE.md` update.
7. **Operations.** Self-hosted `iroh-relay` config and deployment notes,
   env-var relay selection, manual test matrix run and recorded.

## 8. Out of scope

- Native WebRTC (`str0m` or `webrtc-rs`) for direct native↔browser paths.
- TURN. Add only if relayed-through-iroh transfers prove too slow.
- Store-and-forward when the sender is offline.
- Per-chunk merkle verification (`bao-tree`) in the browser; revisit when
  resume needs it.

## 9. Amendments log

2026-09-08, from an adversarial review of this plan (Codex) and the team's
plan-review gate. None changes direction; all are folded into the sections above.

1. Authorization: a random `cap` secret in the link fragment, validated in
   `Hello` before the manifest is served (§3.4, §3.5, §6).
2. iroh on wasm needs `features = ["tls-ring"]` (§3.1, §3.3).
3. `RtcSctpTransport` is not a web-sys feature; `maxMessageSize` is read via
   `js_sys::Reflect` (§3.3, §3.8).
4. `--cfg=web_sys_unstable_apis` is required for `show_save_file_picker`
   (§3.1).
5. Chunk payload budget subtracts framing; advertised limits are never rounded
   up (§3.6).
6. Credit-based receive window so a slow sink cannot exhaust receiver memory,
   including the service-worker port path (§3.6).
7. Blocked sends race against close, error, cancel, and transport switch (§3.8).
8. Tests added for each of the above (§3.11).
9. `check.sh` will be scoped to this crate in step 1: its workspace-wide wasm
   check cross-compiles the `git2`-based `file-tree-merge` crate and can never
   pass, and CI does not run it (§7 step 1).
10. Both `Cargo.lock` files are git-tracked; the workspace lockfile is committed
    with every step.

2026-09-08, from a second independent review of this plan:

11. `Hello` advertises WebRTC capability; the pair uses the data channel only
    when both sides can, otherwise the iroh stream from the start (§3.5).
12. `Control::Credit` is now in the protocol, with initial grant and increment
    semantics stated (§3.5).
13. Transport switch is an explicit handshake: tagged frames on one stream, a
    dedicated control reader during blocked sends, `UseRelay` with the
    receiver's committed offset, `RelayReady` under a new epoch, stale epochs
    discarded (§3.5, §3.8).
14. A visible prepare phase hashes files before the link is shown, with a
    snapshot rule that refuses files changed since hashing (§3.13).
15. The service-worker sink is best-effort with explicit manual cases for
    first-load control, worker termination, cancellation, and recovery (§3.7).
