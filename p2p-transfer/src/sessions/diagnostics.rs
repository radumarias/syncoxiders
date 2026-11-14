//! Browser network diagnostics: local relay checks plus an optional peer ping test.
//!
//! See `docs/diagnostics.md`. Reports never contain tickets, capabilities or peer
//! addresses; the peer test shares a short session id instead.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use iroh_tickets::endpoint::EndpointTicket;

use super::Notifier;
use crate::node::{DiagnosticNode, RelayChoice};

#[derive(Default)]
struct Peer {
    node: Option<Arc<DiagnosticNode>>,
    target: Option<EndpointTicket>,
    link: Option<String>,
    session: Option<String>,
    outcome: Option<String>,
    busy: bool,
    generation: u64,
}

/// What a diagnostics screen can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticsIntent {
    /// Re-run the local checks.
    RunChecks,
    /// Listen for a peer (no target) or ping the peer from the opened link (target).
    StartPeerTest,
    StopPeerTest,
}

/// Everything the diagnostics screen shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagnosticsSnapshot {
    /// The shareable local-checks link (`/diags`, no peer address).
    pub local_link: String,
    pub report: Option<String>,
    pub running: bool,
    /// This page was opened from a peer-test link and can ping it.
    pub has_target: bool,
    /// The test-only link to send to the other device while listening.
    pub peer_link: Option<String>,
    pub session: Option<String>,
    pub outcome: Option<String>,
    pub busy: bool,
    /// Successful incoming pings, while listening.
    pub completed: Option<usize>,
    /// Relay WebSocket attempts the log shows as cancelled.
    pub cancelled_websockets: usize,
}

impl DiagnosticsSnapshot {
    /// The full text "Copy report" puts on the clipboard.
    pub fn full_report(&self) -> Option<String> {
        let report = self.report.as_ref()?;
        Some(format!(
            "{report}\nSession ID: {}\nPeer test: {}\nSuccessful incoming pings: {}\nCancelled relay WebSocket attempts logged: {}",
            self.session.as_deref().unwrap_or("none"),
            self.outcome.as_deref().unwrap_or("not run"),
            self.completed.unwrap_or(0),
            self.cancelled_websockets,
        ))
    }
}

/// The local checks and the peer test. Lives as long as the app; screens reset it.
pub struct DiagnosticsSession {
    relay: RelayChoice,
    notifier: Notifier,
    report: Arc<Mutex<Option<String>>>,
    running: Arc<AtomicBool>,
    peer: Arc<Mutex<Peer>>,
}

/// The shareable local-checks page on this origin.
pub fn diagnostics_url() -> Option<String> {
    Some(format!(
        "{}/diags",
        web_sys::window()?.location().origin().ok()?
    ))
}

impl DiagnosticsSession {
    pub(super) fn new(relay: RelayChoice, notifier: Notifier) -> Self {
        Self {
            relay,
            notifier,
            report: Arc::default(),
            running: Arc::new(AtomicBool::new(false)),
            peer: Arc::default(),
        }
    }

    pub fn snapshot(&self) -> DiagnosticsSnapshot {
        let cancelled_websockets = crate::logging::terminal_buffer()
            .lock()
            .map(|logs| {
                logs.iter()
                    .filter(|line| line.contains("WsMeta::connect future was dropped"))
                    .count()
            })
            .unwrap_or(0);
        let peer = self.peer.lock().unwrap();
        DiagnosticsSnapshot {
            local_link: diagnostics_url().unwrap_or_default(),
            report: self.report.lock().unwrap().clone(),
            running: self.running.load(Ordering::Acquire),
            has_target: peer.target.is_some(),
            peer_link: peer.link.clone(),
            session: peer.session.clone(),
            outcome: peer.outcome.clone(),
            busy: peer.busy,
            completed: peer.node.as_ref().map(|node| node.completed_probes()),
            cancelled_websockets,
        }
    }

