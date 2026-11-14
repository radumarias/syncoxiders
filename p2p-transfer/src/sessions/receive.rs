//! Receiving files: open a link → bind → handshake → Save → transfer → verify.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use n0_future::task;

use super::text::{humanize, MISSING_CAP};
use super::{BearerLink, ErrorSlot, Notifier, Platform};
use crate::file_io::AnySink;
#[cfg(not(target_arch = "wasm32"))]
use crate::file_io::{self, FsSink};
use crate::node::{FragmentParams, Node, RelayChoice};
use crate::transfer::{
    Path as TransferPath, Phase, ReceiveCommand, ReceiveOptions, TransferHandle, TransferProgress,
};

/// One file this node received and saved, as the UI lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub location: String,
    pub when: String,
    pub path: TransferPath,
    /// Newly received bytes per second in this session; absent for an empty/already saved file.
    pub average_bytes_per_sec: Option<f64>,
}

/// Where a Save should write.
pub enum SaveTarget {
    /// One staged, verified file per manifest entry in this directory (native).
    #[cfg(not(target_arch = "wasm32"))]
    Directory(std::path::PathBuf),
    /// The browser's own route: a File System Access picker, the service-worker stream or
    /// memory, or OPFS when the user chose to keep a local copy. Dispatch it from inside
    /// the Save click handler: the picker needs the gesture's transient activation.
    #[cfg(target_arch = "wasm32")]
    Browser,
    /// Caller-built sinks in manifest order (tests, embedders).
    Sinks(Vec<AnySink>),
}

/// What a Receive screen can ask for.
pub enum ReceiveIntent {
    /// The link text box changed.
    SetInput(String),
    /// Open whatever is in the link box (see [`super::Intent::OpenLink`]).
    OpenInput,
    /// Accept the offered files. Ignored unless the engine is in `AwaitingSave` and no
    /// other Save is pending.
    Save(SaveTarget),
    /// Stop connecting or receiving. The session aborts its sinks and shuts its node down.
    Cancel,
    /// Browser only: keep a durable local copy in OPFS so a stopped transfer can resume.
    SetKeepLocalCopy(bool),
}

/// Everything a Receive screen shows.
#[derive(Clone, PartialEq)]
pub struct ReceiveSnapshot {
    /// Distinct per opened link, so a UI can reset per-receive view state.
    pub id: u64,
    /// The link box. Kept after an error so the user can fix it, cleared after success so
    /// the bearer credential is not put back on screen.
    pub input: BearerLink,
    /// The node is binding; no progress exists yet.
    pub opening: bool,
    /// The engine's progress once the node is up. Terminal phases are folded into
    /// [`ReceiveSnapshot::error`] / the received list and then cleared.
    pub progress: Option<TransferProgress>,
    pub error: Option<String>,
    /// QA flags from the link, for a small diagnostic line; empty normally.
    pub flags: String,
    /// A Save was clicked and its sinks are being created or were submitted.
    pub save_pending: bool,
    /// Browser only; always false natively.
    pub keep_local_copy: bool,
    /// The user picked a specific local copy to continue.
    pub resuming: bool,
}

impl std::fmt::Debug for ReceiveSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiveSnapshot")
            .field("id", &self.id)
            .field("input", &self.input)
            .field("opening", &self.opening)
            .field("phase", &self.progress.as_ref().map(|p| &p.phase))
            .field("error", &self.error)
            .field("save_pending", &self.save_pending)
            .finish_non_exhaustive()
    }
}

impl ReceiveSnapshot {
    /// The manifest on offer, while the engine waits for Save.
    pub fn manifest(&self) -> Option<&[crate::protocol::FileMeta]> {
        match self.progress.as_ref().map(|p| &p.phase) {
            Some(Phase::AwaitingSave { manifest }) => Some(manifest),
            _ => None,
        }
    }

    /// Connecting or receiving: Cancel applies and other activities must not replace it.
    pub fn active(&self) -> bool {
        self.opening || self.progress.is_some()
    }
}

type ReceiveStartup = Arc<Mutex<Option<Result<TransferHandle, String>>>>;

/// One destination selection per receive session. Errors and dismissal allow a retry;
/// a submitted Save remains pending until the engine leaves AwaitingSave.
pub(super) struct SaveAttempt {
    pending: Arc<AtomicBool>,
    submitted: bool,
}

impl SaveAttempt {
    pub(super) fn begin(pending: &Arc<AtomicBool>) -> Option<Self> {
        if pending.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(Self {
            pending: pending.clone(),
            submitted: false,
        })
    }

