#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod blob_store;
mod node;

#[cfg(test)]
mod tests;

pub use app::P2PTransfer;
pub use blob_store::{Blob, BlobCollection, BlobHash, BlobInfo, BlobStore};

#[cfg(not(target_arch = "wasm32"))]
pub use blob_store::{
    BaoBlob,
    BaoStore,
    BaoReceiver,
    BLOCK_SIZE,
};