    pub fn dispatch(&mut self, intent: DiagnosticsIntent) {
        match intent {
            DiagnosticsIntent::RunChecks => self.run_checks(),
            DiagnosticsIntent::StartPeerTest => self.run_peer(),
            DiagnosticsIntent::StopPeerTest => self.stop_peer(),
        }
        self.notifier.notify();
    }

    /// Listening for incoming pings: the count changes without a notification.
    pub(super) fn listening(&self) -> bool {
        self.peer.lock().unwrap().node.is_some()
    }

    /// Forget any peer test and start from scratch, optionally aimed at `target`, or with
    /// an explanatory `outcome` already set.
    pub(super) fn reset(&mut self, target: Option<EndpointTicket>, outcome: Option<String>) {
        self.stop_peer();
        let mut peer = self.peer.lock().unwrap();
        *peer = Peer {
            generation: peer.generation,
            ..Peer::default()
        };
        if let Some(ticket) = target {
            peer.session = Some(DiagnosticNode::session_id(&ticket));
            peer.target = Some(ticket);
        }
        peer.outcome = outcome;
    }

    pub(super) fn run_checks(&mut self) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        *self.report.lock().unwrap() = None;
        let report = self.report.clone();
        let running = self.running.clone();
        let notifier = self.notifier.clone();
        wasm_bindgen_futures::spawn_local(async move {
            *report.lock().unwrap() = Some(crate::diagnostics::collect().await);
            running.store(false, Ordering::Release);
            notifier.notify();
        });
    }

    fn run_peer(&mut self) {
        let (target, generation) = {
            let mut peer = self.peer.lock().unwrap();
            if peer.busy || peer.node.is_some() {
                return;
            }
            peer.busy = true;
            peer.outcome = None;
            (peer.target.clone(), peer.generation)
        };
        let state = self.peer.clone();
        let notifier = self.notifier.clone();
        let relay = self.relay.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let outcome = match DiagnosticNode::bind(relay).await {
                Ok(node) => {
                    if let Some(ticket) = target {
                        let outcome = if node.probe(&ticket).await.is_ok() {
                            "Peer ping succeeded (iroh dial, stream and reply)".to_string()
                        } else {
                            "Peer ping failed or timed out (see terminal for network warnings)"
                                .to_string()
                        };
                        node.shutdown().await;
                        outcome
                    } else {
                        match node.ticket().await {
                            Ok(ticket) => {
                                let base = diagnostics_url().unwrap_or_default();
                                let link = format!("{base}#diagnostics={ticket}");
                                let node = Arc::new(node);
                                let stale = {
                                    let mut peer = state.lock().unwrap();
                                    if peer.generation == generation {
                                        peer.session = Some(DiagnosticNode::session_id(&ticket));
                                        peer.link = Some(link);
                                        peer.node = Some(node.clone());
                                        false
                                    } else {
                                        true
                                    }
                                };
                                if stale {
                                    node.shutdown().await;
                                    return;
                                }
                                "Listening for a diagnostic peer; keep this tab open".to_string()
                            }
                            Err(_) => {
                                node.shutdown().await;
                                "Could not register diagnostic endpoint with relay".to_string()
                            }
                        }
                    }
                }
                Err(_) => "Could not start diagnostic endpoint".to_string(),
            };
            let mut peer = state.lock().unwrap();
            if peer.generation == generation {
                peer.outcome = Some(outcome);
                peer.busy = false;
            }
            drop(peer);
            notifier.notify();
        });
    }

    pub(super) fn stop_peer(&mut self) {
        let mut peer = self.peer.lock().unwrap();
        peer.generation = peer.generation.wrapping_add(1);
        peer.busy = false;
        if let Some(node) = peer.node.take() {
            wasm_bindgen_futures::spawn_local(async move { node.shutdown().await });
        }
        peer.link = None;
        peer.outcome = Some("Stopped listening for diagnostic peers".into());
    }
}
