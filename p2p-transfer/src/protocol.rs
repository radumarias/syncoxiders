//! Wire protocol: control messages and chunk framing (design §4.3).
//!
//! Every frame is `[tag][postcard bytes][payload?]`. On a byte-stream transport (the iroh
//! control stream) each frame is preceded by a `u32` little-endian length; on an
//! `RTCDataChannel` there is **no** length prefix — one frame is one SCTP message and the
//! message boundary *is* the framing. That asymmetry is why [`max_payload`] takes a
//! `prefixed` flag: the same 64 KiB limit yields different payload budgets on the two
//! transports, and an advertised limit is never rounded upward.

use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::blob_store::{hex, BlobHash};

/// ALPN of this protocol family. Incompatible frame semantics are negotiated
/// with [`PROTOCOL_VERSION`] inside `Hello`, so keep this ALPN stable.
pub const ALPN: &[u8] = b"syncoxiders/p2p-transfer/1";
/// Protocol version carried in [`Control::Hello`].
pub const PROTOCOL_VERSION: u16 = 2;
/// Length of the 128-bit link capability (design §2.6).
pub const CAP_LEN: usize = 16;
/// Largest *complete* frame we ever emit, on any transport.
pub const DEFAULT_CHUNK: usize = 64 * 1024;
/// `u32` LE length prefix — byte-stream transports only, never on the data channel.
pub const LEN_PREFIX: usize = 4;
/// One tag byte in front of every frame.
pub const TAG_LEN: usize = 1;
/// Worst-case postcard-encoded [`ChunkHeader`]: `file` u32 ≤ 5 + `epoch` u32 ≤ 5 +
/// `offset` u64 ≤ 10 + `len` u32 ≤ 5.
pub const CHUNK_HEADER_MAX: usize = 25;
/// Framing cost of one chunk on a length-prefixed transport (the iroh control stream).
pub const CHUNK_OVERHEAD_STREAM: usize = LEN_PREFIX + TAG_LEN + CHUNK_HEADER_MAX;
/// Framing cost of one chunk on a message transport (`RTCDataChannel`).
pub const CHUNK_OVERHEAD_DC: usize = TAG_LEN + CHUNK_HEADER_MAX;
/// A transport that cannot carry ~1 KiB of payload per frame is not used for chunks.
/// There is deliberately no floor that raises a peer's advertised limit.
pub const MIN_USABLE_FRAME: usize = 1024;
/// Hard reader limit on every transport: bounds memory against a hostile peer.
pub const MAX_FRAME: usize = 1024 * 1024;
/// Manifest cap, so a manifest always fits inside [`MAX_FRAME`].
pub const MAX_MANIFEST_FILES: usize = 2_000;
/// Default initial credit grant the receiver sends right after every [`Control::Request`].
/// Overridable per receive with the fragment token `win=<MiB>`; the sender never assumes it.
pub const INITIAL_WINDOW: u64 = 4 * 1024 * 1024;
/// `win=` values above this are clamped by the fragment parser (QA knob, not a product limit).
pub const MAX_WINDOW_MIB: u64 = 64;
/// The receiver coalesces credit grants to roughly one per MiB consumed.
pub const CREDIT_GRAIN: u64 = 1024 * 1024;

const TAG_CONTROL: u8 = 0;
const TAG_CHUNK: u8 = 1;

/// One entry of the manifest.
///
/// `name` is sender-supplied and **must** be sanitised by the receiver before it touches a
/// filesystem ([`crate::file_io::sanitize_name`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileMeta {
    pub name: String,
    pub size: u64,
    pub hash: BlobHash,
}

