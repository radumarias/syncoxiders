//! Oxfer's web UI: Dioxus components over the `p2p_transfer::sessions` core.
//!
//! This crate renders and binds; it holds no transfer, retry or session logic. Each
//! screen reads the core's [`Snapshot`](p2p_transfer::sessions::Snapshot) from a signal
//! and sends [`Intent`](p2p_transfer::sessions::Intent)s back through [`core::Core`].
//!
//! Module map for contributors:
//! - [`core`]: the `Oxfer` handle, its snapshot signal and the refresh loop.
//! - [`route`]: path routes. The URL fragment is the share capability and is never
//!   parsed, logged or rewritten here; the core alone reads and scrubs it.
//! - [`appearance`]: theme and light/dark, applied as `data-theme` / `data-mode` on the
//!   root element over CSS variables generated from `p2p_transfer::theme`.
//! - [`screens`]: one component per route, plus the shared header and widgets.

#![warn(clippy::all, rust_2018_idioms)]

#[cfg(target_arch = "wasm32")]
mod appearance;
#[cfg(target_arch = "wasm32")]
mod core;
#[cfg(target_arch = "wasm32")]
mod route;
#[cfg(target_arch = "wasm32")]
mod screens;

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    p2p_transfer::init_logging();
    dioxus::launch(screens::App);
}

/// The UI is browser-only; this keeps `cargo build --all` working on native targets.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "oxfer-web is the browser UI. Build it with `dx build --platform web -p oxfer-web` \
         from p2p-transfer/, or run the native app with `cargo run --release`."
    );
}
