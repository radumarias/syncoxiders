//! Transport-agnostic transfer engine (design §4.6).
//!
//! The two session cores ([`run_sender_on`], [`run_receiver_on`]) are generic over a control
//! transport ([`FrameTx`] / [`FrameRx`]) and a WebRTC factory ([`DcFactory`]), so every
//! in-memory test drives the same code the iroh wrappers ([`run_sender`], [`run_receiver`])
//! run in production. The four transport seams travel together in [`SessionIo`].
//!
//! **Rule §4.6.0-R** (normative for both cores, once the bodies land): long-lived work is an
//! *arm* of the session `select!`, polled as a future; an arm's handler never awaits anything
//! unbounded — where it must await, it does so inside an inner `select!` against `cancel` and
//! a named deadline. The only exemptions are [`ControlTx::send`] and the data channel's send,
//! which carry their own stall deadline internally.

use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh_tickets::endpoint::EndpointTicket;
use n0_future::task::{self, AbortOnDropHandle, JoinHandle};
use n0_future::time::{sleep, sleep_until, timeout, Duration, Instant};
use n0_future::MaybeFuture;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch, Notify};
use tokio_util::sync::CancellationToken;

use crate::file_io::{open_source, AnySink, SavedFile, SharedFile, Sink, Source};
use crate::node::{FragmentParams, Node, SinkPref};
use crate::protocol::{
    self, cap_eq, decode, encode_chunk, encode_control, max_payload, ChunkHeader, Control,
    FileMeta, Frame, ProtocolError, CAP_LEN, CREDIT_GRAIN, DEFAULT_CHUNK, INITIAL_WINDOW,
    LEN_PREFIX, MAX_FRAME, PROTOCOL_VERSION,
};

/// Frames the control writer task may hold before a sender blocks.
const CONTROL_QUEUE: usize = 4;
/// A peer that stops reading control frames for this long is dead ([`ControlTx::send`]).
pub const CONTROL_SEND_STALL: Duration = Duration::from_secs(30);
/// A peer that opens a stream and says nothing must not pin a task.
pub const HELLO_DEADLINE: Duration = Duration::from_secs(10);
/// How long a `Request` may sit parked waiting for the data channel to open.
pub const DCEP_DEADLINE: Duration = Duration::from_secs(2);
/// Bound for the short auxiliary awaits (SDP promises, ICE, sink abort).
pub const AUX_DEADLINE: Duration = Duration::from_secs(5);
/// No frame of the current epoch for this long triggers a switch or a timeout.
pub const INACTIVITY: Duration = Duration::from_secs(30);
/// A single sink write may not take longer than this.
pub const WRITE_DEADLINE: Duration = Duration::from_secs(60);
/// Dial budget for the receiver.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the receiver waits for the data channel to open before falling back to the relay.
pub const WEBRTC_OPEN: Duration = Duration::from_secs(10);
/// Number of fresh control connections attempted after the first one fails.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 4;
/// Initial reconnect delay. It doubles up to [`MAX_RECONNECT_BACKOFF`].
pub const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);
pub const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(8);
/// Read-ahead granularity of [`SourceReader`]. Sender memory is bounded by two of these.
pub const READ_AHEAD: usize = 1024 * 1024;
/// Progress is published at most this often, unless a phase changed or a MiB went by.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);
/// ...or this many bytes, whichever comes first.
const PROGRESS_BYTES: u64 = 1024 * 1024;
/// Throughput is averaged over windows of at least this long.
const RATE_WINDOW: Duration = Duration::from_millis(250);
/// Weight of the newest sample in the throughput average.
const RATE_ALPHA: f64 = 0.3;
/// Do not display a stale rate indefinitely while the sender waits for credit.
const RATE_IDLE_MIN: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------

/// A transport-level failure. `Cancelled` and `Closed` are both benign for a data-channel
/// send: the session stays alive and waits for the receiver's `UseRelay` + `Request`. Only a
/// control-stream error is session-fatal.
#[derive(Debug)]
pub enum TransportError {
    Closed,
    Cancelled,
    Io(String),
    TooLarge(usize),
    Timeout,
    Protocol(ProtocolError),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "transport closed"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Io(m) => write!(f, "transport error: {m}"),
            Self::TooLarge(n) => write!(f, "frame too large: {n} bytes"),
            Self::Timeout => write!(f, "transport timed out"),
            Self::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Why an authorization check failed. The two cases need different words in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailure {
    /// The link carried no access code at all.
    MissingCap,
    /// The sender rejected the code we offered.
    Rejected,
}

/// Session-level errors. `app.rs` renders `to_string()`.
#[derive(Debug)]
pub enum TransferError {
    Unauthorized(AuthFailure),
    Version {
        theirs: u16,
    },
    Protocol(ProtocolError),
    HashMismatch {
        file: u32,
    },
    /// A session-fatal `Error` frame from the peer.
    Remote(String),
    Transport(TransportError),
    Io(String),
    /// A destination operation timed out. Its commit state is uncertain, so it is never retried.
    SinkTimeout,
    /// A fresh connection offered different content than the authenticated original manifest.
    ManifestChanged,
    Timeout,
    Cancelled,
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized(AuthFailure::MissingCap) => write!(
                f,
                "this link is missing its access code; ask the sender for the full link"
            ),
            Self::Unauthorized(AuthFailure::Rejected) => write!(
                f,
                "the sender rejected this link (wrong or expired access code)"
            ),
            Self::Version { theirs } => {
                write!(f, "the other side speaks protocol version {theirs}")
            }
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
            Self::HashMismatch { file } => {
                write!(f, "file {file} failed its integrity check")
            }
            Self::Remote(m) => write!(f, "{m}"),
            Self::Transport(e) => write!(f, "{e}"),
            Self::Io(m) => write!(f, "{m}"),
            Self::SinkTimeout => write!(
                f,
                "the destination stopped responding; retrying could corrupt the saved file"
            ),
            Self::ManifestChanged => write!(
                f,
                "the sender is sharing different files; resume requires the same names, sizes, hashes and ordering"
            ),
            Self::Timeout => write!(f, "timed out"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<TransportError> for TransferError {
    fn from(e: TransportError) -> Self {
        Self::Transport(e)
    }
}

// ---------------------------------------------------------------------------------------
// Transport traits
// ---------------------------------------------------------------------------------------

/// Which kind of path bytes are travelling over. `Direct` is an `RTCDataChannel` in the
/// browser, or an iroh IP path natively.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Path {
    #[default]
    Unknown,
    Direct,
    Relayed,
}

/// Write half of a frame transport.
pub trait FrameTx {
    fn send(&self, frame: Bytes) -> impl Future<Output = Result<(), TransportError>>;
    fn max_frame(&self) -> usize;
    /// True when this transport writes a `u32` LE length in front of every frame (iroh
    /// streams), false when it preserves message boundaries itself (`RTCDataChannel`).
    /// Feeds [`protocol::max_payload`].
    fn is_length_prefixed(&self) -> bool;
    fn path(&self) -> Path;
}

/// Read half of a frame transport. `Ok(None)` means the peer closed cleanly.
pub trait FrameRx {
    fn recv(&mut self) -> impl Future<Output = Result<Option<Bytes>, TransportError>>;
}

/// Write half of the iroh control stream.
///
/// Every send goes through a bounded queue into one writer task, so the session loop, the
/// serve future and the error paths can all send without sharing a `&mut`. The send is
/// bounded by construction: it races the queue reservation against cancellation and a 30 s
/// stall deadline, which is what makes it exempt from rule §4.6.0-R's inner-race requirement.
pub struct ControlTx {
    tx: mpsc::Sender<Bytes>,
    path: Path,
    cancel: CancellationToken,
}

impl FrameTx for ControlTx {
    async fn send(&self, frame: Bytes) -> Result<(), TransportError> {
        // Fast path first: this is the per-chunk send path, and arming the stall timer is
        // not free — on wasm every `sleep` allocates a JS closure and a
        // `setTimeout`/`clearTimeout` pair, which would be one per chunk. While the writer
        // task keeps up, the queue has room and the timer is never armed at all.
        if self.cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        match self.tx.try_reserve() {
            Ok(permit) => {
                permit.send(frame);
                return Ok(());
            }
            Err(mpsc::error::TrySendError::Closed(())) => return Err(TransportError::Closed),
            Err(mpsc::error::TrySendError::Full(())) => {}
        }
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(TransportError::Cancelled),
            _ = sleep(CONTROL_SEND_STALL) => Err(TransportError::Timeout),
            reserved = self.tx.reserve() => match reserved {
                Ok(permit) => {
                    permit.send(frame);
                    Ok(())
                }
                Err(_) => Err(TransportError::Closed),
            },
        }
    }

    fn max_frame(&self) -> usize {
        DEFAULT_CHUNK
    }

    fn is_length_prefixed(&self) -> bool {
        true
    }

    fn path(&self) -> Path {
        self.path
    }
}

/// Read half of the iroh control stream.
///
/// Frames are reassembled in an owned buffer rather than by awaiting `read_frame` directly,
/// because `recv` is polled as a `select!` arm: dropping a half-finished `read_frame` would
/// lose the bytes it had already taken off the stream and desynchronise the framing. Filling
/// an owned buffer with `read_buf` (cancel-safe) keeps every partial read across a
/// cancellation. The wire format is exactly the one `protocol::write_frame` produces.
pub struct ControlRx {
    recv: RecvStream,
    buf: BytesMut,
}

impl FrameRx for ControlRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        loop {
            if let Some(frame) = take_frame(&mut self.buf)? {
                return Ok(Some(frame));
            }
            let read = self
                .recv
                .read_buf(&mut self.buf)
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
            if read == 0 {
                return if self.buf.is_empty() {
                    Ok(None)
                } else {
                    Err(TransportError::Io("truncated frame".to_string()))
                };
            }
        }
    }
}

/// Split one complete length-prefixed frame off the front of `buf`, if it is fully there.
fn take_frame(buf: &mut BytesMut) -> Result<Option<Bytes>, TransportError> {
    if buf.len() < LEN_PREFIX {
        return Ok(None);
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME {
        return Err(TransportError::TooLarge(len));
    }
    if buf.len() < LEN_PREFIX + len {
        return Ok(None);
    }
    let _ = buf.split_to(LEN_PREFIX);
    Ok(Some(buf.split_to(len).freeze()))
}

/// Split an accepted or opened bidirectional stream into the control halves.
///
/// The returned handle owns the writer task; drop it to stop writing.
pub fn split_control(
    send: SendStream,
    recv: RecvStream,
    path: Path,
    cancel: CancellationToken,
) -> (ControlTx, ControlRx, AbortOnDropHandle<()>) {
    let (tx, mut queue) = mpsc::channel::<Bytes>(CONTROL_QUEUE);
    let writer = task::spawn(async move {
        let mut send = send;
        while let Some(frame) = queue.recv().await {
            if let Err(e) = protocol::write_frame(&mut send, &frame).await {
                log::debug!("control writer stopped: {e}");
                return;
            }
        }
        let _ = send.finish();
    });
    (
        ControlTx { tx, path, cancel },
        ControlRx {
            recv,
            buf: BytesMut::new(),
        },
        AbortOnDropHandle::new(writer),
    )
}

// ---------------------------------------------------------------------------------------
// WebRTC seam
// ---------------------------------------------------------------------------------------

/// Engine-level peer-connection state, so no web-sys type leaks into this module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PcState {
    #[default]
    New,
    Connecting,
    Connected,
    Failed,
    Closed,
}

impl PcState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed | Self::Closed)
    }
}

/// One trickle-ICE candidate, in the shape the `Ice` control frame carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

/// Which side of the WebRTC handshake we are. The sender offers, the receiver answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcRole {
    Offerer,
    Answerer,
}

/// The WebRTC surface the engine needs, so the session loops never name a browser type and
/// tests can inject a fake.
pub trait DataChannel {
    type Tx: FrameTx;
    type Rx: FrameRx;

    fn create_offer(&mut self) -> impl Future<Output = Result<String, TransportError>>;
    fn accept_offer(&mut self, sdp: &str) -> impl Future<Output = Result<String, TransportError>>;
    fn accept_answer(&mut self, sdp: &str) -> impl Future<Output = Result<(), TransportError>>;
    fn add_ice(&mut self, c: &IceCandidate) -> impl Future<Output = Result<(), TransportError>>;
    /// `None` once gathering is complete.
    fn next_local_ice(&mut self) -> impl Future<Output = Option<IceCandidate>>;
    fn open_watch(&self) -> watch::Receiver<bool>;
    fn state_watch(&self) -> watch::Receiver<PcState>;
    fn max_message_size(&self) -> Option<usize>;
    fn path_kind(&self) -> impl Future<Output = Path>;
    /// Hand out the send and receive halves, once. The session keeps the channel itself so a
    /// late `Answer` or `Ice` can still be applied and `close` still works.
    fn split(
        &mut self,
        cancel: CancellationToken,
        relay_switch: Arc<Notify>,
        receive_window: usize,
    ) -> Option<(Self::Tx, Self::Rx)>;
    fn close(&self);
}