/// Control messages. They travel **only** on the iroh control stream.
///
/// `Debug` is hand-written (never derived) so that `Hello` prints `cap: <redacted>`: the
/// terminal buffer is user-visible and copyable, and one `debug!("{frame:?}")` with a derived
/// `Debug` would put the capability there. `PartialEq` exists for the round-trip tests; the
/// authorization check must go through [`cap_eq`], never `==`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Control {
    /// The receiver's first frame. `webrtc` advertises `RTCDataChannel` capability;
    /// `cap` is the link capability of design §2.6. The sender answers with its own `Hello`
    /// (cap zero-filled, ignored by the receiver) only after [`cap_eq`] succeeds.
    Hello {
        version: u16,
        webrtc: bool,
        cap: [u8; CAP_LEN],
    },
    Manifest {
        files: Vec<FileMeta>,
    },
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Ice {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
    /// Resume-capable request; `file` indexes the manifest. Grants **nothing** by itself:
    /// the receiver's `Credit { epoch, initial_window }` follows it on the same ordered stream.
    Request {
        file: u32,
        offset: u64,
        epoch: u32,
    },
    /// Payload bytes the sender may *additionally* put in flight for this epoch (an
    /// increment, never a cumulative total). Control stream only, in both transport modes.
    Credit {
        epoch: u32,
        bytes: u64,
    },
    /// The receiver gave up on WebRTC: the sender sets `relay_forced` and closes the data
    /// channel. Never aborts a running serve — the receiver's next `Request` pre-empts.
    UseRelay,
    /// Sent on the same transport as that file's chunks, with the same epoch.
    Done {
        file: u32,
        epoch: u32,
    },
    /// `file`/`epoch` `None` → session-fatal; `Some` → scoped to that file and epoch.
    Error {
        file: Option<u32>,
        epoch: Option<u32>,
        message: String,
    },
    /// Sent only after every file passed BLAKE3 verification and the sink
    /// finished. A sender must not infer success from a full progress bar or
    /// a closed transport; only this receipt establishes remote success.
    Verified {
        files: u32,
        bytes: u64,
    },
    /// Keep the connection alive until the sender has observed `Verified`.
    VerifiedAck,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hello {
                version, webrtc, ..
            } => f
                .debug_struct("Hello")
                .field("version", version)
                .field("webrtc", webrtc)
                .field("cap", &"<redacted>")
                .finish(),
            Self::Manifest { files } => f
                .debug_struct("Manifest")
                .field("files", &files.len())
                .finish(),
            Self::Offer { sdp } => f
                .debug_struct("Offer")
                .field("sdp_len", &sdp.len())
                .finish(),
            Self::Answer { sdp } => f
                .debug_struct("Answer")
                .field("sdp_len", &sdp.len())
                .finish(),
            Self::Ice {
                sdp_mid,
                sdp_mline_index,
                ..
            } => f
                .debug_struct("Ice")
                .field("sdp_mid", sdp_mid)
                .field("sdp_mline_index", sdp_mline_index)
                .finish(),
            Self::Request {
                file,
                offset,
                epoch,
            } => f
                .debug_struct("Request")
                .field("file", file)
                .field("offset", offset)
                .field("epoch", epoch)
                .finish(),
            Self::Credit { epoch, bytes } => f
                .debug_struct("Credit")
                .field("epoch", epoch)
                .field("bytes", bytes)
                .finish(),
            Self::UseRelay => f.write_str("UseRelay"),
            Self::Done { file, epoch } => f
                .debug_struct("Done")
                .field("file", file)
                .field("epoch", epoch)
                .finish(),
            Self::Error {
                file,
                epoch,
                message,
            } => f
                .debug_struct("Error")
                .field("file", file)
                .field("epoch", epoch)
                .field("message", message)
                .finish(),
            Self::Verified { files, bytes } => f
                .debug_struct("Verified")
                .field("files", files)
                .field("bytes", bytes)
                .finish(),
            Self::VerifiedAck => f.write_str("VerifiedAck"),
        }
    }
}

/// Header of one chunk frame. `epoch` is the epoch of the `Request` that asked for it, which
/// is what makes in-flight chunks from an aborted serve harmless (design §2.4 invariant I2).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHeader {
    pub file: u32,
    pub epoch: u32,
    pub offset: u64,
    pub len: u32,
}

/// A decoded frame. The chunk payload borrows from the buffer it was decoded from.
#[derive(Debug, PartialEq)]
pub enum Frame<'a> {
    Control(Control),
    Chunk {
        header: ChunkHeader,
        payload: &'a [u8],
    },
}

