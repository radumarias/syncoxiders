//! Browser `RTCDataChannel`, with iroh carrying signalling and serving as fallback.

use std::cell::RefCell;
use std::sync::Arc;

use bytes::Bytes;
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use n0_future::time::{sleep, Duration};
use send_wrapper::SendWrapper;
use tokio::sync::{mpsc, watch, Notify};
use tokio_util::sync::CancellationToken;
use wasm_bindgen::{closure::Closure, prelude::wasm_bindgen, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::protocol::MAX_FRAME;
use crate::transfer::{
    ByteBudget, DataChannel, DcFactory, DcRole, FrameRx, FrameTx, IceCandidate, Path, PcState,
    TransportError,
};

pub const ICE_SERVERS: &[&str] = &[
    "stun:stun.cloudflare.com:3478",
    "stun:stun.l.google.com:19302",
];
pub const CONNECT_TIMEOUT: Duration = crate::transfer::WEBRTC_OPEN;
pub const BUFFER_STALL: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MESSAGE: usize = 64 * 1024;

#[wasm_bindgen(module = "/assets/webrtc-channel.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = createPeer)]
    fn create_peer(
        role: &str,
        ice_servers: &JsValue,
        on_frame: &Function,
        on_ice: &Function,
        on_open: &Function,
        on_state: &Function,
    ) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = createOffer)]
    fn create_offer(peer: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = acceptOffer)]
    fn accept_offer(peer: &JsValue, sdp: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = acceptAnswer)]
    fn accept_answer(peer: &JsValue, sdp: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = addIce)]
    fn add_ice(
        peer: &JsValue,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = sendFrame)]
    fn send_frame(peer: &JsValue, frame: &Uint8Array) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = maxMessageSize)]
    fn max_message_size(peer: &JsValue) -> f64;
    #[wasm_bindgen(catch, js_name = pathKind)]
    fn path_kind(peer: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = peerConnection)]
    fn peer_connection(peer: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = closePeer)]
    fn close_peer(peer: &JsValue);
}

thread_local! {
    static DEBUG_PC: RefCell<Option<web_sys::RtcPeerConnection>> = const { RefCell::new(None) };
}

fn js_error(error: JsValue) -> TransportError {
    let message = error
        .dyn_ref::<js_sys::Error>()
        .map(js_sys::Error::message)
        .map(String::from)
        .unwrap_or_else(|| format!("{error:?}"));
    TransportError::Io(message)
}

async fn promise_string(promise: Promise) -> Result<String, TransportError> {
    JsFuture::from(promise)
        .await
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| TransportError::Io("WebRTC returned a non-string SDP".to_string()))
}

fn state(value: &str) -> PcState {
    match value {
        "new" => PcState::New,
        "connecting" => PcState::Connecting,
        "connected" => PcState::Connected,
        "failed" | "disconnected" => PcState::Failed,
        "closed" => PcState::Closed,
        _ => PcState::Connecting,
    }
}

fn ice(value: JsValue) -> Option<IceCandidate> {
    if value.is_null() || value.is_undefined() {
        return None;
    }
    let get = |name| Reflect::get(&value, &JsValue::from_str(name)).ok();
    Some(IceCandidate {
        candidate: get("candidate")?.as_string()?,
        sdp_mid: get("sdpMid").and_then(|value| value.as_string()),
        sdp_mline_index: get("sdpMLineIndex")
            .and_then(|value| value.as_f64())
            .map(|value| value as u16),
    })
}

#[derive(Clone, Debug)]
pub struct WebRtcFactory {
    pub ice_servers: Vec<String>,
}

impl Default for WebRtcFactory {
    fn default() -> Self {
        Self {
            ice_servers: ICE_SERVERS
                .iter()
                .map(|server| (*server).to_string())
                .collect(),
        }
    }
}

impl DcFactory for WebRtcFactory {
    type Dc = PeerChannel;

    fn available(&self) -> bool {
        Reflect::has(&js_sys::global(), &JsValue::from_str("RTCPeerConnection")).unwrap_or(false)
    }

    fn create(&self, role: DcRole) -> Result<Self::Dc, TransportError> {
        log::debug!("creating browser WebRTC peer as {role:?}");
        PeerChannel::new(role, &self.ice_servers)
    }
}