/// Creates data channels — and answers the one question that decides the transport before
/// anything is negotiated: can this build do WebRTC at all?
pub trait DcFactory {
    type Dc: DataChannel;
    /// The `webrtc` bit this side puts in `Hello`.
    fn available(&self) -> bool;
    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError>;
}

/// The factory for builds with no WebRTC: native, and the `relay` fragment flag.
///
/// Deliberately `Clone` but not `Copy`, so a caller reads the same on both targets — the wasm
/// factory carries its ICE server list and can never be `Copy`.
#[derive(Clone, Debug, Default)]
pub struct NoWebRtc;

impl DcFactory for NoWebRtc {
    type Dc = NeverDc;

    fn available(&self) -> bool {
        false
    }

    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError> {
        let _ = role;
        Err(TransportError::Closed)
    }
}

/// The data channel [`NoWebRtc`] never creates.
#[derive(Debug)]
pub enum NeverDc {}

/// The send half [`NeverDc`] never has.
#[derive(Debug)]
pub enum NeverTx {}

/// The receive half [`NeverDc`] never has.
#[derive(Debug)]
pub enum NeverRx {}

impl FrameTx for NeverTx {
    fn send(&self, frame: Bytes) -> impl Future<Output = Result<(), TransportError>> {
        let _ = frame;
        async move { match *self {} }
    }

    fn max_frame(&self) -> usize {
        match *self {}
    }

    fn is_length_prefixed(&self) -> bool {
        match *self {}
    }

    fn path(&self) -> Path {
        match *self {}
    }
}

impl FrameRx for NeverRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        match *self {}
    }
}

impl DataChannel for NeverDc {
    type Tx = NeverTx;
    type Rx = NeverRx;

    async fn create_offer(&mut self) -> Result<String, TransportError> {
        match *self {}
    }

    fn accept_offer(&mut self, sdp: &str) -> impl Future<Output = Result<String, TransportError>> {
        let _ = sdp;
        async move { match *self {} }
    }

    fn accept_answer(&mut self, sdp: &str) -> impl Future<Output = Result<(), TransportError>> {
        let _ = sdp;
        async move { match *self {} }
    }

    fn add_ice(&mut self, c: &IceCandidate) -> impl Future<Output = Result<(), TransportError>> {
        let _ = c;
        async move { match *self {} }
    }

    async fn next_local_ice(&mut self) -> Option<IceCandidate> {
        match *self {}
    }

    fn open_watch(&self) -> watch::Receiver<bool> {
        match *self {}
    }

    fn state_watch(&self) -> watch::Receiver<PcState> {
        match *self {}
    }

    fn max_message_size(&self) -> Option<usize> {
        match *self {}
    }

    async fn path_kind(&self) -> Path {
        match *self {}
    }

    fn split(
        &mut self,
        cancel: CancellationToken,
        relay_switch: Arc<Notify>,
        receive_window: usize,
    ) -> Option<(Self::Tx, Self::Rx)> {
        let _ = (cancel, relay_switch, receive_window);
        match *self {}
    }

    fn close(&self) {
        match *self {}
    }
}

/// The WebRTC factory of this build: the browser one on wasm, [`NoWebRtc`] natively.
#[cfg(target_arch = "wasm32")]
pub type DcFactoryImpl = crate::webrtc::WebRtcFactory;
/// The WebRTC factory of this build: the browser one on wasm, [`NoWebRtc`] natively.
#[cfg(not(target_arch = "wasm32"))]
pub type DcFactoryImpl = NoWebRtc;

/// The factory the iroh wrappers and the node's protocol handler use.
pub fn default_dc_factory() -> DcFactoryImpl {
    DcFactoryImpl::default()
}

// ---------------------------------------------------------------------------------------
// Session plumbing
// ---------------------------------------------------------------------------------------

/// The transport seams of one session, bundled so each core keeps a small signature.
///
/// Built by the iroh wrappers and by tests. `close` is the connection-close seam:
/// `conn.close(code, reason)` in production, a recorder in tests.
pub struct SessionIo<T: FrameTx, R: FrameRx, F: DcFactory> {
    pub ctrl_tx: T,
    pub ctrl_rx: R,
    pub close: CloseSeam,
    pub dc: F,
}

/// How a session closes its connection: `conn.close(code, reason)` in production, a recorder
/// in tests.
pub type CloseSeam = Box<dyn FnOnce(u32, &[u8]) + Send>;

/// Byte-budgeted admission for a queue fed from a callback and drained by a reader.
///
/// The data channel's inbound queue is budgeted in **bytes** (`initial_window + MAX_FRAME`),
/// never in frames: 4 MiB of credit is 65 frames at 64 KiB but 512 at 8 KiB, so a frame cap
/// would make a conforming sender look hostile at small frame sizes.
#[derive(Debug)]
pub struct ByteBudget {
    budget: AtomicUsize,
    in_queue: AtomicUsize,
}

impl ByteBudget {
    pub fn new(budget: usize) -> Self {
        Self {
            budget: AtomicUsize::new(budget),
            in_queue: AtomicUsize::new(0),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget.load(Ordering::Acquire)
    }

    /// Set before a callback-fed transport begins delivering frames.
    pub fn set_budget(&self, budget: usize) {
        self.budget.store(budget, Ordering::Release);
    }

    pub fn in_queue(&self) -> usize {
        self.in_queue.load(Ordering::Acquire)
    }

    /// Reserve `len` bytes of queue space, or report that the peer overran the window.
    pub fn try_admit(&self, len: usize) -> bool {
        self.in_queue
            .try_update(Ordering::AcqRel, Ordering::Acquire, |in_queue| {
                in_queue
                    .checked_add(len)
                    .filter(|next| *next <= self.budget())
            })
            .is_ok()
    }

    /// Give `len` bytes of queue space back once the reader took them.
    pub fn release(&self, len: usize) {
        let _ = self
            .in_queue
            .try_update(Ordering::AcqRel, Ordering::Acquire, |in_queue| {
                Some(in_queue.saturating_sub(len))
            });
    }
}

// ---------------------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------------------

/// Where a session is. `AwaitingSave` carries the manifest the user is about to accept;
/// `Complete` carries the saved files, which is how the app learns where they went.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Connecting,
    Reconnecting { attempt: u32, max_attempts: u32 },
    Handshake,
    AwaitingSave { manifest: Vec<protocol::FileMeta> },
    Signaling,
    Transferring,
    Switching,
    Verifying,
    Complete { saved: Vec<SavedFile> },
    Failed,
    Cancelled,
}

impl Phase {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete { .. } | Self::Failed | Self::Cancelled)
    }
}

/// One session's progress. Updated with `send_modify` so the wrapper's path/RTT sampler and
/// the core never clobber each other's fields.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferProgress {
    pub phase: Phase,
    pub path: Path,
    pub file_index: Option<u32>,
    pub file_name: Option<String>,
    pub file_done: u64,
    pub file_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub bytes_per_sec: f64,
    /// Complete-frame limit negotiated for the active transport. `None` on the control stream.
    pub frame_size: Option<usize>,
    /// RTT of the control connection's selected path, so the credit-window bound of §2.5 is
    /// readable next to a measured throughput. `None` until sampled.
    pub rtt_ms: Option<u32>,
    pub error: Option<String>,
}

impl TransferProgress {
    pub fn connecting() -> Self {
        Self {
            phase: Phase::Connecting,
            path: Path::Unknown,
            file_index: None,
            file_name: None,
            file_done: 0,
            file_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            bytes_per_sec: 0.0,
            frame_size: None,
            rtt_ms: None,
            error: None,
        }
    }
}

impl Default for TransferProgress {
    fn default() -> Self {
        Self::connecting()
    }
}

// ---------------------------------------------------------------------------------------
// Options and commands
// ---------------------------------------------------------------------------------------

/// Sender-side knobs. `Debug` is hand-written so the capability never reaches a log line.
#[derive(Clone)]
pub struct SenderOptions {
    /// This share's capability (design §2.6).
    pub cap: [u8; CAP_LEN],
    pub hello_deadline: Duration,
    pub dcep_deadline: Duration,
    pub aux_deadline: Duration,
}

impl SenderOptions {
    pub fn new(cap: [u8; CAP_LEN]) -> Self {
        Self {
            cap,
            hello_deadline: HELLO_DEADLINE,
            dcep_deadline: DCEP_DEADLINE,
            aux_deadline: AUX_DEADLINE,
        }
    }
}

impl std::fmt::Debug for SenderOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SenderOptions")
            .field("cap", &"<redacted>")
            .field("hello_deadline", &self.hello_deadline)
            .field("dcep_deadline", &self.dcep_deadline)
            .field("aux_deadline", &self.aux_deadline)
            .finish()
    }
}

/// Receiver-side knobs. Every fragment-derived field lives here, so adding a QA hook later
/// never changes a signature the app lane already built against.
#[derive(Clone)]
pub struct ReceiveOptions {
    /// Never offer WebRTC, even when a factory is available (fragment `relay`).
    pub force_relay: bool,
    pub inactivity: Duration,
    pub write_deadline: Duration,
    pub aux_deadline: Duration,
    pub connect_timeout: Duration,
    pub webrtc_open: Duration,
    pub max_reconnect_attempts: u32,
    pub reconnect_backoff: Duration,
    pub max_reconnect_backoff: Duration,
    /// The link's access code (design §2.6). `None` fails before dialling.
    pub cap: Option<[u8; CAP_LEN]>,
    /// Fragment `sink=`; consumed by `pick_sinks` in the app's Save task.
    pub sink_pref: SinkPref,
    /// Fragment `killdc=`; consumed by the session.
    pub kill_dc_after: Option<u64>,
    /// Fragment `win=<MiB>`; the explicit initial credit grant, and the size of the data
    /// channel's inbound byte budget.
    pub initial_window: u64,
}

impl Default for ReceiveOptions {
    fn default() -> Self {
        Self {
            force_relay: false,
            inactivity: INACTIVITY,
            write_deadline: WRITE_DEADLINE,
            aux_deadline: AUX_DEADLINE,
            connect_timeout: CONNECT_TIMEOUT,
            webrtc_open: WEBRTC_OPEN,
            max_reconnect_attempts: MAX_RECONNECT_ATTEMPTS,
            reconnect_backoff: RECONNECT_BACKOFF,
            max_reconnect_backoff: MAX_RECONNECT_BACKOFF,
            cap: None,
            sink_pref: SinkPref::Auto,
            kill_dc_after: None,
            initial_window: INITIAL_WINDOW,
        }
    }
}

impl ReceiveOptions {
    /// The one place a parsed fragment becomes receive options.
    pub fn from_fragment(p: &FragmentParams) -> Self {
        Self {
            force_relay: p.force_relay,
            cap: p.cap,
            sink_pref: p.sink_pref,
            kill_dc_after: p.kill_dc_after,
            initial_window: p.window.unwrap_or(INITIAL_WINDOW),
            ..Self::default()
        }
    }
}

impl std::fmt::Debug for ReceiveOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiveOptions")
            .field("force_relay", &self.force_relay)
            .field("cap", &"<redacted>")
            .field("sink_pref", &self.sink_pref)
            .field("kill_dc_after", &self.kill_dc_after)
            .field("initial_window", &self.initial_window)
            .finish_non_exhaustive()
    }
}

/// What the app tells a running receive session to do.
pub enum ReceiveCommand {
    /// Sinks in manifest order, created inside the Save click's task.
    Save(Vec<AnySink>),
    /// Durable sinks plus BLAKE3 states reconstructed from their checkpointed prefixes.
    Resume(Vec<ResumeFile>),
}

/// One durable file prepared for restart recovery.
pub struct ResumeFile {
    pub meta: FileMeta,
    pub sink: AnySink,
    pub hasher: blake3::Hasher,
}

// ---------------------------------------------------------------------------------------
// Session entry points
// ---------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------
// Engine internals
// ---------------------------------------------------------------------------------------

/// The payload budget the receiver has granted for the current epoch.
///
/// The session loop feeds it from `Credit` frames and the serve future waits on it, which is
/// what lets a sender block on flow control while still reading control frames.
#[derive(Debug)]
struct Window {
    granted: AtomicU64,
    notify: Notify,
}

impl Window {
    fn new() -> Self {
        Self {
            granted: AtomicU64::new(0),
            notify: Notify::new(),
        }
    }

