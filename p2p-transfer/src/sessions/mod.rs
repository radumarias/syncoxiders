//! The UI-agnostic session core.
//!
//! Every Oxfer UI (egui on native today, Dioxus on the web) is a thin adapter over this
//! module. It owns every handle, task and state machine between the user's gestures and the
//! engine (`transfer`, `node`, `chat`, `file_io`), and exposes three things:
//!
//! * **snapshots**: [`Oxfer::snapshot`] returns a plain, `Clone + PartialEq` view model.
//!   No egui, Dioxus or `web_sys` types appear in it, so a UI can diff it, store it in a
//!   signal, or print it in a test. Share links are wrapped in [`BearerLink`], whose `Debug`
//!   never prints the capability.
//! * **intents**: [`Oxfer::dispatch`] takes an [`Intent`], the user's request in domain
//!   terms (open this link, save here, stop sharing). Intents that do not apply in the
//!   current state are ignored, so a stale button press cannot corrupt a session.
//! * **change notification**: [`Notifier`] wakes the UI whenever a background task changed
//!   something (a callback for egui's `request_repaint`, a watch for async UIs). Engine
//!   progress that has no callback of its own is covered by [`Snapshot::live`]: while it is
//!   true, re-snapshot at ~10 Hz.
//!
//! A UI's frame loop is: `tick()` → `snapshot()` → render → `dispatch()` what the user did.
//! Browser and OS services the sessions need sit behind [`Platform`]. Files from pickers
//! and drops enter through the [`PickQueue`].
//!
//! ```no_run
//! use p2p_transfer::sessions::{Config, Intent, Oxfer, ReceiveIntent};
//!
//! let mut oxfer = Oxfer::new(Config::for_this_target());
//! oxfer.dispatch(Intent::OpenLink("https://oxfer.app/#endpoint…&cap=…".into()));
//! loop {
//!     oxfer.tick();
//!     let snapshot = oxfer.snapshot();
//!     if snapshot.receive.as_ref().and_then(|r| r.manifest()).is_some() {
//!         // offer Save: oxfer.dispatch(Intent::Receive(ReceiveIntent::Save(...)))
//!         break;
//!     }
//! #   break;
//! }
//! ```
//!
//! Invariants carried over from the egui app: no `Send` bound on sources or sinks (browser
//! JS handles are `!Send`), capabilities are never logged, and nothing here allocates in
//! proportion to peer input.

use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::watch;

pub mod chat;
#[cfg(target_arch = "wasm32")]
pub mod copies;
#[cfg(target_arch = "wasm32")]
pub mod diagnostics;
#[cfg(target_arch = "wasm32")]
mod location;
pub mod platform;
pub mod receive;
pub mod send;
pub mod text;

pub use chat::{ChatIntent, ChatLine, ChatRole, ChatSession, ChatSnapshot, ChatStatus, RoomLinks};
#[cfg(target_arch = "wasm32")]
pub use copies::{CopiesIntent, CopiesSnapshot, LocalCopies};
#[cfg(target_arch = "wasm32")]
pub use diagnostics::{DiagnosticsIntent, DiagnosticsSession, DiagnosticsSnapshot};
#[cfg(not(target_arch = "wasm32"))]
pub use platform::NativePlatform;
#[cfg(target_arch = "wasm32")]
pub use platform::WebPlatform;
pub use platform::{Platform, TestPlatform, Visibility, WakeLock};
pub use receive::{ReceiveIntent, ReceiveSession, ReceiveSnapshot, ReceivedFile, SaveTarget};
pub use send::{
    LinkState, PeerSnapshot, PickQueue, PickedFile, Preparing, SendIntent, SendSession,
    SendSnapshot,
};

pub use crate::chat::Wake;
use crate::node::{Node, RelayChoice};

/// A link that carries a bearer capability (a share link, a room link, or what the user
/// pasted). `Debug` redacts it; read it with [`BearerLink::as_str`] only to show or copy it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct BearerLink(pub(crate) String);

impl BearerLink {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for BearerLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            f.write_str("BearerLink(\"\")")
        } else {
            f.write_str("BearerLink(<redacted>)")
        }
    }
}

/// Wakes the UI after background work changed something.
///
/// Cloning shares it. Both mechanisms fire on every [`Notifier::notify`]: the optional wake
/// callback (egui: `Context::request_repaint`) and a revision counter that async UIs await
/// with [`Notifier::subscribe`].
#[derive(Clone)]
pub struct Notifier {
    revision: Arc<watch::Sender<u64>>,
    wake: Arc<RwLock<Option<Wake>>>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self {
            revision: Arc::new(watch::channel(0).0),
            wake: Arc::default(),
        }
    }
}

