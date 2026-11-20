use dioxus::prelude::*;
use p2p_transfer::i18n::Msg;
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
            h1 { id: "diag-title", {Msg::NetworkDiagnostics.text()} }
            p { {Msg::DiagCompareReports.text()} }
            if let Some(session) = &diag.session {
                p { class: "mono", {Msg::DiagSessionId.with(&[("session", &session)])} }
            }
            if diag.has_target || (diag.peer_link.is_none() && diag.completed.is_none()) {
                button {
                    r#type: "button",
                    class: "button primary",
                    disabled: diag.busy,
                    onclick: run(DiagnosticsIntent::StartPeerTest),
                    if diag.has_target { {Msg::TestPeerConnection.text()} } else { {Msg::StartPeerTest.text()} }
                }
            }
            if let Some(peer_link) = &diag.peer_link {
                p { {Msg::DiagPeerTestInstructions.text()} }
                p { class: "mono wrap", {peer_link.clone()} }
                div { class: "actions",
                    CopyButton { text: peer_link.clone(), label: Msg::CopyPeerTestLink.text(), class: "outline" }
                    button { r#type: "button", class: "button outline", onclick: run(DiagnosticsIntent::StopPeerTest), {Msg::StopPeerTest.text()} }
                }
            }
            if diag.busy {
                p { role: "status", class: "busy", {Msg::DiagPeerStarting.text()} }
            }
            if let Some(outcome) = &diag.outcome {
                p { role: "status", {outcome.clone()} }
            }
            if let Some(completed) = diag.completed {
                p { {Msg::DiagIncomingPings.with(&[("completed", &completed)])} }
            }
            p { {Msg::DiagLocalLinkNote.text()} }
            p { class: "mono wrap", {diag.local_link.clone()} }
            CopyButton { text: diag.local_link.clone(), label: Msg::CopyLocalChecksLink.text(), class: "outline" }
            if diag.running {
                p { role: "status", class: "busy", {Msg::DiagRelayTesting.text()} }
            } else {
                button { r#type: "button", class: "button outline", onclick: run(DiagnosticsIntent::RunChecks), {Msg::RunChecksAgain.text()} }
            }
            if let Some(full) = diag.full_report() {
                p { {Msg::DiagReportReview.text()} }
                CopyButton { text: full.clone(), label: "Copy report", class: "outline" }
                pre { class: "report", {full} }
            }
        }
    }
}
