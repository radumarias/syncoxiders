# OSS Scanner build for p2p-transfer (https://github.com/anthropics/oss-scanner).
#
# The scanner builds this with the repository root as the context and with network access, then studies the
# finished image offline. Only the p2p-transfer crate is built; the other workspace members are out of scope
# (see threat_model.md next to this file). Everything the build and the tests need is fetched here.

# p2p-transfer/package.json needs Node >= 22, newer than Debian's; the JS tests use only Node built-ins.
FROM node:22-trixie-slim AS node

FROM debian:trixie
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV DEBIAN_FRONTEND=noninteractive
# Pinned Debian package versions break on every point release. clang and llvm-ar compile ring's C code for
# wasm32, which gcc cannot target.
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git build-essential clang llvm pkg-config firefox-esr \
    && rm -rf /var/lib/apt/lists/*
COPY --from=node /usr/local/bin/node /usr/local/bin/node

# Install the toolchain p2p-transfer/rust-toolchain pins and make it the default, so native cargo commands work
# from the workspace root too. wasm32 builds belong in p2p-transfer/, whose .cargo/config.toml sets their flags.
ENV PATH=/root/.cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
COPY p2p-transfer/rust-toolchain /src/p2p-transfer/rust-toolchain
WORKDIR /src/p2p-transfer
RUN rustup toolchain install \
    && rustup default "$(rustup show active-toolchain | cut -d' ' -f1)"

# Browser tooling, at the trunk and wasm-pack versions build-web.sh and CI use. A geckodriver on PATH stops
# wasm-pack from downloading one, which it could not do offline.
RUN cargo install --locked trunk@0.21.14 wasm-pack@0.13.1 geckodriver@0.37.1

# trunk and wasm-pack need a wasm-bindgen CLI of exactly the version in Cargo.lock. With one on PATH they use it
# instead of downloading it. Only the lockfile is copied first, so editing the sources does not rebuild it.
COPY Cargo.lock /src/Cargo.lock
RUN cargo install --locked \
      "wasm-bindgen-cli@$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p}' /src/Cargo.lock)"

COPY . /src
# Every crate the lockfile names, for every target (native and wasm32).
RUN cargo fetch --locked

# The native crate with its tests, the wasm32 library check ./check.sh runs, and the browser app, in the dev
# profile. trunk stays in debug: a release build would download wasm-opt.
RUN cargo build --locked -p p2p-transfer --all-targets --all-features
RUN cargo check --locked -p p2p-transfer --all-features --lib --target wasm32-unknown-unknown
RUN trunk build --locked

# Run the cheap tests so the image is known to work, without letting a failing test abort the build.
RUN cargo test --locked -p p2p-transfer --all-features --no-fail-fast \
    || echo "WARNING: cargo test reported failures"
RUN node --test tests/*.test.mjs || echo "WARNING: node --test reported failures"
# Real headless Firefox runs of every tests/*_wasm.rs: the WebRTC transport, relay selection and the persistent
# resume store.
RUN for file in tests/*_wasm.rs; do \
      name="$(basename "$file" .rs)"; \
      wasm-pack test --headless --firefox -- --test "$name" || echo "WARNING: wasm-pack test $name failed"; \
    done

ENV CARGO_NET_OFFLINE=true TRUNK_OFFLINE=true
