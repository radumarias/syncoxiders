# p2p-transfer contributor guide

## Project

`p2p-transfer` is an `eframe`/`egui` application for native and browser file
transfer. Upstream iroh 1.1 supplies authenticated endpoints, share tickets,
signalling, and the relay transport. Browser-to-browser WebRTC data transfer is
implemented as a direct `RTCDataChannel`, with offer/answer/ICE travelling over
iroh's encrypted control stream and the iroh stream retained as fallback.

The parent Cargo workspace has `p2p-transfer` as its default member, and
`p2p-transfer/web` (`oxfer-web`, the Dioxus browser UI) as a member.

## Toolchain and commands

`rust-toolchain` is in this directory and pins nightly with `rustfmt`, `clippy`,
and `wasm32-unknown-unknown`. Run commands from `p2p-transfer/`; running Cargo
from the workspace root may select the user's stable toolchain instead.

- `cargo run --release` — native desktop app.
- `trunk serve` — egui browser build at `http://127.0.0.1:8080`.
- `trunk build` — static `dist/`.
- `dx serve --platform web -p oxfer-web` / `dx build --platform web -p oxfer-web` — the
  Dioxus browser UI (dx 0.7.10; `-p` is required in this workspace).
- `bash build-web.sh` — release web build plus cf Build Output packaging and a bundle-size
  report. `OXFER_WEB_UI=egui` (default, Trunk) or `dioxus` (`dx bundle`); same `dist/`
  layout. Production switches per [`docs/web-ui-cutover.md`](docs/web-ui-cutover.md).
- `npx cf deploy --prebuilt` — upload the packaged Worker (`cf` from `package.json`).
- Pushes to `main` that touch this crate deploy via `.github/workflows/oxfer-web.yml`.

Cloudflare: this crate has no Wrangler config. Use `cf` (`cf --help`,
`cf cli search …`). Do not fall back to Wrangler. `cf deploy` without
`--prebuilt` is for Vite/Wrangler bundling and is wrong here; Trunk output is
packaged first. Details: [`docs/cloudflare-workers.md`](docs/cloudflare-workers.md).
- `./check.sh` — required gate: native and wasm checks, fmt, clippy with
  `-D warnings` on both targets, tests, doctests, OPFS worker fault tests,
  real Firefox WebRTC and persistence tests, `trunk build`, and the web crate's
  fmt/clippy and `dx build`. It requires Node.js, Firefox, `wasm-pack`, Trunk, dx,
  and clang with `llvm-ar`: every wasm32 build compiles ring's C code, which gcc
  cannot target.
- `cargo test <substring>` — run a focused unit test.
- `npm run agent -- send <file>` / `npm run agent -- recv <link> <dir>` — scripted
  sender and receiver for automation and AI agents (`agent/oxfer-agent.mjs`,
  served as `/oxfer-agent.mjs`; see [`docs/agent-transfer.md`](docs/agent-transfer.md)).
  Buttons are found by primary-blue colour in screenshots, so a palette change
  must update `PRIMARY_BLUE` there.

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

- `src/sessions/` — the UI-agnostic session core (#66). `Oxfer` owns every
  handle and task between gestures and the engine: `SendSession` (pick → hash →
  bind → link → peers), `ReceiveSession` (link → bind → Save → verify),
  `ChatSession`, and on wasm `LocalCopies`, `DiagnosticsSession` and URL intake
  (`location.rs`). UIs call `tick()`, render `snapshot()` (plain `Clone +
  PartialEq` view models, links wrapped in redacting `BearerLink`), and
  `dispatch(Intent)`; `Notifier` wakes them, and `Snapshot::live` asks for
  ~10 Hz polling while engine progress moves. Browser/OS services sit behind
  the `Platform` trait (`NativePlatform`, `WebPlatform`, `TestPlatform`);
  user-facing wording lives in `sessions::text`. New session logic goes here,
  never in a UI adapter.
- `src/i18n.rs` + `locales/<lang>/oxfer.ftl` — user-facing copy shared by every UI, in Fluent.
  `Msg::X.text()` / `Msg::X.with(&[("name", &value)])`; add the id to `locales/en/oxfer.ftl`
  and the `messages!` list together (a test fails if they drift). Move a sentence here when a second
  UI needs it (61 are shared so far; UI-only copy may stay literal); a new language is a new `.ftl` plus a `Locale` variant.
- `src/app.rs` — the egui adapter (feature `egui-ui`, on by default): theme,
  layout, rendering and view state only (copied flags, focus, open panels,
  native file dialogs). It holds no transfer, retry or session logic.
  `--no-default-features` builds the engine plus `sessions` without egui.
- `src/theme.rs` — the 13 colour tokens × 3 themes × light/dark and shape tokens.
  egui builds its palette from `Palette::of`; the web UI loads `css_variables()`. Chat member
  colours (`member_slot`/`member_color`, `--member-N`) live here too.
  Change colours here, never in a UI.
- `web/` — `oxfer-web`, the Dioxus 0.7 DOM UI (browser only; the native binary is a
  stub so `cargo build --all` works). `core.rs` holds `Oxfer` plus a `Signal<Snapshot>`
  refreshed on `Notifier` changes (10 Hz while `live`); `route.rs` is a path router
  (`/`, `/send`, `/receive`, `/chat`, `/copies`, `/diags`) that never parses, logs or
  rewrites the fragment and keeps route and `Activity` in step; `screens/` are
  components; `assets/oxfer.css` is the base CSS over the generated variables. The
  core runs with `manage_history: false`. Dispatch Save and the file picker
  synchronously inside the click handler (transient activation).
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
  download responses with bounded demand/ack flow. Trunk's fixed names and
  navigations are network-first; dx's `-dxh<hash>` assets are cache-first.
- `src/chat.rs` — chat rooms on their own ALPN (`oxfer/chat/1`): a host endpoint that
  checks the link capability, names guests and relays messages; `ChatHandle` is the
  session's handle for both roles. `assets/chat-bridge.js` exposes the open room to page
  scripts (`oxfer:chat` DOM events, `window.oxfer.chat.send`) for the agent script;
  `sessions::chat` installs it.
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

Without camera or microphone permission, Firefox gathers ICE candidates only on
the default-route interface, so this test needs a default route (but no
Internet); with none, as under `docker run --network none`, it times out.

Durable OPFS checkpoint/reopen behavior runs in a real browser worker with:

```sh
wasm-pack test --headless --firefox -- --test resume_wasm
```

Both commands, plus `node --test tests/resume_worker.test.mjs` for deterministic
cross-worker race and failure injection, are part of `./check.sh` and CI.
