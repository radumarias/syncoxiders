use dioxus::prelude::*;
use p2p_transfer::sessions::Intent;

use super::{receive::ReceivedList, NavLink, PickButton, Pill};
use crate::core::use_core;
use crate::route::Route;

const STEPS: [(&str, &str); 3] = [
    ("1. Pick a file", "Choose a photo, video, or document."),
    (
        "2. Send the link",
        "Send it to the person you want to share with.",
    ),
    (
        "3. They save it",
        "They open your link and choose where to save it.",
    ),
];

#[component]
pub fn Home() -> Element {
    let core = use_core();
    let sharing = core.snap().read().send.sharing;
    rsx! {
        if sharing {
            ActiveShare {}
        }
        section { class: "card hero", aria_labelledby: "home-title",
            Pill { text: "NO ACCOUNT NEEDED" }
            h1 { id: "home-title", "Send files without cloud storage." }
            p { class: "lead", "Pick a file, send them a link, and they save it on their device." }
            p { "Your file is encrypted while it travels. Oxfer doesn't store a copy on a server or in the cloud." }
            div { class: "actions",
                PickButton { class: "wide", "Send files" }
                button {
                    r#type: "button",
                    class: "button outline wide",
                    onclick: move |_| core.dispatch(Intent::HostChat { for_agents: false }),
                    "Start a chat room"
                }
            }
            p { class: "muted",
                "Got a link? Open it to see the file and choose where to save it, or "
                NavLink { route: Route::Receive, "paste it here" }
                "."
            }
        }
        section { aria_labelledby: "steps-title",
            h2 { id: "steps-title", "From your device to theirs" }
            ol { class: "steps",
                for (title, body) in STEPS {
                    li { key: "{title}", class: "card step",
                        h3 { {title} }
                        p { {body} }
                    }
                }
            }
        }
        ReceivedList {}
    }
}

/// Sharing goes on while the user looks elsewhere; say so and offer the way back.
#[component]
pub fn ActiveShare() -> Element {
    rsx! {
        section { class: "card notice", aria_labelledby: "active-share",
            h2 { id: "active-share", "Your files are still available" }
            p { "People with your link can still download them." }
            NavLink { route: Route::Send, class: "button outline", "Back to sharing" }
        }
    }
}
