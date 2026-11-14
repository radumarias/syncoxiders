#!/usr/bin/env bash
# Cloudflare static assets (Linux x86_64). No global Trunk or dx install needed.
#
# OXFER_WEB_UI selects the web UI that is built into dist/:
#   egui    (default) the Trunk build of the egui canvas app, today's production UI.
#   dioxus  the Dioxus DOM UI in web/ (`dx bundle`), shipped after cut-over (#67).
# Cut-over is changing the default below; both produce the same dist/ layout for
# package-cf-output.mjs, sw.js and _headers.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
web_ui="${OXFER_WEB_UI:-egui}"
case "$web_ui" in
    egui | dioxus) ;;
    *)
        echo "OXFER_WEB_UI must be egui or dioxus, not '$web_ui'." >&2
        exit 1
        ;;
esac

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

build_egui() {
    # Use the prebuilt musl binary to avoid compiling Trunk during each web build.
    local trunk_version=0.21.14
    local trunk_archive=trunk-x86_64-unknown-linux-musl.tar.gz
    local trunk_sha256=a67f4054b249fe9acc5fabc25de1aebf19783aca3ad6ff64bf34d7da44d0ea20
    curl --proto '=https' --tlsv1.2 --fail --show-error --location --retry 3 \
        "https://github.com/trunk-rs/trunk/releases/download/v${trunk_version}/${trunk_archive}" \
        -o "$tools/$trunk_archive"
    echo "$trunk_sha256  $tools/$trunk_archive" | sha256sum --check
    tar -xzf "$tools/$trunk_archive" -C "$tools" trunk

    "$tools/trunk" build --release --locked --public-url /
}

build_dioxus() {
    # Prebuilt dx; it fetches its own pinned wasm-bindgen, wasm-opt and esbuild.
    local dx_version=0.7.10
    local dx_archive=dx-x86_64-unknown-linux-gnu.tar.gz
    local dx_sha256=4363e4ed2a3f1eb7f4d38d2d59aed59ce43271c44c16b425e92c89a64761fbe7
    curl --proto '=https' --tlsv1.2 --fail --show-error --location --retry 3 \
        "https://github.com/DioxusLabs/dioxus/releases/download/v${dx_version}/${dx_archive}" \
        -o "$tools/$dx_archive"
    echo "$dx_sha256  $tools/$dx_archive" | sha256sum --check
    tar -xzf "$tools/$dx_archive" -C "$tools" dx

    # dx keeps earlier hashed bundles in its output directory: start from an empty one.
    local target_dir
    target_dir="$(cargo metadata --format-version 1 --no-deps | node -e \
        'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"
    local web_out="$target_dir/dx/oxfer-web/release/web"
    rm -rf "$web_out" dist
    DO_NOT_TRACK=1 "$tools/dx" bundle --platform web -p oxfer-web --release --locked
    cp -R "$web_out/public" dist

    # The same static files Trunk copies (index.html's copy-file links), minus the
    # egui-only canvas helpers (keyboard-focus.js, theme.js).
    mkdir -p dist/assets
    cp assets/sw.js assets/_headers assets/llms.txt agent/oxfer-agent.mjs theme.html agents-demo.html dist/
    cp -R assets/demo dist/assets/demo
    cp assets/oxfer-favicon-light.svg assets/oxfer-favicon-dark.svg assets/favicon.js \
        assets/oxfer-ox.svg assets/oxfer-mark.svg assets/oxfer-wordmark-light.svg \
        assets/oxfer-wordmark-dark.svg assets/diagnostics.js assets/manifest.json \
        assets/icon-1024.png assets/icon-256.png assets/icon_ios_touch_192.png \
        assets/maskable_icon_x512.png dist/assets/
}

"build_$web_ui"

# Cloudflare limits each static asset to 25 MiB, including the WebAssembly binary.
oversized="$(find dist -type f -size +26214400c -print)"
if [[ -n "$oversized" ]]; then
    printf 'Assets exceed the Cloudflare 25 MiB limit:\n%s\n' "$oversized" >&2
    exit 1
fi
test -s dist/index.html
test -s dist/sw.js
cmp assets/_headers dist/_headers
if [[ "$web_ui" == dioxus ]]; then
    # Budget: 2 MiB of brotli wasm (Cloudflare serves brotli). Raise it deliberately.
    node report-bundle-size.mjs dist --budget-brotli 2097152
else
    node report-bundle-size.mjs dist
fi
node package-cf-output.mjs
