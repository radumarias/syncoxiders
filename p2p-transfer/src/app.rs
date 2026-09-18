//! The egui layer: mode, flows and rendering.
//!
//! This module owns no bytes and no protocol. It holds handles (`SharedFiles`, `Node`,
//! `TransferHandle`), turns user gestures into engine commands, and renders whatever the
//! engine publishes on its progress watch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::{Button, Color32, CornerRadius, RichText, Stroke, Ui};
use n0_future::task::{self, AbortOnDropHandle};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::file_io::{self, FileOrigin, FileSnapshot, SharedFile, SharedFiles};
use crate::logging;
use crate::node::{FragmentParams, Node, RelayChoice};
use crate::protocol::FileMeta;
use crate::transfer::{
    Path as TransferPath, Phase, ReceiveCommand, ReceiveOptions, TransferHandle,
};

/// Shown next to a link, because the link *is* the credential (design §2.6).
const LINK_WARNING: &str =
    "Anyone with this link can download these files — it contains an access code.";
/// What a receiver is told when the link carries no `cap` (design §2.6, §4.8.2).
const MISSING_CAP: &str = "this link is missing its access code; ask the sender for the full link";
/// Both sentences of design §2.6 for a capability the sender rejected.
const CAP_REJECTED: &str =
    "this link is not valid for these files. Ask the sender for a fresh link — the old one stops \
     working when they restart sharing.";

// ── Stitch Design System — theme-aware color palette ──────────────────────────

#[derive(Clone, Copy)]
struct Tc {
    bg: Color32,
    surface_lowest: Color32,
    surface_low: Color32,
    surface: Color32,
    surface_high: Color32,
    primary: Color32,
    on_primary: Color32,
    secondary: Color32,
    on_surface: Color32,
    on_surface_var: Color32,
    outline: Color32,
    outline_var: Color32,
    error: Color32,
}

impl Tc {
    const fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(19, 19, 27),                // #13131b
            surface_lowest: Color32::from_rgb(13, 13, 21),    // #0d0d15
            surface_low: Color32::from_rgb(27, 27, 35),       // #1b1b23
            surface: Color32::from_rgb(31, 31, 39),           // #1f1f27
            surface_high: Color32::from_rgb(41, 41, 50),      // #292932
            primary: Color32::from_rgb(192, 193, 255),        // #c0c1ff
            on_primary: Color32::from_rgb(16, 0, 169),        // #1000a9
            secondary: Color32::from_rgb(78, 222, 163),       // #4edea3
            on_surface: Color32::from_rgb(228, 225, 237),     // #e4e1ed
            on_surface_var: Color32::from_rgb(199, 196, 215), // #c7c4d7
            outline: Color32::from_rgb(144, 143, 160),        // #908fa0
            outline_var: Color32::from_rgb(70, 69, 84),       // #464554
            error: Color32::from_rgb(255, 180, 171),          // #ffb4ab
        }
    }

    const fn light() -> Self {
        Self {
            bg: Color32::from_rgb(244, 243, 255),             // #f4f3ff
            surface_lowest: Color32::from_rgb(255, 255, 255), // #ffffff
            surface_low: Color32::from_rgb(238, 237, 255),    // #eeedff
            surface: Color32::from_rgb(230, 229, 249),        // #e6e5f9
            surface_high: Color32::from_rgb(216, 215, 235),   // #d8d7eb
            primary: Color32::from_rgb(65, 67, 199),          // #4143c7
            on_primary: Color32::from_rgb(255, 255, 255),     // #ffffff
            secondary: Color32::from_rgb(0, 108, 74),         // #006c4a
            on_surface: Color32::from_rgb(27, 26, 39),        // #1b1a27
            on_surface_var: Color32::from_rgb(71, 69, 85),    // #474555
            outline: Color32::from_rgb(120, 118, 127),        // #78767f
            outline_var: Color32::from_rgb(199, 197, 208),    // #c7c5d0
            error: Color32::from_rgb(186, 26, 26),            // #ba1a1a
        }
    }

    fn of(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    fn for_ui(ui: &Ui) -> Self {
        Self::of(ui.visuals().dark_mode)
    }
}

// ── App state ────────────────────────────────────────────────────────────────

/// One file this node received and saved, as the UI lists it.
#[derive(Clone, Debug)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub location: String,
    pub when: String,
}

/// A picked file waiting for `logic()` to turn it into a [`PrepareHandle`].
///
/// The wasm file picker completes in a JS callback that cannot hold `&mut self`, so both
/// targets hand picks over through this queue and one code path drains it.
struct PendingPick {
    name: String,
    origin: FileOrigin,
    snapshot: FileSnapshot,
}

/// Hashing one picked file. The link is withheld until every one of these is gone
/// (design §4.8.2, plan amendment 14), so a receiver never sees a non-final hash.
struct PrepareHandle {
    name: String,
    progress: watch::Receiver<f32>,
    /// Abort-on-drop is right here: hashing owns nothing that needs cleanup.
    _task: AbortOnDropHandle<()>,
}

impl PrepareHandle {
    /// The hashing task drops its `watch::Sender` when it ends, whatever the outcome.
    fn finished(&self) -> bool {
        self.progress.has_changed().is_err()
    }
}

/// Everything one receive needs.
///
/// Boxed inside [`Mode`] because a parsed `FragmentParams` (which carries an
/// `EndpointTicket`) makes this variant ~230 bytes against ~24 for the next largest, and
/// every `Mode` value — including `Home` — would otherwise be that wide.
struct ReceiveState {
    input: String,
    params: FragmentParams,
    handle: Option<TransferHandle>,
    error: Option<String>,
    save_pending: Arc<AtomicBool>,
}

