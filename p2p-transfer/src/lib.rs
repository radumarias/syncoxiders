#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod blob_store;
pub mod file_io;
pub mod logging;
pub mod node;
pub mod protocol;
pub mod transfer;
#[cfg(target_arch = "wasm32")]
pub mod webrtc;

#[cfg(test)]
mod tests;

pub use app::P2PTransfer;
pub use blob_store::{Blob, BlobCollection, BlobHash, BlobInfo, BlobStore};
pub use logging::init_logging;

#[cfg(not(target_arch = "wasm32"))]
pub use blob_store::{BaoBlob, BaoReceiver, BaoStore, BLOCK_SIZE};