pub struct PeerChannel {
    peer: SendWrapper<JsValue>,
    frame_rx: Option<mpsc::UnboundedReceiver<Bytes>>,
    frame_budget: Arc<ByteBudget>,
    ice_rx: mpsc::UnboundedReceiver<Option<IceCandidate>>,
    open: watch::Sender<bool>,
    state: watch::Sender<PcState>,
    split: bool,
    _frame_callback: SendWrapper<Closure<dyn FnMut(Uint8Array)>>,
    _ice_callback: SendWrapper<Closure<dyn FnMut(JsValue)>>,
    _open_callback: SendWrapper<Closure<dyn FnMut(bool)>>,
    _state_callback: SendWrapper<Closure<dyn FnMut(String)>>,
}

impl PeerChannel {
    fn new(role: DcRole, ice_servers: &[String]) -> Result<Self, TransportError> {
        let (frame_tx, frame_rx) = mpsc::unbounded_channel();
        // Tightened to the receiver's negotiated credit window in `split`, before chunks flow.
        let frame_budget = Arc::new(ByteBudget::new(MAX_FRAME));
        let (ice_tx, ice_rx) = mpsc::unbounded_channel();
        let (open, _) = watch::channel(false);
        let (state_tx, _) = watch::channel(PcState::New);

        let budget = frame_budget.clone();
        let overflow_state = state_tx.clone();
        let frame_callback = Closure::new(move |array: Uint8Array| {
            let len = array.length() as usize;
            if len > MAX_FRAME || !budget.try_admit(len) {
                overflow_state.send_replace(PcState::Failed);
                return;
            }
            let frame = Bytes::from(array.to_vec());
            if frame_tx.send(frame).is_err() {
                budget.release(len);
            }
        });
        let ice_callback = Closure::new(move |value: JsValue| {
            log::debug!(
                "WebRTC local ICE {}",
                if value.is_null() {
                    "gathering complete"
                } else {
                    "candidate ready"
                }
            );
            let _ = ice_tx.send(ice(value));
        });
        let open_tx = open.clone();
        let open_callback = Closure::new(move |value: bool| {
            log::info!(
                "WebRTC data channel {}",
                if value { "opened" } else { "closed" }
            );
            open_tx.send_replace(value);
        });
        let callback_state = state_tx.clone();
        let state_callback = Closure::new(move |value: String| {
            log::debug!("WebRTC peer state: {value}");
            callback_state.send_replace(state(&value));
        });

        let servers = Array::new();
        for server in ice_servers {
            servers.push(&JsValue::from_str(server));
        }
        let role = match role {
            DcRole::Offerer => "offerer",
            DcRole::Answerer => "answerer",
        };
        let peer = create_peer(
            role,
            &servers,
            frame_callback.as_ref().unchecked_ref(),
            ice_callback.as_ref().unchecked_ref(),
            open_callback.as_ref().unchecked_ref(),
            state_callback.as_ref().unchecked_ref(),
        )
        .map_err(js_error)?;
        if let Ok(pc) = peer_connection(&peer).dyn_into::<web_sys::RtcPeerConnection>() {
            let _ = Reflect::set(&js_sys::global(), &JsValue::from_str("__p2p"), pc.as_ref());
            DEBUG_PC.with(|debug| *debug.borrow_mut() = Some(pc));
        }
        Ok(Self {
            peer: SendWrapper::new(peer),
            frame_rx: Some(frame_rx),
            frame_budget,
            ice_rx,
            open,
            state: state_tx,
            split: false,
            _frame_callback: SendWrapper::new(frame_callback),
            _ice_callback: SendWrapper::new(ice_callback),
            _open_callback: SendWrapper::new(open_callback),
            _state_callback: SendWrapper::new(state_callback),
        })
    }
}

impl Drop for PeerChannel {
    fn drop(&mut self) {
        close_peer(&self.peer);
    }
}

pub struct DcTx {
    peer: SendWrapper<JsValue>,
    max_frame: usize,
    cancel: CancellationToken,
    relay_switch: Arc<Notify>,
}