impl Notifier {
    pub fn notify(&self) {
        self.revision.send_modify(|r| *r = r.wrapping_add(1));
        let wake = self.wake.read().ok().and_then(|w| w.clone());
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Install (or replace) the wake callback.
    pub fn set_wake(&self, wake: Wake) {
        if let Ok(mut slot) = self.wake.write() {
            *slot = Some(wake);
        }
    }

    /// A receiver whose `changed()` resolves after each notification.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }
}

/// The app-wide "something failed" slot spawned tasks report into. The Send screen shows
/// it in place; an open receive takes it into its own error line.
#[derive(Clone, Default)]
pub struct ErrorSlot(Arc<Mutex<Option<String>>>);

impl ErrorSlot {
    /// Log and record a failure for whichever screen is showing.
    pub fn report(&self, message: String) {
        log::error!("{message}");
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(message);
        }
    }

    pub fn peek(&self) -> Option<String> {
        self.0.lock().ok().and_then(|e| e.clone())
    }

    pub fn take(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut e| e.take())
    }

    pub fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

/// How to build an [`Oxfer`].
pub struct Config {
    pub relay: RelayChoice,
    pub platform: Arc<dyn Platform>,
    /// Browser only: let the core push `/` and `/diags` history entries and route those
    /// paths itself. A UI with its own router (Dioxus) sets this to false and dispatches
    /// [`Intent::OpenDiagnostics`] / [`Intent::GoHome`] from its routes instead. Fragments
    /// (share links) are always handled by the core.
    pub manage_history: bool,
}

impl Config {
    /// The product configuration for the target being compiled: `P2P_RELAY_URL` or n0's
    /// relays, and the native or browser platform.
    pub fn for_this_target() -> Self {
        Self {
            relay: RelayChoice::from_env(),
            #[cfg(not(target_arch = "wasm32"))]
            platform: Arc::new(NativePlatform),
            #[cfg(target_arch = "wasm32")]
            platform: Arc::new(WebPlatform),
            manage_history: true,
        }
    }
}

/// What the user is doing; a UI maps it to a screen or route.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Home,
    Send,
    Receive,
    Chat,
    #[cfg(target_arch = "wasm32")]
    Diagnostics,
}

/// Everything any screen shows, in one value.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub activity: Activity,
    /// The share outlives navigation, so it is always present (idle when nothing is shared).
    pub send: SendSnapshot,
    pub receive: Option<ReceiveSnapshot>,
    pub chat: Option<ChatSnapshot>,
    /// Files received in this run, oldest first.
    pub received: Vec<ReceivedFile>,
    /// Picks and drops are accepted now (not during a receive or a room).
    pub can_pick_files: bool,
    /// Some file is hashing: Home, links and rooms are refused until it finishes.
    pub preparing: bool,
    /// The page should stay awake now.
    pub needs_wake_lock: bool,
    pub wake_lock: WakeLock,
    /// Progress may change without a notification; re-snapshot at ~10 Hz while true.
    pub live: bool,
    #[cfg(target_arch = "wasm32")]
    pub copies: CopiesSnapshot,
    /// Present while [`Activity::Diagnostics`] is showing.
    #[cfg(target_arch = "wasm32")]
    pub diagnostics: Option<DiagnosticsSnapshot>,
}

/// The user's requests, in domain terms.
pub enum Intent {
    /// Back to Home (or out of a room). Refused while a file is still hashing.
    GoHome,
    /// A blank receive to paste a link into. Refused while a file is still hashing.
    OpenReceive,
    /// Open a pasted link or bare fragment: a share link starts a receive, a room link
    /// joins the room.
    OpenLink(String),
    /// From Home or Diagnostics, back to the share that is still running.
    ReturnToSharing,
    /// Open a room on this device; idempotent while one is hosted here.
    HostChat {
        for_agents: bool,
    },
    #[cfg(target_arch = "wasm32")]
    OpenDiagnostics,
    /// Open a blank receive that continues this local copy.
    #[cfg(target_arch = "wasm32")]
    ResumeLocalCopy(String),
    Send(SendIntent),
    Receive(ReceiveIntent),
    Chat(ChatIntent),
    #[cfg(target_arch = "wasm32")]
    Copies(CopiesIntent),
    #[cfg(target_arch = "wasm32")]
    Diagnostics(DiagnosticsIntent),
}

