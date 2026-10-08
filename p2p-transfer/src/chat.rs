//! Chat rooms.
//!
//! A room is an ephemeral iroh endpoint owned by the host's page. Guests dial it with the
//! room link (ticket plus capability, the same grammar as a share link with a `chat` token)
//! and keep one bidirectional stream open. The host assigns display names, relays every
//! message to every member in one order, and announces joins and leaves. Nothing is stored:
//! the room ends when the host's page closes.
//!
//! Transport pieces are shared with the file protocol: [`split_control`] for the framed
//! stream, [`cap_eq`] for the access code, and the same relay selection as every other
//! endpoint. The frames themselves are [`ChatMsg`] on their own ALPN.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{EndpointId, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use n0_future::task::{self, AbortOnDropHandle, JoinHandle};
use n0_future::time::{timeout, Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::node::{self, Node, NodeError, RelayChoice};
use crate::protocol::{
    cap_eq, clean_chat_name, clean_chat_text, decode_chat, encode_chat, ChatMsg, CAP_LEN,
    CHAT_ALPN, CHAT_VERSION, MAX_CHAT_MEMBERS,
};
use crate::transfer::{split_control, FrameRx, FrameTx, Path};

/// How long a guest has to send `Hello` after connecting, and a host to answer it.
const HELLO_DEADLINE: Duration = Duration::from_secs(10);
/// Frames queued per guest before the host stops waiting for that guest. Larger than
/// [`HISTORY`] so a join can replay the whole history without filling the queue.
const OUTBOUND_QUEUE: usize = 128;
/// Messages replayed to a guest that joins late, so a share link posted a moment before
/// the other side arrived is not lost.
const HISTORY: usize = 32;
/// Events queued for the UI before older ones are dropped.
const EVENT_QUEUE: usize = 256;
/// Messages one guest may send per [`RATE_WINDOW`].
const RATE_LIMIT: usize = 30;
const RATE_WINDOW: Duration = Duration::from_secs(10);
/// The display name a host gets when it does not pick one.
const HOST_NAME: &str = "Host";
/// The display name a guest gets when it does not pick one.
const GUEST_NAME: &str = "Guest";

/// What a room session reports to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// Host only: the room is dialable.
    Ready {
        link: String,
        agent_link: String,
        you: String,
    },
    /// Guest only: the host accepted the `Hello`.
    Connected {
        you: String,
        members: Vec<String>,
    },
    Message {
        from: String,
        text: String,
        seq: u64,
    },
    Joined {
        name: String,
    },
    Left {
        name: String,
    },
    /// The session is over because of an error; the message is for display.
    Failed(String),
    /// The session ended normally (host closed the room, or this side left).
    Closed,
}

/// What the UI asks a room session to do.
#[derive(Debug)]
pub enum ChatCommand {
    Say(String),
}

/// The app's handle on one room session, host or guest. Dropping it leaves the room.
pub struct ChatHandle {
    pub events: mpsc::Receiver<ChatEvent>,
    pub commands: mpsc::Sender<ChatCommand>,
    pub cancel: CancellationToken,
    _task: JoinHandle<()>,
}

impl Drop for ChatHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl ChatHandle {
    /// Open a room on this device. `base_url` is the origin the links are built on.
    pub fn host(relay: RelayChoice, name: String, base_url: String) -> ChatHandle {
        Self::spawn(move |events, commands, cancel| async move {
            if let Err(error) = run_host(relay, name, base_url, &events, commands, cancel).await {
                let _ = events.send(ChatEvent::Failed(error.to_string())).await;
            }
        })
    }

    /// Join the room a link points at.
    pub fn join(
        relay: RelayChoice,
        ticket: EndpointTicket,
        cap: [u8; CAP_LEN],
        name: String,
    ) -> ChatHandle {
        Self::spawn(move |events, commands, cancel| async move {
            if let Err(error) = run_guest(relay, ticket, cap, name, &events, commands, cancel).await
            {
                let _ = events.send(ChatEvent::Failed(error.to_string())).await;
            }
        })
    }

    fn spawn<F, Fut>(run: F) -> ChatHandle
    where
        F: FnOnce(mpsc::Sender<ChatEvent>, mpsc::Receiver<ChatCommand>, CancellationToken) -> Fut,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (events_tx, events) = mpsc::channel(EVENT_QUEUE);
        let (commands, commands_rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let task = task::spawn(run(events_tx, commands_rx, cancel.clone()));
        ChatHandle {
            events,
            commands,
            cancel,
            _task: task,
        }
    }

