use dioxus::prelude::*;
use p2p_transfer::sessions::text::{self, format_size};
use p2p_transfer::sessions::{Intent, ReceiveIntent, SaveTarget};
use p2p_transfer::transfer::Phase;

use super::{ErrorText, Pill, Progress, TransferAttention};
use crate::core::use_core;

#[component]
pub fn Receive() -> Element {
    let core = use_core();
    let snap = core.snap();
    let snap = snap.read();
    let Some(receive) = snap.receive.clone() else {
        // The route effect is opening a receive; this frame has none yet.
        return rsx! {};
    };
    let progress = receive.progress.clone();
    let running = receive.opening || progress.as_ref().is_some_and(|p| !p.phase.is_terminal());
    let awaiting_save = progress
        .as_ref()
        .is_some_and(|p| matches!(p.phase, Phase::AwaitingSave { .. }))
        && !receive.save_pending;

    rsx! {
        section { class: "card", aria_labelledby: "receive-title",
            div { class: "title-row",
                h1 { id: "receive-title", "Receive on this device" }
                Pill { text: "ENCRYPTED" }
            }
            if running {
                TransferAttention { sender: false, awaiting_save, wake_lock: snap.wake_lock }
            }
            match (&progress, receive.opening) {
                (None, true) => rsx! {
                    p { role: "status", class: "busy", "Connecting to the sender…" }
                    p { class: "muted", "Looking for the device sharing this file. This can take a moment." }
                    CancelButton {}
                },
                (None, false) => rsx! { LinkForm { input: receive.input.as_str().to_string() } },
                (Some(p), _) => rsx! {
                    div { class: "title-row", role: "status",
                        span { class: "phase", {text::phase_text(&p.phase)} }
                        span { class: "pill", {text::path_badge(&p.phase, p.path)} }
                        if let Some(speed) = text::transfer_speed(p) {
                            span { class: "meta", {speed} }
                        }
                    }
                    if let Phase::Reconnecting { attempt, max_attempts } = p.phase {
                        p { class: "muted", "Trying again ({attempt} of {max_attempts}). Keep the sender's tab open. We'll try to continue where we stopped." }
                    }
                    if let Phase::AwaitingSave { manifest } = &p.phase {
                        ul { class: "files", aria_label: "Files offered",
                            for (index, meta) in manifest.iter().enumerate() {
                                li { key: "{index}", class: "file",
                                    span { class: "file-name", {meta.name.clone()} }
                                    span { class: "meta", {format_size(meta.size)} }
                                }
                            }
                        }
                        p { "Oxfer checks the file after it's saved." }
                        SaveControls { keep_local_copy: receive.keep_local_copy, save_pending: receive.save_pending, resuming: receive.resuming }
                    } else if p.bytes_total > 0 {
                        Progress { fraction: p.bytes_done as f32 / p.bytes_total as f32, label: "Download progress" }
                        p { class: "meta", "{format_size(p.bytes_done)} of {format_size(p.bytes_total)}" }
                    }
                    if !p.phase.is_terminal() {
                        CancelButton {}
                    } else {
                        button {
                            r#type: "button",
                            class: "button outline",
                            onclick: move |_| core.dispatch(Intent::OpenReceive),
                            "Open another link"
                        }
                    }
                },
            }
            if !receive.flags.is_empty() {
                p { class: "meta", {receive.flags.clone()} }
            }
            if let Some(error) = &receive.error {
                ErrorText { text: text::humanize(error) }
            }
        }
        ReceivedList {}
    }
}

#[component]
fn LinkForm(input: String) -> Element {
    let core = use_core();
    rsx! {
        form {
            class: "link-form",
            onsubmit: move |event| {
                event.prevent_default();
                core.dispatch(Intent::Receive(ReceiveIntent::OpenInput));
            },
            label { r#for: "receive-link", "Paste the full link someone sent you." }
            input {
                id: "receive-link",
                class: "mono",
                r#type: "url",
                autocomplete: "off",
                spellcheck: "false",
                placeholder: "Paste a link here",
                value: input,
                oninput: move |event| core.dispatch(Intent::Receive(ReceiveIntent::SetInput(event.value()))),
            }
            button { r#type: "submit", class: "button primary", "Open link" }
        }
    }
}

#[component]
fn SaveControls(keep_local_copy: bool, save_pending: bool, resuming: bool) -> Element {
    let core = use_core();
    let label = if keep_local_copy {
        "Save a copy in this browser"
    } else {
        "Choose where to save"
    };
    rsx! {
        label { class: "check",
            input {
                r#type: "checkbox",
                checked: keep_local_copy,
                disabled: save_pending,
                onchange: move |event| core.dispatch(Intent::Receive(ReceiveIntent::SetKeepLocalCopy(event.checked()))),
            }
            "Keep a copy here so I can continue later"
        }
        if keep_local_copy {
            p { class: "muted small",
                "This copy is stored in your browser, not in Downloads yet. If the transfer stops, reopen the sender's link to continue. Download the finished file below. Clearing browser data, private browsing, or running low on storage may remove it."
            }
        }
        button {
            r#type: "button",
            class: "button primary wide",
            disabled: save_pending,
            // Dispatched inside the click: the save picker needs this user gesture.
            onclick: move |_| core.dispatch(Intent::Receive(ReceiveIntent::Save(SaveTarget::Browser))),
            {label}
        }
        if save_pending {
            p { role: "status", class: "busy",
                if resuming { "Checking your saved copy…" } else { "Opening your save location…" }
            }
        }
    }
}

#[component]
fn CancelButton() -> Element {
    let core = use_core();
    rsx! {
        button {
            r#type: "button",
            class: "button outline",
            onclick: move |_| core.dispatch(Intent::Receive(ReceiveIntent::Cancel)),
            "Cancel"
        }
    }
}

/// Files saved during this visit.
#[component]
pub fn ReceivedList() -> Element {
    let core = use_core();
    let snap = core.snap();
    let snap = snap.read();
    if snap.received.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "card", aria_labelledby: "received-title",
            h2 { id: "received-title", "Received ({snap.received.len()})" }
            ul { class: "files",
                for (index, file) in snap.received.iter().enumerate() {
                    li { key: "{index}", class: "file received",
                        span { class: "file-name", {file.name.clone()} }
                        span { class: "meta", "{format_size(file.size)} · {file.when}" }
                        span { class: "meta", {file.location.clone()} }
                        if let Some(rate) = file.average_bytes_per_sec {
                            span { class: "meta", "Avg download (this session): {text::format_rate(rate)}" }
                        }
                        span { class: "meta", {text::transfer_path_text(file.path)} }
                    }
                }
            }
        }
    }
}
