//! Receive (#68): paste or open a link, preview what is offered, save, follow the transfer
//! and see the verified result. One component tree for every width: the progress and file
//! cards sit side by side when the main area is wide enough (`@container frame-main`), and
//! on phones the main action is pinned above the tab bar (`var(--frame-bottom)`).
//!
//! The core folds a finished receive into `snap.received` and clears its progress, so the
//! verified summary is view state: the files that joined the list during this receive.

use std::rc::Rc;

use dioxus::prelude::*;
use p2p_transfer::protocol::FileMeta;
use p2p_transfer::sessions::text::{self, format_size};
use p2p_transfer::sessions::{Intent, ReceiveIntent, ReceivedFile, SaveTarget, WakeLock};
use p2p_transfer::transfer::{Phase, TransferProgress};
use wasm_bindgen::{closure::Closure, JsCast as _, JsValue};

use super::{ErrorText, Pill, Progress, TransferAttention};
use crate::core::{use_core, Core};

#[component]
pub fn Receive() -> Element {
    let core = use_core();
    let snap = core.snap();
    // (receive id, how many files were in the received list when it started). Files past
    // that count arrived during this receive and make up its verified summary.
    let mut seen = use_signal(|| {
        let snap = snap.peek();
        (snap.receive.as_ref().map(|r| r.id), snap.received.len())
    });
    // The manifest is in the snapshot only while waiting for Save; keep it per receive so
    // the per-file list survives into the transfer.
    let mut offered = use_signal(|| None::<(u64, Vec<FileMeta>)>);

    use_effect(move || {
        let snap = snap.read();
        let Some(receive) = snap.receive.as_ref() else {
            return;
        };
        if seen.peek().0 != Some(receive.id) {
            seen.set((Some(receive.id), snap.received.len()));
        }
        let known = offered.peek().as_ref().map(|(id, _)| *id) == Some(receive.id);
        if let (false, Some(manifest)) = (known, receive.manifest()) {
            offered.set(Some((receive.id, manifest.to_vec())));
        }
    });

    let snap = snap.read();
    let Some(receive) = snap.receive.clone() else {
        // The route effect is opening a receive; this frame has none yet.
        return rsx! {};
    };
    let receive_id = receive.id;
    let received_count = snap.received.len();
    let wake_lock = snap.wake_lock;
    let files: Vec<FileMeta> = offered()
        .filter(|(id, _)| *id == receive_id)
        .map(|(_, files)| files)
        .unwrap_or_default();
    let (seen_id, seen_count) = seen();
    let fresh: Vec<ReceivedFile> = if seen_id == Some(receive_id) {
        snap.received
            .get(seen_count..)
            .map(<[ReceivedFile]>::to_vec)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let (body, actions, idle) = match (&receive.progress, receive.opening) {
        (Some(progress), _) => match &progress.phase {
            Phase::AwaitingSave { manifest } => (
                rsx! {
                    Offer {
                        progress: progress.clone(),
                        manifest: manifest.clone(),
                        keep_local_copy: receive.keep_local_copy,
                        save_pending: receive.save_pending,
                        resuming: receive.resuming,
                        wake_lock,
                    }
                },
                rsx! {
                    SaveButton { keep_local_copy: receive.keep_local_copy, save_pending: receive.save_pending }
                    CancelButton {}
                },
                false,
            ),
            _ => (
                rsx! {
                    Transfer {
                        progress: progress.clone(),
                        files: files.clone(),
                        keep_local_copy: receive.keep_local_copy,
                        wake_lock,
                    }
                },
                rsx! { CancelButton {} },
                false,
            ),
        },
        (None, true) => (
            rsx! {
                section { class: "card receive-status", aria_labelledby: "receive-status-title",
                    h2 { id: "receive-status-title", class: "busy", role: "status", "Connecting to the sender…" }
                    p { class: "muted", "Looking for the device sharing this file. This can take a moment." }
                }
                TransferAttention { sender: false, awaiting_save: false, wake_lock }
            },
            rsx! { CancelButton {} },
            false,
        ),
        (None, false) if !fresh.is_empty() && receive.error.is_none() => (
            rsx! { Verified { files: fresh.clone() } },
            rsx! {
                button {
                    r#type: "button",
                    class: "button primary wide",
                    onclick: move |_| seen.set((Some(receive_id), received_count)),
                    "Done"
                }
            },
            false,
        ),
        (None, false) => (
            rsx! {
                div { class: "card receive-link",
                    LinkForm { input: receive.input.as_str().to_string() }
                    p { class: "muted small receive-tip",
                        "Tip: press Ctrl+V (⌘V on a Mac) anywhere on this page to open a copied link."
                    }
                }
            },
            rsx! {},
            true,
        ),
    };

    rsx! {
        div { class: "receive",
            div { class: "title-row receive-head",
                h1 { id: "receive-title", "Receive on this device" }
                Pill { text: "ENCRYPTED" }
            }
            {body}
            if !receive.flags.is_empty() {
                p { class: "meta", {receive.flags.clone()} }
            }
            if let Some(error) = &receive.error {
                ErrorText { text: text::humanize(error) }
            }
            if !idle {
                div { class: "receive-actions", {actions} }
            }
            if idle {
                ReceivedList {}
            }
        }
    }
}

/// "3 files · 5.2 GB".
fn files_summary(count: usize, total: u64) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!("{count} {noun} · {}", format_size(total))
}