    /// Everything the session has reported since the last call, without blocking.
    pub fn drain(&mut self) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }
}

// ── Host ─────────────────────────────────────────────────────────────────────

struct Member {
    id: EndpointId,
    name: String,
    outbound: mpsc::Sender<Bytes>,
    cancel: CancellationToken,
}

/// Shared state of one hosted room.
struct Room {
    host: String,
    cap: [u8; CAP_LEN],
    members: Mutex<Vec<Member>>,
    /// The last [`HISTORY`] `Said` frames, replayed after `Welcome`.
    history: Mutex<VecDeque<ChatMsg>>,
    seq: AtomicU64,
    events: mpsc::Sender<ChatEvent>,
    cancel: CancellationToken,
}

impl Room {
    fn publish(&self, event: ChatEvent) {
        // The UI drains every frame; if it is not drawing, dropping an event is fine.
        let _ = self.events.try_send(event);
    }

    /// Queue a frame for every member. A member whose queue is full is behind by
    /// `OUTBOUND_QUEUE` frames and is disconnected rather than slowing the room.
    fn broadcast(&self, msg: &ChatMsg) {
        let Ok(frame) = encode_chat(msg) else {
            return;
        };
        let members = self.members.lock().unwrap();
        for member in members.iter() {
            if member.outbound.try_send(frame.clone()).is_err() {
                member.cancel.cancel();
            }
        }
    }

    fn say(&self, from: &str, text: &str) {
        let text = clean_chat_text(text);
        if text.is_empty() {
            return;
        }
        let seq = self.seq.fetch_add(1, Ordering::AcqRel) + 1;
        let said = ChatMsg::Said {
            from: from.to_string(),
            text: text.clone(),
            seq,
        };
        {
            let mut history = self.history.lock().unwrap();
            if history.len() >= HISTORY {
                history.pop_front();
            }
            history.push_back(said.clone());
        }
        self.broadcast(&said);
        self.publish(ChatEvent::Message {
            from: from.to_string(),
            text,
            seq,
        });
    }

    fn roster(&self, members: &[Member]) -> Vec<String> {
        std::iter::once(self.host.clone())
            .chain(members.iter().map(|m| m.name.clone()))
            .collect()
    }

    /// The requested name, or a default, made unique within the room.
    fn assign_name(&self, requested: &str, members: &[Member]) -> String {
        let base = {
            let cleaned = clean_chat_name(requested);
            if cleaned.is_empty() {
                GUEST_NAME.to_string()
            } else {
                cleaned
            }
        };
        let taken =
            |candidate: &str| candidate == self.host || members.iter().any(|m| m.name == candidate);
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|candidate| !taken(candidate))
            .expect("an unused name exists")
    }

    /// Add a guest and queue its `Welcome` under one lock, so no broadcast can slip in
    /// before the welcome. Returns the assigned name and the guest's outbound queue.
    fn register(
        &self,
        id: EndpointId,
        requested: &str,
        cancel: CancellationToken,
    ) -> Result<(String, mpsc::Receiver<Bytes>), &'static str> {
        let mut members = self.members.lock().unwrap();
        if members.len() >= MAX_CHAT_MEMBERS {
            return Err("room is full");
        }
        let name = self.assign_name(requested, &members);
        let (outbound, outbound_rx) = mpsc::channel::<Bytes>(OUTBOUND_QUEUE);
        let welcome = ChatMsg::Welcome {
            you: name.clone(),
            members: self.roster(&members),
        };
        let frame = encode_chat(&welcome).map_err(|_| "could not encode the welcome")?;
        let _ = outbound.try_send(frame);
        for said in self.history.lock().unwrap().iter() {
            if let Ok(frame) = encode_chat(said) {
                let _ = outbound.try_send(frame);
            }
        }
        members.push(Member {
            id,
            name: name.clone(),
            outbound,
            cancel,
        });
        Ok((name, outbound_rx))
    }

    fn remove(&self, id: EndpointId) -> Option<String> {
        let mut members = self.members.lock().unwrap();
        let index = members.iter().position(|m| m.id == id)?;
        Some(members.remove(index).name)
    }
}

