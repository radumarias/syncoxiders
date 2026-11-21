//! Browser URL intake: share and room links arrive as fragments, diagnostics as `/diags`.
//!
//! A fragment never reaches a server, but it does stay on the machine: URL bar, history,
//! session restore, a pasted screenshot. The ticket and the capability are removed from
//! the URL immediately; the QA flags stay so a reload keeps them (design §4.7.1).

use iroh_tickets::endpoint::EndpointTicket;
use wasm_bindgen::JsValue;

use super::{Active, Oxfer};
use crate::node::{Node, SinkPref};

impl Oxfer {
    /// Accept each new URL fragment (and, with `manage_history`, the `/diags` path),
    /// including navigation within the already-open page. [`Oxfer::tick`] calls it; a UI
    /// may also call it straight from `hashchange` / `popstate`.
    pub fn sync_location(&mut self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(hash) = window.location().hash() else {
            return;
        };
        let path = window.location().pathname().unwrap_or_default();
        let route = format!("{path}{hash}");
        if self.last_route.as_ref() == Some(&route) {
            return;
        }
        self.last_route = Some(route);
        if self.manage_history {
            if path.trim_end_matches('/') == "/diags" && hash.is_empty() {
                if !matches!(self.active, Active::Diagnostics) {
                    self.enter_diagnostics(None, None);
                }
                return;
            }
            if path != "/diags" && hash.is_empty() && matches!(self.active, Active::Diagnostics) {
                self.diagnostics.stop_peer();
                self.active = Active::Home;
                return;
            }
        }
        if self.last_hash.as_ref() == Some(&hash) {
            return;
        }
        self.last_hash = Some(hash.clone());
        self.accept_fragment(&window, &hash);
        self.deps.notifier.notify();
    }

    pub(super) fn open_diagnostics(&mut self) {
        if matches!(self.active, Active::Diagnostics) || self.is_preparing() {
            return;
        }
        if let Some(url) = super::diagnostics::diagnostics_url() {
            self.push_path(&url);
        }
        self.enter_diagnostics(None, None);
    }

    /// Push a same-origin history entry, when this core owns history.
    pub(super) fn push_path(&mut self, url: &str) {
        if !self.manage_history {
            return;
        }
        if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
            let _ = history.push_state_with_url(&JsValue::NULL, "", Some(url));
        }
        self.last_route = None;
        self.last_hash = None;
    }

    fn enter_diagnostics(&mut self, target: Option<EndpointTicket>, outcome: Option<String>) {
        self.diagnostics.reset(target, outcome);
        self.active = Active::Diagnostics;
        self.diagnostics.run_checks();
    }

    /// The location as the next [`Oxfer::sync_location`] will see it, so a `replaceState`
    /// (which fires no event) is not mistaken for a new navigation.
    fn remember_location(&mut self, window: &web_sys::Window) {
        let hash = window.location().hash().unwrap_or_default();
        self.last_route = Some(format!(
            "{}{hash}",
            window.location().pathname().unwrap_or_default(),
        ));
        self.last_hash = Some(hash);
    }

    fn accept_fragment(&mut self, window: &web_sys::Window, hash: &str) {
        let on_diags_path = window.location().pathname().ok().as_deref() == Some("/diags");
        if let Some(raw) = hash.strip_prefix("#diagnostics=") {
            // Remove the ephemeral peer ticket from browser history immediately.
            if let Ok(history) = window.history() {
                let href = window.location().href().unwrap_or_default();
                let base = href.split('#').next().unwrap_or(&href);
                let scrubbed = if on_diags_path {
                    base.to_string()
                } else {
                    format!("{base}#diagnostics")
                };
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&scrubbed));
            }
            self.remember_location(window);
            match raw.parse::<EndpointTicket>() {
                Ok(ticket) => self.enter_diagnostics(Some(ticket), None),
                Err(_) => self.enter_diagnostics(None, Some("Invalid diagnostics link".into())),
            }
            return;
        }
        if hash == "#diagnostics" {
            self.enter_diagnostics(None, None);
            return;
        }
        let params = Node::parse_fragment(hash);
        let carries_link = params.ticket.is_some()
            || params.cap.is_some()
            || params.chat
            || params.error.is_some();
        if on_diags_path && carries_link {
            // A transfer fragment on the diagnostics path is neither a peer diagnostic
            // ticket nor a valid diagnostics URL. Discard it rather than starting a receive
            // which the path router would cancel.
            if let Ok(history) = window.history() {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some("/diags"));
            }
            self.last_route = Some("/diags".into());
            self.last_hash = Some(String::new());
            self.enter_diagnostics(
                None,
                Some("This page runs diagnostics only; open file links on the home page".into()),
            );
            return;
        }
        if params.chat && params.ticket.is_none() && params.cap.is_none() && params.error.is_none()
        {
            // `#chat` (optionally `#chat&agent`) with no ticket: open a new room here. The
            // URL is kept as is, and `sync_location` already recorded it.
            self.host_chat(params.agent);
            return;
        }
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
        // replaceState does not fire hashchange. Remember the scrubbed value now so opening
        // the same original link again is still treated as a new navigation.
        self.remember_location(window);
        self.open_link(&href);
    }
}
