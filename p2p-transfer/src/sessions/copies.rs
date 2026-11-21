//! Browser-only library of opt-in local copies (OPFS).
//!
//! Neither this state nor any share link is persisted by a UI; the storage module owns
//! versioned file checkpoints. See `docs/resumable-transfers.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::Notifier;
use crate::file_io::web::resume::{self, StoredTransfer};

/// What a "Saved files in this browser" list can ask for. Continuing a copy is
/// [`super::Intent::ResumeLocalCopy`], because it opens a receive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopiesIntent {
    Refresh,
    /// Delete the copy and its checkpoints. Confirm first: this cannot be undone.
    Discard(String),
    /// Hand one verified file to the browser's download route.
    Export(String, usize),
}

/// Everything the local-copies list shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CopiesSnapshot {
    pub entries: Vec<StoredTransfer>,
    pub error: Option<String>,
    pub busy: bool,
}

/// Shared handle: receives refresh it after durable saves.
#[derive(Clone)]
pub struct LocalCopies {
    entries: Arc<Mutex<Vec<StoredTransfer>>>,
    error: Arc<Mutex<Option<String>>>,
    busy: Arc<AtomicBool>,
    notifier: Notifier,
}

impl LocalCopies {
    pub(super) fn new(notifier: Notifier) -> Self {
        Self {
            entries: Arc::default(),
            error: Arc::default(),
            busy: Arc::new(AtomicBool::new(false)),
            notifier,
        }
    }

    pub fn snapshot(&self) -> CopiesSnapshot {
        CopiesSnapshot {
            entries: self.entries.lock().unwrap().clone(),
            error: self.error.lock().unwrap().clone(),
            busy: self.busy.load(Ordering::Acquire),
        }
    }

    pub(super) fn refresh(&self) {
        self.dispatch(CopiesIntent::Refresh);
    }

    /// Run one action, then reload the list. Ignored while another action runs.
    pub fn dispatch(&self, intent: CopiesIntent) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        self.notifier.notify();
        let library = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = match intent {
                CopiesIntent::Refresh => Ok(()),
                CopiesIntent::Discard(id) => resume::discard(&id).await,
                CopiesIntent::Export(id, index) => resume::export(&id, index).await,
            };
            let result = match result {
                Ok(()) => resume::list().await,
                Err(error) => Err(error),
            };
            match result {
                Ok(entries) => {
                    *library.entries.lock().unwrap() = entries;
                    *library.error.lock().unwrap() = None;
                }
                Err(error) => *library.error.lock().unwrap() = Some(error.to_string()),
            }
            library.busy.store(false, Ordering::Release);
            library.notifier.notify();
        });
    }
}