#[derive(Clone)]
struct RoomHandler(Arc<Room>);

impl std::fmt::Debug for RoomHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoomHandler").finish_non_exhaustive()
    }
}

impl ProtocolHandler for RoomHandler {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let room = self.0.clone();
        let (done_tx, done_rx) = oneshot::channel();
        // Same shape as the file protocol handler: `accept` must return a `Send` future on
        // every target, so the session runs in its own task.
        let _task = task::spawn(async move {
            let result = serve_guest(room, connection).await;
            done_tx.send(result).ok();
        });
        done_rx
            .await
            .map_err(AcceptError::from_err)?
            .map_err(AcceptError::from)
    }
}

fn io_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

async fn serve_guest(room: Arc<Room>, connection: Connection) -> std::io::Result<()> {
    let id = connection.remote_id();
    log::debug!("chat: guest connection accepted");
    let (send, recv) = timeout(HELLO_DEADLINE, connection.accept_bi())
        .await
        .map_err(|_| io_error("guest opened no stream"))?
        .map_err(std::io::Error::other)?;
    let member_cancel = room.cancel.child_token();
    let (tx, mut rx, _writer) = split_control(send, recv, Path::Relayed, member_cancel.clone());

    let first = timeout(HELLO_DEADLINE, rx.recv())
        .await
        .map_err(|_| io_error("guest sent no Hello"))?
        .map_err(|e| io_error(e.to_string()))?
        .ok_or_else(|| io_error("guest closed before Hello"))?;
    let reject = |message: &str| ChatMsg::Error {
        message: message.to_string(),
    };
    let requested = match decode_chat(&first) {
        Ok(ChatMsg::Hello { version, cap, name }) => {
            if version != CHAT_VERSION {
                let _ = tx
                    .send(encode_chat(&reject("unsupported chat version")).unwrap())
                    .await;
                return Err(io_error("unsupported chat version"));
            }
            if !cap_eq(&cap, &room.cap) {
                let _ = tx.send(encode_chat(&reject("unauthorized")).unwrap()).await;
                connection.close(1u8.into(), b"unauthorized");
                return Err(io_error("unauthorized"));
            }
            name
        }
        _ => {
            let _ = tx
                .send(encode_chat(&reject("expected a Hello frame")).unwrap())
                .await;
            return Err(io_error("expected a Hello frame"));
        }
    };

    let (name, outbound_rx) = match room.register(id, &requested, member_cancel.clone()) {
        Ok(registered) => registered,
        Err(message) => {
            let _ = tx.send(encode_chat(&reject(message)).unwrap()).await;
            return Err(io_error(message));
        }
    };
    let _forwarder = AbortOnDropHandle::new(task::spawn({
        let cancel = member_cancel.clone();
        let mut outbound_rx = outbound_rx;
        async move {
            while let Some(frame) = outbound_rx.recv().await {
                if tx.send(frame).await.is_err() {
                    cancel.cancel();
                    return;
                }
            }
        }
    }));
    room.broadcast(&ChatMsg::Joined { name: name.clone() });
    room.publish(ChatEvent::Joined { name: name.clone() });
    log::info!("chat: {name} joined");

    let mut recent: VecDeque<Instant> = VecDeque::with_capacity(RATE_LIMIT);
    let outcome = loop {
        let frame = tokio::select! {
            biased;
            _ = member_cancel.cancelled() => break Ok(()),
            frame = rx.recv() => frame,
        };
        match frame {
            Ok(Some(frame)) => match decode_chat(&frame) {
                Ok(ChatMsg::Say { text }) => {
                    let now = Instant::now();
                    while recent
                        .front()
                        .is_some_and(|t| now.duration_since(*t) > RATE_WINDOW)
                    {
                        recent.pop_front();
                    }
                    if recent.len() >= RATE_LIMIT {
                        room.broadcast_to(id, &reject("too many messages; slow down"));
                        break Err(io_error("rate limit"));
                    }
                    recent.push_back(now);
                    room.say(&name, &text);
                }
                Ok(ChatMsg::Error { message }) => break Err(io_error(message)),
                Ok(_) => break Err(io_error("unexpected chat frame")),
                Err(error) => break Err(io_error(error.to_string())),
            },
            Ok(None) => break Ok(()),
            Err(error) => break Err(io_error(error.to_string())),
        }
    };

    if let Some(name) = room.remove(id) {
        room.broadcast(&ChatMsg::Left { name: name.clone() });
        room.publish(ChatEvent::Left { name: name.clone() });
        log::info!("chat: {name} left");
    }
    connection.close(0u8.into(), b"bye");
    outcome
}

