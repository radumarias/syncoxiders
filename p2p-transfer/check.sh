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
node --test tests/*.test.mjs
sh deploy/relay/render.sh --check
wasm-pack test --headless --firefox -- --test webrtc_wasm
wasm-pack test --headless --firefox -- --test relay_wasm
wasm-pack test --headless --firefox -- --test resume_wasm
trunk build
# Every Trunk copy-file entry in index.html is a byte copy of its source, and
# no dist HTML file has an inline <script> (Trunk.toml inject_scripts = false;
# the CSP would block it). build-web.sh runs the same check.
node package-cf-output.mjs --check-dist