/// Remaining time at the current speed, only while bytes are moving.
fn time_left(progress: &TransferProgress) -> Option<String> {
    let moving = matches!(progress.phase, Phase::Transferring)
        && progress.bytes_per_sec.is_finite()
        && progress.bytes_per_sec > 0.0
        && progress.bytes_total > progress.bytes_done;
    if !moving {
        return None;
    }
    let left = (progress.bytes_total - progress.bytes_done) as f64 / progress.bytes_per_sec;
    let seconds = left.ceil() as u64;
    Some(match seconds {
        0..=59 => format!("about {seconds} s left"),
        60..=3599 => format!("about {} min left", seconds.div_ceil(60)),
        _ => format!(
            "about {} h {} min left",
            seconds / 3600,
            (seconds % 3600) / 60
        ),
    })
}

/// Phases where nothing visible moves for a while, so a spinner shows work is going on.
fn is_waiting(phase: &Phase) -> bool {
    matches!(
        phase,
        Phase::Connecting
            | Phase::Handshake
            | Phase::Signaling
            | Phase::Switching
            | Phase::Verifying
    )
}

#[component]
fn LinkForm(input: String) -> Element {
    let core = use_core();
    use_paste_anywhere(core);
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

/// Ctrl/⌘+V anywhere on the screen opens a copied link (#68). Pastes into a text field are
/// left to the field. The text goes straight to the core, which validates the link and
/// scrubs the capability; it is never logged or shown here.
fn use_paste_anywhere(core: Core) {
    let listener = use_hook(move || {
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(link) = pasted_link(&event) {
                event.prevent_default();
                core.dispatch(Intent::OpenLink(link));
            }
        });
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document
                .add_event_listener_with_callback("paste", closure.as_ref().unchecked_ref());
        }
        Rc::new(closure)
    });
    use_drop(move || {
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let _ = document
                .remove_event_listener_with_callback("paste", (*listener).as_ref().unchecked_ref());
        }
    });
}

/// The pasted text, unless the paste is aimed at something editable.
fn pasted_link(event: &web_sys::Event) -> Option<String> {
    if let Some(target) = event
        .target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let tag = target.tag_name();
        if tag.eq_ignore_ascii_case("input")
            || tag.eq_ignore_ascii_case("textarea")
            || target.is_content_editable()
        {
            return None;
        }
    }
    // `clipboardData` read by name: it saves enabling another web-sys feature.
    let data = js_sys::Reflect::get(event, &JsValue::from_str("clipboardData")).ok()?;
    let text = data
        .dyn_into::<web_sys::DataTransfer>()
        .ok()?
        .get_data("text")
        .ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// What the sender offers, before anything is written.
#[component]
fn Offer(
    progress: TransferProgress,
    manifest: Vec<FileMeta>,
    keep_local_copy: bool,
    save_pending: bool,
    resuming: bool,
    wake_lock: WakeLock,
) -> Element {
    let core = use_core();
    let total: u64 = manifest.iter().map(|meta| meta.size).sum();
    rsx! {
        section { class: "card receive-offer", aria_labelledby: "receive-offer-title",
            p { class: "phase", role: "status", {text::phase_text(&progress.phase)} }
            h2 { id: "receive-offer-title", {files_summary(manifest.len(), total)} }
            ul { class: "files receive-files", aria_label: "Files offered",
                for (index, meta) in manifest.iter().enumerate() {
                    li { key: "{index}", class: "file receive-file",
                        span { class: "file-name", {meta.name.clone()} }
                        span { class: "meta", {format_size(meta.size)} }
                    }
                }
            }
            p { class: "muted small", "Oxfer checks the file after it's saved." }
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
            if save_pending {
                p { role: "status", class: "busy",
                    if resuming { "Checking your saved copy…" } else { "Opening your save location…" }
                }
            }
        }
        TransferAttention { sender: false, awaiting_save: !save_pending, wake_lock }
    }
}

