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
trunk build