impl Room {
    fn broadcast_to(&self, id: EndpointId, msg: &ChatMsg) {
        let Ok(frame) = encode_chat(msg) else {
            return;
        };
        let members = self.members.lock().unwrap();
        if let Some(member) = members.iter().find(|m| m.id == id) {
            let _ = member.outbound.try_send(frame);
        }
    }
}

async fn run_host(
    relay: RelayChoice,
    name: String,
    base_url: String,
    events: &mpsc::Sender<ChatEvent>,
    mut commands: mpsc::Receiver<ChatCommand>,
    cancel: CancellationToken,
) -> Result<(), NodeError> {
    let secret = SecretKey::generate();
    let cap = node::derive_cap(&secret);
    let endpoint = timeout(
        node::BIND_TIMEOUT,
        node::endpoint_builder(&relay)?
            .secret_key(secret)
            .alpns(vec![CHAT_ALPN.to_vec()])
            .bind(),
    )
    .await
    .map_err(|_| NodeError::Bind("chat endpoint startup timed out".into()))?
    .map_err(|error| NodeError::Bind(error.to_string()))?;
    let host = {
        let cleaned = clean_chat_name(&name);
        if cleaned.is_empty() {
            HOST_NAME.to_string()
        } else {
            cleaned
        }
    };
    let room = Arc::new(Room {
        host: host.clone(),
        cap,
        members: Mutex::new(Vec::new()),
        history: Mutex::new(VecDeque::new()),
        seq: AtomicU64::new(0),
        events: events.clone(),
        cancel: cancel.clone(),
    });
    let router = Router::builder(endpoint.clone())
        .accept(CHAT_ALPN, RoomHandler(room.clone()))
        .spawn();
    let ticket = match node::dialable_ticket(&endpoint, &relay).await {
        Ok(ticket) => ticket,
        Err(error) => {
            let _ = router.shutdown().await;
            endpoint.close().await;
            return Err(error);
        }
    };
    let _ = events
        .send(ChatEvent::Ready {
            link: Node::chat_link(&base_url, &ticket, &cap, false),
            agent_link: Node::chat_link(&base_url, &ticket, &cap, true),
            you: host.clone(),
        })
        .await;
    log::info!("chat: room open");

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            command = commands.recv() => match command {
                Some(ChatCommand::Say(text)) => room.say(&host, &text),
                None => break,
            },
        }
    }
    cancel.cancel();
    let _ = router.shutdown().await;
    endpoint.close().await;
    let _ = events.send(ChatEvent::Closed).await;
    log::info!("chat: room closed");
    Ok(())
}

// ── Guest ────────────────────────────────────────────────────────────────────

