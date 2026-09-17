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

// ==========================================================================================
// `Node` — the shipping protocol (design §4.7).
//
// It lands beside `EchoNode` on purpose: `app.rs` keeps calling the old type until the app
// step switches over, so this commit compiles with both present. The integrator deletes
// `EchoNode`, `Echo` and its ALPN when the app no longer references them.
// ==========================================================================================

use crate::file_io::SharedFiles;
use crate::protocol::{cap_from_hex, cap_to_hex, ALPN, CAP_LEN, MAX_WINDOW_MIB};
use crate::transfer::{
    default_dc_factory, run_sender, DcFactoryImpl, SenderOptions, TransferProgress,
};
use iroh::{RelayMap, RelayMode, RelayUrl, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use n0_future::time::{timeout, Duration, Instant};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

/// What the user is told when a fragment carried something unusable instead of a ticket.
const DAMAGED_LINK: &str = "this link is damaged; ask the sender to copy it again";
/// ...and when it carried an access code but no ticket at all.
const INCOMPLETE_LINK: &str = "this link is incomplete; ask the sender for the full link";

/// Context string of the capability derivation. Changing it invalidates every live link.
const CAP_CONTEXT: &str = "syncoxiders/p2p-transfer cap v1";
/// How long a finished peer entry stays visible before it is pruned.
const PEER_LINGER: Duration = Duration::from_secs(30);
/// Budget for `Endpoint::online()` before a share reports itself offline.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(15);
/// Budget for one dial, for callers that bring no options of their own. A receive session
/// passes its own `ReceiveOptions::connect_timeout`, which is defined from this same value.
const DIAL_TIMEOUT: Duration = crate::transfer::CONNECT_TIMEOUT;
/// How long a relay-less node waits for a local address to appear.
const LOCAL_ADDR_TIMEOUT: Duration = Duration::from_secs(3);
/// Poll interval while waiting for that address.
const LOCAL_ADDR_POLL: Duration = Duration::from_millis(100);

/// Which relay infrastructure a node uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayChoice {
    /// n0's public relays, with their address lookup.
    N0,
    /// A self-hosted relay, and no publishing to n0's infrastructure.
    Custom(RelayUrl),
    /// No relay at all: LAN and tests.
    None,
}

impl RelayChoice {
    /// `P2P_RELAY_URL` at compile time selects a self-hosted relay; otherwise n0's.
    pub fn from_env() -> Self {
        match option_env!("P2P_RELAY_URL") {
            Some(url) => match url.parse::<RelayUrl>() {
                Ok(url) => Self::Custom(url),
                Err(e) => {
                    log::warn!("P2P_RELAY_URL is not a valid relay URL ({e}); using the default");
                    Self::N0
                }
            },
            None => Self::N0,
        }
    }
}

/// Why a node could not be bound, addressed or dialled.
#[derive(Debug)]
pub enum NodeError {
    Bind(String),
    Connect(String),
    /// The endpoint never came online within its budget.
    Offline,
    Ticket(iroh_tickets::ParseError),
    Relay(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind(m) => write!(f, "could not start the node: {m}"),
            Self::Connect(m) => write!(f, "could not connect: {m}"),
            Self::Offline => write!(f, "could not reach the network"),
            Self::Ticket(e) => write!(f, "invalid link: {e}"),
            Self::Relay(m) => write!(f, "relay error: {m}"),
        }
    }
}

impl std::error::Error for NodeError {}

/// Which receiver sink a QA run wants. `Auto` is the product behaviour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SinkPref {
    #[default]
    Auto,
    Fsa,
    Sw,
    Mem,
}

/// Everything the URL fragment can carry. Values are client-only and never travel in a query
/// string: a page's query string reaches the server's own access logs through `Referer`,
/// while a fragment is never sent to any server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FragmentParams {
    pub dev: bool,
    pub force_relay: bool,
    pub sink_pref: SinkPref,
    pub kill_dc_after: Option<u64>,
    /// `win=<MiB>` in bytes.
    pub window: Option<u64>,
    /// The link's access code — a bearer credential, scrubbed from the URL with the ticket.
    pub cap: Option<[u8; CAP_LEN]>,
    pub ticket: Option<EndpointTicket>,
    pub error: Option<String>,
}

/// One connected peer, as the sender's UI sees it.
struct PeerEntry {
    id: EndpointId,
    progress: watch::Receiver<TransferProgress>,
    since: Instant,
    terminal_at: Option<Instant>,
}

/// The peers currently talking to this node.
#[derive(Clone, Default)]
pub struct Peers(Arc<Mutex<Vec<PeerEntry>>>);

impl Peers {
    /// Record a newly accepted connection.
    pub fn register(&self, id: EndpointId, progress: watch::Receiver<TransferProgress>) {
        if let Ok(mut peers) = self.0.lock() {
            peers.push(PeerEntry {
                id,
                progress,
                since: Instant::now(),
                terminal_at: None,
            });
        }
    }