/// Framing errors. Wire input only — never a programming error.
#[derive(Debug)]
pub enum ProtocolError {
    Empty,
    UnknownTag(u8),
    Codec(postcard::Error),
    LengthMismatch { declared: u32, actual: usize },
    TooLarge(usize),
    Version { theirs: u16 },
    InvalidReceipt,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty frame"),
            Self::UnknownTag(t) => write!(f, "unknown frame tag {t}"),
            Self::Codec(e) => write!(f, "frame codec error: {e}"),
            Self::LengthMismatch { declared, actual } => {
                write!(f, "chunk declared {declared} bytes but carried {actual}")
            }
            Self::TooLarge(n) => write!(f, "frame too large: {n} bytes"),
            Self::Version { theirs } => write!(f, "unsupported protocol version {theirs}"),
            Self::InvalidReceipt => write!(f, "invalid verified transfer receipt"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<ProtocolError> for std::io::Error {
    fn from(e: ProtocolError) -> Self {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    }
}

/// `tag(0) + postcard(control)`. Never larger than [`MAX_FRAME`] (an SDP is ~10 KiB).
pub fn encode_control(c: &Control) -> Result<Bytes, ProtocolError> {
    if let Control::Manifest { files } = c {
        if files.len() > MAX_MANIFEST_FILES {
            return Err(ProtocolError::TooLarge(files.len()));
        }
    }
    let body = postcard::to_allocvec(c).map_err(ProtocolError::Codec)?;
    let total = TAG_LEN + body.len();
    if total > MAX_FRAME {
        return Err(ProtocolError::TooLarge(total));
    }
    let mut buf = BytesMut::with_capacity(total);
    buf.put_u8(TAG_CONTROL);
    buf.put_slice(&body);
    Ok(buf.freeze())
}

/// `tag(1) + postcard(header) + payload`, one allocation.
///
/// The header goes into a stack buffer — [`CHUNK_HEADER_MAX`] is its worst case — so the
/// only allocation on this per-chunk path is the frame itself.
///
/// # Panics
/// If `header.len` does not match `payload.len()` — a caller-side programming error, never
/// something a peer can trigger.
pub fn encode_chunk(header: ChunkHeader, payload: &[u8]) -> Bytes {
    assert_eq!(
        header.len as usize,
        payload.len(),
        "ChunkHeader::len must equal the payload length"
    );
    let mut header_buf = [0u8; CHUNK_HEADER_MAX];
    let head = postcard::to_slice(&header, &mut header_buf)
        .expect("a ChunkHeader always fits in CHUNK_HEADER_MAX");
    let mut buf = BytesMut::with_capacity(TAG_LEN + head.len() + payload.len());
    buf.put_u8(TAG_CHUNK);
    buf.put_slice(head);
    buf.put_slice(payload);
    buf.freeze()
}

/// Inverse of [`encode_control`] and [`encode_chunk`].
pub fn decode(frame: &[u8]) -> Result<Frame<'_>, ProtocolError> {
    if frame.len() > MAX_FRAME {
        return Err(ProtocolError::TooLarge(frame.len()));
    }
    let (tag, rest) = frame.split_first().ok_or(ProtocolError::Empty)?;
    match *tag {
        TAG_CONTROL => {
            let control: Control = postcard::from_bytes(rest).map_err(ProtocolError::Codec)?;
            if let Control::Manifest { files } = &control {
                if files.len() > MAX_MANIFEST_FILES {
                    return Err(ProtocolError::TooLarge(files.len()));
                }
            }
            Ok(Frame::Control(control))
        }
        TAG_CHUNK => {
            let (header, payload): (ChunkHeader, &[u8]) =
                postcard::take_from_bytes(rest).map_err(ProtocolError::Codec)?;
            if header.len as usize != payload.len() {
                return Err(ProtocolError::LengthMismatch {
                    declared: header.len,
                    actual: payload.len(),
                });
            }
            Ok(Frame::Chunk { header, payload })
        }
        other => Err(ProtocolError::UnknownTag(other)),
    }
}

/// Largest payload whose **complete encoded frame** is at most `limit` bytes.
///
/// `prefixed` says whether this transport writes a `u32` LE length in front of every frame
/// (iroh streams: `true`; `RTCDataChannel`: `false`). Saturating: 0 when `limit` cannot even
/// carry the framing. The worst-case header width is used, so
/// `encode_chunk(..).len() (+ LEN_PREFIX) <= limit` holds for every offset and length.
pub fn max_payload(limit: usize, prefixed: bool) -> usize {
    limit.saturating_sub(if prefixed {
        CHUNK_OVERHEAD_STREAM
    } else {
        CHUNK_OVERHEAD_DC
    })
}

/// Outcome of frame-size negotiation for a transport that advertises a maximum message size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkPlan {
    Frame(usize),
    TooSmall,
}

