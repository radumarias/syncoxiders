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
node --test tests/favicon.test.mjs
node --test tests/package-cf-output.test.mjs
node --test tests/web-pages.test.mjs
sh deploy/relay/render.sh --check
wasm-pack test --headless --firefox -- --test webrtc_wasm
wasm-pack test --headless --firefox -- --test relay_wasm
wasm-pack test --headless --firefox -- --test resume_wasm
trunk build
cmp assets/theme.js dist/assets/theme.js
cmp assets/favicon.js dist/assets/favicon.js
cmp assets/oxfer-favicon-light.svg dist/assets/oxfer-favicon-light.svg
cmp assets/oxfer-favicon-dark.svg dist/assets/oxfer-favicon-dark.svg
cmp assets/_headers dist/_headers
cmp assets/boot.js dist/assets/boot.js
cmp assets/app-init.js dist/assets/app-init.js
cmp assets/theme-lab.js dist/assets/theme-lab.js
cmp assets/legal.css dist/assets/legal.css
cmp theme.html dist/theme.html
cmp privacy.html dist/privacy.html
cmp terms.html dist/terms.html
cmp abuse.html dist/abuse.html
# Trunk.toml inject_scripts = false: no inline loader for the CSP to block.
# Any <script> without a src attribute counts, whatever else it carries
# (a nonce, type or defer).
node -e 'if (/<script\b(?![^>]*\ssrc\s*=)[^>]*>/i.test(require("fs").readFileSync(process.argv[1], "utf8"))) process.exit(1)' dist/index.html || {
    echo "dist/index.html contains an inline <script>; the CSP would block it." >&2
    exit 1
}