    pub(super) fn submitted(mut self) {
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

/// What finishing a receive produced, for [`super::Oxfer`] to file away.
pub(super) enum Finished {
    Saved(Vec<ReceivedFile>),
    Ended,
}

/// One receive, from link to verified files. Dropping it cancels the transfer.
pub struct ReceiveSession {
    id: u64,
    relay: RelayChoice,
    platform: Arc<dyn Platform>,
    notifier: Notifier,
    errors: ErrorSlot,
    input: String,
    params: FragmentParams,
    handle: Option<TransferHandle>,
    error: Option<String>,
    opening: bool,
    /// Each receive owns its startup result. Leaving, cancelling, or opening another link
    /// drops this slot, so a late completion cannot replace the new session.
    startup: ReceiveStartup,
    save_pending: Arc<AtomicBool>,
    keep_local_copy: bool,
    /// Set only when the user selected a specific partial copy to resume.
    resume_target: Option<String>,
    /// Hands the local-copies library a refresh after a durable save.
    #[cfg(target_arch = "wasm32")]
    copies: super::copies::LocalCopies,
}

impl ReceiveSession {
    pub(super) fn new(id: u64, deps: &super::Deps, input: String) -> Self {
        Self {
            id,
            relay: deps.relay.clone(),
            platform: deps.platform.clone(),
            notifier: deps.notifier.clone(),
            errors: deps.errors.clone(),
            input,
            params: FragmentParams::default(),
            handle: None,
            error: None,
            opening: false,
            startup: Arc::default(),
            save_pending: Arc::new(AtomicBool::new(false)),
            keep_local_copy: false,
            resume_target: None,
            #[cfg(target_arch = "wasm32")]
            copies: deps.copies.clone(),
        }
    }

    /// A blank receive that will continue the local copy `id` (browser only).
    #[cfg(target_arch = "wasm32")]
    pub(super) fn resume(mut self, id: String) -> Self {
        self.keep_local_copy = true;
        self.resume_target = Some(id);
        self
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn snapshot(&self) -> ReceiveSnapshot {
        ReceiveSnapshot {
            id: self.id,
            input: BearerLink(self.input.clone()),
            opening: self.opening,
            progress: self.handle.as_ref().map(TransferHandle::latest),
            error: self.error.clone(),
            flags: super::text::flag_summary(&self.params),
            save_pending: self.save_pending.load(Ordering::Acquire),
            keep_local_copy: self.keep_local_copy,
            resuming: self.resume_target.is_some(),
        }
    }

    /// Connecting or receiving.
    pub fn active(&self) -> bool {
        self.opening || self.handle.is_some()
    }

    /// Validate `params` and start binding. Errors land in the snapshot, not a panic.
    pub(super) fn open(&mut self, params: FragmentParams) {
        self.error = None;
        if let Some(error) = params.error.clone() {
            self.params = params;
            self.error = Some(error);
            return;
        }
        let Some(ticket) = params.ticket.clone() else {
            self.params = params;
            self.error = Some("this is not a share link".to_string());
            return;
        };
        // A bare ticket authorizes nothing (design §2.6): refuse before dialling, and say why,
        // so a truncated link never looks like a connectivity failure.
        if params.cap.is_none() {
            self.params = params;
            self.error = Some(MISSING_CAP.to_string());
            return;
        }

        let opts = ReceiveOptions::from_fragment(&params);
        self.params = params;
        self.opening = true;
        let relay = self.relay.clone();
        let notifier = self.notifier.clone();
        let slot = Arc::downgrade(&self.startup);

        task::spawn(async move {
            // A receiver binds its own node on demand; `run_receiver` shuts it down again on
            // every exit path, so a cancelled receive leaks nothing.
            let result = match Node::bind(Arc::new(Mutex::new(Vec::new())), relay).await {
                Ok(node) if slot.strong_count() == 0 => {
                    node.shutdown().await;
                    return;
                }
                Ok(node) => Ok(TransferHandle::start_receive(Arc::new(node), ticket, opts)),
                Err(e) => Err(e.to_string()),
            };
            if let Some(slot) = slot.upgrade() {
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(result);
                }
            }
            notifier.notify();
        });
    }

    pub(super) fn dispatch(&mut self, intent: ReceiveIntent) {
        match intent {
            ReceiveIntent::SetInput(input) => self.input = input,
            // Routed by `Oxfer`, which may turn the link into a chat join instead.
            ReceiveIntent::OpenInput => {}
            ReceiveIntent::Save(target) => self.save(target),
            ReceiveIntent::Cancel => self.cancel(),
            ReceiveIntent::SetKeepLocalCopy(keep) => {
                if !self.save_pending.load(Ordering::Acquire) {
                    self.keep_local_copy = keep && cfg!(target_arch = "wasm32");
                }
            }
        }
    }

    pub(super) fn input(&self) -> &str {
        &self.input
    }

    fn cancel(&mut self) {
        // Dropping the handle cancels the session, which then aborts its sinks, closes the
        // connection and shuts its node down on its own.
        self.opening = false;
        self.startup = Arc::default();
        self.handle = None;
        self.keep_local_copy = false;
        self.resume_target = None;
        #[cfg(target_arch = "wasm32")]
        self.copies.refresh();
    }

    /// Per tick: adopt the bound handle, take a reported error, finish a terminal phase.
    pub(super) fn tick(&mut self, when: &str) -> Option<Finished> {
        self.adopt_pending_handle();
        let finished = self.poll_terminal(when);
        self.drain_error();
        finished
    }

    /// Adopt a receive handle built by the on-demand bind task.
    fn adopt_pending_handle(&mut self) {
        let pending = self.startup.lock().ok().and_then(|mut h| h.take());
        if let Some(result) = pending {
            self.opening = false;
            match result {
                Ok(handle) => {
                    // The engine's watch has no callback; forward its changes so a UI that
                    // repaints only on notification still sees every phase.
                    let mut progress = handle.progress.clone();
                    let notifier = self.notifier.clone();
                    task::spawn(async move {
                        while progress.changed().await.is_ok() {
                            notifier.notify();
                        }
                    });
                    self.handle = Some(handle);
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    /// Move a failure reported by a spawned task (Save, sink creation) into this panel.
    fn drain_error(&mut self) {
        if self.error.is_some() {
            return;
        }
        if let Some(taken) = self.errors.take() {
            self.opening = false;
            self.error = Some(taken);
        }
    }

    /// Record a finished receive exactly once, then drop the handle.
    ///
    /// The terminal phase arrives on the same watch the screen already reads, so there is no
    /// second source of truth; dropping the handle afterwards also stops cloning the terminal
    /// progress value on every later snapshot.
    fn poll_terminal(&mut self, when: &str) -> Option<Finished> {
        let progress = self.handle.as_ref()?.latest();
        if !progress.phase.is_terminal() {
            return None;
        }
        let path = progress.path;
        let mut finished = Finished::Ended;
        match progress.phase {
            Phase::Complete { saved } => {
                finished = Finished::Saved(
                    saved
                        .into_iter()
                        .enumerate()
                        .map(|(index, f)| ReceivedFile {
                            name: f.name,
                            size: f.size,
                            location: f.location,
                            when: when.to_string(),
                            path,
                            average_bytes_per_sec: progress
                                .file_average_bytes_per_sec
                                .get(index)
                                .copied()
                                .flatten(),
                        })
                        .collect(),
                );
                // The URL was scrubbed as soon as it was accepted; after success, do not put
                // the bearer credential back on screen.
                self.input.clear();
            }
            Phase::Failed => {
                self.error = Some(
                    progress
                        .error
                        .as_deref()
                        .map(humanize)
                        .unwrap_or_else(|| "the transfer failed".to_string()),
                );
            }
            Phase::Cancelled => self.error = Some("the transfer was cancelled".to_string()),
            _ => {}
        }
        self.handle = None;
        self.keep_local_copy = false;
        self.resume_target = None;
        #[cfg(target_arch = "wasm32")]
        self.copies.refresh();
        Some(finished)
    }

    pub(super) fn needs_wake_lock(&self) -> bool {
        let progress = self.handle.as_ref().map(TransferHandle::latest);
        receive_needs_wake_lock(
            self.opening,
            self.save_pending.load(Ordering::Acquire),
            progress.as_ref().map(|p| &p.phase),
        )
    }

    /// Build one sink per manifest entry and hand them to the session.
    ///
    /// Invoke the browser picker promptly after the Save gesture, without preceding
    /// asynchronous work. Transient activation is browser-controlled, not an await count.
    fn save(&mut self, target: SaveTarget) {
        let Some(handle) = &self.handle else { return };
        let Phase::AwaitingSave { manifest } = handle.latest().phase else {
            return;
        };
        let Some(attempt) = SaveAttempt::begin(&self.save_pending) else {
            return;
        };
        self.platform.retry_wake_lock();
        self.error = None;
        let commands = handle.commands.clone();
        let errors = self.errors.clone();

        let sinks = match target {
            SaveTarget::Sinks(sinks) => sinks,
            #[cfg(not(target_arch = "wasm32"))]
            SaveTarget::Directory(dir) => {
                let mut sinks = Vec::with_capacity(manifest.len());
                for meta in &manifest {
                    match FsSink::create(&dir, &file_io::sanitize_name(&meta.name)) {
                        Ok(sink) => sinks.push(AnySink::Fs(sink)),
                        Err(e) => {
                            errors.report(format!("could not create {}: {e}", meta.name));
                            return;
                        }
                    }
                }
                sinks
            }
            #[cfg(target_arch = "wasm32")]
            SaveTarget::Browser => {
                self.save_in_browser(manifest, attempt, handle.cancel.clone());
                return;
            }
        };
        if sinks.len() != manifest.len() {
            errors.report("one destination is needed per offered file".to_string());
            return;
        }
        if commands.try_send(ReceiveCommand::Save(sinks)).is_err() {
            errors.report("the transfer is no longer running".to_string());
        } else {
            attempt.submitted();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn save_in_browser(
        &self,
        manifest: Vec<crate::protocol::FileMeta>,
        attempt: SaveAttempt,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        use crate::file_io;

        let errors = self.errors.clone();
        if self.keep_local_copy {
            if let Some(expected) = &self.resume_target {
                match file_io::web::resume::manifest_id(&manifest) {
                    Ok(id) if &id == expected => {}
                    Ok(_) => {
                        errors.report(
                            "this link offers different files than the selected local copy; \
                             ask the sender for the same files, names and ordering"
                                .to_string(),
                        );
                        return;
                    }
                    Err(error) => {
                        errors.report(error.to_string());
                        return;
                    }
                }
            }
        }
        let Some(handle) = &self.handle else { return };
        let commands = handle.commands.clone();
        let pref = self.params.sink_pref;
        let keep_local_copy = self.keep_local_copy;
        let notifier = self.notifier.clone();
        let library = self.copies.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if cancel.is_cancelled() {
                return;
            }
            if keep_local_copy {
                // Persistent copies are keyed by the complete manifest, not the link.
                // Reopening a fresh sender link can therefore resume the same bytes
                // without storing a bearer capability on this device.
                match file_io::web::resume::prepare(&manifest).await {
                    Ok(files) => {
                        let permit = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => None,
                            result = commands.reserve() => result.ok(),
                        };
                        if let Some(permit) = permit.filter(|_| !cancel.is_cancelled()) {
                            permit.send(ReceiveCommand::Resume(files));
                            attempt.submitted();
                        } else {
                            for file in files {
                                file_io::Sink::abort(file.sink).await;
                            }
                        }
                    }
                    Err(_) if cancel.is_cancelled() => {}
                    Err(error) => errors.report(error.to_string()),
                }
                library.refresh();
                notifier.notify();
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
                        notifier.notify();
                        return;
                    }
                    for sink in sinks {
                        file_io::Sink::abort(sink).await;
                    }
                    if !cancel.is_cancelled() {
                        errors.report("the transfer is no longer running".to_string());
                    }
                }
                // Dismissing a picker is not an error: stay in AwaitingSave so Save can be
                // clicked again.
                Err(file_io::SinkError::Cancelled) => {}
                Err(_) if cancel.is_cancelled() => {}
                Err(e) => errors.report(e.to_string()),
            }
            notifier.notify();
        });
    }
}

/// The page must stay awake while connecting, transferring, or saving — but not while a
/// preview just sits waiting for the user to pick a destination.
pub fn receive_needs_wake_lock(opening: bool, save_pending: bool, phase: Option<&Phase>) -> bool {
    opening
        || phase.is_some_and(|phase| {
            !phase.is_terminal() && (save_pending || !matches!(phase, Phase::AwaitingSave { .. }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_receiver_wake_lock_requires_active_session_or_save_attempt() {
        let preview = Phase::AwaitingSave { manifest: vec![] };
        let done = Phase::Complete { saved: vec![] };
        assert!(receive_needs_wake_lock(true, false, None));
        assert!(!receive_needs_wake_lock(false, true, None));
        assert!(!receive_needs_wake_lock(false, false, Some(&preview)));
        assert!(receive_needs_wake_lock(false, true, Some(&preview)));
        assert!(receive_needs_wake_lock(
            false,
            true,
            Some(&Phase::Transferring)
        ));
        assert!(!receive_needs_wake_lock(false, true, Some(&done)));
    }

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
