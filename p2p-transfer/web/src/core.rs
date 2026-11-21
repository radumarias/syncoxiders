//! The session core as Dioxus state.
//!
//! [`Core`] is `Copy`: grab it with [`use_core`] in any component. Read the view model
//! with [`Core::snap`] (a signal, so reading it subscribes the component) and act with
//! [`Core::dispatch`]. A background loop refreshes the snapshot whenever the core's
//! [`Notifier`](p2p_transfer::sessions::Notifier) fires, and at ~10 Hz while
//! `Snapshot::live` asks for polling.

use std::pin::pin;

use dioxus::prelude::*;
use futures_util::future::select;
use p2p_transfer::sessions::{Config, CopiesIntent, Intent, Oxfer, PickQueue, Snapshot};
use wasm_bindgen::{closure::Closure, JsCast as _};

/// Polling interval while engine progress moves without notifications.
const LIVE_POLL_MS: u32 = 100;

#[derive(Clone, Copy)]
pub struct Core {
    oxfer: CopyValue<Oxfer>,
    snap: Signal<Snapshot>,
}

impl Core {
    /// The current view model. Reading it subscribes the calling component.
    pub fn snap(&self) -> Signal<Snapshot> {
        self.snap
    }

    /// Hand an intent to the core and refresh the snapshot in the same event, so a
    /// handler that needs transient user activation (Save, the file picker) keeps it.
    pub fn dispatch(&self, intent: Intent) {
        self.oxfer.write_unchecked().dispatch(intent);
        self.refresh();
    }

    /// Fold in finished background work and publish a new snapshot if it changed.
    pub fn refresh(&self) {
        let snapshot = {
            let mut oxfer = self.oxfer.write_unchecked();
            oxfer.tick();
            oxfer.snapshot()
        };
        let mut snap = self.snap;
        if *snap.peek() != snapshot {
            snap.set(snapshot);
        }
    }

    /// Where file pickers and drops queue files; the next refresh starts sharing them.
    pub fn picks(&self) -> PickQueue {
        self.oxfer.read_unchecked().picks()
    }

    /// Report a user-facing error the core should show (e.g. the picker failed).
    pub fn report(&self, message: String) {
        self.oxfer.read_unchecked().report(message);
        self.refresh();
    }
}

pub fn use_core() -> Core {
    use_context::<Core>()
}

/// Create the core once for the app, provide it as context and start its refresh loop.
pub fn use_core_provider() -> Core {
    let core = use_context_provider(|| {
        // This UI owns history through its own router; the core still reads and scrubs
        // the fragment (share links) on every refresh.
        let oxfer = Oxfer::new(Config {
            manage_history: false,
            ..Config::for_this_target()
        });
        let snap = Signal::new(oxfer.snapshot());
        Core {
            oxfer: CopyValue::new(oxfer),
            snap,
        }
    });
    use_hook(move || {
        let notifier = core.oxfer.read_unchecked().notifier().clone();
        let mut changes = notifier.subscribe();
        // A pasted or clicked link changes only the fragment: wake the loop, which
        // refreshes, and the core's tick reads the new fragment.
        let on_hash = Closure::<dyn FnMut(web_sys::Event)>::new({
            let notifier = notifier.clone();
            move |_| notifier.notify()
        });
        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("hashchange", on_hash.as_ref().unchecked_ref());
        }
        // The listener lives as long as the page.
        on_hash.forget();
        core.dispatch(Intent::Copies(CopiesIntent::Refresh));
        spawn(async move {
            loop {
                core.refresh();
                if core.snap.peek().live {
                    let timeout = gloo_timers::future::TimeoutFuture::new(LIVE_POLL_MS);
                    let _ = select(pin!(changes.changed()), timeout).await;
                } else if changes.changed().await.is_err() {
                    break;
                }
            }
        });
    });
    core
}