    fn grant(&self, bytes: u64) {
        self.granted.fetch_add(bytes, Ordering::AcqRel);
        // `notify_one` stores a permit, so a grant landing between the serve's check and its
        // wait is not lost — `notify_waiters` would drop it and stall until the next grant.
        self.notify.notify_one();
    }

    fn granted(&self) -> u64 {
        self.granted.load(Ordering::Acquire)
    }
}

/// Where one serve writes. Both variants are `Arc`s, so the serve future owns its transport
/// and borrows nothing from the session loop — which is what lets it be a `select!` arm that
/// a `Request` can drop at any frame boundary.
enum ServeTx<C: FrameTx, D: FrameTx> {
    Control(Arc<C>),
    Dc(Arc<D>),
}

impl<C: FrameTx, D: FrameTx> ServeTx<C, D> {
    fn max_frame(&self) -> usize {
        match self {
            Self::Control(t) => t.max_frame(),
            Self::Dc(t) => t.max_frame(),
        }
    }

    fn is_length_prefixed(&self) -> bool {
        match self {
            Self::Control(t) => t.is_length_prefixed(),
            Self::Dc(t) => t.is_length_prefixed(),
        }
    }

    fn path(&self) -> Path {
        match self {
            Self::Control(t) => t.path(),
            Self::Dc(t) => t.path(),
        }
    }

    async fn send(&self, frame: Bytes) -> Result<(), TransportError> {
        match self {
            Self::Control(t) => t.send(frame).await,
            Self::Dc(t) => t.send(frame).await,
        }
    }

    /// A failed send on the data channel leaves the session alive — the receiver will ask for
    /// the rest over the relay. On the control stream there is nothing left to talk over.
    fn failure(&self, file: u32, epoch: u32, error: TransportError) -> ServeOutcome {
        match self {
            Self::Control(_) => ServeOutcome::ControlFailed(error),
            Self::Dc(_) => ServeOutcome::DcFailed { file, epoch },
        }
    }
}

/// Read-ahead over a [`Source`], so a 64 KiB chunk does not become a 64 KiB read.
///
/// Memory is bounded by one buffer of [`READ_AHEAD`] plus the chunk being framed.
pub struct SourceReader<S: Source> {
    src: S,
    buf: Bytes,
    buf_start: u64,
    read_size: usize,
}

impl<S: Source> SourceReader<S> {
    pub fn new(src: S) -> Self {
        Self {
            src,
            buf: Bytes::new(),
            buf_start: 0,
            read_size: READ_AHEAD,
        }
    }

    /// Up to `len` bytes at `offset`, as a zero-copy slice of the read-ahead buffer. May
    /// return fewer bytes than asked for; empty means the source ended early.
    pub async fn next(&mut self, offset: u64, len: usize) -> std::io::Result<Bytes> {
        // Cover the whole chunk, not just its first byte: refilling only when `offset` falls
        // outside the buffer would emit a short frame at every read-ahead boundary.
        let buf_end = self.buf_start + self.buf.len() as u64;
        let wanted_end = (offset + len as u64).min(self.src.size());
        let covered = offset >= self.buf_start && wanted_end <= buf_end && offset < buf_end;
        if !covered {
            self.buf = self.src.read(offset, self.read_size.max(len)).await?;
            self.buf_start = offset;
        }
        let start = (offset - self.buf_start) as usize;
        let end = (start + len).min(self.buf.len());
        Ok(self.buf.slice(start..end))
    }
}

/// Rate-limits progress publication and keeps the throughput average.
struct ProgressMeter {
    published_at: Instant,
    published_bytes: u64,
    sampled_at: Instant,
    sampled_bytes: u64,
    rate: f64,
}

impl ProgressMeter {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            // Backdated so the first call is always due: a transfer small enough to finish
            // inside one interval must still report its path and its size.
            published_at: now - PROGRESS_INTERVAL,
            published_bytes: 0,
            sampled_at: now,
            sampled_bytes: 0,
            rate: 0.0,
        }
    }

    /// Whether `bytes_done` is worth publishing yet, updating the throughput average on the
    /// way. Progress is a UI signal, not an accounting record: rate-limiting it keeps a fast
    /// transfer from spending its time in `watch` notifications.
    fn due(&mut self, bytes_done: u64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.sampled_at);
        if elapsed >= RATE_WINDOW {
            let sample = (bytes_done.saturating_sub(self.sampled_bytes)) as f64
                / elapsed.as_secs_f64().max(f64::EPSILON);
            self.rate = if self.rate == 0.0 {
                sample
            } else {
                RATE_ALPHA * sample + (1.0 - RATE_ALPHA) * self.rate
            };
            self.sampled_at = now;
            self.sampled_bytes = bytes_done;
        }
        if now.duration_since(self.published_at) >= PROGRESS_INTERVAL
            || bytes_done.saturating_sub(self.published_bytes) >= PROGRESS_BYTES
        {
            self.published_at = now;
            self.published_bytes = bytes_done;
            true
        } else {
            false
        }
    }

    fn rate(&self) -> f64 {
        self.rate
    }
}

/// One file being served under one epoch.
struct ServeJob<C: FrameTx, D: FrameTx> {
    tx: ServeTx<C, D>,
    file: SharedFile,
    index: u32,
    epoch: u32,
    offset: u64,
    bytes_total: u64,
    window: Arc<Window>,
    progress: watch::Sender<TransferProgress>,
}

/// The request the sender is currently working on: parked until its transport is decided,
/// then started. The credit window travels with it, so there is no state in which a serve
/// could begin without the window its `Credit` frames are landing in.
struct Serving {
    file: u32,
    offset: u64,
    epoch: u32,
    window: Arc<Window>,
    initial_credit_seen: bool,
    acknowledged: u64,
    started: bool,
}

/// How a serve ended. Only a control-stream failure is session-fatal.
enum ServeOutcome {
    Done,
    FileError {
        file: u32,
        epoch: u32,
        message: String,
    },
    DcFailed {
        file: u32,
        epoch: u32,
    },
    ControlFailed(TransportError),
}

/// Serve one file, from `offset` to the end, under one epoch.
///
/// The credit wait lives here rather than in the session loop precisely because this future is
/// an arm of that loop: while it blocks, `Credit`, `Request`, `UseRelay` and `Ice` keep being
/// read. Dropping it mid-serve is safe at every frame boundary — the receiver either discards
/// what arrives under the old epoch or re-requests from its own committed offset.
async fn serve_file<C: FrameTx, D: FrameTx>(job: ServeJob<C, D>) -> ServeOutcome {
    let ServeJob {
        tx,
        file,
        index,
        epoch,
        mut offset,
        bytes_total,
        window,
        progress,
    } = job;

    let source = match open_source(&file.origin, &file.snapshot).await {
        Ok(source) => source,
        Err(e) => return file_error(index, epoch, describe_io(&e)),
    };
    let size = file.meta.size;
    let mut reader = SourceReader::new(source);
    let mut in_flight: u64 = 0;
    let path = tx.path();
    let frame_size = tx.max_frame();

    // Constant for the life of this serve: the transport's frame size does not move under it.
    let budget = max_payload(frame_size, tx.is_length_prefixed());
    if budget == 0 && offset < size {
        return file_error(
            index,
            epoch,
            "this transport cannot carry a chunk".to_string(),
        );
    }

    // The name, index and totals never change while one file is being served, so they are
    // published once rather than cloned into every progress update.
    progress.send_modify(|p| {
        p.phase = Phase::Transferring;
        p.path = path;
        p.file_index = Some(index);
        p.file_name = Some(file.meta.name.clone());
        p.file_total = size;
        p.bytes_total = bytes_total;
        p.frame_size = Some(frame_size);
    });

    while offset < size {
        // Flow control: never put more in flight than the receiver has granted for this epoch.
        let available = loop {
            let available = window.granted().saturating_sub(in_flight);
            if available > 0 {
                break available;
            }
            window.notify.notified().await;
        };
        let want = (budget as u64).min(size - offset).min(available) as usize;
        let chunk = match reader.next(offset, want).await {
            Ok(chunk) => chunk,
            Err(e) => return file_error(index, epoch, describe_io(&e)),
        };
        if chunk.is_empty() {
            return file_error(
                index,
                epoch,
                "the file ended before its declared size".to_string(),
            );
        }
        let len = chunk.len() as u64;
        let header = ChunkHeader {
            file: index,
            epoch,
            offset,
            len: chunk.len() as u32,
        };
        if let Err(e) = tx.send(encode_chunk(header, &chunk)).await {
            return tx.failure(index, epoch, e);
        }
        offset += len;
        in_flight += len;
    }

    // `Done` carries no payload and is never gated on credit: gating it would deadlock the
    // transfer on the last grant.
    let done = match encode_control(&Control::Done { file: index, epoch }) {
        Ok(frame) => frame,
        Err(e) => return file_error(index, epoch, e.to_string()),
    };
    if let Err(e) = tx.send(done).await {
        return tx.failure(index, epoch, e);
    }
    progress.send_modify(|p| {
        p.bytes_total = bytes_total;
    });
    ServeOutcome::Done
}

fn file_error(file: u32, epoch: u32, message: String) -> ServeOutcome {
    ServeOutcome::FileError {
        file,
        epoch,
        message,
    }
}

/// A read that failed because the file moved under us reads the same on every platform: on
/// wasm the browser reports a changed `File` as `NotReadableError` mid-read, which
/// `WebFileSource` maps to the same `InvalidData`.
fn describe_io(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::InvalidData {
        // One owner for this sentence, so the wire message and the sink's own error cannot
        // drift apart.
        crate::file_io::changed_file().to_string()
    } else {
        e.to_string()
    }
}

/// Destination timeouts are not ordinary network timeouts: the browser or filesystem may
/// have committed the operation just before the deadline, so replaying it is unsafe.
async fn bounded_sink<T>(
    cancel: &CancellationToken,
    deadline: Duration,
    fut: impl Future<Output = T>,
) -> Result<T, TransferError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(TransferError::Cancelled),
        _ = sleep(deadline) => Err(TransferError::SinkTimeout),
        value = fut => Ok(value),
    }
}

async fn send_control<X: FrameTx>(tx: &X, control: &Control) -> Result<(), TransportError> {
    let frame = encode_control(control).map_err(TransportError::Protocol)?;
    tx.send(frame).await
}

fn close_session(close: &mut Option<CloseSeam>, code: u32, reason: &[u8]) {
    if let Some(close) = close.take() {
        close(code, reason);
    }
}

/// A future that never completes, for disabling a `select!` arm that has nothing to watch.
async fn never<T>() -> T {
    std::future::pending().await
}

async fn next_ice<D: DataChannel>(pc: Option<&mut D>) -> Option<IceCandidate> {
    match pc {
        Some(pc) => pc.next_local_ice().await,
        None => never().await,
    }
}

async fn recv_frame<X: FrameRx>(rx: Option<&mut X>) -> Result<Option<Bytes>, TransportError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => never().await,
    }
}

async fn watch_open(rx: Option<&mut watch::Receiver<bool>>, want: bool) {
    match rx {
        Some(rx) => {
            let _ = rx.wait_for(|open| *open == want).await;
        }
        None => never().await,
    }
}

async fn watch_terminal(rx: Option<&mut watch::Receiver<PcState>>) {
    match rx {
        Some(rx) => {
            let _ = rx.wait_for(|state| state.is_terminal()).await;
        }
        None => never().await,
    }
}

async fn at_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => never().await,
    }
}

/// Work that needs `&mut` on the peer connection, deferred out of a `select!` arm's handler so
/// no arm has to hold a borrow of it.
enum Signal {
    Answer(String),
    Offer(String),
    Ice(IceCandidate),
    SendIce(IceCandidate),
    CloseDc,
}

// ---------------------------------------------------------------------------------------
// Sender session
// ---------------------------------------------------------------------------------------

