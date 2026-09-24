#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod blob_store;
#[cfg(target_arch = "wasm32")]
mod diagnostics;
pub mod file_io;
pub mod logging;
pub mod node;
pub mod protocol;
pub mod transfer;
#[cfg(target_arch = "wasm32")]
pub mod webrtc;

// The unit suite uses native filesystem and multi-thread Tokio test helpers. Browser-specific
// integration tests live in `tests/*_wasm.rs` and run through wasm-bindgen-test.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

pub use app::P2PTransfer;
pub use blob_store::{Blob, BlobCollection, BlobHash, BlobInfo, BlobStore};
pub use logging::init_logging;

/// The exact source revision baked into this binary or browser bundle.
pub const BUILD_REVISION: &str = env!("OXFER_GIT_REVISION");
/// ISO 8601 commit date and time (with timezone), stable for this revision.
pub const BUILD_COMMITTED_AT: &str = env!("OXFER_COMMITTED_AT");
/// Visible identity of the running build, independent of the hosting site's cache.
pub const BUILD_LABEL: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION"),
    " · ",
    env!("OXFER_GIT_REVISION")
);

#[cfg(not(target_arch = "wasm32"))]
pub use blob_store::{BaoBlob, BaoReceiver, BaoStore, BLOCK_SIZE};
