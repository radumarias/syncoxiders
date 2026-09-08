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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh_tickets::endpoint::EndpointTicket;
use n0_future::task::{self, AbortOnDropHandle, JoinHandle};
use n0_future::time::{sleep, timeout, Duration};
use tokio::sync::{mpsc, watch, Notify};
use tokio_util::sync::CancellationToken;

use crate::file_io::{AnySink, SavedFile, SharedFile};
use crate::node::{FragmentParams, Node, SinkPref};
use crate::protocol::{self, ProtocolError, CAP_LEN, DEFAULT_CHUNK, INITIAL_WINDOW};

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
pub struct ControlRx {
    recv: RecvStream,
}

impl FrameRx for ControlRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        protocol::read_frame(&mut self.recv)
            .await
            .map_err(|e| TransportError::Io(e.to_string()))
    }
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
        ControlRx { recv },
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
    ) -> Option<(Self::Tx, Self::Rx)> {
        let _ = (cancel, relay_switch);
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
    budget: usize,
    in_queue: AtomicUsize,
}

impl ByteBudget {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            in_queue: AtomicUsize::new(0),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
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
                    .filter(|next| *next <= self.budget)
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
}

// ---------------------------------------------------------------------------------------
// Session entry points
// ---------------------------------------------------------------------------------------

/// Transport-generic sender core. Bodies land with the engine step; the signature is frozen.
pub(crate) async fn run_sender_on<T: FrameTx, R: FrameRx, F: DcFactory>(
    io: SessionIo<T, R, F>,
    files: Arc<Vec<SharedFile>>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: SenderOptions,
) -> Result<(), TransferError> {
    let _ = (io, files, progress, cancel, opts);
    Err(TransferError::Io("engine not yet implemented".to_string()))
}

/// Transport-generic receiver core. Bodies land with the engine step; the signature is frozen.
pub(crate) async fn run_receiver_on<T: FrameTx, R: FrameRx, F: DcFactory>(
    io: SessionIo<T, R, F>,
    commands: mpsc::Receiver<ReceiveCommand>,
    progress: watch::Sender<TransferProgress>,
    cancel: CancellationToken,
    opts: ReceiveOptions,
) -> Result<Vec<SavedFile>, TransferError> {
    let _ = (io, commands, progress, cancel, opts);
    Err(TransferError::Io("engine not yet implemented".to_string()))
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
    // `Node::connect` carries its own default budget for callers that have no options; a
    // receive session uses the one it was configured with.
    let conn = timeout(opts.connect_timeout, node.connect(&ticket))
        .await
        .map_err(|_| TransferError::Timeout)?
        .map_err(|e| TransferError::Transport(TransportError::Io(e.to_string())))?;
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| TransferError::Transport(TransportError::Io(e.to_string())))?;
    let (ctrl_tx, ctrl_rx, _writer) = split_control(send, recv, iroh_path(), cancel.clone());
    let io = SessionIo {
        ctrl_tx,
        ctrl_rx,
        close: Box::new(move |code, reason| conn.close(code.into(), reason)),
        dc: default_dc_factory(),
    };
    run_receiver_on(io, commands, progress, cancel, opts).await
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
    ) -> Option<(Self::Tx, Self::Rx)> {
        let _ = (cancel, relay_switch);
        self.shared.halves.lock().expect("halves poisoned").take()
    }

    fn close(&self) {
        self.shared.open.send_replace(false);
        self.shared.state.send_replace(PcState::Closed);
    }
}