/// Transport-generic sender core (design §4.6.1).
pub(crate) async fn run_sender_on<T: FrameTx, R: FrameRx, F: DcFactory>(
    io: SessionIo<T, R, F>,
    files: Arc<Vec<SharedFile>>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: SenderOptions,
) -> Result<(), TransferError> {
    let SessionIo {
        ctrl_tx,
        mut ctrl_rx,
        close,
        dc,
    } = io;
    let ctrl_tx = Arc::new(ctrl_tx);
    let mut close = Some(close);

    // --- handshake ---------------------------------------------------------------------
    // A peer that opens a stream and then says nothing must not pin this task.
    let first = match timeout(opts.hello_deadline, ctrl_rx.recv()).await {
        Err(_) => return Err(TransferError::Timeout),
        Ok(Err(e)) => return Err(TransferError::Transport(e)),
        Ok(Ok(None)) => return Err(TransferError::Transport(TransportError::Closed)),
        Ok(Ok(Some(frame))) => frame,
    };
    let hello = match decode(&first) {
        Ok(Frame::Control(Control::Hello {
            version,
            webrtc,
            cap,
        })) => (version, webrtc, cap),
        Ok(_) => {
            let _ = send_control(&*ctrl_tx, &session_error("expected a Hello frame")).await;
            close_session(&mut close, 0, b"protocol");
            return Err(TransferError::Version { theirs: 0 });
        }
        Err(e) => {
            let _ = send_control(&*ctrl_tx, &session_error("unreadable first frame")).await;
            close_session(&mut close, 0, b"protocol");
            return Err(TransferError::Protocol(e));
        }
    };
    let (their_version, their_webrtc, their_cap) = hello;
    if their_version != PROTOCOL_VERSION {
        let _ = send_control(&*ctrl_tx, &session_error("unsupported protocol version")).await;
        close_session(&mut close, 0, b"version");
        return Err(TransferError::Version {
            theirs: their_version,
        });
    }

    // --- authorization gate (design §2.6, invariant I4) ---------------------------------
    // Nothing has been written to this connection yet, and if the capability does not match,
    // the single `Error` below is the only thing that ever will be: no Hello, no Manifest, no
    // Offer. The comparison is constant time and the offered value is never logged.
    if !cap_eq(&their_cap, &opts.cap) {
        let _ = send_control(&*ctrl_tx, &session_error("unauthorized")).await;
        close_session(&mut close, 0, b"unauthorized");
        log::debug!("rejected a connection: capability mismatch");
        return Err(TransferError::Unauthorized(AuthFailure::Rejected));
    }

    if files.is_empty() {
        let _ = send_control(&*ctrl_tx, &session_error("no files")).await;
        close_session(&mut close, 0, b"no files");
        return Err(TransferError::Io("no files are shared".to_string()));
    }

    send_control(
        &*ctrl_tx,
        &Control::Hello {
            version: PROTOCOL_VERSION,
            webrtc: dc.available(),
            // The sender proves nothing with a capability; the receiver ignores this field.
            cap: [0u8; CAP_LEN],
        },
    )
    .await?;
    let manifest: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();
    let bytes_total: u64 = manifest.iter().map(|m| m.size).sum();
    send_control(&*ctrl_tx, &Control::Manifest { files: manifest }).await?;
    progress.send_modify(|p| {
        p.phase = Phase::Handshake;
        p.bytes_total = bytes_total;
        p.path = ctrl_tx.path();
    });

    // --- capability selection (design §4.6.1 step 3) ------------------------------------
    let mut pc: Option<F::Dc> = None;
    let mut dc_open: Option<watch::Receiver<bool>> = None;
    let mut pc_state: Option<watch::Receiver<PcState>> = None;
    let mut dc_tx: Option<Arc<<F::Dc as DataChannel>::Tx>> = None;
    let mut gathering = false;
    let relay_switch = Arc::new(Notify::new());

    if dc.available() && their_webrtc {
        match dc.create(DcRole::Offerer) {
            Ok(mut channel) => match timeout(opts.aux_deadline, channel.create_offer()).await {
                Ok(Ok(sdp)) => {
                    dc_open = Some(channel.open_watch());
                    pc_state = Some(channel.state_watch());
                    gathering = true;
                    pc = Some(channel);
                    send_control(&*ctrl_tx, &Control::Offer { sdp }).await?;
                }
                _ => {
                    channel.close();
                    log::debug!("no offer could be created; serving over the relay");
                }
            },
            Err(e) => log::debug!("no data channel available ({e}); serving over the relay"),
        }
    }

    // --- session loop ------------------------------------------------------------------
    let mut relay_forced = false;
    let mut epoch_seen: u32 = 0;
    let mut serving: Option<Serving> = None;
    let mut dcep_deadline: Option<Instant> = None;
    let mut dc_expired = false;
    let mut signals: Vec<Signal> = Vec::new();
    let mut serve = std::pin::pin!(MaybeFuture::default());
    let mut verified = false;
    let mut receipt_deadline = None;
    // One rate meter for the whole session: restarting it for each file or
    // relay epoch would hide throughput for small files and resumed transfers.
    let mut acknowledged_bytes = 0;
    let mut meter = ProgressMeter::new();
    let mut last_ack_at: Option<Instant> = None;
    let outcome: Result<(), TransferError>;

    loop {
        if verified {
            // The receiver has already verified every hash and finished its
            // sinks. Wait for it to read our acknowledgment before closing
            // the iroh connection: send_control only enqueues a frame.
            tokio::select! {
                biased;
                _ = at_deadline(receipt_deadline) => { outcome = Ok(()); break; }
                _ = cancel.cancelled() => { outcome = Ok(()); break; }
                frame = ctrl_rx.recv() => match frame {
                    Ok(Some(frame)) => {
                        if let Ok(Frame::Control(Control::Error { message, .. })) = decode(&frame) {
                            outcome = Err(TransferError::Remote(message));
                            break;
                        }
                    }
                    Ok(None) | Err(_) => { outcome = Ok(()); break; }
                },
            }
            continue;
        }
        // Applied before the next wait for the same reason as in the receiver: a queued
        // `Answer` or candidate must not sit until something else happens to wake the loop.
        for signal in signals.drain(..) {
            match signal {
                Signal::Answer(sdp) => {
                    if let Some(pc) = pc.as_mut() {
                        let _ = timeout(opts.aux_deadline, pc.accept_answer(&sdp)).await;
                    }
                }
                Signal::Offer(_) => {}
                Signal::Ice(candidate) => {
                    if let Some(pc) = pc.as_mut() {
                        // Late candidates are applied, never rejected.
                        let _ = timeout(opts.aux_deadline, pc.add_ice(&candidate)).await;
                    }
                }
                Signal::SendIce(candidate) => {
                    // Fatal, exactly as on the receiver: if the control stream cannot take a
                    // frame, the session has nothing left to talk over.
                    send_control(
                        &*ctrl_tx,
                        &Control::Ice {
                            candidate: candidate.candidate,
                            sdp_mid: candidate.sdp_mid,
                            sdp_mline_index: candidate.sdp_mline_index,
                        },
                    )
                    .await?;
                }
                Signal::CloseDc => {
                    if let Some(pc) = pc.as_ref() {
                        pc.close();
                    }
                }
            }
        }

        let parked = serving.as_ref().is_some_and(|s| !s.started);
        let watching_ice = gathering && pc.is_some();
        let watching_open = parked && dc_open.is_some();
        let watching_state = parked && pc_state.is_some();
        let rate_idle_deadline = last_ack_at.map(|at| {
            // At a slow receive rate, accumulating a 256 KiB credit can
            // legitimately take much longer than three seconds. Allow
            // two expected credit intervals before calling the sample
            // stale, but never outlive the session's inactivity budget.
            let seconds = if meter.rate > 0.0 {
                (2.0 * CREDIT_GRAIN as f64 / meter.rate)
                    .clamp(RATE_IDLE_MIN.as_secs_f64(), INACTIVITY.as_secs_f64())
            } else {
                INACTIVITY.as_secs_f64()
            };
            at + Duration::from_secs_f64(seconds)
        });

        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                outcome = Err(TransferError::Cancelled);
                break;
            }
            frame = ctrl_rx.recv() => {
                let frame = match frame {
                    Ok(Some(frame)) => frame,
                    Ok(None) => {
                        outcome = Err(TransferError::Transport(TransportError::Closed));
                        break;
                    }
                    Err(e) => { outcome = Err(TransferError::Transport(e)); break; }
                };
                let control = match decode(&frame) {
                    Ok(Frame::Control(control)) => control,
                    // A chunk on the sender's control stream is a receiver bug; ignore it
                    // rather than tearing down a session that is otherwise healthy.
                    Ok(Frame::Chunk { .. }) => continue,
                    Err(e) => { outcome = Err(TransferError::Protocol(e)); break; }
                };
                match control {
                    Control::Request { file, offset, epoch } => {
                        // A Request always pre-empts: whatever was being served, and whatever
                        // was parked, belongs to an older epoch from this point on.
                        serve.as_mut().set_none();
                        serving = None;
                        dcep_deadline = None;
                        dc_expired = false;
                        let valid = (file as usize) < files.len()
                            && epoch > epoch_seen
                            && offset <= files[file as usize].meta.size;
                        if !valid {
                            let _ = send_control(&*ctrl_tx, &Control::Error {
                                file: Some(file),
                                epoch: Some(epoch),
                                message: "invalid request".to_string(),
                            }).await;
                            continue;
                        }
                        // The epoch and its window are set here, before any parking: the
                        // receiver's initial `Credit` is already behind this frame on the same
                        // ordered stream, and arrives while the request may still be parked.
                        epoch_seen = epoch;
                        let bytes_before: u64 =
                            files[..file as usize].iter().map(|f| f.meta.size).sum();
                        progress.send_modify(|p| {
                            p.file_done = offset;
                            p.bytes_done = bytes_before + offset;
                        });
                        serving = Some(Serving {
                            file,
                            offset,
                            epoch,
                            window: Arc::new(Window::new()),
                            initial_credit_seen: false,
                            acknowledged: offset,
                            started: false,
                        });
                    }
                    Control::Credit { epoch, bytes } => {
                        match serving.as_mut().filter(|s| s.epoch == epoch) {
                            Some(current) => {
                                current.window.grant(bytes);
                                if !current.initial_credit_seen {
                                    // The first grant is capacity, not bytes written. Start
                                    // timing when the receiver is ready to receive.
                                    current.initial_credit_seen = true;
                                    if acknowledged_bytes == 0 {
                                        meter = ProgressMeter::new();
                                    }
                                } else {
                                    let index = current.file as usize;
                                    let size = files[index].meta.size;
                                    let next = current.acknowledged.saturating_add(bytes).min(size);
                                    let newly_acked = next - current.acknowledged;
                                    current.acknowledged = next;
                                    if newly_acked > 0 {
                                        acknowledged_bytes += newly_acked;
                                        last_ack_at = Some(Instant::now());
                                    }
                                    let due = meter.due(acknowledged_bytes);
                                    if due || current.acknowledged == size {
                                        let bytes_before: u64 =
                                            files[..index].iter().map(|f| f.meta.size).sum();
                                        let (done, rate) = (current.acknowledged, meter.rate());
                                        progress.send_modify(|p| {
                                            p.file_done = done;
                                            p.bytes_done = bytes_before + done;
                                            p.bytes_per_sec = rate;
                                        });
                                    }
                                }
                                log::debug!("credit +{bytes} for epoch {epoch}");
                            }
                            None => log::debug!("discarded credit for stale epoch {epoch}"),
                        }
                    }
                    Control::UseRelay => {
                        // A state flag only: the running serve is left alone, because the
                        // receiver's next Request is what pre-empts it (design §2.3).
                        relay_forced = true;
                        dc_tx = None;
                        dc_open = None;
                        signals.push(Signal::CloseDc);
                        relay_switch.notify_one();
                        log::debug!("receiver asked for the relay path");
                    }
                    Control::Answer { sdp } => signals.push(Signal::Answer(sdp)),
                    Control::Ice { candidate, sdp_mid, sdp_mline_index } => {
                        signals.push(Signal::Ice(IceCandidate { candidate, sdp_mid, sdp_mline_index }));
                    }
                    Control::Error { message, .. } => {
                        outcome = Err(TransferError::Remote(message));
                        break;
                    }
                    Control::Verified { files: count, bytes } => {
                        if count as usize != files.len() || bytes != bytes_total {
                            outcome = Err(TransferError::Protocol(ProtocolError::InvalidReceipt));
                            break;
                        }
                        verified = true;
                        receipt_deadline = Some(Instant::now() + opts.aux_deadline);
                        progress.send_modify(|p| {
                            p.file_done = p.file_total;
                            p.bytes_done = bytes_total;
                            p.bytes_per_sec = 0.0;
                            p.phase = Phase::Verifying;
                        });
                        if let Err(e) = send_control(&*ctrl_tx, &Control::VerifiedAck).await {
                            log::debug!("could not acknowledge verified receipt: {e}");
                            outcome = Ok(());
                            break;
                        }
                    }
                    _ => {}
                }
            }
            _ = at_deadline(rate_idle_deadline), if rate_idle_deadline.is_some() => {
                last_ack_at = None;
                meter.rate = 0.0;
                meter.sampled_at = Instant::now();
                meter.sampled_bytes = acknowledged_bytes;
                progress.send_modify(|p| p.bytes_per_sec = 0.0);
            }
            candidate = next_ice(pc.as_mut()), if watching_ice => {
                match candidate {
                    Some(candidate) => signals.push(Signal::SendIce(candidate)),
                    None => gathering = false,
                }
            }
            _ = watch_open(dc_open.as_mut(), true), if watching_open => {}
            _ = watch_terminal(pc_state.as_mut()), if watching_state => {
                // The channel will never open; start the parked request on the relay.
                dc_expired = true;
            }
            _ = at_deadline(dcep_deadline), if parked => dc_expired = true,
            served = &mut serve => {
                match served {
                    ServeOutcome::Done => {
                        log::debug!("serve complete");
                    }
                    ServeOutcome::FileError { file, epoch, message } => {
                        log::debug!("file {file} could not be served: {message}");
                        let _ = send_control(&*ctrl_tx, &Control::Error {
                            file: Some(file),
                            epoch: Some(epoch),
                            message,
                        }).await;
                    }
                    ServeOutcome::DcFailed { file, epoch } => {
                        // The session stays up: the receiver notices the dead channel and
                        // re-requests over the relay under a new epoch.
                        log::debug!("data channel failed while serving file {file} (epoch {epoch})");
                    }
                    ServeOutcome::ControlFailed(e) => {
                        outcome = Err(TransferError::Transport(e));
                        break;
                    }
                }
            }
        }

        // Start a parked request as soon as its transport is decided (design §2.4).
        if let Some(request) = serving.as_ref().filter(|s| !s.started) {
            let (file, offset, epoch) = (request.file, request.offset, request.epoch);
            let open_now = dc_open.as_ref().map(|w| *w.borrow()).unwrap_or(false);
            let dead = pc_state
                .as_ref()
                .map(|w| w.borrow().is_terminal())
                .unwrap_or(true);

            let tx = if relay_forced || pc.is_none() || dead || dc_expired {
                Some(ServeTx::Control(ctrl_tx.clone()))
            } else if open_now {
                if dc_tx.is_none() {
                    if let Some(pc) = pc.as_mut() {
                        if let Some((tx, _rx)) = pc.split(
                            cancel.clone(),
                            relay_switch.clone(),
                            INITIAL_WINDOW as usize,
                        ) {
                            dc_tx = Some(Arc::new(tx));
                        }
                    }
                }
                dc_tx.clone().map(ServeTx::Dc)
            } else {
                // Park: never await the channel opening inside a handler, or the very `Ice`
                // frames it needs would stop flowing (design §2.4).
                if dcep_deadline.is_none() {
                    dcep_deadline = Some(Instant::now() + opts.dcep_deadline);
                }
                None
            };

            if let Some(tx) = tx {
                dcep_deadline = None;
                let window = request.window.clone();
                log::debug!(
                    "serving file {file} from offset {offset} under epoch {epoch} on {:?}",
                    tx.path()
                );
                serve.as_mut().set_future(serve_file(ServeJob {
                    tx,
                    file: files[file as usize].clone(),
                    index: file,
                    epoch,
                    offset,
                    bytes_total,
                    window,
                    progress: progress.clone(),
                }));
                if let Some(request) = serving.as_mut() {
                    request.started = true;
                }
            }
        }
    }

    match &outcome {
        Ok(()) => {
            progress.send_modify(|p| p.phase = Phase::Complete { saved: Vec::new() });
            close_session(&mut close, 0, b"done");
        }
        Err(TransferError::Cancelled) => {
            progress.send_modify(|p| p.phase = Phase::Cancelled);
            close_session(&mut close, 0, b"cancelled");
        }
        Err(e) => {
            let message = e.to_string();
            progress.send_modify(|p| {
                p.phase = Phase::Failed;
                p.error = Some(message);
            });
            close_session(&mut close, 0, b"failed");
        }
    }
    outcome
}