/// One destination selection per receive session. Errors and dismissal allow a retry;
/// a submitted Save remains pending until the engine leaves AwaitingSave.
struct SaveAttempt {
    pending: Arc<AtomicBool>,
    submitted: bool,
}

impl SaveAttempt {
    fn begin(pending: &Arc<AtomicBool>) -> Option<Self> {
        if pending.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(Self {
            pending: pending.clone(),
            submitted: false,
        })
    }

    fn submitted(mut self) {
        self.submitted = true;
    }
}

impl Drop for SaveAttempt {
    fn drop(&mut self) {
        if !self.submitted {
            self.pending.store(false, Ordering::Release);
        }
    }
}

/// What the app is doing. One value replaces the six booleans the pre-migration app used.
#[derive(Default)]
enum Mode {
    #[default]
    Home,
    Send {
        preparing: Vec<PrepareHandle>,
    },
    Receive(Box<ReceiveState>),
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct P2PTransfer {
    /// The only persisted field: where native receives are written.
    #[cfg(not(target_arch = "wasm32"))]
    save_directory: Option<std::path::PathBuf>,

    #[serde(skip)]
    mode: Mode,
    /// Handles and metadata for everything this node offers — never bytes.
    #[serde(skip)]
    shared_files: SharedFiles,
    /// `Some` while sharing; written by the bind task.
    #[serde(skip)]
    node: Arc<Mutex<Option<Arc<Node>>>>,
    #[serde(skip)]
    link: Arc<Mutex<Option<String>>>,
    /// Where every spawned task reports a failure. `logic()` drains it to the panel that is
    /// showing, so a task never needs to know which mode the app is in.
    #[serde(skip)]
    node_error: Arc<Mutex<Option<String>>>,
    #[serde(skip)]
    received_files: Arc<Mutex<Vec<ReceivedFile>>>,
    /// Picks waiting to become `PrepareHandle`s (see [`PendingPick`]).
    #[serde(skip)]
    pending_picks: Arc<Mutex<Vec<PendingPick>>>,
    /// A receive handle built by the on-demand bind task, waiting for `logic()` to adopt it.
    #[serde(skip)]
    pending_handle: Arc<Mutex<Option<TransferHandle>>>,
    /// Set the moment a bind task is spawned, not when it finishes: `node` stays `None` for
    /// the whole bind, so two picks in quick succession would otherwise start two nodes and
    /// the second would silently replace the first one's link.
    #[serde(skip)]
    sharing: bool,
    #[serde(skip)]
    show_terminal_view: bool,
    #[serde(skip)]
    last_dark_mode: Option<bool>,
    #[serde(skip)]
    fragment_checked: bool,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    file_input_closure: Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
}

impl Default for P2PTransfer {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            save_directory: None,
            mode: Mode::Home,
            shared_files: Arc::new(Mutex::new(Vec::new())),
            node: Arc::new(Mutex::new(None)),
            link: Arc::new(Mutex::new(None)),
            node_error: Arc::new(Mutex::new(None)),
            received_files: Arc::new(Mutex::new(Vec::new())),
            pending_picks: Arc::new(Mutex::new(Vec::new())),
            pending_handle: Arc::new(Mutex::new(None)),
            sharing: false,
            show_terminal_view: false,
            last_dark_mode: None,
            fragment_checked: false,
            #[cfg(target_arch = "wasm32")]
            file_input_closure: None,
        }
    }
}

// ── Flows ────────────────────────────────────────────────────────────────────