/// What every session is built from.
pub(crate) struct Deps {
    relay: RelayChoice,
    platform: Arc<dyn Platform>,
    notifier: Notifier,
    errors: ErrorSlot,
    #[cfg(target_arch = "wasm32")]
    copies: LocalCopies,
}

enum Active {
    Home,
    Send,
    Receive(Box<ReceiveSession>),
    Chat(Box<ChatSession>),
    #[cfg(target_arch = "wasm32")]
    Diagnostics,
}

/// The whole app, minus pixels.
pub struct Oxfer {
    deps: Deps,
    picks: PickQueue,
    active: Active,
    send: SendSession,
    received: Vec<ReceivedFile>,
    /// Ids for receives and rooms, so snapshots of successive sessions never compare equal.
    next_id: u64,
    #[cfg(target_arch = "wasm32")]
    diagnostics: DiagnosticsSession,
    #[cfg(target_arch = "wasm32")]
    manage_history: bool,
    /// The last `path + hash` [`Oxfer::sync_location`] acted on.
    #[cfg(target_arch = "wasm32")]
    last_route: Option<String>,
    /// The last fragment [`Oxfer::sync_location`] accepted. A path-only navigation (a UI
    /// router moving from `/` to `/chat`) carries the fragment along unchanged and must not
    /// replay it: `#chat` would open a second room.
    #[cfg(target_arch = "wasm32")]
    last_hash: Option<String>,
}

impl Default for Oxfer {
    fn default() -> Self {
        Self::new(Config::for_this_target())
    }
}

impl Oxfer {
    pub fn new(config: Config) -> Self {
        let notifier = Notifier::default();
        let errors = ErrorSlot::default();
        let deps = Deps {
            relay: config.relay,
            platform: config.platform,
            #[cfg(target_arch = "wasm32")]
            copies: LocalCopies::new(notifier.clone()),
            notifier,
            errors,
        };
        let send = SendSession::new(
            deps.relay.clone(),
            deps.platform.clone(),
            deps.notifier.clone(),
            deps.errors.clone(),
        );
        Self {
            picks: PickQueue::new(deps.errors.clone(), deps.notifier.clone()),
            #[cfg(target_arch = "wasm32")]
            diagnostics: DiagnosticsSession::new(deps.relay.clone(), deps.notifier.clone()),
            #[cfg(target_arch = "wasm32")]
            manage_history: config.manage_history,
            #[cfg(target_arch = "wasm32")]
            last_route: None,
            #[cfg(target_arch = "wasm32")]
            last_hash: None,
            next_id: 0,
            deps,
            active: Active::Home,
            send,
            received: Vec::new(),
        }
    }

    /// Where pickers and drops hand files over. Clone it into callbacks freely.
    pub fn picks(&self) -> PickQueue {
        self.picks.clone()
    }

    pub fn notifier(&self) -> &Notifier {
        &self.deps.notifier
    }

    pub fn platform(&self) -> &dyn Platform {
        self.deps.platform.as_ref()
    }

    /// Report a failure the UI itself ran into (a picker that would not open).
    pub fn report(&self, message: String) {
        self.deps.errors.report(message);
        self.deps.notifier.notify();
    }

    pub fn activity(&self) -> Activity {
        match self.active {
            Active::Home => Activity::Home,
            Active::Send => Activity::Send,
            Active::Receive(_) => Activity::Receive,
            Active::Chat(_) => Activity::Chat,
            #[cfg(target_arch = "wasm32")]
            Active::Diagnostics => Activity::Diagnostics,
        }
    }

    /// Picks and drops start or join a share everywhere except a receive or a room.
    pub fn can_pick_files(&self) -> bool {
        !matches!(self.active, Active::Receive(_) | Active::Chat(_))
    }

