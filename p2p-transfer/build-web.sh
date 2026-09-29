#!/usr/bin/env bash
# Cloudflare static assets (Linux x86_64). No global Trunk install needed.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if [[ -f Cargo.lock ]]; then
    echo "Remove the stale p2p-transfer/Cargo.lock; builds must use ../Cargo.lock." >&2
    exit 1
fi

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
test -s dist/index.html
test -s dist/sw.js
cmp assets/_headers dist/_headers
node package-cf-output.mjs
