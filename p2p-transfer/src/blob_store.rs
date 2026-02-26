//! Content-addressed blob storage using BLAKE3 Bao tree hashing.

use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;

/// A BLAKE3 hash representing content-addressed data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobHash(pub [u8; 32]);

impl BlobHash {
    /// Create a hash from bytes using BLAKE3
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let hash = blake3::hash(bytes);
        BlobHash(*hash.as_bytes())
    }

    /// Convert to hex string for display/sharing
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            anyhow::bail!("Invalid hash length: expected 32 bytes, got {}", bytes.len());
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(BlobHash(arr))
    }
}

impl std::fmt::Display for BlobHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Metadata about a stored blob
#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub hash: BlobHash,
    pub size: u64,
    pub name: String,
}

/// A blob with its data and metadata
#[derive(Debug, Clone)]
pub struct Blob {
    pub info: BlobInfo,
    pub data: Vec<u8>,
}

impl Blob {
    /// Create a new blob from data and name
    pub fn new(name: String, data: Vec<u8>) -> Self {
        let hash = BlobHash::from_bytes(&data);
        let size = data.len() as u64;
        Blob {
            info: BlobInfo { hash, size, name },
            data,
        }
    }

    /// Get the content hash
    pub fn hash(&self) -> BlobHash {
        self.info.hash
    }

    /// Verify data integrity
    pub fn verify(&self) -> bool {
        BlobHash::from_bytes(&self.data) == self.info.hash
    }
}

/// In-memory blob store for managing content-addressed data
#[derive(Debug, Clone, Default)]
pub struct BlobStore {
    blobs: Arc<std::sync::Mutex<HashMap<BlobHash, Blob>>>,
}