    /// Current peers, pruning as it goes: an entry whose session task is gone disappears at
    /// once, a finished one lingers briefly so the UI can show how it ended.
    pub fn snapshot(&self) -> Vec<(EndpointId, TransferProgress)> {
        let Ok(mut peers) = self.0.lock() else {
            return Vec::new();
        };
        let now = Instant::now();
        let mut out = Vec::with_capacity(peers.len());
        peers.retain_mut(|entry| {
            if entry.progress.has_changed().is_err() {
                log::debug!(
                    "peer {} pruned after {:?}",
                    entry.id.fmt_short(),
                    now.duration_since(entry.since)
                );
                return false;
            }
            // Decide on the borrow, clone only what survives: this runs every UI frame and a
            // finished entry carries a `SavedFile` per received file.
            if entry.progress.borrow().phase.is_terminal() {
                let terminal_at = *entry.terminal_at.get_or_insert(now);
                if now.duration_since(terminal_at) > PEER_LINGER {
                    return false;
                }
            }
            out.push((entry.id, entry.progress.borrow().clone()));
            true
        });
        out
    }
}

/// A bound endpoint offering `files` under one fresh key and one fresh capability.
pub struct Node {
    endpoint: Endpoint,
    router: Router,
    files: SharedFiles,
    peers: Peers,
    relay: RelayChoice,
    cap: [u8; CAP_LEN],
}

impl Node {
    /// Bind a node with a **fresh** key, so shares from one person are unlinkable.
    ///
    /// The share's capability is derived from that same key with
    /// `blake3::derive_key(CAP_CONTEXT, secret)`: 128 bits of unpredictable material from a
    /// CSPRNG draw the node already makes, one-way (a leaked capability never exposes the
    /// key), deterministic (the link is stable for the life of the share) and needing no
    /// second RNG dependency on either target. It is never sent to a server and never logged.
    pub async fn bind(files: SharedFiles, relay: RelayChoice) -> Result<Self, NodeError> {
        let secret = SecretKey::generate();
        let cap = derive_cap(&secret);
        let builder = match &relay {
            RelayChoice::N0 => Endpoint::builder(presets::N0),
            RelayChoice::Custom(url) => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Custom(RelayMap::from(url.clone()))),
            RelayChoice::None => {
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
            }
        };
        let endpoint = builder
            .secret_key(secret)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| NodeError::Bind(e.to_string()))?;

        let peers = Peers::default();
        let serve = Serve {
            files: files.clone(),
            peers: peers.clone(),
            opts: SenderOptions::new(cap),
            dc: default_dc_factory(),
        };
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, serve)
            .spawn();

        Ok(Self {
            endpoint,
            router,
            files,
            peers,
            relay,
            cap,
        })
    }

    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// This share's capability. Never log it, never put it in a query string.
    pub fn cap(&self) -> [u8; CAP_LEN] {
        self.cap
    }

    pub fn files(&self) -> SharedFiles {
        self.files.clone()
    }

    pub fn peers(&self) -> Vec<(EndpointId, TransferProgress)> {
        self.peers.snapshot()
    }

    /// The addressing half of a share link.
    ///
    /// With a relay we wait for the endpoint to come online, so the ticket carries a reachable
    /// relay address. Without one there is nothing to wait for but a local address.
    pub async fn ticket(&self) -> Result<EndpointTicket, NodeError> {
        match self.relay {
            RelayChoice::None => self.local_ticket().await,
            _ => {
                timeout(ONLINE_TIMEOUT, self.endpoint.online())
                    .await
                    .map_err(|_| NodeError::Offline)?;
                Ok(EndpointTicket::new(self.endpoint.addr()))
            }
        }
    }

    /// A relay-less share can only be dialled over an IP address, so wait for one to appear.
    /// In a browser none ever does — there is no dialable local address there — and this
    /// reports `Offline` on the same deadline as anywhere else, which is why it needs no
    /// separate wasm body.
    async fn local_ticket(&self) -> Result<EndpointTicket, NodeError> {
        let deadline = Instant::now() + LOCAL_ADDR_TIMEOUT;
        loop {
            let addr = self.endpoint.addr();
            if addr.ip_addrs().next().is_some() {
                return Ok(EndpointTicket::new(addr));
            }
            if Instant::now() >= deadline {
                return Err(NodeError::Offline);
            }
            n0_future::time::sleep(LOCAL_ADDR_POLL).await;
        }
    }

    /// Dial the endpoint a ticket points at.
    pub async fn connect(&self, ticket: &EndpointTicket) -> Result<Connection, NodeError> {
        let addr = ticket.endpoint_addr().clone();
        timeout(DIAL_TIMEOUT, self.endpoint.connect(addr, ALPN))
            .await
            .map_err(|_| NodeError::Connect("timed out".to_string()))?
            .map_err(|e| NodeError::Connect(e.to_string()))
    }

    /// Build a share link: `{base}#{[dev&]}{ticket}&cap={32 hex}`.
    ///
    /// The ticket's string form never contains `#` or `&`, so the grammar stays unambiguous,
    /// and the capability token is always last.
    pub fn link(base_url: &str, ticket: &EndpointTicket, cap: &[u8; CAP_LEN], dev: bool) -> String {
        let base = base_url.split('#').next().unwrap_or(base_url);
        let mut link = String::with_capacity(base.len() + 128);
        link.push_str(base);
        link.push('#');
        if dev {
            link.push_str("dev&");
        }
        link.push_str(&ticket.to_string());
        link.push_str("&cap=");
        link.push_str(&cap_to_hex(cap));
        link
    }

    /// Parse a URL fragment (with or without its leading `#`).
    ///
    /// Tokens are separated by `&` and are order-insensitive. What an unrecognised token means
    /// depends on whether a ticket was found:
    ///
    /// * a ticket parsed → unknown tokens are ignored, so a link written by a later version
    ///   still opens here;
    /// * no ticket, and something was there that is not a known flag → the link is damaged (a
    ///   truncated or re-wrapped paste), and says so instead of looking like a network fault
    ///   several seconds later;
    /// * no ticket but an access code → the link is incomplete: only half of it was copied.
    ///
    /// Only known flags (a bare `#dev`) is not an error — it is simply not a receive link.
    /// No message ever echoes the offending token: a mangled capability is still a secret.
    pub fn parse_fragment(fragment: &str) -> FragmentParams {
        let mut params = FragmentParams::default();
        let mut unrecognised = false;
        let fragment = fragment.strip_prefix('#').unwrap_or(fragment);
        for token in fragment.split('&').filter(|t| !t.is_empty()) {
            if token == "dev" {
                params.dev = true;
            } else if token == "relay" {
                params.force_relay = true;
            } else if let Some(value) = token.strip_prefix("sink=") {
                match value {
                    "fsa" => params.sink_pref = SinkPref::Fsa,
                    "sw" => params.sink_pref = SinkPref::Sw,
                    "mem" => params.sink_pref = SinkPref::Mem,
                    _ => {}
                }
            } else if let Some(value) = token.strip_prefix("killdc=") {
                params.kill_dc_after = value.parse::<u64>().ok();
            } else if let Some(value) = token.strip_prefix("win=") {
                params.window = value
                    .parse::<u64>()
                    .ok()
                    .map(|mib| mib.clamp(1, MAX_WINDOW_MIB) * 1024 * 1024);
            } else if let Some(value) = token.strip_prefix("cap=") {
                params.cap = cap_from_hex(value);
                if params.cap.is_none() {
                    params
                        .error
                        .get_or_insert_with(|| "bad access code".to_string());
                }
            } else if let Ok(ticket) = token.parse::<EndpointTicket>() {
                params.ticket = Some(ticket);
            } else {
                unrecognised = true;
            }
        }
        if params.ticket.is_none() {
            if unrecognised {
                params.error.get_or_insert_with(|| DAMAGED_LINK.to_string());
            } else if params.cap.is_some() {
                params
                    .error
                    .get_or_insert_with(|| INCOMPLETE_LINK.to_string());
            }
        }
        params
    }

    /// Stop serving. Idempotent, and takes `&self` because both the app and the sessions hold
    /// the same `Arc<Node>`.
    pub async fn shutdown(&self) {
        if let Err(e) = self.router.shutdown().await {
            log::debug!("router shutdown: {e}");
        }
        self.endpoint.close().await;
    }
}

