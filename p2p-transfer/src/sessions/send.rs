//! Sharing files: pick → hash → bind a node → publish a link → serve peers.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use n0_future::task::{self, AbortOnDropHandle};
use tokio::sync::watch;

use super::{BearerLink, ErrorSlot, Notifier, Platform};
use crate::file_io::{self, FileOrigin, FileSnapshot, SharedFile, SharedFiles};
use crate::node::{Node, RelayChoice};
use crate::protocol::FileMeta;
use crate::transfer::TransferProgress;

/// A file the user picked or dropped, captured before hashing.
///
/// The snapshot is taken *before* hashing, so `open_source` can tell later whether the bytes
/// still match the hash the manifest advertises.
pub struct PickedFile {
    pub name: String,
    pub origin: FileOrigin,
    pub snapshot: FileSnapshot,
}

impl PickedFile {
    /// A native path from a picker or a drop. Folders are refused with
    /// [`file_io::FOLDER_REFUSED`] in the error.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(path: std::path::PathBuf) -> std::io::Result<Self> {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let snapshot = file_io::snapshot_path(&path)?;
        Ok(Self {
            name,
            origin: FileOrigin::Path(path),
            snapshot,
        })
    }

    /// A browser `File`. `File` freezes name, size and mtime at pick time, so this snapshot
    /// is the one the browser will still report later.
    #[cfg(target_arch = "wasm32")]
    pub fn from_web_file(file: web_sys::File) -> Self {
        let snapshot = file_io::snapshot_web(&file);
        Self {
            name: file.name(),
            origin: FileOrigin::Web(send_wrapper::SendWrapper::new(file)),
            snapshot,
        }
    }

    /// In-memory bytes (tests, generated content).
    pub fn from_bytes(name: impl Into<String>, bytes: bytes::Bytes) -> Self {
        let snapshot = FileSnapshot {
            size: bytes.len() as u64,
            modified_ms: None,
        };
        Self {
            name: name.into(),
            origin: FileOrigin::Memory(bytes),
            snapshot,
        }
    }
}

/// Where pickers and drops hand files over.
///
/// A browser picker completes in a JS callback that cannot borrow the session, so every
/// source pushes here and [`super::Oxfer::tick`] drains it. Cloning shares the queue.
#[derive(Clone)]
pub struct PickQueue {
    picks: Arc<Mutex<Vec<PickedFile>>>,
    errors: ErrorSlot,
    notifier: Notifier,
}

impl PickQueue {
    pub(super) fn new(errors: ErrorSlot, notifier: Notifier) -> Self {
        Self {
            picks: Arc::default(),
            errors,
            notifier,
        }
    }

    /// Queue one file and wake the UI.
    pub fn push(&self, file: PickedFile) {
        if let Ok(mut picks) = self.picks.lock() {
            picks.push(file);
        }
        self.notifier.notify();
    }

    /// Queue a native path, reporting an unreadable path or folder by name.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn push_path(&self, path: std::path::PathBuf) {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match PickedFile::from_path(path) {
            Ok(file) => self.push(file),
            Err(e) => self.errors.report(format!("could not read {name}: {e}")),
        }
    }

    /// Queue dropped browser files after probing each one: a dropped folder is a `File`
    /// whose reads fail, and must be refused by name instead of failing at hash time.
    ///
    /// No wake lock here: a drop is not a user activation. The lock follows once hashing
    /// starts.
    #[cfg(target_arch = "wasm32")]
    pub fn push_dropped(&self, files: Vec<web_sys::File>) {
        let queue = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            for file in files {
                match file_io::web::probe_readable(&file).await {
                    Ok(()) => queue.push(PickedFile::from_web_file(file)),
                    Err(e) => {
                        // The browser's error is a raw DOMException dump; keep it in the log.
                        log::debug!("probe of {} failed: {e}", file.name());
                        queue.errors.report(format!(
                            "could not read {} ({})",
                            file.name(),
                            file_io::FOLDER_REFUSED
                        ));
                    }
                }
            }
            queue.notifier.notify();
        });
    }

    pub(super) fn drain(&self) -> Vec<PickedFile> {
        self.picks
            .lock()
            .map(|mut picks| picks.drain(..).collect())
            .unwrap_or_default()
    }

    /// Files waiting for the next tick.
    pub fn len(&self) -> usize {
        self.picks.lock().map(|p| p.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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

/// One file being hashed.
#[derive(Clone, Debug, PartialEq)]
pub struct Preparing {
    pub name: String,
    /// 0.0 ..= 1.0
    pub progress: f32,
}

/// Where the share link stands.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkState {
    /// Nothing is shared.
    Idle,
    /// At least one file is still hashing; no link may be shown yet.
    Preparing,
    /// Files are ready and the node is binding or waiting to be reachable.
    Binding,
    /// Send this.
    Ready(BearerLink),
    /// Files are ready but the node could not start; [`SendIntent::RetrySharing`] retries.
    Failed,
}

/// One peer downloading from this share.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerSnapshot {
    /// The peer's endpoint id; show [`super::text::short_id`] of it.
    pub id: String,
    pub progress: TransferProgress,
}

