use crate::tree_creator::HashKind;
use anyhow::Result;
use crc32fast::Hasher;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub mod apply_change;
pub mod change_tree;
pub mod change_tree_merge;
pub mod path_walker;
pub mod tree_creator;

pub const MNT_DIR: &str = "mnt";
pub const REPO_DIR: &str = "repo";
pub const TREE_DIR: &str = "tree";

pub trait IterRef {
    /// The type of the elements being iterated over.
    type Item;

    /// Which kind of iterator are we turning this into?
    type Iter: Iterator<Item = Self::Item>;

    /// Creates an iterator from a value.
    fn iter(&self) -> Self::Iter;
}

pub fn hash_eq(h1: &Option<(HashKind, String)>, h2: &Option<(HashKind, String)>) -> bool {
    if let Some(hash_src) = h1 {
        if let Some(hash_dst) = h2 {
            if hash_src.0 == hash_dst.0 && hash_src.1 == hash_dst.1 {
                return true;
            }
        }
    }
    false
}

pub fn crc_eq(src: &Path, dst: &Path) -> Result<bool> {
    let src_crc = crc(src)?;
    let dst_crc = crc(dst)?;
    Ok(src_crc == dst_crc)
}

pub fn crc(path: &Path) -> Result<u32> {
    // Open the file
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Create a CRC32 hasher
    let mut hasher = Hasher::new();
    let mut buffer = [0u8; 4096];

    // Read the file in chunks and update the hasher
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    // Finalize the hasher to get the checksum
    let checksum = hasher.finalize();

    Ok(checksum)
}
