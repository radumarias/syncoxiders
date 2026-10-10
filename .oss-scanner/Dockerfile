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
# Pinned Debian package versions break on every point release.
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git build-essential pkg-config firefox-esr \
    && rm -rf /var/lib/apt/lists/*
COPY --from=node /usr/local/bin/node /usr/local/bin/node

# The toolchain p2p-transfer/rust-toolchain pins (nightly with rustfmt, clippy and wasm32-unknown-unknown). It is
# also made the default, so cargo uses the same compiler when run from the workspace root.
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

COPY . /src
# trunk and wasm-pack need a wasm-bindgen CLI of exactly the version in Cargo.lock. With one on PATH they use it
# instead of downloading it.
RUN cargo install --locked wasm-bindgen-cli --version \
      "$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p}' /src/Cargo.lock)"
# Every crate the lockfile names, for every target (native and wasm32).
RUN cargo fetch --locked

# The native crate and its tests, and the browser app, built as ./check.sh builds them (debug, with debug info).
# trunk stays in debug: a release build would download wasm-opt.
RUN cargo build --locked -p p2p-transfer --all-targets
RUN trunk build --locked

# Run the cheap tests so the image is known to work, without letting a failing test abort the build.
# online_test_* need the public iroh relay, which the scan cannot reach.
RUN (cargo test --locked -p p2p-transfer --all-targets -- --skip online_test_ \
      && cargo test --locked -p p2p-transfer --doc) \
    || echo "WARNING: cargo test reported failures"
RUN node --test tests/*.test.mjs || echo "WARNING: node --test reported failures"
# Real headless Firefox runs of the WebRTC transport, relay selection and the persistent resume store.
RUN for test in webrtc_wasm relay_wasm resume_wasm; do \
      wasm-pack test --headless --firefox -- --test "$test" || echo "WARNING: wasm-pack test $test failed"; \
    done

ENV CARGO_NET_OFFLINE=true