fn session_error(message: &str) -> Control {
    Control::Error {
        file: None,
        epoch: None,
        message: message.to_string(),
    }
}

// ---------------------------------------------------------------------------------------
// Receiver session
// ---------------------------------------------------------------------------------------

/// Which transport delivered the most recent frame of the current epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arrival {
    Control,
    Dc,
}

/// The file currently being received.
struct FileState {
    index: u32,
    expected: u64,
    epoch_start: u64,
    granted: u64,
    consumed_since_grant: u64,
    last_grant_at: Instant,
    hasher: blake3::Hasher,
    active_rx: Arrival,
}

/// Transport-generic receiver core (design §4.6.2).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) async fn run_receiver_on<T: FrameTx, R: FrameRx, F: DcFactory>(
    io: SessionIo<T, R, F>,
    mut commands: mpsc::Receiver<ReceiveCommand>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError> {
    let mut state = ReceiverState::default();
    let result = run_receiver_session(
        io,
        &mut commands,
        &progress,
        &cancel,
        &opts,
        &mut state,
        false,
    )
    .await;
    finish_receiver(result, &mut state, &progress, &opts).await
}

/// Run one authenticated control connection while keeping the receiver state outside it.
/// The iroh wrapper can therefore replace a failed connection without replacing sinks,
/// verified files, offsets, or hash state.
async fn run_receiver_session<T: FrameTx, R: FrameRx, F: DcFactory>(
    io: SessionIo<T, R, F>,
    commands: &mut mpsc::Receiver<ReceiveCommand>,
    progress: &watch::Sender<TransferProgress>,
    cancel: &CancellationToken,
    opts: &ReceiveOptions,
    state: &mut ReceiverState,
    reconnecting: bool,
) -> Result<Vec<SavedFile>, TransferError> {
    let SessionIo {
        ctrl_tx,
        mut ctrl_rx,
        close,
        dc,
    } = io;
    let ctrl_tx = Arc::new(ctrl_tx);
    let mut close = Some(close);

    // A link with no access code is refused before anything is dialled or asked for.
    let Some(cap) = opts.cap else {
        return Err(TransferError::Unauthorized(AuthFailure::MissingCap));
    };

    // A fresh control connection resumes immediately over the encrypted relay. It does not
    // spend another WebRTC negotiation interval before asking for the known remaining offset.
    let offer_webrtc = !reconnecting && dc.available() && !opts.force_relay;
    send_control(
        &*ctrl_tx,
        &Control::Hello {
            version: PROTOCOL_VERSION,
            webrtc: offer_webrtc,
            cap,
        },
    )
    .await?;
    progress.send_modify(|p| {
        p.phase = Phase::Handshake;
        p.path = ctrl_tx.path();
    });

    let result = match receiver_handshake(&mut ctrl_rx, opts.inactivity).await {
        Err(e) => Err(e),
        Ok(manifest) if state.manifest.is_empty() => {
            state.bytes_total = manifest.iter().map(|m| m.size).sum();
            state.manifest = manifest.clone();
            progress.send_modify(|p| {
                p.phase = Phase::AwaitingSave { manifest };
                p.bytes_total = state.bytes_total;
            });
            receive_loop(
                ReceiveCtx {
                    ctrl_tx: &ctrl_tx,
                    ctrl_rx: &mut ctrl_rx,
                    dc: &dc,
                    commands,
                    progress,
                    cancel,
                    opts,
                },
                state,
                false,
            )
            .await
        }
        Ok(manifest) if state.manifest != manifest => Err(TransferError::ManifestChanged),
        Ok(_) => {
            if state.sinks.is_none() {
                let manifest = state.manifest.clone();
                progress.send_modify(|p| {
                    p.phase = Phase::AwaitingSave { manifest };
                    p.bytes_total = state.bytes_total;
                });
            }
            {
                let mut session_opts = opts.clone();
                if reconnecting {
                    session_opts.force_relay = true;
                }
                receive_loop(
                    ReceiveCtx {
                        ctrl_tx: &ctrl_tx,
                        ctrl_rx: &mut ctrl_rx,
                        dc: &dc,
                        commands,
                        progress,
                        cancel,
                        opts: &session_opts,
                    },
                    state,
                    reconnecting,
                )
                .await
            }
        }
    };
    close_session(
        &mut close,
        0,
        if result.is_ok() {
            b"done"
        } else if reconnecting {
            b"reconnect"
        } else {
            b"failed"
        },
    );
    result
}

/// Publish one terminal result and clean up unfinished destinations exactly once.
async fn finish_receiver(
    result: Result<Vec<SavedFile>, TransferError>,
    state: &mut ReceiverState,
    progress: &watch::Sender<TransferProgress>,
    opts: &ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError> {
    match &result {
        Ok(saved) => {
            let saved = saved.clone();
            progress.send_modify(|p| {
                p.phase = Phase::Complete { saved };
                p.bytes_done = p.bytes_total;
            });
        }
        Err(e) => {
            abort_sinks(&mut state.sinks, opts.aux_deadline).await;
            let cancelled = matches!(e, TransferError::Cancelled);
            let message = e.to_string();
            progress.send_modify(|p| {
                p.phase = if cancelled {
                    Phase::Cancelled
                } else {
                    Phase::Failed
                };
                p.error = Some(message);
            });
        }
    }
    result
}

/// Everything the receive loop carries between iterations.
struct ReceiverState {
    manifest: Vec<FileMeta>,
    bytes_total: u64,
    sinks: Option<Vec<Option<AnySink>>>,
    saved: Vec<SavedFile>,
    current: Option<FileState>,
    next_file: u32,
    epoch: u32,
    meter: ProgressMeter,
    bytes_done: u64,
    resume_hashers: Vec<Option<blake3::Hasher>>,
}

impl Default for ReceiverState {
    fn default() -> Self {
        Self {
            manifest: Vec::new(),
            bytes_total: 0,
            sinks: None,
            saved: Vec::new(),
            current: None,
            next_file: 0,
            epoch: 0,
            meter: ProgressMeter::new(),
            bytes_done: 0,
            resume_hashers: Vec::new(),
        }
    }
}

/// Read the sender's `Hello` and `Manifest`.
async fn receiver_handshake<R: FrameRx>(
    ctrl_rx: &mut R,
    deadline: Duration,
) -> Result<Vec<FileMeta>, TransferError> {
    let mut manifest = None;
    let mut greeted = false;
    while manifest.is_none() {
        let frame = match timeout(deadline, ctrl_rx.recv()).await {
            Err(_) => return Err(TransferError::Timeout),
            Ok(Err(e)) => return Err(TransferError::Transport(e)),
            // The sender always writes an `Error` before closing, so a bare close is a
            // network fault and must read as one — never as a rejection.
            Ok(Ok(None)) => return Err(TransferError::Transport(TransportError::Closed)),
            Ok(Ok(Some(frame))) => frame,
        };
        match decode(&frame).map_err(TransferError::Protocol)? {
            Frame::Control(Control::Hello { version, .. }) => {
                if version != PROTOCOL_VERSION {
                    return Err(TransferError::Version { theirs: version });
                }
                greeted = true;
            }
            Frame::Control(Control::Manifest { files }) => {
                if !greeted {
                    return Err(TransferError::Protocol(ProtocolError::Empty));
                }
                manifest = Some(files);
            }
            Frame::Control(Control::Error { message, .. }) => {
                return Err(if message == "unauthorized" {
                    TransferError::Unauthorized(AuthFailure::Rejected)
                } else {
                    TransferError::Remote(message)
                });
            }
            _ => {}
        }
    }
    Ok(manifest.unwrap_or_default())
}

/// The session's transports and knobs, bundled so the loop keeps one parameter.
struct ReceiveCtx<'a, T: FrameTx, R: FrameRx, F: DcFactory> {
    ctrl_tx: &'a Arc<T>,
    ctrl_rx: &'a mut R,
    dc: &'a F,
    commands: &'a mut mpsc::Receiver<ReceiveCommand>,
    progress: &'a watch::Sender<TransferProgress>,
    cancel: &'a CancellationToken,
    opts: &'a ReceiveOptions,
}

