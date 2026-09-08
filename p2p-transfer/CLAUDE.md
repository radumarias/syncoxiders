# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project context

`p2p-transfer` is a Rust browser-and-native peer-to-peer file transfer app built on `eframe`/`egui` (UI) and a forked `iroh` (P2P transport with WebRTC relay so it works in the browser). It is one crate in the parent `syncoxiders` Cargo workspace (`../Cargo.toml`), and is the workspace's `default-members`, so workspace-level `cargo` commands target this crate by default.

The `README.md` is the unmodified upstream `eframe_template` README — treat it as boilerplate, not as documentation for this project.

## Toolchain and build targets

- `rust-toolchain` pins **nightly** with `rustfmt`, `clippy`, and the `wasm32-unknown-unknown` target preinstalled. Do not switch to stable.
- Two build targets matter and behave differently:
  - **Native** (`x86_64-unknown-linux-gnu` etc.): full feature set, including `bao-tree` merkle verification and a tokio runtime with net features.
  - **WASM** (`wasm32-unknown-unknown`): tokio is reduced to `default-features = false, features = ["io-util", "macros", "sync", "rt"]` (no `net`) on purpose — iroh's net features do not compile to WASM. `bao-tree` is **native-only** and gated with `#[cfg(not(target_arch = "wasm32"))]`. `.cargo/config.toml` adds `--cfg getrandom_backend="wasm_js"` for wasm32 — required by `getrandom 0.3` to compile to wasm.
- `[patch.crates-io]` in both this `Cargo.toml` and the workspace root pins forks of `iroh`, `n0-watcher`, `netwatch`, and `portmapper` (`anchalshivank/*` branches) that add WebRTC/WASM support. Don't naively bump those dependencies — the upstream crates do not yet compile to WASM in this configuration.

## Common commands

Run from this crate's directory unless noted.

- `cargo run --release` — run the native desktop app (egui window, 400x300).
- `trunk serve` — build WASM and serve at `http://127.0.0.1:8080`. Use `http://127.0.0.1:8080/index.html#dev` to bypass the service-worker cache during development (`assets/sw.js` aggressively caches the build).
- `trunk build --release` — produce static `dist/` for deployment.
- `./check.sh` — runs the full CI gate locally: `cargo check` (native + wasm lib), `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, doctests, and `trunk build`. **Match this before claiming a change is done** — clippy is `-D warnings` so any warning fails CI.
- `cargo test --workspace --all-targets --all-features` — full test run (native).
- `cargo test --test <name>` is **not** how individual tests are run here — there are no `tests/` integration files; tests live in `src/tests.rs` and `src/blob_store.rs#tests`. Use `cargo test <substring>` to filter, e.g. `cargo test local_test_blob_store` or `cargo test online_test_small_file_transfer -- --nocapture`.
- `cargo check --target wasm32-unknown-unknown --lib` — fast check that wasm-specific cfg gates still compile.

### Test naming convention

Tests in `src/tests.rs` use a prefix that signals their requirements:

- `local_test_*` — pure in-process tests, safe in any environment.
- `online_test_*` — exercise real `iroh` endpoints, network discovery, and the WebRTC relay. They use short `tokio::time::timeout`s (3–5s) and several deliberately swallow `Err`/timeout outcomes so the suite is non-flaky offline. **Treat a passing online test as "did not regress" rather than "verified working"** — to actually exercise the transfer path, run with `--nocapture` and check the assertion paths were reached.

## Architecture

Three modules carry essentially all the logic:

- **`src/app.rs`** — `P2PTransfer` (the eframe `App` impl) holds the UI state. Almost every shareable piece of state is `Arc<Mutex<...>>` because async tasks (file pickers, the iroh node, transfer streams) write back into the UI from outside the egui frame callback. File picking diverges by target: native uses `rfd::FileDialog` synchronously, WASM uses `web-sys` + a `wasm_bindgen::closure::Closure` stored on the struct (`file_input_closure`). The `received_files` / `shared_files` / `terminal_logs` fields are the integration points — node code pushes into them.
- **`src/node.rs`** — `EchoNode` wraps an `iroh::Endpoint` + `Router` running a custom protocol (`Echo`, ALPN `b"iroh/example-browser-echo/0"`). Transport is `TransportMode::WebrtcRelay` so the same code path works in the browser. The wire format on each bidirectional stream is hand-rolled little-endian framing: filename length (u32), filename bytes, data length (u64), then per-file: `name_len(u32) | name | data_len(u64) | total_chunks(u32) | blake3_hash(32 bytes) | (chunk_idx(u32) | chunk_size(u32) | chunk_data){total_chunks}`. **Chunk size is hardcoded `256 * 1024` (256KB)** in `Echo::handle_connection_0` — there's a unit test asserting that constant, so changing it requires updating the test. `connect()` returns a `Stream<Item = ConnectEvent>` driven by `task::spawn` (n0-future), which is what the UI consumes to render progress. The two file stores (`BlobStore`, `BaoStore`) are kept on the node; `bao_store` is `Option<BaoStore>` and `#[cfg]`-gated to native only.
- **`src/blob_store.rs`** — two layered abstractions:
  - `BlobStore`/`Blob`/`BlobHash` — simple BLAKE3 content-addressed in-memory map, available on both targets. `BlobCollection::to_share_string` / `from_share_string` is the wire format for shareable hash bundles (`hex_hash:name,hex_hash:name,...`). Note the embedded `mod hex` — it's a hand-rolled hex codec; do not pull in the `hex` crate without a reason.
  - `mod bao` (native only) — `BaoStore`/`BaoBlob`/`BaoReceiver` build a `bao_tree::PreOrderMemOutboard` with `BlockSize::from_chunk_log(4)` (16KB chunks, matching `iroh-blobs`), enabling per-chunk merkle verification on the receiver side. This is intended for BitTorrent-like streaming integrity; it is independent from the 256KB framing in `node.rs` (different layers, different chunk sizes — don't conflate them).

The native and WASM variants of `main` (in `src/main.rs`) bootstrap differently: native creates a `tokio::runtime::Runtime` and enters it before `eframe::run_native`; WASM uses `wasm_bindgen_futures::spawn_local` and `eframe::WebRunner`, hides the `loading_text` div on success, and replaces it with a crash message on failure.

## Conventions worth honoring

- `src/lib.rs` and `src/main.rs` start with `#![warn(clippy::all, rust_2018_idioms)]`. Clippy is gated `-D warnings` in `check.sh` — fix lints, don't `#[allow]` them away.
- Anything new that pulls in tokio's `net`, real filesystem, or other non-wasm-friendly APIs **must** be `#[cfg(not(target_arch = "wasm32"))]` or it will break the wasm build (and `check.sh`).
- The `assets/sw.js` service worker caches the wasm bundle aggressively. If you change asset filenames, also update `assets/sw.js`'s `filesToCache`, or testers will load stale code unless they hit `#dev`.
- `Trunk.toml` sets `filehash = false`; don't enable hashed filenames without updating the service worker too.