    pub fn is_preparing(&self) -> bool {
        self.send.is_preparing()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            activity: self.activity(),
            send: self.send.snapshot(),
            receive: match &self.active {
                Active::Receive(r) => Some(r.snapshot()),
                _ => None,
            },
            chat: match &self.active {
                Active::Chat(c) => Some(c.snapshot()),
                _ => None,
            },
            received: self.received.clone(),
            can_pick_files: self.can_pick_files(),
            preparing: self.is_preparing(),
            needs_wake_lock: self.needs_wake_lock(),
            wake_lock: self.deps.platform.wake_lock(),
            live: self.live(),
            #[cfg(target_arch = "wasm32")]
            copies: self.deps.copies.snapshot(),
            #[cfg(target_arch = "wasm32")]
            diagnostics: matches!(self.active, Active::Diagnostics)
                .then(|| self.diagnostics.snapshot()),
        }
    }

    /// Fold in what background tasks finished. Call once per frame, or after each
    /// notification; it is cheap when nothing changed.
    pub fn tick(&mut self) {
        #[cfg(target_arch = "wasm32")]
        self.sync_location();

        self.drain_picks();
        self.send.prune();
        if let Active::Receive(r) = &mut self.active {
            let when = self.deps.platform.now_label();
            if let Some(receive::Finished::Saved(files)) = r.tick(&when) {
                self.received.extend(files);
            }
        }
        if let Active::Chat(c) = &mut self.active {
            c.tick();
        }
        self.deps.platform.set_wake_lock(self.needs_wake_lock());
    }

    pub fn dispatch(&mut self, intent: Intent) {
        match intent {
            Intent::GoHome => self.go_home(),
            Intent::OpenReceive => {
                if !self.is_preparing() {
                    self.enter_receive(String::new());
                }
            }
            Intent::OpenLink(input) => self.open_link(&input),
            Intent::ReturnToSharing => self.return_to_sharing(),
            Intent::HostChat { for_agents } => self.host_chat(for_agents),
            #[cfg(target_arch = "wasm32")]
            Intent::OpenDiagnostics => self.open_diagnostics(),
            #[cfg(target_arch = "wasm32")]
            Intent::ResumeLocalCopy(id) => {
                let receive = self.new_receive(String::new()).resume(id);
                self.active = Active::Receive(Box::new(receive));
            }
            Intent::Send(intent) => self.send.dispatch(intent),
            Intent::Receive(ReceiveIntent::OpenInput) => {
                if let Active::Receive(r) = &self.active {
                    let input = r.input().to_string();
                    self.open_link(&input);
                }
            }
            Intent::Receive(intent) => {
                if let Active::Receive(r) = &mut self.active {
                    r.dispatch(intent);
                }
            }
            Intent::Chat(intent) => {
                if let Active::Chat(c) = &mut self.active {
                    c.dispatch(intent);
                }
            }
            #[cfg(target_arch = "wasm32")]
            Intent::Copies(intent) => self.deps.copies.dispatch(intent),
            #[cfg(target_arch = "wasm32")]
            Intent::Diagnostics(intent) => self.diagnostics.dispatch(intent),
        }
        self.deps.notifier.notify();
    }

    fn needs_wake_lock(&self) -> bool {
        self.send.needs_wake_lock()
            || match &self.active {
                Active::Receive(r) => r.needs_wake_lock(),
                // A room lives only while its page is awake.
                Active::Chat(c) => c.running(),
                _ => false,
            }
    }

    fn live(&self) -> bool {
        self.send.needs_poll()
            || match &self.active {
                Active::Receive(r) => r.active(),
                #[cfg(target_arch = "wasm32")]
                Active::Diagnostics => self.diagnostics.listening(),
                _ => false,
            }
    }

    fn next_id(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    fn new_receive(&mut self, input: String) -> ReceiveSession {
        let id = self.next_id();
        ReceiveSession::new(id, &self.deps, input)
    }

    fn enter_receive(&mut self, input: String) {
        let receive = self.new_receive(input);
        self.active = Active::Receive(Box::new(receive));
    }

    /// Turn pasted text — a whole link or a bare fragment — into a receive or a room.
    fn open_link(&mut self, input: &str) {
        let fragment = input.split_once('#').map(|(_, f)| f).unwrap_or(input);
        let params = Node::parse_fragment(fragment);
        #[cfg(target_arch = "wasm32")]
        self.diagnostics.reset(None, None);
        if params.chat && params.error.is_none() {
            let id = self.next_id();
            let wake = chat::wake_for(&self.deps.notifier);
            self.leave_room_first();
            self.active = Active::Chat(Box::new(ChatSession::join(
                id,
                self.deps.relay.clone(),
                &params,
                wake,
            )));
            return;
        }
        let mut receive = self.new_receive(input.to_string());
        receive.open(params);
        self.active = Active::Receive(Box::new(receive));
    }

    /// Dropping an old room removes the page-script bridge, which would undo a bridge the
    /// new room installed if the old one outlived its construction.
    fn leave_room_first(&mut self) {
        if matches!(self.active, Active::Chat(_)) {
            self.active = Active::Home;
        }
    }

    fn host_chat(&mut self, for_agents: bool) {
        if let Active::Chat(c) = &mut self.active {
            if c.role() == ChatRole::Host && c.running() {
                if for_agents {
                    c.mark_for_agents();
                }
                return;
            }
        }
        if self.is_preparing() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.diagnostics.reset(None, None);
        let id = self.next_id();
        let wake = chat::wake_for(&self.deps.notifier);
        self.leave_room_first();
        self.active = Active::Chat(Box::new(ChatSession::host(
            id,
            self.deps.relay.clone(),
            self.deps.platform.as_ref(),
            wake,
            for_agents,
        )));
    }

    fn go_home(&mut self) {
        if self.is_preparing() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            if matches!(self.active, Active::Diagnostics) {
                self.push_path("/");
            }
            self.diagnostics.stop_peer();
        }
        self.active = Active::Home;
    }

    fn return_to_sharing(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if matches!(self.active, Active::Diagnostics) {
            self.push_path("/");
            self.diagnostics.stop_peer();
        }
        self.active = Active::Send;
    }

    /// Turn queued picks into hashing tasks, entering Send on the first one.
    fn drain_picks(&mut self) {
        let picks = self.picks.drain();
        if picks.is_empty() {
            return;
        }
        // A pick can land after the activity changed under it (a dropped file's async
        // probe, or a link opened while the picker was up); it must not tear down a
        // receive or a room.
        if !self.can_pick_files() {
            log::debug!(
                "discarding {} pick(s) that arrived outside Send",
                picks.len()
            );
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if matches!(self.active, Active::Diagnostics) {
            self.diagnostics.stop_peer();
        }
        self.active = Active::Send;
        self.send.add_files(picks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline() -> Oxfer {
        Oxfer::new(Config {
            relay: RelayChoice::None,
            platform: Arc::new(TestPlatform::default()),
            manage_history: false,
        })
    }

    #[test]
    fn local_test_bearer_links_never_print_their_capability() {
        let link = BearerLink("https://oxfer.app/#endpoint&cap=00112233".into());
        assert_eq!(format!("{link:?}"), "BearerLink(<redacted>)");
        assert_eq!(link.as_str(), "https://oxfer.app/#endpoint&cap=00112233");
        assert_eq!(format!("{:?}", BearerLink::default()), "BearerLink(\"\")");
    }

    #[test]
    fn local_test_receive_preserves_the_link_that_was_opened() {
        let mut oxfer = offline();
        let link = "https://oxfer.app/#ticket=invalid";
        oxfer.dispatch(Intent::OpenLink(link.into()));
        let receive = oxfer.snapshot().receive.expect("expected receive panel");
        assert_eq!(receive.input.as_str(), link);
        assert!(receive.error.is_some());
        assert!(!receive.opening);
        assert!(format!("{receive:?}").contains("<redacted>"));
    }

    #[test]
    fn local_test_link_without_access_code_is_refused_before_dialling() {
        let mut oxfer = offline();
        oxfer.dispatch(Intent::OpenReceive);
        let ticket = "endpointaa".to_string();
        oxfer.dispatch(Intent::Receive(ReceiveIntent::SetInput(format!(
            "https://oxfer.app/#{ticket}"
        ))));
        oxfer.dispatch(Intent::Receive(ReceiveIntent::OpenInput));
        let receive = oxfer.snapshot().receive.unwrap();
        assert!(!receive.opening);
        assert!(receive.error.is_some());
    }

    #[test]
    fn local_test_startup_keeps_polling_without_user_input() {
        let oxfer = offline();
        assert!(!oxfer.snapshot().live);
    }

    #[test]
    fn local_test_notifier_wakes_callback_and_watchers() {
        let notifier = Notifier::default();
        let mut watcher = notifier.subscribe();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        notifier.set_wake(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        notifier.notify();
        notifier.notify();
        assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert!(watcher.has_changed().unwrap());
        assert_eq!(*watcher.borrow_and_update(), 2);
    }

    #[test]
    fn local_test_picks_are_discarded_outside_send() {
        let mut oxfer = offline();
        oxfer.dispatch(Intent::OpenReceive);
        oxfer.picks().push(PickedFile::from_bytes(
            "a.txt",
            bytes::Bytes::from_static(b"hi"),
        ));
        oxfer.tick();
        let snapshot = oxfer.snapshot();
        assert_eq!(snapshot.activity, Activity::Receive);
        assert!(!snapshot.can_pick_files);
        assert!(snapshot.send.preparing.is_empty());
        assert_eq!(snapshot.send.link, LinkState::Idle);
        assert!(oxfer.picks().is_empty());
    }

    #[test]
    fn local_test_open_new_receive_link_clears_previous_input() {
        let mut oxfer = offline();
        oxfer.dispatch(Intent::OpenReceive);
        oxfer.dispatch(Intent::Receive(ReceiveIntent::SetInput(
            "previous private link".into(),
        )));
        oxfer.dispatch(Intent::OpenReceive);
        assert!(oxfer.snapshot().receive.unwrap().input.is_empty());
    }

    #[test]
    fn local_test_successive_sessions_have_distinct_ids() {
        let mut oxfer = offline();
        oxfer.dispatch(Intent::OpenReceive);
        let first = oxfer.snapshot().receive.unwrap().id;
        oxfer.dispatch(Intent::OpenReceive);
        assert_ne!(oxfer.snapshot().receive.unwrap().id, first);
    }

    /// The acceptance test for the core: a full send → receive through `Oxfer` alone, with
    /// no UI, on loopback endpoints.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_test_core_drives_a_full_send_and_receive_without_a_ui() {
        use crate::file_io::{AnySink, MemSink};
        use std::time::Duration;

        async fn until<T>(oxfer: &mut Oxfer, mut done: impl FnMut(&Snapshot) -> Option<T>) -> T {
            let mut changes = oxfer.notifier().subscribe();
            loop {
                oxfer.tick();
                let snapshot = oxfer.snapshot();
                if let Some(value) = done(&snapshot) {
                    return value;
                }
                // `live` progress has no notification of its own; poll it like a UI would.
                let _ = tokio::time::timeout(Duration::from_millis(20), changes.changed()).await;
            }
        }

        let data = bytes::Bytes::from((0..300_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>());
        let sender_platform = Arc::new(TestPlatform::default());
        let mut sender = Oxfer::new(Config {
            relay: RelayChoice::None,
            platform: sender_platform.clone(),
            manage_history: false,
        });
        let mut receiver = offline();

        tokio::time::timeout(Duration::from_secs(30), async {
            sender
                .picks()
                .push(PickedFile::from_bytes("notes.bin", data.clone()));
            let link = until(&mut sender, |s| match &s.send.link {
                LinkState::Ready(link) => Some(link.as_str().to_string()),
                _ => None,
            })
            .await;
            let sent = sender.snapshot();
            assert_eq!(sent.activity, Activity::Send);
            assert_eq!(sent.send.files[0].name, "notes.bin");
            assert!(sent.needs_wake_lock && sender_platform.wake_lock_wanted());

            receiver.dispatch(Intent::OpenLink(link));
            let manifest = until(&mut receiver, |s| {
                let r = s.receive.as_ref()?;
                assert!(r.error.is_none(), "{:?}", r.error);
                r.manifest().map(<[_]>::to_vec)
            })
            .await;
            assert_eq!(manifest.len(), 1);
            assert_eq!(manifest[0].size, data.len() as u64);

            let sink = AnySink::Mem(MemSink::new(manifest[0].name.clone(), data.len()));
            receiver.dispatch(Intent::Receive(ReceiveIntent::Save(SaveTarget::Sinks(
                vec![sink],
            ))));
            assert!(receiver.snapshot().receive.unwrap().save_pending);

            let received = until(&mut receiver, |s| {
                if let Some(error) = s.receive.as_ref().and_then(|r| r.error.clone()) {
                    panic!("receive failed: {error}");
                }
                (!s.received.is_empty()).then(|| s.received.clone())
            })
            .await;
            assert_eq!(received.len(), 1);
            assert_eq!(received[0].name, "notes.bin");
            assert_eq!(received[0].size, data.len() as u64);
            let done = receiver.snapshot().receive.unwrap();
            assert!(done.input.is_empty(), "a finished receive forgets its link");
            assert!(!done.active() && !receiver.snapshot().needs_wake_lock);

            // The sender saw the peer finish, then stops sharing on request.
            until(&mut sender, |s| {
                s.send
                    .peers
                    .iter()
                    .any(|p| matches!(p.progress.phase, crate::transfer::Phase::Complete { .. }))
                    .then_some(())
            })
            .await;
            sender.dispatch(Intent::Send(SendIntent::StopSharing));
            let stopped = sender.snapshot();
            assert!(!stopped.send.sharing);
            assert!(!matches!(stopped.send.link, LinkState::Ready(_)));
            assert!(!sender_platform.wake_lock_wanted());
        })
        .await
        .expect("send and receive finished in time");
    }
}