/// Negotiate the complete-frame size for a transport.
///
/// * `None` (limit unknown) → `Frame(DEFAULT_CHUNK)`
/// * `Some(m)` → `Frame(min(m, DEFAULT_CHUNK))` while that is at least [`MIN_USABLE_FRAME`]
/// * otherwise → `TooSmall`, and chunks never ride this transport
///
/// An advertised limit is **never** rounded up: sending an SCTP message larger than the peer's
/// `maxMessageSize` silently kills the channel.
pub fn negotiate_chunk(remote_max_message_size: Option<usize>) -> ChunkPlan {
    match remote_max_message_size {
        None => ChunkPlan::Frame(DEFAULT_CHUNK),
        Some(m) => {
            let frame = m.min(DEFAULT_CHUNK);
            if frame >= MIN_USABLE_FRAME {
                ChunkPlan::Frame(frame)
            } else {
                ChunkPlan::TooSmall
            }
        }
    }
}

/// Constant-time comparison of two capabilities.
///
/// Folds XOR over **all** bytes into one accumulator — no early return, no short-circuit —
/// and passes the accumulator through [`std::hint::black_box`] before the single comparison,
/// so LLVM cannot turn it back into an early-exit `memcmp`. Never log, echo or `Debug`-print
/// an offered capability.
pub fn cap_eq(a: &[u8; CAP_LEN], b: &[u8; CAP_LEN]) -> bool {
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    std::hint::black_box(acc) == 0
}

/// Link encoding of a capability: exactly 32 lowercase hex characters.
pub fn cap_to_hex(cap: &[u8; CAP_LEN]) -> String {
    hex::encode(cap)
}

/// Inverse of [`cap_to_hex`]. Rejects any other length, uppercase and non-hex input.
pub fn cap_from_hex(s: &str) -> Option<[u8; CAP_LEN]> {
    // `hex::decode` already rejects every non-hex byte; the link grammar adds only the
    // exact-length and lowercase-only rules on top of it.
    if s.len() != CAP_LEN * 2 || s.bytes().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let bytes = hex::decode(s).ok()?;
    let mut out = [0u8; CAP_LEN];
    out.copy_from_slice(&bytes);
    Some(out)
}

/// Write one length-prefixed frame (`u32` LE length, then the frame) to a byte stream.
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, frame: &[u8]) -> std::io::Result<()> {
    if frame.len() > MAX_FRAME {
        return Err(ProtocolError::TooLarge(frame.len()).into());
    }
    let len = frame.len() as u32;
    w.write_all(&len.to_le_bytes()).await?;
    w.write_all(frame).await?;
    Ok(())
}

/// Read one length-prefixed frame from a byte stream.
///
/// `Ok(None)` on a clean EOF *before* a length prefix. A declared length above [`MAX_FRAME`]
/// is rejected **before** anything is allocated.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Option<Bytes>> {
    let mut len_buf = [0u8; LEN_PREFIX];
    let mut filled = 0;
    while filled < LEN_PREFIX {
        match r.read(&mut len_buf[filled..]).await? {
            0 if filled == 0 => return Ok(None),
            0 => return Err(truncated("frame length prefix")),
            n => filled += n,
        }
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(ProtocolError::TooLarge(len).into());
    }
    // `read_buf` fills the buffer's uninitialised tail, so a frame is never memset to zero
    // just to be overwritten; `take` keeps the read from running into the next frame.
    let mut buf = BytesMut::with_capacity(len);
    let mut limited = (&mut *r).take(len as u64);
    while buf.len() < len {
        if limited.read_buf(&mut buf).await? == 0 {
            return Err(truncated("frame body"));
        }
    }
    Ok(Some(buf.freeze()))
}

fn truncated(what: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        format!("truncated {what}"),
    )
}
