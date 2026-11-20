use dioxus::prelude::*;
use p2p_transfer::i18n::Msg;
use p2p_transfer::sessions::text::format_size;
use p2p_transfer::sessions::{CopiesIntent, Intent};

use super::{ErrorText, NavLink};
use crate::core::use_core;
use crate::route::Route;

/// Copies kept in this browser's private storage (OPFS) for resumable downloads.
#[component]
pub fn Copies() -> Element {
    let core = use_core();
    let snap = core.snap();
    let snap = snap.read();
    let copies = &snap.copies;
    let receiving = snap.receive.as_ref().is_some_and(|r| r.active());
    let locked = copies.busy || receiving;
    let mut confirm = use_signal(|| None::<String>);

    rsx! {
        section { class: "card", aria_labelledby: "copies-title", aria_busy: copies.busy,
            div { class: "title-row",
                h1 { id: "copies-title", {Msg::SavedFilesInThisBrowser.text()} }
                button {
                    r#type: "button",
                    class: "button outline small",
                    disabled: copies.busy,
                    onclick: move |_| core.dispatch(Intent::Copies(CopiesIntent::Refresh)),
                    "Refresh"
                }
            }
            p { class: "muted",
                "To continue an unfinished download, open the sender's link again. If they closed the app, ask for a new link to the same files. These copies stay in this browser, not on a server. They may disappear if browser data is cleared or space runs low. Links aren't saved here."
            }
            if copies.entries.is_empty() && !copies.busy {
                p { "Nothing is saved in this browser." }
                NavLink { route: Route::Receive, class: "button outline", "Receive a file" }
            }
            ul { class: "copies",
                for entry in copies.entries.iter() {
                    li { key: "{entry.id}", class: "copy",
                        ul { class: "files",
                            for (index, file) in entry.files.iter().enumerate() {
                                li { key: "{index}", class: "file",
                                    span { class: "file-name", {file.name.clone()} }
                                    span { class: "meta", "{format_size(file.written)} / {format_size(file.size)}" }
                                    if file.verified {
                                        span { class: "pill ok", {Msg::ReadyToSave.text()} }
                                        button {
                                            r#type: "button",
                                            class: "button outline small",
                                            disabled: locked,
                                            onclick: {
                                                let id = entry.id.clone();
                                                move |_| core.dispatch(Intent::Copies(CopiesIntent::Export(id.clone(), index)))
                                            },
                                            {Msg::DownloadFile.text()}
                                        }
                                    } else {
                                        span { class: "pill", "UNFINISHED" }
                                    }
                                }
                            }
                        }
                        div { class: "actions",
                            if entry.files.iter().any(|f| !f.verified) {
                                button {
                                    r#type: "button",
                                    class: "button outline",
                                    disabled: locked,
                                    onclick: {
                                        let id = entry.id.clone();
                                        move |_| core.dispatch(Intent::ResumeLocalCopy(id.clone()))
                                    },
                                    {Msg::ContinueDownload.text()}
                                }
                            }
                            if confirm().as_deref() == Some(entry.id.as_str()) {
                                span { role: "alert", {Msg::CopiesDeleteConfirm.text()} }
                                button {
                                    r#type: "button",
                                    class: "button outline danger",
                                    disabled: locked,
                                    onclick: {
                                        let id = entry.id.clone();
                                        move |_| {
                                            core.dispatch(Intent::Copies(CopiesIntent::Discard(id.clone())));
                                            confirm.set(None);
                                        }
                                    },
                                    {Msg::DeletePermanently.text()}
                                }
                                button { r#type: "button", class: "button outline", onclick: move |_| confirm.set(None), "Keep" }
                            } else {
                                button {
                                    r#type: "button",
                                    class: "button outline",
                                    disabled: locked,
                                    onclick: {
                                        let id = entry.id.clone();
                                        move |_| confirm.set(Some(id.clone()))
                                    },
                                    {Msg::DeleteSavedCopy.text()}
                                }
                            }
                        }
                    }
                }
            }
            if let Some(error) = &copies.error {
                ErrorText { text: error.clone() }
            }
        }
    }
}