impl FrameTx for DcTx {
    async fn send(&self, frame: Bytes) -> Result<(), TransportError> {
        if frame.len() > self.max_frame {
            return Err(TransportError::TooLarge(frame.len()));
        }
        let bytes = Uint8Array::new_with_length(
            frame
                .len()
                .try_into()
                .map_err(|_| TransportError::TooLarge(frame.len()))?,
        );
        bytes.copy_from(&frame);
        let promise = send_frame(&self.peer, &bytes).map_err(js_error)?;
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(TransportError::Cancelled),
            _ = self.relay_switch.notified() => Err(TransportError::Closed),
            _ = sleep(BUFFER_STALL) => Err(TransportError::Timeout),
            result = JsFuture::from(promise) => result.map(|_| ()).map_err(js_error),
        }
    }

    fn max_frame(&self) -> usize {
        self.max_frame
    }

    fn is_length_prefixed(&self) -> bool {
        false
    }

    fn path(&self) -> Path {
        Path::Direct
    }
}

pub struct DcRx {
    frames: mpsc::UnboundedReceiver<Bytes>,
    budget: Arc<ByteBudget>,
}

impl FrameRx for DcRx {
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError> {
        let frame = self.frames.recv().await;
        if let Some(frame) = &frame {
            self.budget.release(frame.len());
        }
        Ok(frame)
    }
}

impl DataChannel for PeerChannel {
    type Tx = DcTx;
    type Rx = DcRx;

    async fn create_offer(&mut self) -> Result<String, TransportError> {
        promise_string(create_offer(&self.peer).map_err(js_error)?).await
    }

    async fn accept_offer(&mut self, sdp: &str) -> Result<String, TransportError> {
        promise_string(accept_offer(&self.peer, sdp).map_err(js_error)?).await
    }

    async fn accept_answer(&mut self, sdp: &str) -> Result<(), TransportError> {
        JsFuture::from(accept_answer(&self.peer, sdp).map_err(js_error)?)
            .await
            .map(|_| ())
            .map_err(js_error)
    }

    async fn add_ice(&mut self, candidate: &IceCandidate) -> Result<(), TransportError> {
        JsFuture::from(
            add_ice(
                &self.peer,
                &candidate.candidate,
                candidate.sdp_mid.as_deref(),
                candidate.sdp_mline_index,
            )
            .map_err(js_error)?,
        )
        .await
        .map(|_| ())
        .map_err(js_error)
    }

    async fn next_local_ice(&mut self) -> Option<IceCandidate> {
        self.ice_rx.recv().await.flatten()
    }

    fn open_watch(&self) -> watch::Receiver<bool> {
        self.open.subscribe()
    }

    fn state_watch(&self) -> watch::Receiver<PcState> {
        self.state.subscribe()
    }

    fn max_message_size(&self) -> Option<usize> {
        let limit = max_message_size(&self.peer);
        if !limit.is_finite() || limit <= 0.0 {
            return Some(DEFAULT_MAX_MESSAGE);
        }
        Some((limit as usize).min(MAX_FRAME))
    }

    async fn path_kind(&self) -> Path {
        let Ok(promise) = path_kind(&self.peer) else {
            return Path::Direct;
        };
        match JsFuture::from(promise)
            .await
            .ok()
            .and_then(|value| value.as_string())
            .as_deref()
        {
            Some("relayed") => Path::Relayed,
            _ => Path::Direct,
        }
    }

    fn split(
        &mut self,
        cancel: CancellationToken,
        relay_switch: Arc<Notify>,
        receive_window: usize,
    ) -> Option<(Self::Tx, Self::Rx)> {
        if self.split {
            return None;
        }
        self.split = true;
        let frames = self.frame_rx.take()?;
        self.frame_budget
            .set_budget(receive_window.saturating_add(MAX_FRAME));
        let max_frame = self.max_message_size().unwrap_or(DEFAULT_MAX_MESSAGE);
        Some((
            DcTx {
                peer: SendWrapper::new((*self.peer).clone()),
                max_frame,
                cancel,
                relay_switch,
            },
            DcRx {
                frames,
                budget: self.frame_budget.clone(),
            },
        ))
    }

    fn close(&self) {
        close_peer(&self.peer);
    }
}

pub fn debug_pc() -> Option<web_sys::RtcPeerConnection> {
    DEBUG_PC.with(|debug| debug.borrow().clone())
}