#[component]
fn SaveButton(keep_local_copy: bool, save_pending: bool) -> Element {
    let core = use_core();
    let label = if keep_local_copy {
        "Save a copy in this browser"
    } else {
        "Choose where to save"
    };
    rsx! {
        button {
            r#type: "button",
            class: "button primary wide",
            disabled: save_pending,
            // Dispatched inside the click: the save picker needs this user gesture.
            onclick: move |_| core.dispatch(Intent::Receive(ReceiveIntent::Save(SaveTarget::Browser))),
            {label}
        }
    }
}

/// Total and per-file progress, speed and time left, retry attempts.
#[component]
fn Transfer(
    progress: TransferProgress,
    files: Vec<FileMeta>,
    keep_local_copy: bool,
    wake_lock: WakeLock,
) -> Element {
    let reconnecting = match progress.phase {
        Phase::Reconnecting {
            attempt,
            max_attempts,
        } => Some((attempt, max_attempts)),
        _ => None,
    };
    let total: u64 = files.iter().map(|meta| meta.size).sum();
    let summary = if files.is_empty() {
        "Receiving".to_string()
    } else {
        files_summary(files.len(), total)
    };
    let percent = (progress.bytes_total > 0)
        .then(|| progress.bytes_done.saturating_mul(100) / progress.bytes_total);
    let fraction = if progress.bytes_total > 0 {
        progress.bytes_done as f32 / progress.bytes_total as f32
    } else {
        0.0
    };
    let rate_line = match reconnecting {
        Some((attempt, max_attempts)) => Some(format!("attempt {attempt} of {max_attempts}")),
        None => text::transfer_speed(&progress).map(|speed| match time_left(&progress) {
            Some(left) => format!("{speed} · {left}"),
            None => speed,
        }),
    };
    let rows: Vec<(String, u64, FileState)> = files
        .iter()
        .enumerate()
        .map(|(index, meta)| (meta.name.clone(), meta.size, file_state(index, &progress)))
        .collect();

    rsx! {
        div { class: if files.is_empty() { "receive-body" } else { "receive-body receive-split" },
            section { class: "card receive-progress", aria_labelledby: "receive-progress-title",
                h2 { id: "receive-progress-title", class: "receive-summary", {summary} }
                if let Some(percent) = percent {
                    div { class: "receive-percent",
                        span { class: "receive-percent-value", "{percent}%" }
                        if reconnecting.is_some() {
                            span { class: "receive-paused", "paused" }
                        }
                    }
                    div { class: if reconnecting.is_some() { "receive-bar paused" } else { "receive-bar" },
                        Progress { fraction, label: "Download progress" }
                    }
                    div { class: "receive-numbers meta",
                        span { "{format_size(progress.bytes_done)} of {format_size(progress.bytes_total)}" }
                        if let Some(line) = rate_line {
                            span { {line} }
                        }
                    }
                }
                p {
                    class: if is_waiting(&progress.phase) { "phase busy" } else { "phase" },
                    role: "status",
                    {text::phase_text(&progress.phase)}
                }
                StatusPills { progress: progress.clone(), wake_lock }
            }
            if !rows.is_empty() {
                section { class: "card receive-files-card", aria_labelledby: "receive-files-title",
                    h2 { id: "receive-files-title", "Files" }
                    ul { class: "files receive-files",
                        for (index, (name, size, state)) in rows.into_iter().enumerate() {
                            FileRow { key: "{index}", name, size, state }
                        }
                    }
                }
            }
        }
        if let Some((attempt, max_attempts)) = reconnecting {
            section { class: "card receive-notice", aria_labelledby: "receive-notice-title",
                h2 { id: "receive-notice-title", "Connection lost, trying again" }
                p { "Trying again ({attempt} of {max_attempts}). Keep the sender's tab open. We'll try to continue where we stopped." }
            }
            section { class: "card receive-help", aria_labelledby: "receive-help-title",
                h2 { id: "receive-help-title", "If the sender closed their tab" }
                p { class: "muted",
                    if keep_local_copy {
                        "Ask them to reopen Oxfer, add the same files and send you a fresh link. Your copy in this browser continues where it stopped once the files match."
                    } else {
                        "Ask them to reopen Oxfer, add the same files and send you a fresh link, then open it here."
                    }
                }
            }
        }
        TransferAttention { sender: false, awaiting_save: false, wake_lock }
    }
}

