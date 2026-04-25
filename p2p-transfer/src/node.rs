use iroh::endpoint::Connection;
use iroh::{Endpoint, NodeId, TransportMode};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use anyhow::Result;
use async_channel::{Sender, Receiver, unbounded};
use log::info;
use n0_future::boxed::BoxFuture;
use n0_future::{task, Stream};
use std::sync::atomic::AtomicUsize;
use crate::blob_store::{BlobStore, BlobHash, BlobCollection};

// Native: Import Bao tree for BitTorrent-like verified streaming
#[cfg(not(target_arch = "wasm32"))]
use crate::blob_store::{BaoStore, BaoBlob};

pub struct EchoNode{
    router: Router,
    accept_events: Sender<AcceptEvent>,
    shared_files: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>,
    blob_store: BlobStore,
    /// Native only: Bao tree store for BitTorrent-like merkle verification
    #[cfg(not(target_arch = "wasm32"))]
    bao_store: Option<BaoStore>,
}

impl EchoNode {
    pub async fn spawn() -> Result<Self> {
        Self::spawn_with_files(Vec::new()).await
    }

    pub async fn spawn_with_files(files: Vec<(String, Vec<u8>)>) -> Result<Self> {
        // Create blob store and add files
        let blob_store = BlobStore::new();
        for (name, data) in &files {
            let hash = blob_store.add(name.clone(), data.clone());
            info!("📦 Added blob: {} -> {}", name, hash);
        }

        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![Echo::ALPN.to_vec()])
            .bind_transport(TransportMode::WebrtcRelay)
            .await?;
        let (event_sender, _event_receiver) = unbounded();
        let echo = Echo::new(event_sender.clone(), files, blob_store.clone());
        let shared_files = echo.files.clone();
        let router = Router::builder(endpoint)
            .accept(Echo::ALPN, echo)
            .spawn();
        Ok(Self {
            router,
            accept_events: event_sender,
            shared_files,
            blob_store,
            #[cfg(not(target_arch = "wasm32"))]
            bao_store: None,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn spawn_with_bao(files: Vec<(String, Vec<u8>)>) -> Result<Self> {
        // Create both stores
        let blob_store = BlobStore::new();
        let bao_store = BaoStore::new();

        // Add files to both stores
        for (name, data) in &files {
            // Simple blob store
            let hash = blob_store.add(name.clone(), data.clone());
            info!("📦 Added blob: {} -> {}", name, hash);

            // Bao tree store (BitTorrent-like merkle tree)
            let bao_blob = bao_store.add(name.clone(), data.clone());
            info!("🌳 Added to Bao tree: {} -> {} ({} chunks)",
                  name, bao_blob.hash, bao_blob.chunk_count());
        }

        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![Echo::ALPN.to_vec()])
            .bind_transport(TransportMode::WebrtcRelay)
            .await?;

        let (event_sender, _event_receiver) = unbounded();
        let echo = Echo::new(event_sender.clone(), files, blob_store.clone());
        let shared_files = echo.files.clone();

        let router = Router::builder(endpoint)
            .accept(Echo::ALPN, echo)
            .spawn();

        info!("🚀 Node started with Bao tree (BitTorrent-like) support");
        info!("   Echo protocol: {:?}", String::from_utf8_lossy(Echo::ALPN));

        Ok(Self {
            router,
            accept_events: event_sender,
            shared_files,
            blob_store,
            bao_store: Some(bao_store),
        })
    }

    /// Create a node with blob store for content-addressed storage
    pub async fn spawn_with_blob_store(blob_store: BlobStore) -> Result<Self> {
        let files = blob_store.get_all_files();
        let endpoint = Endpoint::builder()
            .discovery_n0()
            .alpns(vec![Echo::ALPN.to_vec()])
            .bind_transport(TransportMode::WebrtcRelay)
            .await?;
        let (event_sender, _event_receiver) = unbounded();
        let echo = Echo::new(event_sender.clone(), files, blob_store.clone());
        let shared_files = echo.files.clone();
        let router = Router::builder(endpoint)
            .accept(Echo::ALPN, echo)
            .spawn();
        Ok(Self {
            router,
            accept_events: event_sender,
            shared_files,
            blob_store,
            #[cfg(not(target_arch = "wasm32"))]
            bao_store: None,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn get_shared_files(&self) -> std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>> {
        self.shared_files.clone()
    }

    /// Get the blob store for content-addressed operations
    pub fn blob_store(&self) -> &BlobStore {
        &self.blob_store
    }

    /// Add a file to the blob store and return its hash
    pub fn add_blob(&self, name: String, data: Vec<u8>) -> BlobHash {
        let hash = self.blob_store.add(name.clone(), data.clone());
        // Also update the shared files list for compatibility
        if let Ok(mut files) = self.shared_files.lock() {
            files.push((name, data));
        }
        hash
    }

    /// Get a collection of all blob hashes for sharing
    pub fn get_blob_collection(&self) -> BlobCollection {
        let mut collection = BlobCollection::new();
        for info in self.blob_store.list() {
            collection.add(info.hash, info.name);
        }
        collection
    }

    /// Generate a shareable hash string for all blobs
    pub fn get_share_string(&self) -> String {
        self.get_blob_collection().to_share_string()
    }

    /// Native only: Get the Bao tree store
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bao_store(&self) -> Option<&BaoStore> {
        self.bao_store.as_ref()
    }

    /// Native only: Add a file to Bao store (with merkle tree)
    ///
    /// Returns the BaoBlob which contains the root hash and can be used for
    /// BitTorrent-like chunk verification
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_to_bao(&self, name: String, data: Vec<u8>) -> Option<BaoBlob> {
        self.bao_store.as_ref().map(|store| store.add(name, data))
    }

    /// Native only: Get a BaoBlob by its root hash
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_bao_blob(&self, hash: &BlobHash) -> Option<BaoBlob> {
        self.bao_store.as_ref()?.get(hash)
    }

    /// Native only: Extract chunks from a BaoBlob for streaming transfer
    ///
    /// Returns a vector of (chunk_index, chunk_data) pairs that can be
    /// sent over the network and verified on the receiving end
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_bao_chunks(&self, hash: &BlobHash) -> Option<Vec<(usize, Vec<u8>)>> {
        let blob = self.bao_store.as_ref()?.get(hash)?;
        let count = blob.chunk_count() as usize;
        let chunks: Vec<_> = (0..count)
            .filter_map(|i| blob.get_chunk(i as u64).map(|c| (i, c)))
            .collect();
        Some(chunks)
    }

    pub fn subscribe_accept_events(&self) -> Receiver<AcceptEvent> {
        let (tx, rx) = unbounded();
        let _main_sender = self.accept_events.clone();
        rx
    }

    pub fn connect(
        &self,
        node_id: NodeId,
        file_data: Vec<u8>,
        file_name: String
    ) -> impl Stream<Item = ConnectEvent> + Unpin {

        let (event_sender, event_receiver) = async_channel::bounded(16);
        let endpoint = self.router.endpoint().clone();
        task::spawn(async move {
            let res = connect(&endpoint, node_id, file_data, file_name, event_sender.clone()).await;
            let error = res.as_ref().err().map(|e| e.to_string());
            event_sender.send(ConnectEvent::Closed {error}).await.ok();
        });
        Box::pin(event_receiver)
    }
}

#[derive(Debug)]
pub enum TransferEvent {
    FileStart {
        file_name: String,
        file_size: u64,
        total_chunks: u32,
        /// BLAKE3 hash of the file content (for verification)
        blob_hash: Option<String>,
    },
    ChunkReceived {
        file_name: String,
        chunk_index: u32,
        chunk_data: Vec<u8>,
        offset: u64,
    },
    FileComplete {
        file_name: String,
        total_bytes: u64,
        /// Whether the hash verification passed (None if not verified)
        hash_verified: Option<bool>,
    },
}

#[derive(Debug)]
pub enum ConnectEvent {
    Connected,
    Sent {bytes_sent: u64},
    Transfer(TransferEvent),
    Closed {error: Option<String>}
}

#[derive(Debug, Clone)]
pub enum AcceptEvent {

    Accepted {
        node_id: NodeId,
    },
    Echoed {
        node_id: NodeId,
        bytes_sent: u64
    },
    Closed {
        node_id: NodeId,
        error: Option<String>
    }
}

#[derive(Debug, Clone)]
pub struct Echo{
    event_sender: Sender<AcceptEvent>,
    files: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>, // (filename, filedata)
    current_file_index: std::sync::Arc<AtomicUsize>, // Round-robin index
    blob_store: BlobStore,
}

impl Echo{
    pub const ALPN: &[u8] = b"iroh/example-browser-echo/0";
    pub fn new(
        event_sender: Sender<AcceptEvent>,
        files: Vec<(String, Vec<u8>)>,
        blob_store: BlobStore,
    ) -> Self {

        Self {
            event_sender,
            files: std::sync::Arc::new(std::sync::Mutex::new(files)),
            current_file_index: std::sync::Arc::new(AtomicUsize::new(0)),
            blob_store,
        }

    }
}impl Echo {
    async fn handle_connection(self, connection: Connection) -> std::result::Result<(), AcceptError> {

        let node_id  = connection.remote_node_id()?;
        self.event_sender.try_send(AcceptEvent::Accepted {node_id }).ok();
        let res = self.handle_connection_0(&connection).await;
        let error = res.as_ref().err().map(|err| err.to_string());
        self.event_sender.try_send(AcceptEvent::Closed {node_id, error}).ok();
        res


    }

    async fn handle_connection_0(&self, connection: &Connection) -> std::result::Result<(), AcceptError> {
        const CHUNK_SIZE: usize = 256 * 1024;

        let node_id = connection.remote_node_id()?;
        info!("✓ Connection accepted from {}", node_id);
        info!("⏳ Opening bidirectional stream...");

        let (mut send, mut recv) = connection.accept_bi().await?;
        info!("✓ Bidirectional stream established");

        // Read filename length
        let mut name_len_buf = [0u8; 4];
        recv.read_exact(&mut name_len_buf).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let name_len = u32::from_le_bytes(name_len_buf) as usize;

        // Read filename
        let mut name_buf = vec![0u8; name_len];
        recv.read_exact(&mut name_buf).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let _received_file_name = String::from_utf8(name_buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Read file data length
        let mut data_len_buf = [0u8; 8];
        recv.read_exact(&mut data_len_buf).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let data_len = u64::from_le_bytes(data_len_buf) as usize;

        // Read file data
        let mut _received_file_data = vec![0u8; data_len];
        recv.read_exact(&mut _received_file_data).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        info!("✓ Received connection request from receiver");
        info!("📦 Preparing files to send...");

        let files_to_send = if let Ok(files) = self.files.lock() {
            if !files.is_empty() {
                files.clone()
            } else {
                vec![(_received_file_name, _received_file_data)]
            }
        } else {
            vec![(_received_file_name, _received_file_data)]
        };

        let num_files = files_to_send.len() as u32;
        send.write_all(&num_files.to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        info!("📤 Sending {} file(s)", num_files);

        let mut total_bytes_sent = 4; // for num_files

        // Send all files in chunks with blob hashes
        for (idx, (name, data)) in files_to_send.iter().enumerate() {
            // Compute BLAKE3 hash for content verification
            let blob_hash = BlobHash::from_bytes(data);
            let hash_hex = blob_hash.to_hex();
            info!("📁 [{}/{}] Sending file: {} ({} bytes, hash: {})", idx + 1, num_files, name, data.len(), &hash_hex[..16]);

            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u32;
            let data_len = data.len() as u64;
            let total_chunks = ((data_len + CHUNK_SIZE as u64 - 1) / CHUNK_SIZE as u64) as u32;

            info!("  ⚙️  File will be sent in {} chunk(s) of {}KB each", total_chunks, CHUNK_SIZE / 1024);

            // Send metadata including hash
            send.write_all(&name_len.to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            send.write_all(name_bytes).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            send.write_all(&data_len.to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            send.write_all(&total_chunks.to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            // Send blob hash (32 bytes)
            send.write_all(&blob_hash.0).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            info!("  ✓ Metadata and hash sent");

            total_bytes_sent += 4 + name_bytes.len() + 8 + 4 + 32;

            for chunk_idx in 0..total_chunks {
                let offset = chunk_idx as usize * CHUNK_SIZE;
                let chunk_size = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                let chunk_data = &data[offset..offset + chunk_size];

                send.write_all(&chunk_idx.to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                send.write_all(&(chunk_size as u32).to_le_bytes()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                send.write_all(chunk_data).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                total_bytes_sent += 4 + 4 + chunk_size;
                let progress = ((chunk_idx + 1) as f32 / total_chunks as f32 * 100.0) as u32;
                info!("  📤 Chunk {}/{} sent ({}KB) - {}% complete", chunk_idx + 1, total_chunks, chunk_size / 1024, progress);
            }

            info!("✅ File complete: {} ({} bytes in {} chunks)", name, data.len(), total_chunks);
        }

        let bytes_sent = total_bytes_sent;

        self.event_sender.try_send(AcceptEvent::Echoed {node_id, bytes_sent: bytes_sent as u64}).ok();

        info!("📊 Total bytes sent: {} ({:.2} MB)", bytes_sent, bytes_sent as f64 / 1024.0 / 1024.0);
        send.finish()?;
        info!("🔒 Closing connection with {}", node_id);
        connection.closed().await;
        info!("✓ Connection closed successfully");
        Ok(())


    }
}

impl ProtocolHandler for Echo{
    #[allow(refining_impl_trait)]
    fn accept(&self, connection: Connection) -> BoxFuture<std::result::Result<(), AcceptError>> {
        Box::pin(self.clone().handle_connection(connection))
    }
}

async fn connect(
    endpoint: &Endpoint,
    node_id: NodeId,
    file_data: Vec<u8>,
    file_name: String,
    event_sender: Sender<ConnectEvent>
) -> Result<()>{

    info!("🔗 Initiating connection to node: {}", node_id);
    let connection = endpoint.connect(node_id, Echo::ALPN).await?;
    info!("✓ Connection established with {}", node_id);
    event_sender.send(ConnectEvent::Connected).await?;

    info!("⏳ Opening bidirectional stream...");
    let (mut send_stream , mut recv_stream) = connection.open_bi().await?;
    info!("✓ Bidirectional stream opened");
    let event_sender_clone = event_sender.clone();

    let send_task = task::spawn(async move {
        info!("📤 Sending file request...");
        let name_bytes = file_name.as_bytes();
        let name_len = name_bytes.len() as u32;
        send_stream.write_all(&name_len.to_le_bytes()).await?;

        // Send the filename
        send_stream.write_all(name_bytes).await?;

        // Send the file data length as u64
        let data_len = file_data.len() as u64;
        send_stream.write_all(&data_len.to_le_bytes()).await?;

        // Send the file data
        send_stream.write_all(&file_data).await?;

        let bytes_sent = 4 + name_bytes.len() + 8 + file_data.len();
        info!("✓ Request sent ({} bytes)", bytes_sent);
        event_sender_clone.send(ConnectEvent::Sent {
            bytes_sent: bytes_sent as u64,
        })
            .await?;

        send_stream.finish()?;
        anyhow::Ok(())
    });

    // First, read the number of files
    info!("📥 Waiting for file count...");
    let mut num_files_buf = [0u8; 4];
    recv_stream.read_exact(&mut num_files_buf).await?;
    let num_files = u32::from_le_bytes(num_files_buf) as usize;
    info!("📦 Receiving {} file(s)", num_files);

    for file_idx in 0..num_files {
        info!("📁 [{}/{}] Receiving file metadata...", file_idx + 1, num_files);
        // Read file metadata
        let mut name_len_buf = [0u8; 4];
        recv_stream.read_exact(&mut name_len_buf).await?;
        let name_len = u32::from_le_bytes(name_len_buf) as usize;

        let mut name_buf = vec![0u8; name_len];
        recv_stream.read_exact(&mut name_buf).await?;
        let received_file_name = String::from_utf8(name_buf)?;

        let mut data_len_buf = [0u8; 8];
        recv_stream.read_exact(&mut data_len_buf).await?;
        let data_len = u64::from_le_bytes(data_len_buf);

        let mut total_chunks_buf = [0u8; 4];
        recv_stream.read_exact(&mut total_chunks_buf).await?;
        let total_chunks = u32::from_le_bytes(total_chunks_buf);

        // Read blob hash (32 bytes) for verification
        let mut expected_hash_buf = [0u8; 32];
        recv_stream.read_exact(&mut expected_hash_buf).await?;
        let expected_hash = BlobHash(expected_hash_buf);
        let hash_hex = expected_hash.to_hex();

        info!("  ✓ File: {} ({} bytes, {} chunks, hash: {})", received_file_name, data_len, total_chunks, &hash_hex[..16]);

        event_sender.send(ConnectEvent::Transfer(TransferEvent::FileStart {
            file_name: received_file_name.clone(),
            file_size: data_len,
            total_chunks,
            blob_hash: Some(hash_hex),
        })).await?;

        // Collect all chunks for hash verification
        let mut all_data = Vec::with_capacity(data_len as usize);
        let mut total_bytes_received = 0u64;
        for chunk_num in 0..total_chunks {
            let progress = ((chunk_num + 1) as f32 / total_chunks as f32 * 100.0) as u32;
            let mut chunk_idx_buf = [0u8; 4];
            recv_stream.read_exact(&mut chunk_idx_buf).await?;
            let chunk_index = u32::from_le_bytes(chunk_idx_buf);

            let mut chunk_size_buf = [0u8; 4];
            recv_stream.read_exact(&mut chunk_size_buf).await?;
            let chunk_size = u32::from_le_bytes(chunk_size_buf) as usize;

            let mut chunk_data = vec![0u8; chunk_size];
            recv_stream.read_exact(&mut chunk_data).await?;

            let offset = chunk_index as u64 * 256 * 1024;
            total_bytes_received += chunk_size as u64;
            all_data.extend_from_slice(&chunk_data);

            info!("  📥 Chunk {}/{} received ({}KB) - {}% complete", chunk_num + 1, total_chunks, chunk_size / 1024, progress);

            event_sender.send(ConnectEvent::Transfer(TransferEvent::ChunkReceived {
                file_name: received_file_name.clone(),
                chunk_index,
                chunk_data,
                offset,
            })).await?;
        }

        // Verify hash after receiving all data
        let computed_hash = BlobHash::from_bytes(&all_data);
        let hash_verified = computed_hash == expected_hash;
        if hash_verified {
            info!("✅ File complete: {} ({} bytes) - Hash verified ✓", received_file_name, total_bytes_received);
        } else {
            info!("⚠️ File complete: {} ({} bytes) - Hash mismatch!", received_file_name, total_bytes_received);
        }

        event_sender.send(ConnectEvent::Transfer(TransferEvent::FileComplete {
            file_name: received_file_name,
            total_bytes: total_bytes_received,
            hash_verified: Some(hash_verified),
        })).await?;
    }

    info!("🔒 Closing connection...");
    connection.close(1u8.into(), b"done");

    send_task.await??;
    Ok(())

}