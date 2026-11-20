use dioxus::prelude::*;
use p2p_transfer::i18n::Msg;
use p2p_transfer::sessions::text::{self, format_size, LINK_WARNING};
use p2p_transfer::sessions::{Intent, LinkState, SendIntent};

use super::{CopyButton, ErrorText, PickButton, Pill, Progress, TransferAttention};
use crate::core::use_core;

#[component]
pub fn Send() -> Element {
    let core = use_core();
    let snap = core.snap();
    let snap = snap.read();
    let send = &snap.send;
    rsx! {
        section { class: "card", aria_labelledby: "send-title",
            div { class: "title-row",
                h1 { id: "send-title", "Send a file" }
                Pill { text: "ENCRYPTED" }
            }
            if send.serving() {
                TransferAttention { sender: true, awaiting_save: false, wake_lock: snap.wake_lock }
            }
            if send.files.is_empty() && send.preparing.is_empty() {
                p { "Pick one or more files. You get a link to send to the person you choose." }
                p { class: "muted", "You can also drop files anywhere on this page." }
                PickButton { class: "wide", "Choose files" }
            }
            if !send.files.is_empty() {
                ul { class: "files", aria_label: "Shared files",
                    for (index, file) in send.files.iter().enumerate() {
                        li { key: "{index}", class: "file",
                            span { class: "file-name", {file.name.clone()} }
                            span { class: "meta", {format_size(file.size)} }
                        }
                    }
                }
            }
            for (index, preparing) in send.preparing.iter().enumerate() {
                div { key: "p{index}", class: "preparing",
                    p { "Preparing {preparing.name}…" }
                    Progress { fraction: preparing.progress, label: format!("Preparing {}", preparing.name) }
                }
            }
            match &send.link {
                LinkState::Ready(link) => rsx! {
                    div { class: "link-box",
                        label { r#for: "share-link", {Msg::SendThisLink.text()} }
                        input { id: "share-link", class: "mono", readonly: true, value: link.as_str() }
                        div { class: "actions",
                            CopyButton { text: link.as_str().to_string(), class: "primary" }
                            PickButton { class: "outline", "Add files" }
                        }
                        p { class: "muted", {Msg::SendLinkHintChosen.text()} }
                        p { class: "warn", {LINK_WARNING} }
                    }
                },
                LinkState::Preparing | LinkState::Binding => rsx! {
                    p { role: "status", class: "busy", {Msg::GettingYourLinkReady.text()} }
                },
                LinkState::Failed => rsx! {
                    button {
                        r#type: "button",
                        class: "button primary",
                        onclick: move |_| core.dispatch(Intent::Send(SendIntent::RetrySharing)),
                        "Try again"
                    }
                },
                LinkState::Idle => rsx! {},
            }
            if let Some(error) = &send.error {
                ErrorText { text: text::humanize(error) }
            }
            if send.sharing {
                button {
                    r#type: "button",
                    class: "button outline danger",
                    onclick: move |_| core.dispatch(Intent::Send(SendIntent::StopSharing)),
                    "Stop sharing"
                }
            }
        }
        if !send.peers.is_empty() {
            section { class: "card", aria_labelledby: "peers-title",
                h2 { id: "peers-title", "File activity ({send.peers.len()})" }
                ul { class: "peers",
                    for peer in send.peers.iter() {
                        li { key: "{peer.id}", class: "peer",
                            div { class: "title-row",
                                span { class: "mono", {text::short_id(&peer.id).to_string()} }
                                span { {text::phase_text(&peer.progress.phase)} }
                                span { class: "pill", {text::path_badge(&peer.progress.phase, peer.progress.path)} }
                                if let Some(speed) = text::transfer_speed(&peer.progress) {
                                    span { class: "meta", {speed} }
                                }
                            }
                            if peer.progress.bytes_total > 0 {
                                Progress {
                                    fraction: peer.progress.bytes_done as f32 / peer.progress.bytes_total as f32,
                                    label: format!("Transfer to {}", text::short_id(&peer.id)),
                                }
                            }
                            if let Some(error) = &peer.progress.error {
                                ErrorText { text: text::humanize(error) }
                            }
                        }
                    }
                }
            }
        }
    }
}
