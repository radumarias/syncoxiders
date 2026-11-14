# Oxfer (p2p-transfer)

Oxfer sends files directly from one device to another, encrypted, with no copy on a
server. Open <https://oxfer.app>, pick a file, and send the link; the other person opens it
and chooses where to save. Chat rooms work the same way.

Under the hood: [iroh](https://iroh.computer) endpoints and share tickets, a bearer
capability in the link's `#fragment` (it never reaches a server), BLAKE3-verified
transfers, and a direct WebRTC data channel between browsers with iroh as the fallback.
See [`CLAUDE.md`](CLAUDE.md) for the architecture and invariants.

## Layout

| Path | What |
| --- | --- |
| `src/sessions/` | UI-agnostic session core (`Oxfer`: snapshot → render → dispatch). Every UI binds to it. |
| `src/app.rs` | egui UI: the native desktop app, and today's web build (Trunk, canvas). |
| `web/` | Dioxus DOM UI for the browser (mobile-first, accessible). Replaces the egui web build at cut-over. |
| `src/theme.rs` | The colour/shape token table both UIs paint with. |
| `assets/` | JS modules the engine binds to (sinks, WebRTC, resume store, wake lock), service worker, icons. |
| `agent/` | Scripted sender/receiver for automation and AI agents ([docs](docs/agent-transfer.md)). |

## Build and run

Run everything from this directory: `rust-toolchain` pins nightly with the wasm32 target.
wasm builds need clang and `llvm-ar` (ring's C code).

- Native desktop: `cargo run --release`. On Debian/Ubuntu first install
  `libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev`.
- Web, egui (current production): `trunk serve`, then open `http://127.0.0.1:8080/#dev`.
- Web, Dioxus: `dx serve --platform web -p oxfer-web` (dx 0.7.10), or
  `dx build --platform web -p oxfer-web`. Routes are paths (`/send`, `/receive`, `/chat`,
  `/copies`, `/diags`), so a static server needs a single-page fallback to `index.html`.

`#dev` in the URL unregisters the service worker and clears its caches, for loading the
latest build.

## Checks

`./check.sh` is the gate (fmt, clippy `-D warnings` on native and wasm, unit and doc tests,
Node tests, headless Firefox WebRTC/resume tests, `trunk build`, `dx build`). See
[`CLAUDE.md`](CLAUDE.md) for the tools it needs.

## Deploy

Cloudflare Workers static assets via the `cf` CLI, never Wrangler
([setup](docs/cloudflare-workers.md)): `npm ci && npm run build && npm run deploy`.
`npm run build` runs `build-web.sh`, which builds the egui web app by default and the
Dioxus app with `OXFER_WEB_UI=dioxus`; both land in `dist/` with the same layout. The
switch to Dioxus in production follows [`docs/web-ui-cutover.md`](docs/web-ui-cutover.md).