/// The receiver's single `select!` loop, from the manifest to the last file.
///
/// Everything lives in this one loop on purpose (design §4.6.2 step 3): signalling keeps
/// flowing while a file is being received, the data channel can open or die at any point, and
/// nothing outside the loop ever awaits a WebRTC event. Frame handling happens *after* the
/// `select!` so no arm has to hold a borrow of the state it wants to change.
async fn receive_loop<T: FrameTx, R: FrameRx, F: DcFactory>(
    ctx: ReceiveCtx<'_, T, R, F>,
    state: &mut ReceiverState,
    reconnecting: bool,
) -> Result<Vec<SavedFile>, TransferError> {
    let ReceiveCtx {
        ctrl_tx,
        ctrl_rx,
        dc,
        commands,
        progress,
        cancel,
        opts,
    } = ctx;

    let mut pc: Option<F::Dc> = None;
    let mut dc_open: Option<watch::Receiver<bool>> = None;
    let mut pc_state: Option<watch::Receiver<PcState>> = None;
    let mut dc_rx: Option<<F::Dc as DataChannel>::Rx> = None;
    let mut dc_was_open = false;
    let mut gathering = false;
    let mut use_relay = opts.force_relay;
    let mut webrtc_deadline: Option<Instant> = None;
    let mut inactivity_deadline: Option<Instant> = None;
    let mut commands_open = true;
    let mut killed_dc = false;
    let mut signals: Vec<Signal> = Vec::new();
    let relay_switch = Arc::new(Notify::new());

    if reconnecting {
        send_control(&**ctrl_tx, &Control::UseRelay).await?;
        if restart_current(ctrl_tx, state, progress, opts.initial_window).await? {
            inactivity_deadline = Some(Instant::now() + opts.inactivity);
        }
    }

    loop {
        if state.sinks.is_some()
            && state.current.is_none()
            && state.next_file as usize >= state.manifest.len()
        {
            let saved = std::mem::take(&mut state.saved);
            // All hashes matched and every destination has finished. A full
            // progress bar or stream closure alone cannot prove remote save.
            let receipt = Control::Verified {
                files: state.manifest.len() as u32,
                bytes: state.bytes_total,
            };
            if let Err(e) = send_control(&**ctrl_tx, &receipt).await {
                log::debug!("could not send verified transfer receipt: {e}");
                return Ok(saved); // verified local copy remains successful
            }
            // The writer queues control frames, so let the sender consume the
            // receipt before closing this connection. The acknowledgment is
            // best effort: a lost reply cannot invalidate a saved, hashed file.
            match timeout(opts.aux_deadline, ctrl_rx.recv()).await {
                Ok(Ok(Some(frame)))
                    if matches!(decode(&frame), Ok(Frame::Control(Control::VerifiedAck))) => {}
                other => log::debug!("verified receipt was not acknowledged: {other:?}"),
            }
            return Ok(saved);
        }

        let mut incoming: Option<(Result<Option<Bytes>, TransportError>, Arrival)> = None;
        let mut saved_arrived = false;
        let mut dc_gone = false;
        let mut deadline_fired = false;

        // Deferred peer-connection work runs *before* the next wait, never after it: a signal
        // queued by the previous iteration's frame handler (an `Offer`, say) must be applied
        // now, or the loop would block waiting for frames that only the resulting channel can
        // deliver.
        drain_signals(
            SignalCtx {
                ctrl_tx,
                dc,
                opts,
                cancel,
                pc: &mut pc,
                dc_open: &mut dc_open,
                pc_state: &mut pc_state,
                gathering: &mut gathering,
            },
            &mut signals,
        )
        .await?;

        // The data channel opened: take its halves.
        if dc_was_open && dc_rx.is_none() && !use_relay {
            if let Some(pc) = pc.as_mut() {
                let receive_window =
                    usize::try_from(opts.initial_window).unwrap_or(usize::MAX - MAX_FRAME);
                if let Some((_tx, rx)) =
                    pc.split(cancel.clone(), relay_switch.clone(), receive_window)
                {
                    dc_rx = Some(rx);
                }
            }
        }

        // Computed here, after the signals: a channel created one line above must be watched
        // in *this* wait, not the next one.
        let watching_ice = gathering && pc.is_some();
        let watching_open = dc_open.is_some();
        let watching_state = pc_state.is_some();
        let deadline = earliest(webrtc_deadline, inactivity_deadline);

        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TransferError::Cancelled),
            command = commands.recv(), if commands_open => {
                match command {
                    Some(ReceiveCommand::Save(sinks)) => {
                        if sinks.len() != state.manifest.len()
                            || sinks.iter().any(|sink| sink.bytes_written() != 0)
                        {
                            return Err(TransferError::Io(
                                "fresh destinations do not match the file manifest".to_string(),
                            ));
                        }
                        state.sinks = Some(sinks.into_iter().map(Some).collect());
                        state.resume_hashers =
                            (0..state.manifest.len()).map(|_| None).collect();
                        saved_arrived = true;
                    }
                    Some(ReceiveCommand::Resume(files)) => {
                        if files.len() != state.manifest.len()
                            || files
                                .iter()
                                .zip(&state.manifest)
                                .any(|(file, meta)| {
                                    file.meta != *meta || file.sink.bytes_written() > meta.size
                                })
                        {
                            return Err(TransferError::ManifestChanged);
                        }
                        state.bytes_done =
                            files.iter().map(|file| file.sink.bytes_written()).sum();
                        let (sinks, hashers): (Vec<_>, Vec<_>) = files
                            .into_iter()
                            .map(|file| (Some(file.sink), Some(file.hasher)))
                            .unzip();
                        state.sinks = Some(sinks);
                        state.resume_hashers = hashers;
                        progress.send_modify(|p| p.bytes_done = state.bytes_done);
                        saved_arrived = true;
                    }
                    None => commands_open = false,
                }
            }
            frame = ctrl_rx.recv() => incoming = Some((frame, Arrival::Control)),
            frame = recv_frame(dc_rx.as_mut()), if dc_rx.is_some() => {
                incoming = Some((frame, Arrival::Dc));
            }
            candidate = next_ice(pc.as_mut()), if watching_ice => {
                match candidate {
                    Some(candidate) => signals.push(Signal::SendIce(candidate)),
                    None => gathering = false,
                }
            }
            _ = watch_open(dc_open.as_mut(), !dc_was_open), if watching_open => {
                if dc_was_open {
                    dc_gone = true;
                } else {
                    log::info!("WebRTC data channel opened; direct transfer selected");
                    dc_was_open = true;
                }
            }
            _ = watch_terminal(pc_state.as_mut()), if watching_state => {
                dc_gone = true;
                pc_state = None;
            }
            _ = at_deadline(deadline), if deadline.is_some() => deadline_fired = true,
        }

        // --- the data channel died -----------------------------------------------------
        if dc_gone {
            let on_dc = state
                .current
                .as_ref()
                .map(|f| f.active_rx == Arrival::Dc)
                .unwrap_or(false);
            abandon_dc(
                ctrl_tx,
                &mut dc_rx,
                &mut dc_open,
                &mut dc_was_open,
                &mut use_relay,
            )
            .await?;
            // Nothing was riding the channel, so nothing is in danger: no new request, and no
            // epoch burned. Later files simply will not pick it.
            if on_dc {
                restart_current(ctrl_tx, state, progress, opts.initial_window).await?;
                inactivity_deadline = Some(Instant::now() + opts.inactivity);
            }
        }

        // --- deadlines -----------------------------------------------------------------
        if deadline_fired {
            let now = Instant::now();
            if webrtc_deadline.map(|at| now >= at).unwrap_or(false) {
                webrtc_deadline = None;
                if !dc_was_open {
                    log::warn!("WebRTC data channel did not open in time; using the iroh relay");
                    use_relay = true;
                    dc_rx = None;
                    dc_open = None;
                    signals.push(Signal::CloseDc);
                    send_control(&**ctrl_tx, &Control::UseRelay).await?;
                }
            }
            if inactivity_deadline.map(|at| now >= at).unwrap_or(false) {
                inactivity_deadline = None;
                let on_dc = state
                    .current
                    .as_ref()
                    .map(|f| f.active_rx == Arrival::Dc)
                    .unwrap_or(false);
                if on_dc {
                    // Nothing has arrived on the channel for too long: treat it as dead and
                    // ask for the rest over the relay.
                    signals.push(Signal::CloseDc);
                    abandon_dc(
                        ctrl_tx,
                        &mut dc_rx,
                        &mut dc_open,
                        &mut dc_was_open,
                        &mut use_relay,
                    )
                    .await?;
                    if restart_current(ctrl_tx, state, progress, opts.initial_window).await? {
                        inactivity_deadline = Some(Instant::now() + opts.inactivity);
                    }
                } else if state.current.is_some() {
                    return Err(TransferError::Timeout);
                }
            }
        }

        // --- one incoming frame ---------------------------------------------------------
        if let Some((frame, from)) = incoming {
            let frame = match frame {
                Ok(Some(frame)) => Some(frame),
                Ok(None) | Err(TransportError::Closed) if from == Arrival::Dc => {
                    dc_rx = None;
                    None
                }
                Ok(None) => {
                    return Err(TransferError::Transport(TransportError::Closed));
                }
                Err(e) => {
                    if from == Arrival::Dc {
                        dc_rx = None;
                        None
                    } else {
                        return Err(TransferError::Transport(e));
                    }
                }
            };
            if let Some(frame) = frame {
                let decoded = decode(&frame).map_err(TransferError::Protocol)?;
                handle_frame(
                    HandleCtx {
                        ctrl_tx,
                        progress,
                        cancel,
                        opts,
                        from,
                        signals: &mut signals,
                        killed_dc: &mut killed_dc,
                        inactivity_deadline: &mut inactivity_deadline,
                    },
                    state,
                    decoded,
                )
                .await?;
            }
        }

        // --- start the next file when there is one and a transport to ask over ----------
        let ready_to_request = state.sinks.is_some()
            && state.current.is_none()
            && (state.next_file as usize) < state.manifest.len();
        if ready_to_request {
            let waiting_for_dc = !use_relay && pc.is_some() && !dc_was_open;
            if waiting_for_dc {
                if webrtc_deadline.is_none() {
                    // Keep looping while the channel negotiates: this wait is an arm, never an
                    // await inside a handler, which is what keeps ICE flowing meanwhile.
                    webrtc_deadline = Some(Instant::now() + opts.webrtc_open);
                    if state.sinks.is_some() {
                        progress.send_modify(|p| p.phase = Phase::Signaling);
                    }
                }
            } else {
                let index = state.next_file;
                let sink_offset = state
                    .sinks
                    .as_ref()
                    .and_then(|sinks| sinks[index as usize].as_ref())
                    .map(|sink| sink.bytes_written())
                    .unwrap_or(0);
                state.epoch += 1;
                let epoch = state.epoch;
                state.current = Some(FileState {
                    index,
                    expected: sink_offset,
                    epoch_start: sink_offset,
                    granted: opts.initial_window,
                    consumed_since_grant: 0,
                    last_grant_at: Instant::now(),
                    hasher: state
                        .resume_hashers
                        .get_mut(index as usize)
                        .and_then(Option::take)
                        .unwrap_or_default(),
                    active_rx: if dc_was_open && !use_relay {
                        Arrival::Dc
                    } else {
                        Arrival::Control
                    },
                });
                progress.send_modify(|p| {
                    p.phase = Phase::Transferring;
                    p.file_index = Some(index);
                    p.file_name = Some(state.manifest[index as usize].name.clone());
                    p.file_done = sink_offset;
                    p.file_total = state.manifest[index as usize].size;
                });
                request_file(ctrl_tx, index, sink_offset, epoch, opts.initial_window).await?;
                inactivity_deadline = Some(Instant::now() + opts.inactivity);
                if saved_arrived {
                    log::debug!("save accepted; requesting file {index}");
                }
            }
        }
    }
}

/// What applying a queued [`Signal`] needs.
struct SignalCtx<'a, T: FrameTx, F: DcFactory> {
    ctrl_tx: &'a Arc<T>,
    dc: &'a F,
    opts: &'a ReceiveOptions,
    cancel: &'a CancellationToken,
    pc: &'a mut Option<F::Dc>,
    dc_open: &'a mut Option<watch::Receiver<bool>>,
    pc_state: &'a mut Option<watch::Receiver<PcState>>,
    gathering: &'a mut bool,
}

/// Apply the peer-connection work a frame handler queued.
///
/// It is deferred out of the `select!` handlers so no arm has to hold a borrow of the
/// connection, and applied at the top of the next iteration so it never waits on a wake-up.
async fn drain_signals<T: FrameTx, F: DcFactory>(
    ctx: SignalCtx<'_, T, F>,
    signals: &mut Vec<Signal>,
) -> Result<(), TransferError> {
    let SignalCtx {
        ctrl_tx,
        dc,
        opts,
        cancel,
        pc,
        dc_open,
        pc_state,
        gathering,
    } = ctx;
    for signal in signals.drain(..) {
        match signal {
            Signal::Offer(sdp) => match dc.create(DcRole::Answerer) {
                Ok(mut channel) => {
                    match timeout(opts.aux_deadline, channel.accept_offer(&sdp)).await {
                        Ok(Ok(answer)) => {
                            *dc_open = Some(channel.open_watch());
                            *pc_state = Some(channel.state_watch());
                            *gathering = true;
                            *pc = Some(channel);
                            send_control(&**ctrl_tx, &Control::Answer { sdp: answer }).await?;
                        }
                        _ => {
                            channel.close();
                            log::debug!("could not answer the offer; using the relay");
                        }
                    }
                }
                Err(e) => log::debug!("no data channel available ({e}); using the relay"),
            },
            Signal::Ice(candidate) => {
                if let Some(pc) = pc.as_mut() {
                    // Late candidates are applied, never rejected.
                    let _ = timeout(opts.aux_deadline, pc.add_ice(&candidate)).await;
                }
            }
            Signal::SendIce(candidate) => {
                send_control(
                    &**ctrl_tx,
                    &Control::Ice {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    },
                )
                .await?;
            }
            Signal::Answer(_) => {}
            Signal::CloseDc => {
                if let Some(pc) = pc.as_ref() {
                    pc.close();
                }
            }
        }
    }
    let _ = cancel;
    Ok(())
}

