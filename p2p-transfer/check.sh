#!/usr/bin/env bash
# CI-like checks for the p2p-transfer crate (see CLAUDE.md). Scoped to this crate on purpose:
# sibling workspace members (file-tree-merge → git2/libgit2) cannot cross-compile to wasm32.
set -eux

cargo check   --quiet -p p2p-transfer --all-targets
cargo check   --quiet -p p2p-transfer --all-features --lib --target wasm32-unknown-unknown
cargo fmt     -p p2p-transfer -- --check
cargo clippy  --quiet -p p2p-transfer --all-targets --all-features -- -D warnings -W clippy::all
cargo clippy  --quiet -p p2p-transfer --all-features --lib --target wasm32-unknown-unknown -- -D warnings -W clippy::all
# The session core must build without egui: the Dioxus web crate depends on it that way.
cargo clippy  --quiet -p p2p-transfer --no-default-features --lib -- -D warnings -W clippy::all
cargo clippy  --quiet -p p2p-transfer --no-default-features --lib --target wasm32-unknown-unknown -- -D warnings -W clippy::all
# The Dioxus web UI (web/, #67): browser-only, with a native stub binary for `cargo build --all`.
cargo fmt     -p oxfer-web -- --check
cargo clippy  --quiet -p oxfer-web --target wasm32-unknown-unknown -- -D warnings -W clippy::all
cargo clippy  --quiet -p oxfer-web -- -D warnings -W clippy::all
cargo test    --quiet -p p2p-transfer --all-targets --all-features
cargo test    --quiet -p p2p-transfer --doc
node --test tests/resume_worker.test.mjs
node --test tests/diagnostics.test.mjs
node --test tests/webrtc_stats.test.mjs
node --test tests/wake_lock.test.mjs
node --test tests/theme.test.mjs
node --test tests/theme_lab.test.mjs
node --test tests/favicon.test.mjs
node --test tests/package-cf-output.test.mjs
node --test tests/agent_cli.test.mjs
node --test tests/report-bundle-size.test.mjs
wasm-pack test --headless --firefox -- --test webrtc_wasm
wasm-pack test --headless --firefox -- --test relay_wasm
wasm-pack test --headless --firefox -- --test resume_wasm
trunk build
cmp assets/theme.js dist/assets/theme.js
cmp assets/favicon.js dist/assets/favicon.js
cmp assets/oxfer-favicon-light.svg dist/assets/oxfer-favicon-light.svg
cmp assets/oxfer-favicon-dark.svg dist/assets/oxfer-favicon-dark.svg
# The Dioxus build in debug (unhashed, no wasm-opt). `OXFER_WEB_UI=dioxus bash build-web.sh`
# makes the release bundle and checks its size budget.
dx build --platform web -p oxfer-web