impl P2PTransfer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }
        Self::default()
    }

    /// Record a failure for whichever panel is showing.
    fn report(slot: &Arc<Mutex<Option<String>>>, message: String) {
        log::error!("{message}");
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(message);
        }
    }

    // ── Pick ─────────────────────────────────────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // The snapshot is taken *before* hashing, so `open_source` can tell later whether the
        // bytes still match the hash the manifest advertises.
        match file_io::snapshot_path(&path) {
            Ok(snapshot) => {
                if let Ok(mut picks) = self.pending_picks.lock() {
                    picks.push(PendingPick {
                        name,
                        origin: FileOrigin::Path(path),
                        snapshot,
                    });
                }
            }
            Err(e) => Self::report(&self.node_error, format!("could not read {name}: {e}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pick_file(&mut self) {
        use wasm_bindgen::JsCast as _;

        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let input = match document
            .create_element("input")
            .ok()
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            Some(input) => input,
            None => {
                Self::report(
                    &self.node_error,
                    "this browser would not open a file picker".to_string(),
                );
                return;
            }
        };
        input.set_type("file");

        let picks = self.pending_picks.clone();
        let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
            let Some(input) = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            // `File` freezes name, size and mtime at pick time, so this snapshot is the
            // one the browser will still report later.
            let snapshot = file_io::snapshot_web(&file);
            let name = file.name();
            if let Ok(mut picks) = picks.lock() {
                picks.push(PendingPick {
                    name,
                    origin: FileOrigin::Web(send_wrapper::SendWrapper::new(file)),
                    snapshot,
                });
            }
        })
            as Box<dyn FnMut(web_sys::Event)>);

        input.set_onchange(Some(closure.as_ref().unchecked_ref()));
        input.click();
        // Keep the closure alive for as long as the input can fire it.
        self.file_input_closure = Some(closure);
    }

    /// Hash a picked file, then publish it as a `SharedFile`.
    fn begin_prepare(&mut self, pick: PendingPick) -> PrepareHandle {
        let (progress_tx, progress_rx) = watch::channel(0.0f32);
        let files = self.shared_files.clone();
        let errors = self.node_error.clone();
        let label = pick.name.clone();

        let task = task::spawn(async move {
            let PendingPick {
                name,
                origin,
                snapshot,
            } = pick;
            let mut source = match file_io::open_source(&origin, &snapshot).await {
                Ok(source) => source,
                Err(e) => {
                    Self::report(&errors, format!("could not open {name}: {e}"));
                    return;
                }
            };
            let size = file_io::Source::size(&source);
            match file_io::hash_source(&mut source, &progress_tx).await {
                Ok(hash) => {
                    let meta = FileMeta {
                        name: file_io::sanitize_name(&name),
                        size,
                        hash,
                    };
                    if let Ok(mut files) = files.lock() {
                        files.push(SharedFile {
                            meta,
                            origin,
                            snapshot,
                        });
                    }
                    log::info!("ready to share: {name} ({size} bytes)");
                }
                Err(e) => Self::report(&errors, format!("could not hash {name}: {e}")),
            }
            // Dropping `progress_tx` here is what tells the UI this handle is done.
        });

        PrepareHandle {
            name: label,
            progress: progress_rx,
            _task: AbortOnDropHandle::new(task),
        }
    }

    // ── Share ────────────────────────────────────────────────────────────

    /// The page this app is served from, without any fragment — the base of a share link.
    fn base_url() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .and_then(|w| w.location().href().ok())
                .map(|href| href.split('#').next().unwrap_or(&href).to_string())
                .unwrap_or_else(|| "https://syncoxiders.app/".to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            "https://syncoxiders.app/".to_string()
        }
    }

    /// Whether a share link should carry `#dev` — only if this page was opened with it.
    fn dev_flag(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .and_then(|w| w.location().hash().ok())
                .map(|hash| Node::parse_fragment(&hash).dev)
                .unwrap_or(false)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    fn start_sharing(&mut self) {
        if self.sharing {
            return;
        }
        self.sharing = true;
        let files = self.shared_files.clone();
        let node_slot = self.node.clone();
        let link_slot = self.link.clone();
        let errors = self.node_error.clone();
        let base = Self::base_url();
        let dev = self.dev_flag();

        task::spawn(async move {
            let node = match Node::bind(files, RelayChoice::from_env()).await {
                Ok(node) => Arc::new(node),
                Err(e) => {
                    Self::report(&errors, e.to_string());
                    return;
                }
            };
            match node.ticket().await {
                Ok(ticket) => {
                    let link = Node::link(&base, &ticket, &node.cap(), dev);
                    if let Ok(mut slot) = link_slot.lock() {
                        *slot = Some(link);
                    }
                }
                Err(e) => Self::report(&errors, e.to_string()),
            }
            if let Ok(mut slot) = node_slot.lock() {
                *slot = Some(node);
            }
        });
    }

    /// Stop serving. The next share draws a fresh key, so the old link stops working.
    fn stop_sharing(&mut self) {
        self.sharing = false;
        let node = self.node.lock().ok().and_then(|mut n| n.take());
        if let Some(node) = node {
            task::spawn(async move { node.shutdown().await });
        }
        if let Ok(mut link) = self.link.lock() {
            *link = None;
        }
    }

    // ── Receive ──────────────────────────────────────────────────────────

    /// Turn pasted text — a whole link or a bare fragment — into receive parameters.
    fn start_receive(&mut self, input: &str) {
        let fragment = input.split_once('#').map(|(_, f)| f).unwrap_or(input);
        let params = Node::parse_fragment(fragment);

        if let Some(error) = params.error.clone() {
            self.set_receive(params, None, Some(error));
            return;
        }
        let Some(ticket) = params.ticket.clone() else {
            self.set_receive(params, None, Some("this is not a share link".to_string()));
            return;
        };
        // A bare ticket authorizes nothing (design §2.6): refuse before dialling, and say why,
        // so a truncated link never looks like a connectivity failure.
        if params.cap.is_none() {
            self.set_receive(params, None, Some(MISSING_CAP.to_string()));
            return;
        }

        let opts = ReceiveOptions::from_fragment(&params);
        let relay = RelayChoice::from_env();
        let slot = self.pending_handle.clone();
        let errors = self.node_error.clone();
        self.set_receive(params, None, None);

        task::spawn(async move {
            // A receiver binds its own node on demand; `run_receiver` shuts it down again on
            // every exit path, so a cancelled receive leaks nothing.
            match Node::bind(Arc::new(Mutex::new(Vec::new())), relay).await {
                Ok(node) => {
                    let handle = TransferHandle::start_receive(Arc::new(node), ticket, opts);
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(handle);
                    }
                }
                Err(e) => Self::report(&errors, e.to_string()),
            }
        });
    }

    /// Enter (or update) receive mode, keeping whatever the user typed.
    fn set_receive(
        &mut self,
        params: FragmentParams,
        handle: Option<TransferHandle>,
        error: Option<String>,
    ) {
        let input = match &self.mode {
            Mode::Receive(r) => r.input.clone(),
            _ => String::new(),
        };
        self.mode = Mode::Receive(Box::new(ReceiveState {
            input,
            params,
            handle,
            error,
            save_pending: Arc::new(AtomicBool::new(false)),
        }));
    }

    /// Build one sink per manifest entry and hand them to the session.
    ///
    /// Invoke the browser picker promptly after the Save gesture, without preceding
    /// asynchronous work. Transient activation is browser-controlled, not an await count.
    fn save_click(&mut self, manifest: Vec<FileMeta>) {
        let Mode::Receive(r) = &self.mode else {
            return;
        };
        let Some(handle) = &r.handle else { return };
        if !matches!(handle.latest().phase, Phase::AwaitingSave { .. }) {
            return;
        }
        let Some(attempt) = SaveAttempt::begin(&r.save_pending) else {
            return;
        };
        let commands = handle.commands.clone();
        let errors = self.node_error.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = match self.save_directory.clone() {
                Some(dir) => Some(dir),
                None => rfd::FileDialog::new().pick_folder(),
            };
            let Some(dir) = dir else {
                return; // the user dismissed the folder picker: stay in AwaitingSave
            };
            self.save_directory = Some(dir.clone());

            let mut sinks = Vec::with_capacity(manifest.len());
            for meta in &manifest {
                match file_io::FsSink::create(&dir, &file_io::sanitize_name(&meta.name)) {
                    Ok(sink) => sinks.push(file_io::AnySink::Fs(sink)),
                    Err(e) => {
                        Self::report(&errors, format!("could not create {}: {e}", meta.name));
                        return;
                    }
                }
            }
            if commands.try_send(ReceiveCommand::Save(sinks)).is_err() {
                Self::report(&errors, "the transfer is no longer running".to_string());
            } else {
                attempt.submitted();
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let pref = r.params.sink_pref;
            let cancel = handle.cancel.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if cancel.is_cancelled() {
                    return;
                }
                // Keep awaiting the picker even if the receive is cancelled: a native dialog
                // cannot be closed by dropping its Promise, and returned sinks need cleanup.
                match file_io::web::pick_sinks(&manifest, pref).await {
                    Ok(sinks) => {
                        let permit = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => None,
                            result = commands.reserve() => result.ok(),
                        };
                        if let Some(permit) = permit.filter(|_| !cancel.is_cancelled()) {
                            permit.send(ReceiveCommand::Save(sinks));
                            attempt.submitted();
                            return;
                        }
                        for sink in sinks {
                            file_io::Sink::abort(sink).await;
                        }
                        if !cancel.is_cancelled() {
                            Self::report(&errors, "the transfer is no longer running".to_string());
                        }
                    }
                    // Dismissing a picker is not an error: stay in AwaitingSave so Save can be
                    // clicked again.
                    Err(file_io::SinkError::Cancelled) => {}
                    Err(_) if cancel.is_cancelled() => {}
                    Err(e) => Self::report(&errors, e.to_string()),
                }
            });
        }
    }

    /// Read the URL fragment once, start the receive it describes, and scrub the secrets.
    ///
    /// A fragment never reaches a server, but it does stay on the machine — URL bar, history,
    /// session restore, a pasted screenshot. The ticket and the capability are removed
    /// immediately; the QA flags stay so a reload keeps them (design §4.7.1).
    #[cfg(target_arch = "wasm32")]
    fn check_fragment(&mut self) {
        use crate::node::SinkPref;
        use wasm_bindgen::JsValue;

        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(hash) = window.location().hash() else {
            return;
        };
        let params = Node::parse_fragment(&hash);
        if params.ticket.is_none() && params.cap.is_none() && params.error.is_none() {
            return;
        }

        let mut keep: Vec<String> = Vec::new();
        if params.dev {
            keep.push("dev".to_string());
        }
        if params.force_relay {
            keep.push("relay".to_string());
        }
        match params.sink_pref {
            SinkPref::Auto => {}
            SinkPref::Fsa => keep.push("sink=fsa".to_string()),
            SinkPref::Sw => keep.push("sink=sw".to_string()),
            SinkPref::Mem => keep.push("sink=mem".to_string()),
        }
        if let Some(n) = params.kill_dc_after {
            keep.push(format!("killdc={n}"));
        }
        if let Some(bytes) = params.window {
            keep.push(format!("win={}", bytes / (1024 * 1024)));
        }

        let href = window.location().href().unwrap_or_default();
        let base = href.split('#').next().unwrap_or(&href).to_string();
        let scrubbed = if keep.is_empty() {
            base
        } else {
            format!("{base}#{}", keep.join("&"))
        };
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&scrubbed));
        }

        self.start_receive(&hash);
    }

    // ── Per-frame bookkeeping (called from `logic`) ───────────────────────

    /// Turn queued picks into hashing tasks, entering Send mode on the first one.
    fn drain_picks(&mut self) {
        let picks: Vec<PendingPick> = match self.pending_picks.lock() {
            Ok(mut picks) if !picks.is_empty() => picks.drain(..).collect(),
            _ => return,
        };
        for pick in picks {
            let handle = self.begin_prepare(pick);
            match &mut self.mode {
                Mode::Send { preparing } => preparing.push(handle),
                _ => {
                    self.mode = Mode::Send {
                        preparing: vec![handle],
                    }
                }
            }
        }
        // Sharing starts as soon as there is something to share; the link stays hidden until
        // every hash is final (design §4.8.2).
        self.start_sharing();
    }

    /// Drop hashing handles whose task has ended.
    fn prune_preparing(&mut self) {
        if let Mode::Send { preparing } = &mut self.mode {
            preparing.retain(|h| !h.finished());
        }
    }

    /// Adopt a receive handle built by the on-demand bind task.
    fn adopt_pending_handle(&mut self) {
        let pending = self.pending_handle.lock().ok().and_then(|mut h| h.take());
        if let Some(handle) = pending {
            if let Mode::Receive(r) = &mut self.mode {
                r.handle = Some(handle);
            }
            // Not in receive mode any more: dropping the handle cancels the session, which is
            // exactly what leaving the panel should do.
        }
    }

    /// Move any reported failure into the panel that is showing.
    fn drain_error(&mut self) {
        if let Mode::Receive(r) = &mut self.mode {
            if r.error.is_some() {
                return;
            }
            let taken = self.node_error.lock().ok().and_then(|mut e| e.take());
            if taken.is_some() {
                r.error = taken;
            }
        }
    }

    /// Record a finished receive exactly once, then drop the handle.
    ///
    /// The terminal phase arrives on the same watch the panel already reads, so there is no
    /// second source of truth; dropping the handle afterwards also stops cloning the terminal
    /// progress value on every later frame.
    fn poll_receive(&mut self) {
        let Mode::Receive(r) = &mut self.mode else {
            return;
        };
        let Some(h) = &r.handle else { return };
        let progress = h.latest();
        if !progress.phase.is_terminal() {
            return;
        }
        match progress.phase {
            Phase::Complete { saved } => {
                let when = Self::timestamp();
                if let Ok(mut files) = self.received_files.lock() {
                    files.extend(saved.into_iter().map(|f| ReceivedFile {
                        name: f.name,
                        size: f.size,
                        location: f.location,
                        when: when.clone(),
                    }));
                }
            }
            Phase::Failed => {
                r.error = Some(
                    progress
                        .error
                        .clone()
                        .map(|e| Self::humanize(&e))
                        .unwrap_or_else(|| "the transfer failed".to_string()),
                );
            }
            Phase::Cancelled => r.error = Some("the transfer was cancelled".to_string()),
            _ => {}
        }
        r.handle = None;
    }

    /// Turn an engine error string into something a user can act on.
    fn humanize(error: &str) -> String {
        if error.contains("access code is missing") {
            MISSING_CAP.to_string()
        } else if error.contains("access code") {
            CAP_REJECTED.to_string()
        } else {
            error.to_string()
        }
    }

    fn timestamp() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Date::new_0()
                .to_locale_time_string("en-GB")
                .as_string()
                .unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                % 86_400;
            format!(
                "{:02}:{:02}:{:02} UTC",
                secs / 3600,
                (secs % 3600) / 60,
                secs % 60
            )
        }
    }

    fn format_size(size_bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let size = size_bytes as f64;
        if size >= GB {
            format!("{:.2} GB", size / GB)
        } else if size >= MB {
            format!("{:.2} MB", size / MB)
        } else if size >= KB {
            format!("{:.1} KB", size / KB)
        } else {
            format!("{size_bytes} B")
        }
    }

    /// The QA flags this receive is running with (design §7.1), or empty for a plain run.
    fn flag_summary(params: &FragmentParams) -> String {
        use crate::node::SinkPref;
        let mut flags: Vec<String> = Vec::new();
        if params.force_relay {
            flags.push("relay forced".to_string());
        }
        match params.sink_pref {
            SinkPref::Auto => {}
            SinkPref::Fsa => flags.push("sink=fsa".to_string()),
            SinkPref::Sw => flags.push("sink=sw".to_string()),
            SinkPref::Mem => flags.push("sink=mem".to_string()),
        }
        if let Some(n) = params.kill_dc_after {
            flags.push(format!("killdc={n}"));
        }
        if let Some(bytes) = params.window {
            flags.push(format!("win={} MiB", bytes / (1024 * 1024)));
        }
        flags.join(" · ")
    }

    fn path_badge(phase: &Phase, path: TransferPath) -> &'static str {
        if matches!(phase, Phase::Signaling) {
            return "Signaling via relay";
        }
        match path {
            TransferPath::Direct => "Direct",
            TransferPath::Relayed => "Relayed",
            TransferPath::Unknown => "…",
        }
    }

    fn phase_text(phase: &Phase) -> &'static str {
        match phase {
            Phase::Connecting => "Connecting…",
            Phase::Handshake => "Authorizing…",
            Phase::AwaitingSave { .. } => "Ready to save",
            Phase::Signaling => "Negotiating a direct path…",
            Phase::Transferring => "Transferring…",
            Phase::Switching => "Switching to the relay…",
            Phase::Verifying => "Verifying…",
            Phase::Complete { .. } => "Complete",
            Phase::Failed => "Failed",
            Phase::Cancelled => "Cancelled",
        }
    }

    fn apply_theme(ctx: &egui::Context, ui: &mut Ui, dark: bool) {
        let tc = Tc::of(dark);
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = tc.bg;
        v.window_fill = tc.surface;
        v.faint_bg_color = tc.surface_low;
        v.extreme_bg_color = tc.surface_lowest;
        v.code_bg_color = tc.surface_lowest;
        v.widgets.noninteractive.bg_fill = tc.surface_low;
        v.widgets.noninteractive.weak_bg_fill = tc.surface_low;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tc.outline_var);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, tc.outline);
        v.widgets.inactive.bg_fill = tc.surface;
        v.widgets.inactive.weak_bg_fill = tc.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tc.outline_var);
        v.widgets.hovered.bg_fill = tc.surface_high;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, tc.outline);
        v.widgets.active.bg_fill = tc.surface_high;
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, tc.primary);
        v.selection.bg_fill =
            Color32::from_rgba_unmultiplied(tc.primary.r(), tc.primary.g(), tc.primary.b(), 60);
        v.override_text_color = Some(tc.on_surface);
        // Both, and in this order: the context so later frames start correct, and the live
        // `Ui` so *this* frame is already themed. Setting only the context would leave the
        // root `Ui` — built before `logic()` ran — one frame behind on every toggle.
        ctx.set_visuals(v.clone());
        *ui.visuals_mut() = v;
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// A card: the one container every panel below is built from.
fn card(tc: &Tc) -> egui::Frame {
    egui::Frame::new()
        .fill(tc.surface_low)
        .corner_radius(CornerRadius::same(12))
        .stroke(Stroke::new(1.0_f32, tc.outline_var))
        .inner_margin(egui::Margin::same(20))
}