async fn run_guest(
    relay: RelayChoice,
    ticket: EndpointTicket,
    cap: [u8; CAP_LEN],
    name: String,
    events: &mpsc::Sender<ChatEvent>,
    mut commands: mpsc::Receiver<ChatCommand>,
    cancel: CancellationToken,
) -> Result<(), NodeError> {
    let endpoint = timeout(
        node::BIND_TIMEOUT,
        node::endpoint_builder(&relay)?
            .secret_key(SecretKey::generate())
            .bind(),
    )
    .await
    .map_err(|_| NodeError::Bind("chat endpoint startup timed out".into()))?
    .map_err(|error| NodeError::Bind(error.to_string()))?;
    let result = guest_session(&endpoint, ticket, cap, name, events, &mut commands, &cancel).await;
    endpoint.close().await;
    match result {
        Ok(()) => {
            let _ = events.send(ChatEvent::Closed).await;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn guest_session(
    endpoint: &iroh::Endpoint,
    ticket: EndpointTicket,
    cap: [u8; CAP_LEN],
    name: String,
    events: &mpsc::Sender<ChatEvent>,
    commands: &mut mpsc::Receiver<ChatCommand>,
    cancel: &CancellationToken,
) -> Result<(), NodeError> {
    let addr = node::browser_relay_addr(ticket.endpoint_addr().clone())?;
    let connection = timeout(node::DIAL_TIMEOUT, endpoint.connect(addr, CHAT_ALPN))
        .await
        .map_err(|_| {
            NodeError::Connect("the room did not answer; is the host's page still open?".into())
        })?
        .map_err(|error| NodeError::Connect(error.to_string()))?;
    log::debug!("chat: dialled the room");
    let (send, recv) = timeout(HELLO_DEADLINE, connection.open_bi())
        .await
        .map_err(|_| NodeError::Connect("could not open a stream to the room".into()))?
        .map_err(|error| NodeError::Connect(error.to_string()))?;
    let (tx, mut rx, _writer) = split_control(send, recv, Path::Relayed, cancel.clone());
    let hello = ChatMsg::Hello {
        version: CHAT_VERSION,
        cap,
        name: clean_chat_name(&name),
    };
    tx.send(encode_chat(&hello).map_err(|e| NodeError::Connect(e.to_string()))?)
        .await
        .map_err(|e| NodeError::Connect(e.to_string()))?;

    let outcome = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break Ok(()),
            command = commands.recv() => match command {
                Some(ChatCommand::Say(text)) => {
                    let text = clean_chat_text(&text);
                    if text.is_empty() {
                        continue;
                    }
                    let frame = match encode_chat(&ChatMsg::Say { text }) {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    };
                    if let Err(error) = tx.send(frame).await {
                        break Err(NodeError::Connect(error.to_string()));
                    }
                }
                None => break Ok(()),
            },
            frame = rx.recv() => match frame {
                Ok(Some(frame)) => match decode_chat(&frame) {
                    Ok(ChatMsg::Welcome { you, members }) => {
                        let _ = events.send(ChatEvent::Connected { you, members }).await;
                    }
                    Ok(ChatMsg::Said { from, text, seq }) => {
                        let _ = events.send(ChatEvent::Message { from, text, seq }).await;
                    }
                    Ok(ChatMsg::Joined { name }) => {
                        let _ = events.send(ChatEvent::Joined { name }).await;
                    }
                    Ok(ChatMsg::Left { name }) => {
                        let _ = events.send(ChatEvent::Left { name }).await;
                    }
                    Ok(ChatMsg::Error { message }) => {
                        break Err(NodeError::Connect(if message == "unauthorized" {
                            "the room rejected this link (wrong or expired access code)".into()
                        } else {
                            message
                        }));
                    }
                    Ok(_) => break Err(NodeError::Connect("unexpected chat frame".into())),
                    Err(error) => break Err(NodeError::Connect(error.to_string())),
                },
                Ok(None) => break Ok(()),
                Err(error) => break Err(NodeError::Connect(error.to_string())),
            },
        }
    };
    connection.close(0u8.into(), b"bye");
    outcome
}

// ── Machine-readable events (browser bridge) ─────────────────────────────────

/// One JSON object per event, for the browser bridge and the agent script. Hand-rolled so
/// the app needs no JSON dependency; every string goes through [`json_string`].
pub fn event_json(event: &ChatEvent) -> String {
    match event {
        ChatEvent::Ready {
            link,
            agent_link,
            you,
        } => format!(
            r#"{{"type":"ready","link":{},"agentLink":{},"you":{}}}"#,
            json_string(link),
            json_string(agent_link),
            json_string(you)
        ),
        ChatEvent::Connected { you, members } => format!(
            r#"{{"type":"connected","you":{},"members":[{}]}}"#,
            json_string(you),
            members
                .iter()
                .map(|m| json_string(m))
                .collect::<Vec<_>>()
                .join(",")
        ),
        ChatEvent::Message { from, text, seq } => format!(
            r#"{{"type":"message","from":{},"text":{},"seq":{seq}}}"#,
            json_string(from),
            json_string(text)
        ),
        ChatEvent::Joined { name } => {
            format!(r#"{{"type":"joined","name":{}}}"#, json_string(name))
        }
        ChatEvent::Left { name } => format!(r#"{{"type":"left","name":{}}}"#, json_string(name)),
        ChatEvent::Failed(message) => {
            format!(r#"{{"type":"failed","message":{}}}"#, json_string(message))
        }
        ChatEvent::Closed => r#"{"type":"closed"}"#.to_string(),
    }
}

/// A JSON string literal with the escapes RFC 8259 requires.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
