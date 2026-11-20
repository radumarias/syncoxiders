use dioxus::prelude::*;
use p2p_transfer::i18n::Msg;
use p2p_transfer::sessions::Intent;

use super::{receive::ReceivedList, NavLink, PickButton, Pill};
use crate::core::use_core;
use crate::route::Route;

fn steps() -> [(&'static str, &'static str); 3] {
    [
        ("1. Pick a file", Msg::HomeStoryChoose.text()),
        ("2. Send the link", Msg::SendLinkHintShare.text()),
        ("3. They save it", Msg::HomeStoryTheySave.text()),
    ]
}

#[component]
pub fn Home() -> Element {
    let core = use_core();
    let sharing = core.snap().read().send.sharing;
    rsx! {
        if sharing {
            ActiveShare {}
        }
        section { class: "card hero", aria_labelledby: "home-title",
            Pill { text: Msg::NoAccountNeeded.text() }
            h1 { id: "home-title", {Msg::SendFilesWithoutCloudStorage.text()} }
            p { class: "lead", {Msg::HomeStoryPickSend.text()} }
            p { "Your file is encrypted while it travels. Oxfer doesn't store a copy on a server or in the cloud." }
            div { class: "actions",
                PickButton { class: "wide", "Send files" }
                button {
                    r#type: "button",
                    class: "button outline wide",
                    onclick: move |_| core.dispatch(Intent::HostChat { for_agents: false }),
                    {Msg::StartAChatRoom.text()}
                }
            }
            p { class: "muted",
                "Got a link? Open it to see the file and choose where to save it, or "
                NavLink { route: Route::Receive, "paste it here" }
                "."
            }
        }
        section { aria_labelledby: "steps-title",
            h2 { id: "steps-title", {Msg::HomeStoryDirect.text()} }
            ol { class: "steps",
                for (title, body) in steps() {
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
            h2 { id: "active-share", {Msg::SendStillAvailable.text()} }
            p { "People with your link can still download them." }
            NavLink { route: Route::Send, class: "button outline", "Back to sharing" }
        }
    }
}
