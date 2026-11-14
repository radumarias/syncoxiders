use dioxus::prelude::*;
use p2p_transfer::sessions::{DiagnosticsIntent, Intent};

use super::{home::ActiveShare, CopyButton};
use crate::core::use_core;

#[component]
pub fn Diags() -> Element {
    let core = use_core();
    let snap = core.snap();
    let snap = snap.read();
    let Some(diag) = snap.diagnostics.clone() else {
        return rsx! {};
    };
    let run = move |intent| move |_| core.dispatch(Intent::Diagnostics(intent));

    rsx! {
        if snap.send.sharing {
            ActiveShare {}
        }
        section { class: "card", aria_labelledby: "diag-title", aria_busy: diag.running || diag.busy,
            h1 { id: "diag-title", "Network diagnostics" }
            p { "Use the peer test on both devices, then copy each report. Compare their session IDs and timestamps." }
            if let Some(session) = &diag.session {
                p { class: "mono", "Session ID: {session}" }
            }
            if diag.has_target || (diag.peer_link.is_none() && diag.completed.is_none()) {
                button {
                    r#type: "button",
                    class: "button primary",
                    disabled: diag.busy,
                    onclick: run(DiagnosticsIntent::StartPeerTest),
                    if diag.has_target { "Test peer connection" } else { "Start peer test" }
                }
            }
            if let Some(peer_link) = &diag.peer_link {
                p { "Keep this page open and send this test-only link to the other device. It cannot download files." }
                p { class: "mono wrap", {peer_link.clone()} }
                div { class: "actions",
                    CopyButton { text: peer_link.clone(), label: "Copy peer test link", class: "outline" }
                    button { r#type: "button", class: "button outline", onclick: run(DiagnosticsIntent::StopPeerTest), "Stop peer test" }
                }
            }
            if diag.busy {
                p { role: "status", class: "busy", "Starting or dialling the diagnostic peer…" }
            }
            if let Some(outcome) = &diag.outcome {
                p { role: "status", {outcome.clone()} }
            }
            if let Some(completed) = diag.completed {
                p { "Successful incoming peer pings: {completed}" }
            }
            p { "For local checks only, share this link. It contains no peer address or file access code." }
            p { class: "mono wrap", {diag.local_link.clone()} }
            CopyButton { text: diag.local_link.clone(), label: "Copy local checks link", class: "outline" }
            if diag.running {
                p { role: "status", class: "busy", "Testing relay WebSockets and iroh registration (up to 60 seconds if the default fails)…" }
            } else {
                button { r#type: "button", class: "button outline", onclick: run(DiagnosticsIntent::RunChecks), "Run checks again" }
            }
            if let Some(full) = diag.full_report() {
                p { "Review before sharing: the report contains your browser version, time, relay checks and peer test result. It does not include tickets or files." }
                CopyButton { text: full.clone(), label: "Copy report", class: "outline" }
                pre { class: "report", {full} }
            }
        }
    }
}