fn primary_button(tc: &Tc, label: &str) -> Button<'static> {
    Button::new(
        RichText::new(label.to_string())
            .color(tc.on_primary)
            .strong()
            .size(14.0),
    )
    .fill(tc.primary)
    .corner_radius(CornerRadius::same(8))
}

fn outline_button(label: &str, color: Color32) -> Button<'static> {
    Button::new(RichText::new(label.to_string()).color(color).size(13.0))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, color))
        .corner_radius(CornerRadius::same(8))
}

impl P2PTransfer {
    fn show_home(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("P2P File Transfer")
                    .color(tc.on_surface)
                    .size(28.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Secure, direct peer-to-peer sharing — no cloud storage")
                    .color(tc.outline)
                    .size(15.0),
            );
        });
        ui.add_space(32.0);

        let mut pick = false;
        let mut receive = false;
        ui.columns(2, |cols| {
            card(&tc).show(&mut cols[0], |ui| {
                ui.set_min_height(150.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Send")
                            .color(tc.on_surface)
                            .size(19.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Pick a file and share the link it produces.")
                            .color(tc.on_surface_var)
                            .size(13.0),
                    );
                    ui.add_space(16.0);
                    pick = ui.add(primary_button(&tc, "Choose File")).clicked();
                });
            });
            card(&tc).show(&mut cols[1], |ui| {
                ui.set_min_height(150.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Receive")
                            .color(tc.on_surface)
                            .size(19.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Paste a share link to download directly from the sender.")
                            .color(tc.on_surface_var)
                            .size(13.0),
                    );
                    ui.add_space(16.0);
                    receive = ui
                        .add(outline_button("Open a link", tc.secondary))
                        .clicked();
                });
            });
        });

        if pick {
            self.pick_file();
        }
        if receive {
            self.set_receive(FragmentParams::default(), None, None);
        }
    }

    fn show_send(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        let preparing_names: Vec<(String, f32)> = match &self.mode {
            Mode::Send { preparing } => preparing
                .iter()
                .map(|h| (h.name.clone(), *h.progress.borrow()))
                .collect(),
            _ => Vec::new(),
        };
        let ready = self.shared_files.lock().map(|f| f.len()).unwrap_or(0);
        let link = self.link.lock().ok().and_then(|l| l.clone());

        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new("Sharing")
                    .color(tc.on_surface)
                    .size(19.0)
                    .strong(),
            );
            ui.add_space(10.0);

            if let Ok(files) = self.shared_files.lock() {
                for f in files.iter() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&f.meta.name).color(tc.on_surface).size(14.0));
                        ui.label(
                            RichText::new(Self::format_size(f.meta.size))
                                .color(tc.outline)
                                .size(12.0),
                        );
                    });
                }
            }

            // The prepare phase (plan amendment 14): no link while any hash is provisional.
            for (name, pct) in &preparing_names {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Preparing {name}… {:.0} %", pct * 100.0))
                        .color(tc.on_surface_var)
                        .size(13.0),
                );
                ui.add(egui::ProgressBar::new(*pct).desired_height(6.0));
            }

            ui.add_space(14.0);
            match (&link, preparing_names.is_empty()) {
                (Some(link), true) => {
                    ui.label(RichText::new("Share link").color(tc.outline).size(12.0));
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(link)
                                    .color(tc.on_surface)
                                    .monospace()
                                    .size(12.0),
                            );
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button(&tc, "Copy link")).clicked() {
                            ui.ctx().copy_text(link.clone());
                        }
                        if ui.add(outline_button("Stop sharing", tc.outline)).clicked() {
                            self.stop_sharing();
                        }
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new(LINK_WARNING).color(tc.error).size(12.0));
                    ui.label(
                        RichText::new("Keep this tab open while receivers download.")
                            .color(tc.outline)
                            .size(12.0),
                    );
                }
                (_, false) => {
                    ui.label(
                        RichText::new("The link appears once every file is prepared.")
                            .color(tc.outline)
                            .size(12.0),
                    );
                }
                (None, true) if ready > 0 => {
                    ui.label(
                        RichText::new("Starting the node…")
                            .color(tc.outline)
                            .size(12.0),
                    );
                }
                _ => {}
            }

            if let Ok(err) = self.node_error.lock() {
                if let Some(err) = err.as_ref() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(err).color(tc.error).size(12.0));
                }
            }
        });

        self.show_peers(ui);
    }

    /// Per-peer progress for whatever this node is currently serving.
    fn show_peers(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        let peers = self
            .node
            .lock()
            .ok()
            .and_then(|n| n.as_ref().map(|n| n.peers()))
            .unwrap_or_default();
        if peers.is_empty() {
            return;
        }
        ui.add_space(12.0);
        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new(format!("Receivers ({})", peers.len()))
                    .color(tc.on_surface)
                    .size(15.0)
                    .strong(),
            );
            for (id, p) in peers {
                ui.add_space(8.0);
                let short = id.to_string();
                let short = short.get(..12).unwrap_or(&short).to_string();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(short)
                            .color(tc.on_surface_var)
                            .monospace()
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(Self::phase_text(&p.phase))
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.label(
                        RichText::new(Self::path_badge(&p.phase, p.path))
                            .color(tc.secondary)
                            .size(12.0),
                    );
                });
                if p.bytes_total > 0 {
                    let frac = p.bytes_done as f32 / p.bytes_total as f32;
                    ui.add(egui::ProgressBar::new(frac).desired_height(6.0));
                }
                if let Some(err) = &p.error {
                    ui.label(RichText::new(err).color(tc.error).size(11.0));
                    // The engine refuses a file whose bytes no longer match the hash it
                    // published; only the sender can resolve that, so say how.
                    if err.contains("changed since it was shared") {
                        ui.label(
                            RichText::new("Remove and re-add the file to share the new version.")
                                .color(tc.outline)
                                .size(11.0),
                        );
                    }
                }
            }
        });
    }

    fn show_receive(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        let progress = match &self.mode {
            Mode::Receive(r) => r.handle.as_ref().map(|h| h.latest()),
            _ => None,
        };
        let error = match &self.mode {
            Mode::Receive(r) => r.error.clone(),
            _ => None,
        };
        let flags = match &self.mode {
            Mode::Receive(r) => Self::flag_summary(&r.params),
            _ => String::new(),
        };
        let save_pending = match &self.mode {
            Mode::Receive(r) => r.save_pending.load(Ordering::Acquire),
            _ => false,
        };

        let mut submit = false;
        let mut save_manifest: Option<Vec<FileMeta>> = None;
        let mut cancel = false;

        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new("Receive")
                    .color(tc.on_surface)
                    .size(19.0)
                    .strong(),
            );
            ui.add_space(10.0);

            if progress.is_none() {
                ui.label(
                    RichText::new("Paste the share link you were sent")
                        .color(tc.outline)
                        .size(12.0),
                );
                ui.add_space(6.0);
                if let Mode::Receive(r) = &mut self.mode {
                    egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut r.input)
                                    .hint_text("https://…#endpoint…&cap=…")
                                    .frame(egui::Frame::new())
                                    .font(egui::FontId::monospace(13.0))
                                    .text_color(tc.on_surface)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                }
                ui.add_space(10.0);
                submit = ui.add(primary_button(&tc, "Download")).clicked();
            }

            if let Some(p) = &progress {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(Self::phase_text(&p.phase))
                            .color(tc.on_surface)
                            .size(14.0),
                    );
                    ui.label(
                        RichText::new(Self::path_badge(&p.phase, p.path))
                            .color(tc.secondary)
                            .size(12.0),
                    );
                    if p.bytes_per_sec > 0.0 {
                        ui.label(
                            RichText::new(format!(
                                "{}/s",
                                Self::format_size(p.bytes_per_sec as u64)
                            ))
                            .color(tc.outline)
                            .size(12.0),
                        );
                    }
                });

                if let Phase::AwaitingSave { manifest } = &p.phase {
                    ui.add_space(10.0);
                    for meta in manifest {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&meta.name).color(tc.on_surface).size(13.0));
                            ui.label(
                                RichText::new(Self::format_size(meta.size))
                                    .color(tc.outline)
                                    .size(12.0),
                            );
                            let hex = meta.hash.to_hex();
                            ui.label(
                                RichText::new(hex.get(..12).unwrap_or(&hex).to_string())
                                    .color(tc.outline_var)
                                    .monospace()
                                    .size(11.0),
                            );
                        });
                    }
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(!save_pending, primary_button(&tc, "Save"))
                        .clicked()
                    {
                        save_manifest = Some(manifest.clone());
                    }
                    if save_pending {
                        ui.label(
                            RichText::new("Selecting destinations…")
                                .color(tc.outline)
                                .size(12.0),
                        );
                    }
                } else if p.bytes_total > 0 {
                    ui.add_space(10.0);
                    let frac = p.bytes_done as f32 / p.bytes_total as f32;
                    ui.add(egui::ProgressBar::new(frac).desired_height(8.0));
                    ui.label(
                        RichText::new(format!(
                            "{} of {}",
                            Self::format_size(p.bytes_done),
                            Self::format_size(p.bytes_total)
                        ))
                        .color(tc.outline)
                        .size(12.0),
                    );
                }

                if !p.phase.is_terminal() {
                    ui.add_space(10.0);
                    cancel = ui.add(outline_button("Cancel", tc.outline)).clicked();
                }
            }

            if !flags.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(&flags).color(tc.outline_var).size(11.0));
            }
            if let Some(err) = &error {
                ui.add_space(10.0);
                ui.label(RichText::new(err).color(tc.error).size(13.0));
            }
        });

        if submit {
            let input = match &self.mode {
                Mode::Receive(r) => r.input.clone(),
                _ => String::new(),
            };
            self.start_receive(&input);
        }
        if let Some(manifest) = save_manifest {
            self.save_click(manifest);
        }
        if cancel {
            if let Mode::Receive(r) = &mut self.mode {
                // Dropping the handle cancels the session, which then aborts its sinks,
                // closes the connection and shuts its node down on its own.
                r.handle = None;
            }
        }
    }

    fn show_received_files(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        let Ok(files) = self.received_files.lock() else {
            return;
        };
        if files.is_empty() {
            return;
        }
        ui.add_space(12.0);
        card(&tc).show(ui, |ui| {
            ui.label(
                RichText::new(format!("Received ({})", files.len()))
                    .color(tc.on_surface)
                    .size(15.0)
                    .strong(),
            );
            for f in files.iter() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&f.name).color(tc.on_surface).size(13.0));
                    ui.label(
                        RichText::new(Self::format_size(f.size))
                            .color(tc.outline)
                            .size(12.0),
                    );
                    ui.label(RichText::new(&f.when).color(tc.outline_var).size(11.0));
                });
                ui.label(
                    RichText::new(&f.location)
                        .color(tc.on_surface_var)
                        .size(11.0),
                );
            }
        });
    }

    fn show_header(&mut self, ui: &mut Ui, ctx: &egui::Context, tc: &Tc) {
        ui.set_height(54.0);
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new("Syncoxiders")
                    .color(tc.primary)
                    .strong()
                    .size(20.0),
            );

            let at_home = matches!(self.mode, Mode::Home);
            if !at_home {
                ui.add_space(12.0);
                if ui.add(outline_button("🏠 Home", tc.outline)).clicked() {
                    self.mode = Mode::Home;
                }
            }
            let label = match self.mode {
                Mode::Home => "HOME",
                Mode::Send { .. } => "SENDING",
                Mode::Receive(_) => "RECEIVING",
            };
            ui.add_space(12.0);
            ui.label(RichText::new(label).color(tc.outline).size(11.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
                ui.add_space(12.0);
                if !matches!(self.mode, Mode::Receive(_))
                    && ui.add(primary_button(tc, "Choose File")).clicked()
                {
                    self.pick_file();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.add_space(12.0);
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
                #[cfg(target_arch = "wasm32")]
                let _ = ctx;
            });
        });
    }

    fn show_terminal(&mut self, ui: &mut Ui, tc: &Tc) {
        ui.horizontal(|ui| {
            ui.set_height(44.0);
            let chevron = if self.show_terminal_view {
                "▼"
            } else {
                "▲"
            };
            if ui
                .add(
                    Button::new(
                        RichText::new(format!("{chevron} Terminal Output >_"))
                            .color(tc.secondary)
                            .monospace()
                            .size(12.0),
                    )
                    .fill(Color32::TRANSPARENT),
                )
                .clicked()
            {
                self.show_terminal_view = !self.show_terminal_view;
            }
            ui.add_space(12.0);
            if !self.show_terminal_view {
                if let Ok(logs) = logging::terminal_buffer().lock() {
                    let msg = logs
                        .back()
                        .cloned()
                        .unwrap_or_else(|| "No logs yet…".into());
                    ui.label(RichText::new(msg).color(tc.outline).monospace().size(12.0));
                }
            }
        });

        if self.show_terminal_view {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match logging::terminal_buffer().lock() {
                        Ok(logs) if !logs.is_empty() => {
                            for line in logs.iter() {
                                ui.label(
                                    RichText::new(line)
                                        .color(tc.on_surface_var)
                                        .monospace()
                                        .size(12.0),
                                );
                            }
                        }
                        _ => {
                            ui.label(
                                RichText::new("No logs yet…")
                                    .color(tc.outline_var)
                                    .monospace()
                                    .size(12.0),
                            );
                        }
                    }
                });
        }
    }
}