/// Stop using the data channel, tell the sender so later files do not pick it either, and
/// leave the flags in one consistent state. The single owner of that bookkeeping: it used to
/// be written out at each call site, and the copies had already drifted.
async fn abandon_dc<T: FrameTx, X: FrameRx>(
    ctrl_tx: &Arc<T>,
    dc_rx: &mut Option<X>,
    dc_open: &mut Option<watch::Receiver<bool>>,
    dc_was_open: &mut bool,
    use_relay: &mut bool,
) -> Result<(), TransferError> {
    *dc_rx = None;
    *dc_open = None;
    *dc_was_open = false;
    *use_relay = true;
    send_control(&**ctrl_tx, &Control::UseRelay).await?;
    Ok(())
}

/// Ask for the current file again, under a new epoch, from the offset the sink has actually
/// taken — so whatever the abandoned serve still has in flight is discarded rather than mixed
/// in. The hasher is deliberately untouched, which is what makes the resume free.
///
/// Returns whether there was a transfer in flight to rescue.
async fn restart_current<T: FrameTx>(
    ctrl_tx: &Arc<T>,
    state: &mut ReceiverState,
    progress: &watch::Sender<TransferProgress>,
    initial_window: u64,
) -> Result<bool, TransferError> {
    if state.current.is_none() {
        return Ok(false);
    }
    state.epoch += 1;
    let epoch = state.epoch;
    let (index, offset) = {
        let file = state.current.as_mut().expect("checked just above");
        file.epoch_start = file.expected;
        file.granted = initial_window;
        file.consumed_since_grant = 0;
        file.last_grant_at = Instant::now();
        file.active_rx = Arrival::Control;
        (file.index, file.expected)
    };
    progress.send_modify(|p| p.phase = Phase::Switching);
    log::debug!("switching to the relay: file {index} from offset {offset}");
    request_file(ctrl_tx, index, offset, epoch, initial_window).await?;
    Ok(true)
}

/// Send a `Request` and the explicit initial `Credit` that follows it, in that order on the
/// same ordered stream: a `Request` grants nothing by itself.
async fn request_file<T: FrameTx>(
    ctrl_tx: &Arc<T>,
    file: u32,
    offset: u64,
    epoch: u32,
    initial_window: u64,
) -> Result<(), TransferError> {
    send_control(
        &**ctrl_tx,
        &Control::Request {
            file,
            offset,
            epoch,
        },
    )
    .await?;
    send_control(
        &**ctrl_tx,
        &Control::Credit {
            epoch,
            bytes: initial_window,
        },
    )
    .await?;
    log::debug!("requested file {file} from {offset} (epoch {epoch}, window {initial_window})");
    Ok(())
}

fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// What `handle_frame` needs besides the receiver's own state.
struct HandleCtx<'a, T: FrameTx> {
    ctrl_tx: &'a Arc<T>,
    progress: &'a watch::Sender<TransferProgress>,
    cancel: &'a CancellationToken,
    opts: &'a ReceiveOptions,
    from: Arrival,
    signals: &'a mut Vec<Signal>,
    killed_dc: &'a mut bool,
    inactivity_deadline: &'a mut Option<Instant>,
}

/// Apply one decoded frame to the receive state.
async fn handle_frame<T: FrameTx>(
    ctx: HandleCtx<'_, T>,
    state: &mut ReceiverState,
    frame: Frame<'_>,
) -> Result<(), TransferError> {
    let HandleCtx {
        ctrl_tx,
        progress,
        cancel,
        opts,
        from,
        signals,
        killed_dc,
        inactivity_deadline,
    } = ctx;

    match frame {
        Frame::Chunk { header, payload } => {
            let ReceiverState {
                current,
                sinks,
                manifest,
                meter,
                bytes_done,
                bytes_total,
                epoch,
                ..
            } = state;
            let epoch = *epoch;
            let Some(file) = current.as_mut() else {
                return Ok(());
            };
            // A chunk from a serve we have already replaced is not an error — it is exactly
            // what the epoch is for.
            if header.epoch != epoch {
                log::debug!("discarded a chunk from stale epoch {}", header.epoch);
                return Ok(());
            }
            if header.file != file.index
                || header.offset != file.expected
                || payload.len() != header.len as usize
            {
                return Err(TransferError::Protocol(ProtocolError::LengthMismatch {
                    declared: header.len,
                    actual: payload.len(),
                }));
            }
            let Some(sink) = sinks
                .as_mut()
                .and_then(|sinks| sinks[file.index as usize].as_mut())
            else {
                return Ok(());
            };

            // The flow-control point: this returns only once the sink has taken the bytes.
            // Only cancellation and the write deadline stay live during it; the other arms
            // resume afterwards, delayed by at most one bounded write.
            bounded_sink(cancel, opts.write_deadline, sink.write(payload))
                .await?
                .map_err(|e| TransferError::Io(e.to_string()))?;

            file.hasher.update(payload);
            let len = payload.len() as u64;
            file.expected += len;
            file.consumed_since_grant += len;
            *bytes_done += len;
            file.active_rx = from;
            *inactivity_deadline = Some(Instant::now() + opts.inactivity);

            // Credits acknowledge committed bytes as well as replenishing the send
            // window. Send the final partial grant so the sender's progress and
            // measured speed do not get stuck short of the last grain.
            let received = file.expected - file.epoch_start;
            let remaining = file.granted.saturating_sub(received);
            if file.consumed_since_grant >= CREDIT_GRAIN
                || Instant::now().duration_since(file.last_grant_at) >= Duration::from_secs(1)
                || remaining < CREDIT_GRAIN
                || file.expected == manifest[file.index as usize].size
            {
                let bytes = file.consumed_since_grant;
                file.granted += bytes;
                file.consumed_since_grant = 0;
                send_control(&**ctrl_tx, &Control::Credit { epoch, bytes }).await?;
                file.last_grant_at = Instant::now();
            }

            if meter.due(*bytes_done) {
                let (done, total, rate) = (*bytes_done, *bytes_total, meter.rate());
                let (index, written, size) = (
                    file.index,
                    file.expected,
                    manifest[file.index as usize].size,
                );
                let path = if from == Arrival::Dc {
                    Path::Direct
                } else {
                    ctrl_tx.path()
                };
                progress.send_modify(|p| {
                    p.phase = Phase::Transferring;
                    p.path = path;
                    p.file_index = Some(index);
                    p.file_done = written;
                    p.file_total = size;
                    p.bytes_done = done;
                    p.bytes_total = total;
                    p.bytes_per_sec = rate;
                });
            }

            // QA hook: kill the channel from the receiver's own side, so "the data channel
            // died mid-transfer" is a URL rather than a console incantation.
            if let Some(limit) = opts.kill_dc_after {
                if !*killed_dc && file.active_rx == Arrival::Dc && received >= limit {
                    *killed_dc = true;
                    log::debug!("killdc: closing the data channel after {received} bytes");
                    signals.push(Signal::CloseDc);
                }
            }
            Ok(())
        }
        Frame::Control(Control::Done { file: index, epoch }) => {
            let Some(file) = state.current.as_ref() else {
                return Ok(());
            };
            // A stale `Done` must never advance to the next file.
            if epoch != state.epoch || index != file.index {
                log::debug!("discarded a Done from stale epoch {epoch}");
                return Ok(());
            }
            let meta = &state.manifest[index as usize];
            if file.expected != meta.size {
                return Err(TransferError::Protocol(ProtocolError::LengthMismatch {
                    declared: meta.size as u32,
                    actual: file.expected as usize,
                }));
            }
            progress.send_modify(|p| p.phase = Phase::Verifying);
            let digest = crate::blob_store::BlobHash(*file.hasher.finalize().as_bytes());
            if digest != meta.hash {
                return Err(TransferError::HashMismatch { file: index });
            }
            let sink = state
                .sinks
                .as_mut()
                .and_then(|sinks| sinks[index as usize].take());
            if let Some(sink) = sink {
                let saved = bounded_sink(cancel, opts.write_deadline, sink.finish())
                    .await?
                    .map_err(|e| TransferError::Io(e.to_string()))?;
                log::debug!("file {index} verified and saved to {}", saved.location);
                state.saved.push(saved);
            }
            state.current = None;
            state.next_file = index + 1;
            *inactivity_deadline = None;
            Ok(())
        }
        Frame::Control(Control::Error {
            file,
            epoch,
            message,
        }) => match (file, epoch, state.current.as_ref()) {
            (None, _, _) => Err(TransferError::Remote(message)),
            (Some(index), Some(epoch), Some(current))
                if index == current.index && epoch == state.epoch =>
            {
                Err(TransferError::Remote(message))
            }
            _ => {
                log::debug!("discarded a stale error frame: {message}");
                Ok(())
            }
        },
        Frame::Control(Control::Offer { sdp }) => {
            // An offer to a side that advertised no WebRTC is a protocol violation.
            if opts.force_relay {
                return Err(TransferError::Protocol(ProtocolError::Empty));
            }
            signals.push(Signal::Offer(sdp));
            Ok(())
        }
        Frame::Control(Control::Ice {
            candidate,
            sdp_mid,
            sdp_mline_index,
        }) => {
            signals.push(Signal::Ice(IceCandidate {
                candidate,
                sdp_mid,
                sdp_mline_index,
            }));
            Ok(())
        }
        Frame::Control(_) => Ok(()),
    }
}

async fn abort_sinks(sinks: &mut Option<Vec<Option<AnySink>>>, deadline: Duration) {
    let Some(sinks) = sinks else { return };
    for slot in sinks.iter_mut() {
        if let Some(sink) = slot.take() {
            let _ = timeout(deadline, sink.abort()).await;
        }
    }
}

/// Iroh wrapper for one accepted connection — what the node's protocol handler calls.
pub async fn run_sender<F: DcFactory>(
    conn: Connection,
    files: Arc<Vec<SharedFile>>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: SenderOptions,
    dc: F,
) -> Result<(), TransferError> {
    let (send, recv) = timeout(opts.hello_deadline, conn.accept_bi())
        .await
        .map_err(|_| TransferError::Timeout)?
        .map_err(|e| TransferError::Transport(TransportError::Io(e.to_string())))?;
    let (ctrl_tx, ctrl_rx, _writer) = split_control(send, recv, iroh_path(), cancel.clone());
    let io = SessionIo {
        ctrl_tx,
        ctrl_rx,
        close: Box::new(move |code, reason| conn.close(code.into(), reason)),
        dc,
    };
    run_sender_on(io, files, progress, cancel, opts).await
}

/// Iroh wrapper for one receive. Shuts the on-demand receiver node down on **every** exit
/// path, so a cancelled receive never leaks its relay connection.
pub async fn run_receiver(
    node: Arc<Node>,
    ticket: EndpointTicket,
    commands: mpsc::Receiver<ReceiveCommand>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError> {
    let result = receive_over_iroh(&node, ticket, commands, progress, cancel, opts).await;
    node.shutdown().await;
    result
}

async fn receive_over_iroh(
    node: &Node,
    ticket: EndpointTicket,
    commands: mpsc::Receiver<ReceiveCommand>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError> {
    if opts.cap.is_none() {
        return Err(TransferError::Unauthorized(AuthFailure::MissingCap));
    }
    let connect_cancel = cancel.clone();
    let connect_timeout = opts.connect_timeout;
    let connect = || async {
        // `Node::connect` carries its own default budget for callers that have no options; a
        // receive session uses the one it was configured with.
        let conn = timeout(connect_timeout, node.connect(&ticket))
            .await
            .map_err(|_| TransferError::Timeout)?
            .map_err(|e| TransferError::Transport(TransportError::Io(e.to_string())))?;
        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|e| TransferError::Transport(TransportError::Io(e.to_string())))?;
        let (ctrl_tx, ctrl_rx, writer) =
            split_control(send, recv, iroh_path(), connect_cancel.clone());
        Ok((
            SessionIo {
                ctrl_tx,
                ctrl_rx,
                close: Box::new(move |code, reason| conn.close(code.into(), reason)),
                dc: default_dc_factory(),
            },
            writer,
        ))
    };
    run_receiver_reconnecting(connect, commands, progress, cancel, opts).await
}

pub(crate) async fn run_receiver_reconnecting<T, R, F, G, C, Fut>(
    mut connect: C,
    mut commands: mpsc::Receiver<ReceiveCommand>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError>
where
    T: FrameTx,
    R: FrameRx,
    F: DcFactory,
    C: FnMut() -> Fut,
    Fut: Future<Output = Result<(SessionIo<T, R, F>, G), TransferError>>,
{
    let mut state = ReceiverState::default();
    let mut retries = 0;
    let mut backoff = opts.reconnect_backoff;

    let result = loop {
        let io = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(TransferError::Cancelled),
            result = connect() => result,
        };
        let session = match io {
            Ok((io, _connection_guard)) => {
                run_receiver_session(
                    io,
                    &mut commands,
                    &progress,
                    &cancel,
                    &opts,
                    &mut state,
                    retries > 0,
                )
                .await
            }
            Err(error) => Err(error),
        };
        match session {
            Ok(saved) => break Ok(saved),
            Err(error)
                if reconnectable(&error)
                    && retries < opts.max_reconnect_attempts
                    && !cancel.is_cancelled() =>
            {
                retries += 1;
                progress.send_modify(|p| {
                    p.phase = Phase::Reconnecting {
                        attempt: retries,
                        max_attempts: opts.max_reconnect_attempts,
                    };
                    p.path = Path::Unknown;
                    p.bytes_per_sec = 0.0;
                    p.error = None;
                });
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break Err(TransferError::Cancelled),
                    _ = sleep(backoff) => {}
                }
                backoff = backoff.saturating_mul(2).min(opts.max_reconnect_backoff);
            }
            Err(error) => break Err(error),
        }
    };
    finish_receiver(result, &mut state, &progress, &opts).await
}