impl BlobStore {
    /// Create a new empty blob store
    pub fn new() -> Self {
        BlobStore {
            blobs: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Add a blob to the store, returns its hash
    pub fn add(&self, name: String, data: Vec<u8>) -> BlobHash {
        let blob = Blob::new(name, data);
        let hash = blob.hash();
        if let Ok(mut store) = self.blobs.lock() {
            store.insert(hash, blob);
        }
        hash
    }

    /// Get a blob by its hash
    pub fn get(&self, hash: &BlobHash) -> Option<Blob> {
        self.blobs.lock().ok()?.get(hash).cloned()
    }

    /// Check if a blob exists
    pub fn contains(&self, hash: &BlobHash) -> bool {
        self.blobs.lock().ok().map(|s| s.contains_key(hash)).unwrap_or(false)
    }

    /// List all blobs in the store
    pub fn list(&self) -> Vec<BlobInfo> {
        self.blobs
            .lock()
            .ok()
            .map(|s| s.values().map(|b| b.info.clone()).collect())
            .unwrap_or_default()
    }

    /// Remove a blob by hash
    pub fn remove(&self, hash: &BlobHash) -> Option<Blob> {
        self.blobs.lock().ok()?.remove(hash)
    }

    /// Clear all blobs
    pub fn clear(&self) {
        if let Ok(mut store) = self.blobs.lock() {
            store.clear();
        }
    }

    /// Get total size of all blobs
    pub fn total_size(&self) -> u64 {
        self.blobs
            .lock()
            .ok()
            .map(|s| s.values().map(|b| b.info.size).sum())
            .unwrap_or(0)
    }

    /// Get all blobs as (name, data) pairs for sharing
    pub fn get_all_files(&self) -> Vec<(String, Vec<u8>)> {
        self.blobs
            .lock()
            .ok()
            .map(|s| s.values().map(|b| (b.info.name.clone(), b.data.clone())).collect())
            .unwrap_or_default()
    }
}

/// Native Bao tree for BitTorrent-like verified streaming
#[cfg(not(target_arch = "wasm32"))]
pub mod bao {
    use super::*;
    use bao_tree::{
        BaoTree,
        BlockSize,
        io::outboard::PreOrderMemOutboard,
    };

    /// Block size for Bao tree (16KB chunks like iroh-blobs)
    pub const BLOCK_SIZE: BlockSize = BlockSize::from_chunk_log(4); // 2^4 * 1024 = 16KB

    /// A Bao-encoded blob with merkle tree outboard
    #[derive(Debug, Clone)]
    pub struct BaoBlob {
        /// The content hash (root of Bao tree)
        pub hash: BlobHash,
        /// Original data
        pub data: Vec<u8>,
        /// Bao tree outboard (merkle tree nodes)
        pub outboard: Vec<u8>,
        /// File name
        pub name: String,
    }

    impl BaoBlob {
        /// Create a new Bao-encoded blob
        pub fn new(name: String, data: Vec<u8>) -> Self {
            // Compute hash and outboard using PreOrderMemOutboard
            let outboard = PreOrderMemOutboard::create(&data, BLOCK_SIZE);
            let hash = BlobHash(*outboard.root.as_bytes());
            let outboard_data = outboard.data.clone();
            let chunk_count = outboard.tree.chunks().0;

            log::info!(
                "🌳 Created Bao tree for '{}': {} chunks, {} bytes outboard",
                name,
                chunk_count,
                outboard_data.len()
            );

            BaoBlob {
                hash,
                data,
                outboard: outboard_data,
                name,
            }
        }

        /// Get the number of chunks in this blob
        pub fn chunk_count(&self) -> u64 {
            let tree = BaoTree::new(self.data.len() as u64, BLOCK_SIZE);
            tree.chunks().0
        }

        /// Get a specific chunk by index (0-based)
        pub fn get_chunk(&self, index: u64) -> Option<Vec<u8>> {
            let chunk_size = BLOCK_SIZE.bytes() as u64;
            let start = index * chunk_size;
            if start >= self.data.len() as u64 {
                return None;
            }
            let end = std::cmp::min(start + chunk_size, self.data.len() as u64);
            Some(self.data[start as usize..end as usize].to_vec())
        }

        /// Verify a chunk against the Bao tree
        pub fn verify_chunk(&self, index: u64, chunk_data: &[u8]) -> bool {
            let chunk_size = BLOCK_SIZE.bytes() as u64;
            let start = index * chunk_size;
            let end = std::cmp::min(start + chunk_size as u64, self.data.len() as u64);

            if start >= self.data.len() as u64 {
                return false;
            }

            let expected = &self.data[start as usize..end as usize];
            chunk_data == expected
        }

        /// Get the chunk hash for streaming verification
        pub fn chunk_hash(&self, index: u64) -> Option<BlobHash> {
            self.get_chunk(index).map(|c| BlobHash::from_bytes(&c))
        }
    }

    /// Bao tree store for managing content-addressed blobs with merkle trees
    #[derive(Debug, Clone, Default)]
    pub struct BaoStore {
        blobs: Arc<std::sync::Mutex<HashMap<BlobHash, BaoBlob>>>,
    }

    impl BaoStore {
        /// Create a new Bao store
        pub fn new() -> Self {
            BaoStore {
                blobs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            }
        }

        /// Add data to the store, creating Bao tree
        pub fn add(&self, name: String, data: Vec<u8>) -> BaoBlob {
            let blob = BaoBlob::new(name, data);
            let result = blob.clone();
            if let Ok(mut store) = self.blobs.lock() {
                store.insert(blob.hash, blob);
            }
            result
        }

        /// Get a Bao blob by hash
        pub fn get(&self, hash: &BlobHash) -> Option<BaoBlob> {
            self.blobs.lock().ok()?.get(hash).cloned()
        }

        /// Check if blob exists
        pub fn contains(&self, hash: &BlobHash) -> bool {
            self.blobs.lock().ok().map(|s| s.contains_key(hash)).unwrap_or(false)
        }

        /// List all blobs
        pub fn list(&self) -> Vec<BlobInfo> {
            self.blobs
                .lock()
                .ok()
                .map(|s| s.values().map(|b| BlobInfo {
                    hash: b.hash,
                    size: b.data.len() as u64,
                    name: b.name.clone(),
                }).collect())
                .unwrap_or_default()
        }

        /// Get total number of chunks across all blobs
        pub fn total_chunks(&self) -> u64 {
            self.blobs
                .lock()
                .ok()
                .map(|s| s.values().map(|b| b.chunk_count()).sum())
                .unwrap_or(0)
        }
    }

    /// Receiver-side chunk assembler with verification
    #[derive(Debug)]
    pub struct BaoReceiver {
        /// Expected content hash
        pub expected_hash: BlobHash,
        /// Expected total size
        pub expected_size: u64,
        /// Received chunks (index -> data)
        pub chunks: HashMap<u64, Vec<u8>>,
        /// Number of chunks expected
        pub total_chunks: u64,
    }

    impl BaoReceiver {
        /// Create a new receiver for a blob
        pub fn new(expected_hash: BlobHash, expected_size: u64) -> Self {
            let chunk_size = BLOCK_SIZE.bytes() as u64;
            let total_chunks = (expected_size + chunk_size - 1) / chunk_size;
            BaoReceiver {
                expected_hash,
                expected_size,
                chunks: HashMap::new(),
                total_chunks,
            }
        }

        /// Add a received chunk
        pub fn add_chunk(&mut self, index: u64, data: Vec<u8>) -> bool {
            if index >= self.total_chunks {
                return false;
            }
            self.chunks.insert(index, data);
            true
        }

        /// Check if all chunks received
        pub fn is_complete(&self) -> bool {
            self.chunks.len() as u64 == self.total_chunks
        }

        /// Get missing chunk indices
        pub fn missing_chunks(&self) -> Vec<u64> {
            (0..self.total_chunks)
                .filter(|i| !self.chunks.contains_key(i))
                .collect()
        }

        /// Progress as percentage
        pub fn progress(&self) -> f32 {
            if self.total_chunks == 0 {
                return 100.0;
            }
            (self.chunks.len() as f32 / self.total_chunks as f32) * 100.0
        }

        /// Assemble and verify the complete blob
        pub fn assemble_and_verify(&self) -> Result<Vec<u8>> {
            if !self.is_complete() {
                anyhow::bail!("Missing {} chunks", self.missing_chunks().len());
            }

            // Assemble in order
            let mut data = Vec::with_capacity(self.expected_size as usize);
            for i in 0..self.total_chunks {
                if let Some(chunk) = self.chunks.get(&i) {
                    data.extend_from_slice(chunk);
                } else {
                    anyhow::bail!("Missing chunk {}", i);
                }
            }

            // Truncate to exact size (last chunk may be padded)
            data.truncate(self.expected_size as usize);

            // Verify final hash
            let computed_hash = BlobHash::from_bytes(&data);
            if computed_hash != self.expected_hash {
                anyhow::bail!(
                    "Hash mismatch: expected {}, got {}",
                    self.expected_hash,
                    computed_hash
                );
            }

            Ok(data)
        }
    }
}

/// Re-export Bao types on native
#[cfg(not(target_arch = "wasm32"))]
pub use bao::{BaoBlob, BaoStore, BaoReceiver, BLOCK_SIZE};

/// Collection of blob hashes for a transfer session
#[derive(Debug, Clone, Default)]
pub struct BlobCollection {
    pub hashes: Vec<BlobHash>,
    pub names: HashMap<BlobHash, String>,
}

impl BlobCollection {
    pub fn new() -> Self {
        BlobCollection {
            hashes: Vec::new(),
            names: HashMap::new(),
        }
    }

    pub fn add(&mut self, hash: BlobHash, name: String) {
        self.hashes.push(hash);
        self.names.insert(hash, name);
    }

    /// Generate a collection hash (hash of all hashes) for sharing
    pub fn collection_hash(&self) -> BlobHash {
        let mut all_hashes = Vec::new();
        for hash in &self.hashes {
            all_hashes.extend_from_slice(&hash.0);
        }
        BlobHash::from_bytes(&all_hashes)
    }

    /// Encode collection as shareable string (list of hashes)
    pub fn to_share_string(&self) -> String {
        self.hashes
            .iter()
            .map(|h| {
                let name = self.names.get(h).map(|s| s.as_str()).unwrap_or("unknown");
                format!("{}:{}", h.to_hex(), name)
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse from share string
    pub fn from_share_string(s: &str) -> Result<Self> {
        let mut collection = BlobCollection::new();
        for part in s.split(',') {
            let parts: Vec<&str> = part.splitn(2, ':').collect();
            if parts.is_empty() {
                continue;
            }
            let hash = BlobHash::from_hex(parts[0])?;
            let name = parts.get(1).unwrap_or(&"unknown").to_string();
            collection.add(hash, name);
        }
        Ok(collection)
    }
}

// Re-export hex for convenience
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(data: &[u8]) -> String {
        let mut result = String::with_capacity(data.len() * 2);
        for byte in data {
            result.push(HEX_CHARS[(byte >> 4) as usize] as char);
            result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
        }
        result
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, anyhow::Error> {
        if s.len() % 2 != 0 {
            anyhow::bail!("Invalid hex string length");
        }
        let mut result = Vec::with_capacity(s.len() / 2);
        for i in (0..s.len()).step_by(2) {
            let high = hex_char_to_nibble(s.as_bytes()[i])?;
            let low = hex_char_to_nibble(s.as_bytes()[i + 1])?;
            result.push((high << 4) | low);
        }
        Ok(result)
    }

    fn hex_char_to_nibble(c: u8) -> Result<u8, anyhow::Error> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => anyhow::bail!("Invalid hex character"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_hash() {
        let data = b"Hello, World!";
        let hash1 = BlobHash::from_bytes(data);
        let hash2 = BlobHash::from_bytes(data);
        assert_eq!(hash1, hash2);

        let hex = hash1.to_hex();
        let parsed = BlobHash::from_hex(&hex).unwrap();
        assert_eq!(hash1, parsed);
    }

    #[test]
    fn test_blob_store() {
        let store = BlobStore::new();
        let hash = store.add("test.txt".to_string(), b"test data".to_vec());

        assert!(store.contains(&hash));

        let blob = store.get(&hash).unwrap();
        assert_eq!(blob.info.name, "test.txt");
        assert!(blob.verify());
    }

    #[test]
    fn test_blob_collection() {
        let mut collection = BlobCollection::new();
        let hash1 = BlobHash::from_bytes(b"file1");
        let hash2 = BlobHash::from_bytes(b"file2");

        collection.add(hash1, "file1.txt".to_string());
        collection.add(hash2, "file2.txt".to_string());

        let share_string = collection.to_share_string();
        let parsed = BlobCollection::from_share_string(&share_string).unwrap();

        assert_eq!(parsed.hashes.len(), 2);
        assert_eq!(parsed.names.get(&hash1), Some(&"file1.txt".to_string()));
    }
}
