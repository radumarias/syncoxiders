#!/usr/bin/env bash
# Cloudflare static assets (Linux x86_64). No global Trunk install needed.
#
# P2P_RELAY_URL (optional): comma-separated relay URLs compiled into the wasm
# by src/node.rs and rendered into the packaged Content-Security-Policy. Unset,
# empty or ASCII-whitespace-only means n0's public relays; any other character
# outside printable ASCII fails the build. CI passes the repository variable,
# which is an empty string until the owner sets it. Once the legal pages are
# filled, it must list only the operator relay they describe (OPERATOR_RELAY
# in package-cf-output.mjs).
# OXFER_ALLOW_PLACEHOLDERS=1: package even though the legal pages still contain
# [[PLACEHOLDER]] tokens, or are filled but P2P_RELAY_URL is not the operator
# relay alone. For local test builds only; never deploy the result.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if [[ -f Cargo.lock ]]; then
    echo "Remove the stale p2p-transfer/Cargo.lock; builds must use ../Cargo.lock." >&2
    exit 1
fi

# Empty or ASCII-whitespace-only means unset, as in src/node.rs and
# package-cf-output.mjs; unset it so the wasm build sees no value at all. The
# set is spelled out rather than [[:space:]], whose meaning depends on the
# locale.
if [[ -n "${P2P_RELAY_URL+set}" && -z "${P2P_RELAY_URL//[$' \t\n\v\f\r']/}" ]]; then
    unset P2P_RELAY_URL
fi
if [[ -n "${P2P_RELAY_URL+set}" ]]; then
    export P2P_RELAY_URL
else
    echo "P2P_RELAY_URL is not set: building for n0's public relays."
fi
# Before the long build: validate the relay list with the parser that renders
# the CSP (stricter than src/node.rs, so a value it accepts never makes the
# wasm fall back to n0's relays), then run the legal-page guards on the source
# pages (checkLegalPages in package-cf-output.mjs): no [[PLACEHOLDER]] token,
# and once the pages are filled, only the operator relay in P2P_RELAY_URL.
# OXFER_ALLOW_PLACEHOLDERS=1 turns the page guards into warnings; an invalid
# relay list always fails. --check-dist below proves dist holds these pages.
node package-cf-output.mjs --check-relay --check-legal

tools="$PWD/target/pages-tools"
mkdir -p "$tools"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 --fail --show-error --location \
        https://sh.rustup.rs -o "$tools/rustup-init.sh"
    bash "$tools/rustup-init.sh" -y --profile minimal --default-toolchain none --no-modify-path
fi
# Resolve the crate's rust-toolchain, not the workspace root's default toolchain.
rustup target add wasm32-unknown-unknown

# Use the prebuilt musl binary to avoid compiling Trunk during each web build.
trunk_version=0.21.14
trunk_archive=trunk-x86_64-unknown-linux-musl.tar.gz
trunk_sha256=a67f4054b249fe9acc5fabc25de1aebf19783aca3ad6ff64bf34d7da44d0ea20
curl --proto '=https' --tlsv1.2 --fail --show-error --location --retry 3 \
    "https://github.com/trunk-rs/trunk/releases/download/v${trunk_version}/${trunk_archive}" \
    -o "$tools/$trunk_archive"
echo "$trunk_sha256  $tools/$trunk_archive" | sha256sum --check
tar -xzf "$tools/$trunk_archive" -C "$tools" trunk

"$tools/trunk" build --release --locked --public-url /

# Cloudflare limits each static asset to 25 MiB, including the WebAssembly binary.
oversized="$(find dist -type f -size +26214400c -print)"
if [[ -n "$oversized" ]]; then
    printf 'Assets exceed the Cloudflare 25 MiB limit:\n%s\n' "$oversized" >&2
    exit 1
fi
# Every Trunk copy-file entry in index.html (the page scripts, the legal
# pages, _headers, sw.js) is a byte copy of its source, and no dist HTML file
# has an inline <script> (Trunk.toml inject_scripts = false; the CSP would
# block it).
node package-cf-output.mjs --check-dist

node package-cf-output.mjs