// ── eframe glue ──────────────────────────────────────────────────────────────

impl eframe::App for P2PTransfer {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Non-drawing per-frame work. Runs before `ui`, and may show nothing itself.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_arch = "wasm32")]
        if !self.fragment_checked {
            self.fragment_checked = true;
            self.check_fragment();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.fragment_checked = true;
        }

        self.drain_picks();
        self.prune_preparing();
        self.adopt_pending_handle();
        self.poll_receive();
        self.drain_error();

        // Keep repainting only while something is actually moving.
        let busy = match &self.mode {
            Mode::Send { preparing } => !preparing.is_empty(),
            Mode::Receive(r) => r.handle.is_some(),
            Mode::Home => false,
        };
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        // Apply the theme here, not in `logic()`: the root `Ui` is built before `logic()`
        // runs, so a context-only update would leave this frame with the previous palette.
        let dark = ctx.global_style().visuals.dark_mode;
        if self.last_dark_mode != Some(dark) {
            Self::apply_theme(ctx, ui, dark);
            self.last_dark_mode = Some(dark);
        }
        let tc = Tc::of(dark);

        let header_frame = egui::Frame::new()
            .fill(tc.surface)
            .stroke(Stroke::new(1.0_f32, tc.outline_var))
            .inner_margin(egui::Margin {
                left: 24,
                right: 24,
                top: 0,
                bottom: 0,
            });
        egui::Panel::top("header")
            .exact_size(54.0)
            .frame(header_frame)
            .show(ui, |ui| self.show_header(ui, ctx, &tc));

        let terminal_frame = egui::Frame::new()
            .fill(tc.surface_lowest)
            .stroke(Stroke::new(1.0_f32, tc.outline_var))
            .inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 0,
                bottom: 0,
            });
        let terminal_height = if self.show_terminal_view { 200.0 } else { 44.0 };
        egui::Panel::bottom("terminal_bar")
            .exact_size(terminal_height)
            .frame(terminal_frame)
            .show(ui, |ui| self.show_terminal(ui, &tc));

        let content_frame = egui::Frame::new().fill(tc.bg).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 16,
            bottom: 16,
        });
        egui::CentralPanel::default()
            .frame(content_frame)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match self.mode {
                        Mode::Home => self.show_home(ui),
                        Mode::Send { .. } => self.show_send(ui),
                        Mode::Receive(_) => self.show_receive(ui),
                    }
                    self.show_received_files(ui);
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_save_attempt_blocks_duplicates_and_allows_retry() {
        let pending = Arc::new(AtomicBool::new(false));
        let attempt = SaveAttempt::begin(&pending).unwrap();
        assert!(pending.load(Ordering::Acquire));
        assert!(SaveAttempt::begin(&pending).is_none());

        // A picker error or dismissal drops the attempt without submitting a command.
        drop(attempt);
        assert!(!pending.load(Ordering::Acquire));
        assert!(SaveAttempt::begin(&pending).is_some());
    }

    #[test]
    fn local_test_submitted_save_stays_blocked_until_phase_changes() {
        let pending = Arc::new(AtomicBool::new(false));
        SaveAttempt::begin(&pending).unwrap().submitted();
        // There can still be a frame showing AwaitingSave before the engine consumes Save.
        assert!(SaveAttempt::begin(&pending).is_none());
    }

    #[test]
    fn local_test_old_save_attempt_cannot_change_new_receive_state() {
        let old_pending = Arc::new(AtomicBool::new(false));
        let old_attempt = SaveAttempt::begin(&old_pending).unwrap();
        let new_pending = Arc::new(AtomicBool::new(false));
        let new_attempt = SaveAttempt::begin(&new_pending).unwrap();

        drop(old_attempt);
        assert!(new_pending.load(Ordering::Acquire));
        drop(new_attempt);
        assert!(!new_pending.load(Ordering::Acquire));
    }
}
