use crate::blob_store::BlobHash;
use anyhow::Result;
use iroh::endpoint::{presets, Connection};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointId};
use log::info;
use n0_future::{task, Stream};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// The files an [`EchoNode`] currently offers, as `(filename, contents)`.
type EchoFiles = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

pub struct EchoNode {
    router: Router,
    accept_events: mpsc::UnboundedSender<AcceptEvent>,
    shared_files: EchoFiles,
}

impl EchoNode {
    pub async fn spawn() -> Result<Self> {
        Self::spawn_with_files(Vec::new()).await
    }

    pub async fn spawn_with_files(files: Vec<(String, Vec<u8>)>) -> Result<Self> {
        for (name, data) in &files {
            info!("📦 Sharing file: {} ({} bytes)", name, data.len());
        }

        let endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![Echo::ALPN.to_vec()])
            .bind()
            .await?;
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let echo = Echo::new(event_sender.clone(), files);
        let shared_files = echo.files.clone();
        let router = Router::builder(endpoint).accept(Echo::ALPN, echo).spawn();
        Ok(Self {
            router,
            accept_events: event_sender,
            shared_files,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn get_shared_files(&self) -> EchoFiles {
        self.shared_files.clone()
    }

    pub fn subscribe_accept_events(&self) -> mpsc::UnboundedReceiver<AcceptEvent> {
        let (_tx, rx) = mpsc::unbounded_channel();
        let _main_sender = self.accept_events.clone();
        rx
    }

    pub fn connect(
        &self,
        endpoint_id: EndpointId,
        file_data: Vec<u8>,
        file_name: String,
    ) -> impl Stream<Item = ConnectEvent> + Unpin {
        let (event_sender, mut event_receiver) = mpsc::channel(16);
        let endpoint = self.router.endpoint().clone();
        task::spawn(async move {
            let res = connect(
                &endpoint,
                endpoint_id,
                file_data,
                file_name,
                event_sender.clone(),
            )
            .await;
            let error = res.as_ref().err().map(|e| e.to_string());
            event_sender.send(ConnectEvent::Closed { error }).await.ok();
        });
        // `tokio::sync::mpsc::Receiver` is not itself a `Stream`, and the UI
        // drives these events with `StreamExt::next`.
        n0_future::stream::poll_fn(move |cx| event_receiver.poll_recv(cx))
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
        /// Byte offset of this chunk within the file. Only the native receiver
        /// uses it (it seeks and writes straight to disk); the browser receiver
        /// reassembles chunks in arrival order.
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
    Sent,
    Transfer(TransferEvent),
    Closed { error: Option<String> },
}

#[derive(Debug, Clone)]
pub enum AcceptEvent {
    Accepted {
        endpoint_id: EndpointId,
    },
    Echoed {
        endpoint_id: EndpointId,
        bytes_sent: u64,
    },
    Closed {
        endpoint_id: EndpointId,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Echo {
    event_sender: mpsc::UnboundedSender<AcceptEvent>,
    files: EchoFiles, // (filename, filedata)
}

impl Echo {
    pub const ALPN: &[u8] = b"iroh/example-browser-echo/0";

    pub fn new(
        event_sender: mpsc::UnboundedSender<AcceptEvent>,
        files: Vec<(String, Vec<u8>)>,
    ) -> Self {
        Self {
            event_sender,
            files: Arc::new(Mutex::new(files)),
        }
    }

    async fn handle_connection(
        self,
        connection: Connection,
    ) -> std::result::Result<(), AcceptError> {
        let endpoint_id = connection.remote_id();
        self.event_sender
            .send(AcceptEvent::Accepted { endpoint_id })
            .ok();
        let res = self.handle_connection_0(&connection).await;
        let error = res.as_ref().err().map(|err| err.to_string());
        self.event_sender
            .send(AcceptEvent::Closed { endpoint_id, error })
            .ok();
        res
    }

    async fn handle_connection_0(
        &self,
        connection: &Connection,
    ) -> std::result::Result<(), AcceptError> {
        const CHUNK_SIZE: usize = 256 * 1024;

        let endpoint_id = connection.remote_id();
        info!("✓ Connection accepted from {}", endpoint_id);
        info!("⏳ Opening bidirectional stream...");

        let (mut send, mut recv) = connection.accept_bi().await?;
        info!("✓ Bidirectional stream established");

        // Read filename length
        let mut name_len_buf = [0u8; 4];
        recv.read_exact(&mut name_len_buf)
            .await
            .map_err(std::io::Error::other)?;
        let name_len = u32::from_le_bytes(name_len_buf) as usize;

        // Read filename
        let mut name_buf = vec![0u8; name_len];
        recv.read_exact(&mut name_buf)
            .await
            .map_err(std::io::Error::other)?;
        let _received_file_name = String::from_utf8(name_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Read file data length
        let mut data_len_buf = [0u8; 8];
        recv.read_exact(&mut data_len_buf)
            .await
            .map_err(std::io::Error::other)?;
        let data_len = u64::from_le_bytes(data_len_buf) as usize;

        // Read file data
        let mut _received_file_data = vec![0u8; data_len];
        recv.read_exact(&mut _received_file_data)
            .await
            .map_err(std::io::Error::other)?;

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
        send.write_all(&num_files.to_le_bytes())
            .await
            .map_err(std::io::Error::other)?;
        info!("📤 Sending {} file(s)", num_files);

        let mut total_bytes_sent = 4; // for num_files

        // Send all files in chunks with blob hashes
        for (idx, (name, data)) in files_to_send.iter().enumerate() {
            // Compute BLAKE3 hash for content verification
            let blob_hash = BlobHash::from_bytes(data);
            let hash_hex = blob_hash.to_hex();
            info!(
                "📁 [{}/{}] Sending file: {} ({} bytes, hash: {})",
                idx + 1,
                num_files,
                name,
                data.len(),
                &hash_hex[..16]
            );

            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u32;
            let data_len = data.len() as u64;
            let total_chunks = data_len.div_ceil(CHUNK_SIZE as u64) as u32;

            info!(
                "  ⚙️  File will be sent in {} chunk(s) of {}KB each",
                total_chunks,
                CHUNK_SIZE / 1024
            );

            // Send metadata including hash
            send.write_all(&name_len.to_le_bytes())
                .await
                .map_err(std::io::Error::other)?;
            send.write_all(name_bytes)
                .await
                .map_err(std::io::Error::other)?;
            send.write_all(&data_len.to_le_bytes())
                .await
                .map_err(std::io::Error::other)?;
            send.write_all(&total_chunks.to_le_bytes())
                .await
                .map_err(std::io::Error::other)?;
            // Send blob hash (32 bytes)
            send.write_all(&blob_hash.0)
                .await
                .map_err(std::io::Error::other)?;
            info!("  ✓ Metadata and hash sent");

            total_bytes_sent += 4 + name_bytes.len() + 8 + 4 + 32;

            for chunk_idx in 0..total_chunks {
                let offset = chunk_idx as usize * CHUNK_SIZE;
                let chunk_size = std::cmp::min(CHUNK_SIZE, data.len() - offset);
                let chunk_data = &data[offset..offset + chunk_size];

                send.write_all(&chunk_idx.to_le_bytes())
                    .await
                    .map_err(std::io::Error::other)?;
                send.write_all(&(chunk_size as u32).to_le_bytes())
                    .await
                    .map_err(std::io::Error::other)?;
                send.write_all(chunk_data)
                    .await
                    .map_err(std::io::Error::other)?;

                total_bytes_sent += 4 + 4 + chunk_size;
                let progress = ((chunk_idx + 1) as f32 / total_chunks as f32 * 100.0) as u32;
                info!(
                    "  📤 Chunk {}/{} sent ({}KB) - {}% complete",
                    chunk_idx + 1,
                    total_chunks,
                    chunk_size / 1024,
                    progress
                );
            }

            info!(
                "✅ File complete: {} ({} bytes in {} chunks)",
                name,
                data.len(),
                total_chunks
            );
        }

        let bytes_sent = total_bytes_sent;

        self.event_sender
            .send(AcceptEvent::Echoed {
                endpoint_id,
                bytes_sent: bytes_sent as u64,
            })
            .ok();

        info!(
            "📊 Total bytes sent: {} ({:.2} MB)",
            bytes_sent,
            bytes_sent as f64 / 1024.0 / 1024.0
        );
        send.finish()?;
        info!("🔒 Closing connection with {}", endpoint_id);
        connection.closed().await;
        info!("✓ Connection closed successfully");
        Ok(())
    }
}

impl ProtocolHandler for Echo {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        self.clone().handle_connection(connection).await
    }
}

async fn connect(
    endpoint: &Endpoint,
    endpoint_id: EndpointId,
    file_data: Vec<u8>,
    file_name: String,
    event_sender: mpsc::Sender<ConnectEvent>,
) -> Result<()> {
    info!("🔗 Initiating connection to endpoint: {}", endpoint_id);
    let connection = endpoint.connect(endpoint_id, Echo::ALPN).await?;
    info!("✓ Connection established with {}", endpoint_id);
    event_sender.send(ConnectEvent::Connected).await?;

    info!("⏳ Opening bidirectional stream...");
    let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
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
        event_sender_clone.send(ConnectEvent::Sent).await?;

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
        info!(
            "📁 [{}/{}] Receiving file metadata...",
            file_idx + 1,
            num_files
        );
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

        info!(
            "  ✓ File: {} ({} bytes, {} chunks, hash: {})",
            received_file_name,
            data_len,
            total_chunks,
            &hash_hex[..16]
        );

        event_sender
            .send(ConnectEvent::Transfer(TransferEvent::FileStart {
                file_name: received_file_name.clone(),
                file_size: data_len,
                total_chunks,
                blob_hash: Some(hash_hex),
            }))
            .await?;

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

            info!(
                "  📥 Chunk {}/{} received ({}KB) - {}% complete",
                chunk_num + 1,
                total_chunks,
                chunk_size / 1024,
                progress
            );

            event_sender
                .send(ConnectEvent::Transfer(TransferEvent::ChunkReceived {
                    file_name: received_file_name.clone(),
                    chunk_index,
                    chunk_data,
                    offset,
                }))
                .await?;
        }

        // Verify hash after receiving all data
        let computed_hash = BlobHash::from_bytes(&all_data);
        let hash_verified = computed_hash == expected_hash;
        if hash_verified {
            info!(
                "✅ File complete: {} ({} bytes) - Hash verified ✓",
                received_file_name, total_bytes_received
            );
        } else {
            info!(
                "⚠️ File complete: {} ({} bytes) - Hash mismatch!",
                received_file_name, total_bytes_received
            );
        }

        event_sender
            .send(ConnectEvent::Transfer(TransferEvent::FileComplete {
                file_name: received_file_name,
                total_bytes: total_bytes_received,
                hash_verified: Some(hash_verified),
            }))
            .await?;
    }

    info!("🔒 Closing connection...");
    connection.close(1u8.into(), b"done");

    send_task.await??;
    Ok(())
}
