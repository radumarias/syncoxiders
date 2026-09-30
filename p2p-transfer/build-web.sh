#!/usr/bin/env bash
# Cloudflare static assets (Linux x86_64). No global Trunk install needed.
#
# P2P_RELAY_URL (optional): comma-separated relay URLs compiled into the wasm
# by src/node.rs and rendered into the packaged Content-Security-Policy. Unset,
# empty or whitespace-only means n0's public relays. CI passes the repository
# variable, which is an empty string until the owner sets it.
# OXFER_ALLOW_PLACEHOLDERS=1: package even though the legal pages still contain
# [[PLACEHOLDER]] tokens, or describe relay.oxfer.app in a build for n0's
# relays. For local test builds only; never deploy the result.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if [[ -f Cargo.lock ]]; then
    echo "Remove the stale p2p-transfer/Cargo.lock; builds must use ../Cargo.lock." >&2
    exit 1
fi

# Validate the relay list before the long build, with the parser that renders
# the CSP. Empty or whitespace-only means unset, as in src/node.rs; unset it so
# the wasm build sees no value at all.
if [[ -n "${P2P_RELAY_URL+set}" && -z "${P2P_RELAY_URL//[[:space:]]/}" ]]; then
    unset P2P_RELAY_URL
fi
if [[ -n "${P2P_RELAY_URL+set}" ]]; then
    node package-cf-output.mjs --check-relay
    export P2P_RELAY_URL
else
    echo "P2P_RELAY_URL is not set: building for n0's public relays."
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
# Files Trunk copies verbatim (copy-file entries in index.html).
for copied in \
    assets/boot.js assets/app-init.js assets/theme-lab.js assets/legal.css; do
    cmp "$copied" "dist/$copied"
done
for page in theme.html privacy.html terms.html abuse.html; do
    cmp "$page" "dist/$page"
done
# Trunk.toml inject_scripts = false: the page must not carry an inline loader.
if grep -q -E '<script( type="module")?>' dist/index.html; then
    echo "dist/index.html contains an inline <script>; the CSP would block it." >&2
    exit 1
fi

# Legal pages ship with [[OPERATOR_NAME]]-style tokens until the operator's
# details are filled in. Refuse to package them unless explicitly allowed.
placeholders=0
for page in privacy.html terms.html abuse.html; do
    while IFS= read -r token; do
        [[ -n "$token" ]] || continue
        echo "dist/$page still contains the placeholder $token" >&2
        placeholders=1
    done < <(grep -o -E '\[\[[A-Z][A-Z0-9_]*\]\]' "dist/$page" | sort -u || true)
done
if [[ "$placeholders" -ne 0 ]]; then
    if [[ "${OXFER_ALLOW_PLACEHOLDERS:-}" == 1 ]]; then
        echo "OXFER_ALLOW_PLACEHOLDERS=1: packaging anyway. Do not deploy this build." >&2
    else
        echo "Replace the placeholders in privacy.html, terms.html and abuse.html before deploying." >&2
        echo "For a local test build only, set OXFER_ALLOW_PLACEHOLDERS=1." >&2
        exit 1
    fi
fi

# The filled pages describe the operator's relay at relay.oxfer.app, not n0's
# public relays. Refuse to publish them from a build that uses n0's relays.
if [[ "$placeholders" -eq 0 && -z "${P2P_RELAY_URL+set}" ]] &&
    grep -q -F 'relay.oxfer.app' dist/privacy.html dist/terms.html dist/abuse.html; then
    if [[ "${OXFER_ALLOW_PLACEHOLDERS:-}" == 1 ]]; then
        echo "OXFER_ALLOW_PLACEHOLDERS=1: packaging legal pages that describe relay.oxfer.app in a build for n0's relays. Do not deploy this build." >&2
    else
        echo "P2P_RELAY_URL is not set, so this build uses n0's public relays, but the filled privacy notice describes the Oxfer relay; set P2P_RELAY_URL or edit the pages to describe the relay in use." >&2
        echo "For a local test build only, set OXFER_ALLOW_PLACEHOLDERS=1." >&2
        exit 1
    fi
fi

node package-cf-output.mjs
