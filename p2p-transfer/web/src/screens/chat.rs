use dioxus::prelude::*;
use p2p_transfer::i18n::Msg;
use p2p_transfer::sessions::chat::agent_prompt;
use p2p_transfer::sessions::{ChatIntent, ChatRole, ChatStatus, Intent};

use super::{CopyButton, ErrorText, Pill};
use p2p_transfer::theme::member_slot;

use crate::appearance::use_appearance;
use crate::core::use_core;

#[component]
pub fn Chat() -> Element {
    let core = use_core();
    let snap = core.snap();
    let mut draft = use_signal(String::new);
    let theme = *use_appearance().theme.read();
    // A different room starts with an empty draft.
    let room = snap.read().chat.as_ref().map(|chat| chat.id);
    use_effect(use_reactive!(|room| {
        let _ = room;
        draft.set(String::new());
    }));
    let Some(chat) = snap.read().chat.clone() else {
        return rsx! { StartRoom {} };
    };
    let host = chat.role == ChatRole::Host;

    rsx! {
        section { class: "card chat", aria_labelledby: "chat-title",
            div { class: "title-row",
                h1 { id: "chat-title", "Chat room" }
                Pill { text: "ENCRYPTED" }
            }
            match &chat.status {
                ChatStatus::Starting => rsx! {
                    p { role: "status", class: "busy", if host { {Msg::ChatOpeningRoom.text()} } else { {Msg::ChatConnectingRoom.text()} } }
                },
                ChatStatus::Open => rsx! {
                    p { class: "muted", if host { {Msg::ChatKeepOpenRoomCloses.text()} } else { {Msg::ChatKeepOpenTalking.text()} } }
                },
                ChatStatus::Failed(message) => rsx! { ErrorText { text: message.clone() } },
                ChatStatus::Closed => rsx! { p { class: "muted", {Msg::ChatRoomClosed.text()} } },
            }
            if let Some(links) = &chat.links {
                div { class: "link-box",
                    label { r#for: "room-link", {Msg::ChatLinkHintTalk.text()} }
                    input { id: "room-link", class: "mono", readonly: true, value: links.link.as_str() }
                    div { class: "actions",
                        CopyButton { text: links.link.as_str().to_string(), class: "primary" }
                        CopyButton { text: agent_prompt(links.agent.as_str()), label: "Copy for an AI agent", class: "outline" }
                    }
                }
            }
            if !chat.members.is_empty() {
                h2 { "In the room" }
                ul { class: "members",
                    for member in chat.members.iter() {
                        li {
                            key: "{member}",
                            class: "pill member",
                            "data-member": member_slot(theme, member),
                            "data-you": (chat.you.as_deref() == Some(member.as_str())).then_some("true"),
                            if chat.you.as_deref() == Some(member.as_str()) { "{member} (you)" } else { {member.clone()} }
                        }
                    }
                }
            }
            ol { class: "messages", aria_live: "polite", aria_label: "Messages",
                if chat.lines.is_empty() {
                    li { class: "muted", {Msg::ChatNoMessages.text()} }
                }
                for (index, line) in chat.lines.iter().enumerate() {
                    li { key: "{index}", class: if line.from.is_none() { "notice" } else { "message" },
                        if let Some(from) = &line.from {
                            strong { class: "from", "{from}: " }
                        }
                        span { {line.text.clone()} }
                    }
                }
            }
            if chat.open() {
                form {
                    class: "composer",
                    onsubmit: move |event| {
                        event.prevent_default();
                        let text = draft.peek().trim().to_string();
                        if !text.is_empty() {
                            core.dispatch(Intent::Chat(ChatIntent::Say(text)));
                            draft.set(String::new());
                        }
                    },
                    label { r#for: "chat-input", class: "visually-hidden", "Message" }
                    input {
                        id: "chat-input",
                        autocomplete: "off",
                        placeholder: Msg::ChatInputHint.text(),
                        value: draft,
                        oninput: move |event| draft.set(event.value()),
                    }
                    button { r#type: "submit", class: "button primary", "Send" }
                }
            }
            button {
                r#type: "button",
                class: "button outline",
                onclick: move |_| core.dispatch(Intent::GoHome),
                {chat.leave_label()}
            }
        }
    }
}

#[component]
fn StartRoom() -> Element {
    let core = use_core();
    rsx! {
        section { class: "card", aria_labelledby: "chat-start",
            h1 { id: "chat-start", "Chat room" }
            p { "Open an encrypted room and send its link to the person you want to talk to. Nothing is stored; the room closes when you leave." }
            div { class: "actions",
                button {
                    r#type: "button",
                    class: "button primary",
                    onclick: move |_| core.dispatch(Intent::HostChat { for_agents: false }),
                    {Msg::StartAChatRoom.text()}
                }
                button {
                    r#type: "button",
                    class: "button outline",
                    onclick: move |_| core.dispatch(Intent::HostChat { for_agents: true }),
                    "Start a room for AI agents"
                }
            }
            p { class: "muted", "Got a room link? Open it in this browser to join." }
        }
    }
}