/// Where one offered file is in the transfer.
#[derive(Clone, Copy, PartialEq)]
enum FileState {
    Waiting,
    Queued,
    Active { done: u64, total: u64 },
    Received,
    Checking,
}

/// Files arrive in manifest order, so everything before the current file is in.
fn file_state(index: usize, progress: &TransferProgress) -> FileState {
    if matches!(progress.phase, Phase::Verifying) {
        return FileState::Checking;
    }
    match progress.file_index.map(|current| current as usize) {
        Some(current) if index < current => FileState::Received,
        Some(current) if index == current => FileState::Active {
            done: progress.file_done,
            total: progress.file_total,
        },
        Some(_) => FileState::Queued,
        None => FileState::Waiting,
    }
}

#[component]
fn FileRow(name: String, size: u64, state: FileState) -> Element {
    let detail = match state {
        FileState::Active { done, total } if total > 0 => rsx! {
            Progress { fraction: done as f32 / total as f32, label: format!("{name} progress") }
            span { class: "receive-file-state", "{format_size(done)} of {format_size(total)}" }
        },
        FileState::Active { .. } => rsx! { span { class: "receive-file-state", "Receiving" } },
        FileState::Received => rsx! { span { class: "receive-file-state ok", "Received" } },
        FileState::Checking => rsx! { span { class: "receive-file-state", "Checking…" } },
        FileState::Queued => rsx! { span { class: "receive-file-state", "Queued" } },
        FileState::Waiting => rsx! { span { class: "receive-file-state", "Waiting" } },
    };
    rsx! {
        li { class: "file receive-file",
            span { class: "file-name", {name.clone()} }
            span { class: "meta", {format_size(size)} }
            {detail}
        }
    }
}

/// The always-visible status line: connection path, wake lock, encryption and checking.
#[component]
fn StatusPills(progress: TransferProgress, wake_lock: WakeLock) -> Element {
    let path_tone = if matches!(progress.phase, Phase::Reconnecting { .. }) {
        "warn"
    } else {
        "ok"
    };
    let (wake, wake_tone) = match wake_lock {
        WakeLock::Active => ("Awake", "ok"),
        WakeLock::Requesting => ("Waking the screen…", "muted"),
        _ => ("Screen may sleep", "warn"),
    };
    rsx! {
        ul { class: "receive-pills", aria_label: "Transfer status",
            li { span { class: "pill receive-pill {path_tone}", {text::path_badge(&progress.phase, progress.path)} } }
            li { span { class: "pill receive-pill {wake_tone}", {wake} } }
            li { span { class: "pill receive-pill", "E2E · BLAKE3" } }
        }
    }
}

/// The verified result of the receive that just finished.
#[component]
fn Verified(files: Vec<ReceivedFile>) -> Element {
    let total: u64 = files.iter().map(|file| file.size).sum();
    let title = if files.len() == 1 {
        "1 file received".to_string()
    } else {
        format!("{} files received", files.len())
    };
    rsx! {
        section { class: "card receive-done", aria_labelledby: "receive-done-title",
            span { class: "receive-done-mark", aria_hidden: "true",
                svg {
                    view_box: "0 0 24 24",
                    width: "64",
                    height: "64",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "1.8",
                    stroke_linecap: "round",
                    stroke_linejoin: "round",
                    circle { cx: "12", cy: "12", r: "10" }
                    path { d: "m7.5 12.5 3 3 6-6.5" }
                }
            }
            div { role: "status",
                h2 { id: "receive-done-title", {title} }
                p { class: "muted", "{format_size(total)} · every file matched its BLAKE3 hash" }
            }
            span { class: "pill receive-pill ok", "Verified" }
            ul { class: "files receive-files", aria_label: "Files received",
                for (index, file) in files.iter().enumerate() {
                    li { key: "{index}", class: "file receive-file",
                        span { class: "file-name", {file.name.clone()} }
                        span { class: "meta", {format_size(file.size)} }
                        span { class: "meta receive-file-where", {file.location.clone()} }
                    }
                }
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
            class: "button outline wide",
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