/// Everything a Send screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct SendSnapshot {
    /// A node is bound or binding: the share outlives navigation to Home.
    pub sharing: bool,
    /// Increments each time sharing (re)starts, so a UI can reset per-share view state such
    /// as "Copied".
    pub share_id: u64,
    pub files: Vec<FileMeta>,
    pub preparing: Vec<Preparing>,
    pub link: LinkState,
    pub peers: Vec<PeerSnapshot>,
    /// The last reported failure (pick, hash, bind). Shown on Send; Receive takes it.
    pub error: Option<String>,
}

impl SendSnapshot {
    /// Any peer is mid-transfer, so the page must stay awake for it.
    pub fn serving(&self) -> bool {
        self.peers.iter().any(|p| !p.progress.phase.is_terminal())
    }
}

/// What a Send screen can ask for. Files arrive through the [`PickQueue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendIntent {
    /// Stop serving. The next share draws a fresh key, so the old link stops working.
    StopSharing,
    /// Clear the error and bind again after a failed start.
    RetrySharing,
}

/// The sender side: hashing tasks, the bound node and its link.
pub struct SendSession {
    relay: RelayChoice,
    platform: Arc<dyn Platform>,
    notifier: Notifier,
    errors: ErrorSlot,
    files: SharedFiles,
    preparing: Vec<PrepareHandle>,
    /// `Some` while sharing; written by the bind task.
    node: Arc<Mutex<Option<Arc<Node>>>>,
    link: Arc<Mutex<Option<String>>>,
    /// Set the moment a bind task is spawned, not when it finishes: `node` stays `None` for
    /// the whole bind, so two picks in quick succession would otherwise start two nodes and
    /// the second would silently replace the first one's link.
    sharing: Arc<AtomicBool>,
    /// Bumped by every start and stop, so a bind that finishes after a stop (or after a
    /// newer start) shuts its node down instead of publishing it.
    share_id: Arc<AtomicU64>,
}