fn reconnectable(error: &TransferError) -> bool {
    matches!(
        error,
        TransferError::Timeout
            | TransferError::Transport(TransportError::Closed)
            | TransferError::Transport(TransportError::Timeout)
            | TransferError::Transport(TransportError::Io(_))
    )
}

/// The path an iroh control stream starts on. In the browser it is always relayed; natively
/// the wrapper's sampler refines it once a path is selected.
fn iroh_path() -> Path {
    #[cfg(target_arch = "wasm32")]
    {
        Path::Relayed
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Path::Unknown
    }
}

// ---------------------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------------------

/// The app's handle on one receive session.
///
/// `_task` is a plain join handle: dropping it **detaches**, and `Drop` cancels — so the
/// session observes the cancellation, aborts its sinks, closes the connection and shuts its
/// node down on its own. Nothing is ever hard-aborted.
pub struct TransferHandle {
    pub progress: watch::Receiver<TransferProgress>,
    pub commands: mpsc::Sender<ReceiveCommand>,
    pub cancel: CancellationToken,
    _task: JoinHandle<()>,
}

impl Drop for TransferHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl TransferHandle {
    /// Start receiving from `ticket` over `node`.
    pub fn start_receive(
        node: Arc<Node>,
        ticket: EndpointTicket,
        opts: ReceiveOptions,
    ) -> TransferHandle {
        let (progress_tx, progress_rx) = watch::channel(TransferProgress::connecting());
        let (commands, command_rx) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        let task = task::spawn({
            let cancel = cancel.clone();
            async move {
                let result =
                    run_receiver(node, ticket, command_rx, progress_tx.clone(), cancel, opts).await;
                progress_tx.send_modify(|p| {
                    if p.phase.is_terminal() {
                        return;
                    }
                    match result {
                        // The core normally publishes `Complete` itself, before it closes the
                        // connection; this is the belt-and-braces path.
                        Ok(saved) => p.phase = Phase::Complete { saved },
                        Err(TransferError::Cancelled) => p.phase = Phase::Cancelled,
                        Err(e) => {
                            p.error = Some(e.to_string());
                            p.phase = Phase::Failed;
                        }
                    }
                });
            }
        });
        TransferHandle {
            progress: progress_rx,
            commands,
            cancel,
            _task: task,
        }
    }

    pub fn latest(&self) -> TransferProgress {
        self.progress.borrow().clone()
    }
}

// ---------------------------------------------------------------------------------------
// In-memory transports (tests)
// ---------------------------------------------------------------------------------------

/// Test transport: an unbounded queue plus a record of everything that was written.
#[cfg(test)]
pub struct MemTx {
    tx: mpsc::UnboundedSender<Bytes>,
    recorded: Arc<std::sync::Mutex<Vec<Bytes>>>,
    max_frame: usize,
    prefixed: bool,
    path: Path,
}

/// Read half of [`MemTx`].
#[cfg(test)]
pub struct MemRx {
    rx: mpsc::UnboundedReceiver<Bytes>,
}

#[cfg(test)]
impl MemTx {
    /// A connected pair with production-like defaults.
    pub fn pair() -> (MemTx, MemRx) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            MemTx {
                tx,
                recorded: Arc::new(std::sync::Mutex::new(Vec::new())),
                max_frame: DEFAULT_CHUNK,
                prefixed: true,
                path: Path::Relayed,
            },
            MemRx { rx },
        )
    }

    pub fn with_max_frame(mut self, max_frame: usize) -> Self {
        self.max_frame = max_frame;
        self
    }

    pub fn with_prefixed(mut self, prefixed: bool) -> Self {
        self.prefixed = prefixed;
        self
    }

    pub fn with_path(mut self, path: Path) -> Self {
        self.path = path;
        self
    }

    /// Every frame this half accepted, in order.
    pub fn recorded(&self) -> Vec<Bytes> {
        self.recorded.lock().expect("recorder poisoned").clone()
    }
}

#[cfg(test)]
impl FrameTx for MemTx {
    fn send(&self, frame: Bytes) -> impl Future<Output = Result<(), TransportError>> {
        let sent = self.tx.send(frame.clone());
        if sent.is_ok() {
            self.recorded.lock().expect("recorder poisoned").push(frame);
        }
        async move { sent.map_err(|_| TransportError::Closed) }
    }

    fn max_frame(&self) -> usize {
        self.max_frame
    }

    fn is_length_prefixed(&self) -> bool {
        self.prefixed
    }

    fn path(&self) -> Path {
        self.path
    }
}

#[cfg(test)]
impl MemRx {
    /// Stop delivering, as a closed transport would.
    pub fn close(&mut self) {
        self.rx.close();
    }
}

#[cfg(test)]
impl FrameRx for MemRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        Ok(self.rx.recv().await)
    }
}

/// Wraps a factory so a test can say "this side cannot do WebRTC" without a second factory
/// type — which is what the one-sided capability tests need.
#[cfg(test)]
pub struct MaybeDc<F: DcFactory> {
    inner: F,
    available: bool,
}

#[cfg(test)]
impl<F: DcFactory> MaybeDc<F> {
    pub fn new(inner: F, available: bool) -> Self {
        Self { inner, available }
    }
}

#[cfg(test)]
impl<F: DcFactory> DcFactory for MaybeDc<F> {
    type Dc = F::Dc;

    fn available(&self) -> bool {
        self.available
    }

    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError> {
        self.inner.create(role)
    }
}

/// Test data channel: the halves a session gets, plus watches the test drives.
#[cfg(test)]
pub struct MemDc {
    shared: Arc<MemDcShared>,
}

#[cfg(test)]
struct MemDcShared {
    open: watch::Sender<bool>,
    state: watch::Sender<PcState>,
    halves: std::sync::Mutex<Option<(MemTx, MemRx)>>,
    local_ice: std::sync::Mutex<std::collections::VecDeque<IceCandidate>>,
    max_message_size: Option<usize>,
    path: Path,
}

/// What a test drives a [`MemDc`] with.
#[cfg(test)]
pub struct MemDcHandle {
    /// Frames the session wrote on the data channel.
    pub sent: MemRx,
    /// Frames delivered to the session as if the peer had sent them.
    pub inject: MemTx,
    shared: Arc<MemDcShared>,
}

#[cfg(test)]
impl MemDcHandle {
    /// Flip the channel's open state.
    pub fn set_open(&self, open: bool) {
        self.shared.open.send_replace(open);
    }

    /// Move the peer connection to a new state.
    pub fn set_state(&self, state: PcState) {
        self.shared.state.send_replace(state);
    }

    /// Queue a candidate for the session to pick up as its own local ICE.
    pub fn push_local_ice(&self, candidate: IceCandidate) {
        self.shared
            .local_ice
            .lock()
            .expect("ice queue poisoned")
            .push_back(candidate);
    }
}

/// Factory handing out one [`MemDc`].
#[cfg(test)]
pub struct MemDcFactory {
    shared: Arc<MemDcShared>,
}

#[cfg(test)]
impl MemDcFactory {
    /// Two factories whose channels are cross-wired, so two sessions in one test can actually
    /// talk over "the data channel", plus the handles that open and close them.
    pub fn pair(max_message_size: Option<usize>) -> (Self, MemDcHandle, Self, MemDcHandle) {
        let (a_tx, b_rx) = MemTx::pair();
        let (b_tx, a_rx) = MemTx::pair();
        let a_tx = a_tx.with_prefixed(false).with_path(Path::Direct);
        let b_tx = b_tx.with_prefixed(false).with_path(Path::Direct);
        let (a, a_handle) = Self::wired(a_tx, a_rx, max_message_size);
        let (b, b_handle) = Self::wired(b_tx, b_rx, max_message_size);
        (a, a_handle, b, b_handle)
    }

    fn wired(tx: MemTx, rx: MemRx, max_message_size: Option<usize>) -> (Self, MemDcHandle) {
        let (unused_tx, unused_rx) = MemTx::pair();
        let shared = Arc::new(MemDcShared {
            open: watch::channel(false).0,
            state: watch::channel(PcState::New).0,
            halves: std::sync::Mutex::new(Some((tx, rx))),
            local_ice: std::sync::Mutex::new(std::collections::VecDeque::new()),
            max_message_size,
            path: Path::Direct,
        });
        (
            Self {
                shared: shared.clone(),
            },
            MemDcHandle {
                sent: unused_rx,
                inject: unused_tx,
                shared,
            },
        )
    }

    /// A factory plus the handle a test drives it with.
    pub fn new(max_message_size: Option<usize>) -> (Self, MemDcHandle) {
        let (session_tx, test_rx) = MemTx::pair();
        let (test_tx, session_rx) = MemTx::pair();
        let shared = Arc::new(MemDcShared {
            open: watch::channel(false).0,
            state: watch::channel(PcState::New).0,
            halves: std::sync::Mutex::new(Some((session_tx.with_prefixed(false), session_rx))),
            local_ice: std::sync::Mutex::new(std::collections::VecDeque::new()),
            max_message_size,
            path: Path::Direct,
        });
        (
            Self {
                shared: shared.clone(),
            },
            MemDcHandle {
                sent: test_rx,
                inject: test_tx.with_prefixed(false),
                shared,
            },
        )
    }
}

#[cfg(test)]
impl DcFactory for MemDcFactory {
    type Dc = MemDc;

    fn available(&self) -> bool {
        true
    }

    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError> {
        let _ = role;
        Ok(MemDc {
            shared: self.shared.clone(),
        })
    }
}

#[cfg(test)]
impl DataChannel for MemDc {
    type Tx = MemTx;
    type Rx = MemRx;

    async fn create_offer(&mut self) -> Result<String, TransportError> {
        Ok("mem-offer".to_string())
    }

    fn accept_offer(&mut self, sdp: &str) -> impl Future<Output = Result<String, TransportError>> {
        let _ = sdp;
        async move { Ok("mem-answer".to_string()) }
    }

    fn accept_answer(&mut self, sdp: &str) -> impl Future<Output = Result<(), TransportError>> {
        let _ = sdp;
        async move { Ok(()) }
    }

    fn add_ice(&mut self, c: &IceCandidate) -> impl Future<Output = Result<(), TransportError>> {
        let _ = c;
        async move { Ok(()) }
    }

    fn next_local_ice(&mut self) -> impl Future<Output = Option<IceCandidate>> {
        let next = self
            .shared
            .local_ice
            .lock()
            .expect("ice queue poisoned")
            .pop_front();
        async move { next }
    }

    fn open_watch(&self) -> watch::Receiver<bool> {
        self.shared.open.subscribe()
    }

    fn state_watch(&self) -> watch::Receiver<PcState> {
        self.shared.state.subscribe()
    }

    fn max_message_size(&self) -> Option<usize> {
        self.shared.max_message_size
    }

    fn path_kind(&self) -> impl Future<Output = Path> {
        let path = self.shared.path;
        async move { path }
    }

    fn split(
        &mut self,
        cancel: CancellationToken,
        relay_switch: Arc<Notify>,
        receive_window: usize,
    ) -> Option<(Self::Tx, Self::Rx)> {
        let _ = (cancel, relay_switch, receive_window);
        self.shared.halves.lock().expect("halves poisoned").take()
    }

    fn close(&self) {
        self.shared.open.send_replace(false);
        self.shared.state.send_replace(PcState::Closed);
    }
}
