//! Browser data channel (design §4.5), wasm32 only.
//!
//! This module is a stub at this step: [`WebRtcFactory`] reports `available() = false`, so
//! every wasm build says `Hello { webrtc: false }` and rides the relay — which is exactly the
//! staged plan. The WebRTC step replaces the bodies (`type Dc = PeerChannel`,
//! `available() = true`, the live [`debug_pc`] handle) without touching a single caller: the
//! wrappers, the node's `Serve` and the app's `__p2p` getter already compile against these
//! signatures.
//!
//! Closure convention for the real implementation: every `Closure` is stored on the owning
//! struct, nothing is `forget()`-leaked.

use n0_future::time::Duration;

use crate::transfer::{DcFactory, DcRole, NeverDc, TransportError};

pub use crate::transfer::IceCandidate;

/// STUN servers used to gather candidates.
pub const ICE_SERVERS: &[&str] = &[
    "stun:stun.cloudflare.com:3478",
    "stun:stun.l.google.com:19302",
];
/// `bufferedAmountLowThreshold` on the data channel.
pub const BUFFER_LOW: u32 = 1 << 20;
/// A send waits when `bufferedAmount` is above this.
pub const BUFFER_HIGH: u32 = 4 << 20;
/// How long the receiver waits for the channel to open before falling back to the relay.
/// Defined by the engine, so this module and the session deadline can never drift apart.
pub const CONNECT_TIMEOUT: Duration = crate::transfer::WEBRTC_OPEN;
/// Belt-and-braces bound on a send blocked behind backpressure. A channel that is closing,
/// has errored, or whose ICE path died need never drain, so a blocked send always races
/// close, error, cancellation and the transport switch — never the low-water event alone.
pub const BUFFER_STALL: Duration = Duration::from_secs(30);

/// Failures of the browser WebRTC stack.
#[derive(Debug)]
pub enum WebRtcError {
    Js(String),
    Timeout,
    Failed(String),
    Closed,
}

impl std::fmt::Display for WebRtcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Js(m) => write!(f, "browser error: {m}"),
            Self::Timeout => write!(f, "timed out"),
            Self::Failed(m) => write!(f, "connection failed: {m}"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

impl std::error::Error for WebRtcError {}

/// Creates browser data channels. Reports unavailable until the WebRTC step fills it in.
#[derive(Clone, Debug)]
pub struct WebRtcFactory {
    pub ice_servers: Vec<String>,
}

impl Default for WebRtcFactory {
    fn default() -> Self {
        Self {
            ice_servers: ICE_SERVERS.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

impl DcFactory for WebRtcFactory {
    type Dc = NeverDc;

    fn available(&self) -> bool {
        false
    }

    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError> {
        let _ = (role, &self.ice_servers);
        Err(TransportError::Closed)
    }
}

/// The live peer connection, for QA's `window.__p2p.pc`. `None` until the WebRTC step.
pub fn debug_pc() -> Option<web_sys::RtcPeerConnection> {
    None
}