/// Derive a share's capability from its secret key (design §4.7).
fn derive_cap(secret: &SecretKey) -> [u8; CAP_LEN] {
    let derived = blake3::derive_key(CAP_CONTEXT, &secret.to_bytes());
    let mut cap = [0u8; CAP_LEN];
    cap.copy_from_slice(&derived[..CAP_LEN]);
    cap
}

/// The protocol handler behind this node's ALPN: one sender session per accepted connection.
#[derive(Clone)]
struct Serve {
    files: SharedFiles,
    peers: Peers,
    opts: SenderOptions,
    dc: DcFactoryImpl,
}

// Hand-written so the capability inside `SenderOptions` can never reach a log line through a
// `Debug`-printed handler.
impl std::fmt::Debug for Serve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Serve")
            .field("opts", &self.opts)
            .field("dc", &self.dc)
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for Serve {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let id = connection.remote_id();
        // One snapshot per connection: a file added later shows up in the next connection's
        // manifest, and this session's view can never change under it.
        let snapshot = Arc::new(
            self.files
                .lock()
                .map_err(|_| {
                    AcceptError::from(std::io::Error::other("the shared file list is unavailable"))
                })?
                .clone(),
        );
        let (progress_tx, progress_rx) = watch::channel(TransferProgress::connecting());
        self.peers.register(id, progress_rx);

        let (done_tx, done_rx) = oneshot::channel();
        let cancel = CancellationToken::new();
        let (opts, dc) = (self.opts.clone(), self.dc.clone());
        // `accept` must return a `Send` future even on wasm, where the session holds JS
        // handles: spawning the session and awaiting its result over a channel is what keeps
        // this signature satisfiable on both targets.
        let _task = task::spawn(async move {
            let result = run_sender(connection, snapshot, progress_tx, cancel, opts, dc).await;
            done_tx.send(result).ok();
        });
        done_rx
            .await
            .map_err(AcceptError::from_err)?
            .map_err(AcceptError::from_err)
    }
}
