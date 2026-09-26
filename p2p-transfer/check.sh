#!/usr/bin/env bash
# CI-like checks for the p2p-transfer crate (see CLAUDE.md). Scoped to this crate on purpose:
# sibling workspace members (file-tree-merge → git2/libgit2) cannot cross-compile to wasm32.
set -eux

cargo check   --quiet -p p2p-transfer --all-targets
cargo check   --quiet -p p2p-transfer --all-features --lib --target wasm32-unknown-unknown
cargo fmt     -p p2p-transfer -- --check
cargo clippy  --quiet -p p2p-transfer --all-targets --all-features -- -D warnings -W clippy::all
cargo clippy  --quiet -p p2p-transfer --all-features --lib --target wasm32-unknown-unknown -- -D warnings -W clippy::all
cargo test    --quiet -p p2p-transfer --all-targets --all-features
cargo test    --quiet -p p2p-transfer --doc
node --test tests/resume_worker.test.mjs
node --test tests/diagnostics.test.mjs
node --test tests/webrtc_stats.test.mjs
node --test tests/wake_lock.test.mjs
node --test tests/theme.test.mjs
wasm-pack test --headless --firefox -- --test webrtc_wasm
wasm-pack test --headless --firefox -- --test relay_wasm
wasm-pack test --headless --firefox -- --test resume_wasm
trunk build
cmp assets/theme.js dist/assets/theme.js
