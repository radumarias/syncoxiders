# p2p-transfer contributor guide

## Project

`p2p-transfer` is an `eframe`/`egui` application for native and browser file
transfer. Upstream iroh 1.1 supplies authenticated endpoints, share tickets,
signalling, and the relay transport. Browser-to-browser WebRTC data transfer is
implemented as a direct `RTCDataChannel`, with offer/answer/ICE travelling over
iroh's encrypted control stream and the iroh stream retained as fallback.

The parent Cargo workspace has `p2p-transfer` as its default member.
`README.md` is inherited `eframe_template` material, not project documentation.

## Toolchain and commands

`rust-toolchain` is in this directory and pins nightly with `rustfmt`, `clippy`,
and `wasm32-unknown-unknown`. Run commands from `p2p-transfer/`; running Cargo
from the workspace root may select the user's stable toolchain instead.

- `cargo run --release` — native desktop app.
- `trunk serve` — browser build at `http://127.0.0.1:8080`.
- `trunk build` — static `dist/`.
- `./check.sh` — required gate: native and wasm checks, fmt, clippy with
  `-D warnings` on both targets, tests, doctests, and `trunk build`.
- `cargo test <substring>` — run a focused unit test.

There must be one workspace `../Cargo.lock`. A gitignored
`p2p-transfer/Cargo.lock` is stale local state and can make Trunk run a
wasm-bindgen CLI version different from the workspace dependency.

## Browser development

`index.html` registers `assets/sw.js`. App-shell requests are network-first
because `Trunk.toml` disables filename hashing.

- `#dev` unregisters service workers and clears caches. Use it to bypass stale
  builds, but it deliberately disables the service-worker streaming download
  sink.
- Test Chromium's File System Access sink with or without `#dev`.
- Test the service-worker sink without `#dev`, after the page is controlled by
  the current worker. Use the `sink=fsa`, `sink=sw`, and `sink=mem` fragment
  flags to force a route.

The memory fallback is capped at 256 MiB. File System Access and service-worker
routes stream bounded chunks and are required for larger downloads.

## Architecture

- `src/app.rs` — UI state machine. It owns handles and metadata, never file
  bytes. Picking a sender file starts a bounded hashing preparation phase.
  Receiver Save creates sinks before sending `ReceiveCommand::Save`.
- `src/node.rs` — iroh `Node`, endpoint ticket links, capability authorization,
  relay selection, and the sender-side protocol handler.
- `src/protocol.rs` — bounded tagged postcard frames: handshake, manifest,
  request/credit, signalling, chunks, completion, and errors. Never allocate
  before checking `MAX_FRAME`.
- `src/transfer.rs` — transport-independent sender and receiver state machines.
  The receiver grants credit only after a sink accepts bytes. Data-channel
  failure changes epoch and resumes over iroh.
- `src/file_io/mod.rs` — shared source/sink contracts, snapshots, filename
  policy, native per-session reads, transactional native staging, memory
  fallback, and test sinks.
- `src/file_io/web.rs` and `assets/download-sinks.js` — browser `File.slice`
  source, File System Access sink, service-worker streaming sink, and memory
  fallback.
- `assets/sw.js` — app cache plus capability-addressed, single-use streaming
  download responses with bounded demand/ack flow.
- `src/webrtc.rs` and `assets/webrtc-channel.js` — browser data-channel
  implementation: SDP/ICE, bounded inbound queue, `bufferedAmount`
  backpressure, path stats, close/failure signalling, and relay fallback.

Native and wasm share protocol and transfer logic but differ at file I/O and
endpoint transport boundaries. Any native filesystem or tokio-net-only code
must remain under `#[cfg(not(target_arch = "wasm32"))]`. Browser JS objects and
their futures are `!Send`; do not add a `Send` bound to `Source` or `Sink`.

## Invariants

- Share links contain a bearer capability in the fragment. Never log or put it
  in a query string.
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

## Tests

`local_test_*` must be deterministic and offline. `online_test_*` may require
the public relay and must never convert a timeout into a silent pass. Native
loopback endpoint tests use `RelayChoice::None` and should remain part of the
normal suite. The real browser data-channel integration test runs with:

```sh
wasm-pack test --headless --firefox -- --test webrtc_wasm
```

Durable OPFS checkpoint/reopen behavior runs in a real browser worker with:

```sh
wasm-pack test --headless --firefox -- --test resume_wasm
```
