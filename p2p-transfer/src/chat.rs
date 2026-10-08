//! Chat rooms.
//!
//! A room is an ephemeral iroh endpoint owned by the host's page. Guests dial it with the
//! room link (ticket plus capability, the same grammar as a share link with a `chat` token)
//! and keep one bidirectional stream open. The host assigns display names, relays every
//! message to every member in one order, and announces joins and leaves. Nothing is stored:
//! the room ends when the host's page closes.
//!
//! Transport pieces are shared with the file protocol: [`split_control_with_queue`] for the
//! framed stream, [`cap_eq`] for the access code, and the node helpers for binding, dialling
//! and accepting. The frames themselves are [`ChatMsg`] on their own ALPN.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{EndpointId, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use n0_future::task::{self, JoinHandle};
use n0_future::time::{timeout, Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::node::{self, Node, NodeError, RelayChoice};
use crate::protocol::{
    cap_eq, clean_chat_name, clean_chat_text, decode_chat, encode_chat, ChatMsg, CAP_LEN,
    CHAT_ALPN, CHAT_VERSION, MAX_CHAT_MEMBERS,
};
use crate::transfer::{split_control, split_control_with_queue, ControlTx, FrameRx, FrameTx, Path};

/// How long a guest has to send `Hello` after connecting, and a host to answer it.
const HELLO_DEADLINE: Duration = Duration::from_secs(10);
/// Frames queued for one guest before the host disconnects it rather than wait. Larger than
/// [`HISTORY`] so a join can replay the whole history in one go.
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

/// Called after every event is queued, so a UI without its own callback (egui) can repaint
/// on demand instead of polling.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// The session's side of the event channel: queue an event, then wake the UI.
#[derive(Clone)]
struct Outlet {
    events: mpsc::Sender<ChatEvent>,
    wake: Wake,
}

impl Outlet {
    async fn send(&self, event: ChatEvent) {
        let _ = self.events.send(event).await;
        (self.wake)();
    }

    /// Non-blocking, for callers holding a lock. The UI drains every frame; if it is not
    /// drawing, dropping an event is fine.
    fn publish(&self, event: ChatEvent) {
        let _ = self.events.try_send(event);
        (self.wake)();
    }
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
    pub fn host(relay: RelayChoice, name: String, base_url: String, wake: Wake) -> ChatHandle {
        Self::spawn(wake, |out, commands, cancel| {
            run_host(relay, name, base_url, out, commands, cancel)
        })
    }

    /// Join the room a link points at.
    pub fn join(
        relay: RelayChoice,
        ticket: EndpointTicket,
        cap: [u8; CAP_LEN],
        name: String,
        wake: Wake,
    ) -> ChatHandle {
        Self::spawn(wake, |out, commands, cancel| {
            run_guest(relay, ticket, cap, name, out, commands, cancel)
        })
    }

    /// Run a session in its own task; an error ends it with one `Failed` event.
    fn spawn<F, Fut>(wake: Wake, run: F) -> ChatHandle
    where
        F: FnOnce(Outlet, mpsc::Receiver<ChatCommand>, CancellationToken) -> Fut,
        Fut: std::future::Future<Output = Result<(), NodeError>> + Send + 'static,
    {
        let (events_tx, events) = mpsc::channel(EVENT_QUEUE);
        let (commands, commands_rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let out = Outlet {
            events: events_tx,
            wake,
        };
        let session = run(out.clone(), commands_rx, cancel.clone());
        let task = task::spawn(async move {
            if let Err(error) = session.await {
                out.send(ChatEvent::Failed(error.to_string())).await;
            }
        });
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
    /// The guest's control stream, with an [`OUTBOUND_QUEUE`]-deep queue in front of it.
    tx: ControlTx,
    cancel: CancellationToken,
}

/// Shared state of one hosted room.
struct Room {
    host: String,
    cap: [u8; CAP_LEN],
    members: Mutex<Vec<Member>>,
    /// The last [`HISTORY`] `Said` frames, already encoded, replayed after `Welcome`.
    history: Mutex<VecDeque<Bytes>>,
    seq: AtomicU64,
    out: Outlet,
    /// Ends with the room; every member's token is a child of it.
    cancel: CancellationToken,
}

impl Room {
    /// Queue a frame for every member. A member whose queue is full is behind by
    /// [`OUTBOUND_QUEUE`] frames and is disconnected rather than slowing the room.
    fn broadcast(&self, frame: &Bytes) {
        let members = self.members.lock().unwrap();
        for member in members.iter() {
            if !matches!(member.tx.try_send(frame.clone()), Ok(true)) {
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
        let Ok(frame) = encode_chat(&ChatMsg::Said {
            from: from.to_string(),
            text: text.clone(),
            seq,
        }) else {
            return;
        };
        {
            let mut history = self.history.lock().unwrap();
            if history.len() >= HISTORY {
                history.pop_front();
            }
            history.push_back(frame.clone());
        }
        self.broadcast(&frame);
        self.out.publish(ChatEvent::Message {
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

    /// Add a guest and queue its `Welcome` and the history under one lock, so no broadcast
    /// can slip in before the welcome. Returns the assigned name.
    fn register(
        &self,
        id: EndpointId,
        requested: &str,
        tx: ControlTx,
        cancel: CancellationToken,
    ) -> Result<String, &'static str> {
        let mut members = self.members.lock().unwrap();
        if members.len() >= MAX_CHAT_MEMBERS {
            return Err("room is full");
        }
        let name = self.assign_name(requested, &members);
        let welcome = ChatMsg::Welcome {
            you: name.clone(),
            members: self.roster(&members),
        };
        let frame = encode_chat(&welcome).map_err(|_| "could not encode the welcome")?;
        let _ = tx.try_send(frame);
        for frame in self.history.lock().unwrap().iter() {
            let _ = tx.try_send(frame.clone());
        }
        members.push(Member {
            id,
            name: name.clone(),
            tx,
            cancel,
        });
        Ok(name)
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
        node::accept_in_task(move || serve_guest(room, connection)).await
    }
}

fn io_error(message: impl Into<String>) -> std::io::Error {
    std::io::Error::other(message.into())
}

async fn serve_guest(room: Arc<Room>, connection: Connection) -> std::io::Result<()> {
    let id = connection.remote_id();
    let (send, recv) = timeout(HELLO_DEADLINE, connection.accept_bi())
        .await
        .map_err(|_| io_error("guest opened no stream"))?
        .map_err(std::io::Error::other)?;
    let member_cancel = room.cancel.child_token();
    let (tx, mut rx, _writer) = split_control_with_queue(
        send,
        recv,
        Path::Relayed,
        member_cancel.clone(),
        OUTBOUND_QUEUE,
    );

    let first = timeout(HELLO_DEADLINE, rx.recv())
        .await
        .map_err(|_| io_error("guest sent no Hello"))?
        .map_err(|e| io_error(e.to_string()))?
        .ok_or_else(|| io_error("guest closed before Hello"))?;
    let accepted = match decode_chat(&first) {
        Ok(ChatMsg::Hello { version, .. }) if version != CHAT_VERSION => {
            Err("unsupported chat version")
        }
        Ok(ChatMsg::Hello { cap, .. }) if !cap_eq(&cap, &room.cap) => Err("unauthorized"),
        Ok(ChatMsg::Hello { name, .. }) => {
            room.register(id, &name, tx.clone(), member_cancel.clone())
        }
        _ => Err("expected a Hello frame"),
    };
    let reject = |message: &str| {
        encode_chat(&ChatMsg::Error {
            message: message.to_string(),
        })
        .expect("a short error message always encodes")
    };
    let name = match accepted {
        Ok(name) => name,
        Err(message) => {
            let _ = tx.send(reject(message)).await;
            connection.close(1u8.into(), message.as_bytes());
            return Err(io_error(message));
        }
    };
    if let Ok(frame) = encode_chat(&ChatMsg::Joined { name: name.clone() }) {
        room.broadcast(&frame);
    }
    room.out.publish(ChatEvent::Joined { name: name.clone() });
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
                        let _ = tx.try_send(reject("too many messages; slow down"));
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
        if let Ok(frame) = encode_chat(&ChatMsg::Left { name: name.clone() }) {
            room.broadcast(&frame);
        }
        room.out.publish(ChatEvent::Left { name: name.clone() });
        log::info!("chat: {name} left");
    }
    connection.close(0u8.into(), b"bye");
    outcome
}

async fn run_host(
    relay: RelayChoice,
    name: String,
    base_url: String,
    out: Outlet,
    mut commands: mpsc::Receiver<ChatCommand>,
    cancel: CancellationToken,
) -> Result<(), NodeError> {
    let secret = SecretKey::generate();
    let cap = node::derive_cap(&secret);
    let endpoint = node::bind_endpoint(&relay, secret, vec![CHAT_ALPN.to_vec()]).await?;
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
        out: out.clone(),
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
    out.send(ChatEvent::Ready {
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
    out.send(ChatEvent::Closed).await;
    log::info!("chat: room closed");
    Ok(())
}

// ── Guest ────────────────────────────────────────────────────────────────────

async fn run_guest(
    relay: RelayChoice,
    ticket: EndpointTicket,
    cap: [u8; CAP_LEN],
    name: String,
    out: Outlet,
    mut commands: mpsc::Receiver<ChatCommand>,
    cancel: CancellationToken,
) -> Result<(), NodeError> {
    let endpoint = node::bind_endpoint(&relay, SecretKey::generate(), Vec::new()).await?;
    let result = guest_session(&endpoint, ticket, cap, name, &out, &mut commands, &cancel).await;
    endpoint.close().await;
    result?;
    out.send(ChatEvent::Closed).await;
    Ok(())
}

async fn guest_session(
    endpoint: &iroh::Endpoint,
    ticket: EndpointTicket,
    cap: [u8; CAP_LEN],
    name: String,
    out: &Outlet,
    commands: &mut mpsc::Receiver<ChatCommand>,
    cancel: &CancellationToken,
) -> Result<(), NodeError> {
    let connection =
        node::dial(endpoint, &ticket, CHAT_ALPN)
            .await
            .map_err(|error| match error {
                NodeError::Connect(_) => NodeError::Connect(
                    "the room did not answer; is the host's page still open?".into(),
                ),
                other => other,
            })?;
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
                        out.send(ChatEvent::Connected { you, members }).await;
                    }
                    Ok(ChatMsg::Said { from, text, seq }) => {
                        out.send(ChatEvent::Message { from, text, seq }).await;
                    }
                    Ok(ChatMsg::Joined { name }) => out.send(ChatEvent::Joined { name }).await,
                    Ok(ChatMsg::Left { name }) => out.send(ChatEvent::Left { name }).await,
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