impl SendSession {
    pub(super) fn new(
        relay: RelayChoice,
        platform: Arc<dyn Platform>,
        notifier: Notifier,
        errors: ErrorSlot,
    ) -> Self {
        Self {
            relay,
            platform,
            notifier,
            errors,
            files: Arc::new(Mutex::new(Vec::new())),
            preparing: Vec::new(),
            node: Arc::default(),
            link: Arc::default(),
            sharing: Arc::new(AtomicBool::new(false)),
            share_id: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn snapshot(&self) -> SendSnapshot {
        let files: Vec<FileMeta> = self
            .files
            .lock()
            .map(|f| f.iter().map(|f| f.meta.clone()).collect())
            .unwrap_or_default();
        let preparing: Vec<Preparing> = self
            .preparing
            .iter()
            .map(|h| Preparing {
                name: h.name.clone(),
                progress: *h.progress.borrow(),
            })
            .collect();
        let sharing = self.is_sharing();
        let link = self.link.lock().ok().and_then(|l| l.clone());
        let link = match (link, preparing.is_empty()) {
            (_, false) => LinkState::Preparing,
            (Some(link), true) => LinkState::Ready(BearerLink(link)),
            (None, true) if !files.is_empty() && sharing => LinkState::Binding,
            (None, true) if !files.is_empty() => LinkState::Failed,
            (None, true) => LinkState::Idle,
        };
        let peers = self
            .node
            .lock()
            .ok()
            .and_then(|n| n.as_ref().map(|n| n.peers()))
            .unwrap_or_default()
            .into_iter()
            .map(|(id, progress)| PeerSnapshot {
                id: id.to_string(),
                progress,
            })
            .collect();
        SendSnapshot {
            sharing,
            share_id: self.share_id.load(Ordering::Acquire),
            files,
            preparing,
            link,
            peers,
            error: self.errors.peek(),
        }
    }

    pub fn dispatch(&mut self, intent: SendIntent) {
        match intent {
            SendIntent::StopSharing => self.stop_sharing(),
            SendIntent::RetrySharing => {
                self.errors.clear();
                self.start_sharing();
            }
        }
        self.notifier.notify();
    }

    pub fn is_sharing(&self) -> bool {
        self.sharing.load(Ordering::Acquire)
    }

    /// Some file is still hashing. Leaving Send would abort it, so UIs block Home and
    /// other activities meanwhile.
    pub fn is_preparing(&self) -> bool {
        !self.preparing.is_empty()
    }

    /// Hash `picks`, then start sharing as soon as there is something to share; the link
    /// stays hidden until every hash is final (design §4.8.2).
    pub(super) fn add_files(&mut self, picks: Vec<PickedFile>) {
        if picks.is_empty() {
            return;
        }
        for pick in picks {
            let handle = self.begin_prepare(pick);
            self.preparing.push(handle);
        }
        self.start_sharing();
    }

    /// Drop hashing handles whose task has ended.
    pub(super) fn prune(&mut self) {
        self.preparing.retain(|h| !h.finished());
    }

    /// Engine watches have no callback: poll while starting, hashing or serving.
    pub(super) fn needs_poll(&self) -> bool {
        self.is_sharing() || self.is_preparing()
    }

    pub(super) fn needs_wake_lock(&self) -> bool {
        self.is_sharing() || self.is_preparing()
    }

    fn begin_prepare(&mut self, pick: PickedFile) -> PrepareHandle {
        let (progress_tx, progress_rx) = watch::channel(0.0f32);
        let files = self.files.clone();
        let errors = self.errors.clone();
        let notifier = self.notifier.clone();
        let label = pick.name.clone();

        let task = task::spawn(async move {
            let PickedFile {
                name,
                origin,
                snapshot,
            } = pick;
            let mut source = match file_io::open_source(&origin, &snapshot).await {
                Ok(source) => source,
                Err(e) => {
                    errors.report(format!("could not open {name}: {e}"));
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
                Err(e) => errors.report(format!("could not hash {name}: {e}")),
            }
            // Dropping `progress_tx` here is what tells the session this handle is done.
            drop(progress_tx);
            notifier.notify();
        });

        PrepareHandle {
            name: label,
            progress: progress_rx,
            _task: AbortOnDropHandle::new(task),
        }
    }

    fn start_sharing(&mut self) {
        if self.sharing.swap(true, Ordering::AcqRel) {
            return;
        }
        let id = self.share_id.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let current = self.share_id.clone();
        let files = self.files.clone();
        let node_slot = self.node.clone();
        let link_slot = self.link.clone();
        let errors = self.errors.clone();
        let sharing = self.sharing.clone();
        let notifier = self.notifier.clone();
        let relay = self.relay.clone();
        let base = self.platform.base_url();
        let dev = self.platform.dev_flag();

        task::spawn(async move {
            let node = match Node::bind(files, relay).await {
                Ok(node) => Arc::new(node),
                Err(e) => {
                    if current.load(Ordering::Acquire) == id {
                        sharing.store(false, Ordering::Release);
                        errors.report(e.to_string());
                    }
                    notifier.notify();
                    return;
                }
            };
            match node.ticket().await {
                Ok(ticket) if current.load(Ordering::Acquire) == id => {
                    let link = Node::link(&base, &ticket, &node.cap(), dev);
                    if let Ok(mut slot) = link_slot.lock() {
                        *slot = Some(link);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    if current.load(Ordering::Acquire) == id {
                        sharing.store(false, Ordering::Release);
                        errors.report(e.to_string());
                    }
                    notifier.notify();
                    node.shutdown().await;
                    return;
                }
            }
            // `stop_sharing` may have run during the bind: then nobody owns this node.
            if current.load(Ordering::Acquire) != id {
                node.shutdown().await;
                return;
            }
            if let Ok(mut slot) = node_slot.lock() {
                *slot = Some(node);
            }
            notifier.notify();
        });
    }

    fn stop_sharing(&mut self) {
        self.share_id.fetch_add(1, Ordering::AcqRel);
        self.sharing.store(false, Ordering::Release);
        self.platform.set_wake_lock(false);
        let node = self.node.lock().ok().and_then(|mut n| n.take());
        if let Some(node) = node {
            task::spawn(async move { node.shutdown().await });
        }
        if let Ok(mut link) = self.link.lock() {
            *link = None;
        }
    }
}
