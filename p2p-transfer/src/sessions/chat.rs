//! Chat rooms: a hosted or joined room, its roster and transcript.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::mpsc;

use super::{BearerLink, Platform};
use crate::chat::{ChatCommand, ChatEvent, ChatHandle, Wake};
use crate::node::{FragmentParams, RelayChoice};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/assets/chat-bridge.js")]
extern "C" {
    #[wasm_bindgen(js_name = installChatSender)]
    fn install_chat_sender(send: &wasm_bindgen::JsValue, leave: &wasm_bindgen::JsValue);
    #[wasm_bindgen(js_name = removeChatSender)]
    fn remove_chat_sender();
    #[wasm_bindgen(js_name = emitChatEvent)]
    fn emit_chat_event(json: &str);
}

/// Transcript lines kept; older ones scroll off for good.
pub const CHAT_TRANSCRIPT: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatRole {
    Host,
    Guest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatStatus {
    Starting,
    Open,
    Failed(String),
    Closed,
}

/// A hosted room's links: the one for people, and the same link marked for agents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomLinks {
    pub link: BearerLink,
    pub agent: BearerLink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    /// `None` for a room notice (someone joined, the room closed).
    pub from: Option<String>,
    pub text: String,
}

/// Everything a Chat screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatSnapshot {
    /// Distinct per room, so a UI can reset its input and focus state.
    pub id: u64,
    pub role: ChatRole,
    pub status: ChatStatus,
    /// This side's display name, once the room assigned it.
    pub you: Option<String>,
    /// Host only, once the room is dialable.
    pub links: Option<RoomLinks>,
    pub members: Vec<String>,
    pub lines: Vec<ChatLine>,
    /// Created from the "For agents" menu or opened from an `&agent` link.
    pub for_agents: bool,
}

impl ChatSnapshot {
    /// The session is still running: messages can be sent.
    pub fn open(&self) -> bool {
        matches!(self.status, ChatStatus::Open)
    }

    /// The label for the button that leaves this room.
    pub fn leave_label(&self) -> &'static str {
        match (self.role, &self.status) {
            (ChatRole::Host, ChatStatus::Open | ChatStatus::Starting) => "Close room",
            (ChatRole::Guest, ChatStatus::Open | ChatStatus::Starting) => "Leave room",
            _ => "Back to home",
        }
    }
}

/// What a Chat screen can ask for. Leaving is [`super::Intent::GoHome`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatIntent {
    /// Post a message; blank text is ignored.
    Say(String),
}

/// One open room, hosted here or joined from a link. Dropping it leaves the room.
pub struct ChatSession {
    id: u64,
    role: ChatRole,
    you: Option<String>,
    links: Option<RoomLinks>,
    members: Vec<String>,
    lines: VecDeque<ChatLine>,
    status: ChatStatus,
    handle: Option<ChatHandle>,
    for_agents: bool,
    #[cfg(target_arch = "wasm32")]
    bridge: Option<ChatBridge>,
}

impl ChatSession {
    /// Open a room on this device.
    pub(super) fn host(
        id: u64,
        relay: RelayChoice,
        platform: &dyn Platform,
        wake: Wake,
        for_agents: bool,
    ) -> Self {
        let handle = ChatHandle::host(relay, String::new(), platform.base_url(), wake);
        Self::with_handle(id, ChatRole::Host, Some(handle), for_agents, None)
    }

    /// Join the room a parsed link points at.
    pub(super) fn join(id: u64, relay: RelayChoice, params: &FragmentParams, wake: Wake) -> Self {
        let (Some(ticket), Some(cap)) = (params.ticket.clone(), params.cap) else {
            return Self::with_handle(
                id,
                ChatRole::Guest,
                None,
                params.agent,
                Some(super::text::MISSING_CAP.to_string()),
            );
        };
        let name = params.name.clone().unwrap_or_default();
        let handle = ChatHandle::join(relay, ticket, cap, name, wake);
        Self::with_handle(id, ChatRole::Guest, Some(handle), params.agent, None)
    }

    fn with_handle(
        id: u64,
        role: ChatRole,
        handle: Option<ChatHandle>,
        for_agents: bool,
        error: Option<String>,
    ) -> Self {
        let mut state = Self::new(id, role, handle, for_agents);
        if let Some(error) = error {
            // No session will report this, so report it here.
            state.dispatch_event(ChatEvent::Failed(error));
        }
        // Only a running session can send; without one `window.oxfer.chat` stays absent, so
        // a script's `send` throws instead of vanishing.
        #[cfg(target_arch = "wasm32")]
        {
            state.bridge = state.handle.as_ref().map(ChatBridge::install);
        }
        state
    }

    fn new(id: u64, role: ChatRole, handle: Option<ChatHandle>, for_agents: bool) -> Self {
        Self {
            id,
            role,
            you: None,
            links: None,
            members: Vec::new(),
            lines: VecDeque::new(),
            status: ChatStatus::Starting,
            handle,
            for_agents,
            #[cfg(target_arch = "wasm32")]
            bridge: None,
        }
    }

    pub fn snapshot(&self) -> ChatSnapshot {
        ChatSnapshot {
            id: self.id,
            role: self.role,
            status: self.status.clone(),
            you: self.you.clone(),
            links: self.links.clone(),
            members: self.members.clone(),
            lines: self.lines.iter().cloned().collect(),
            for_agents: self.for_agents,
        }
    }

    pub fn dispatch(&mut self, intent: ChatIntent) {
        match intent {
            ChatIntent::Say(text) => {
                if let Some(handle) = &self.handle {
                    queue_say(&handle.commands, &text);
                }
            }
        }
    }

    pub fn role(&self) -> ChatRole {
        self.role
    }

    /// A session is running; `apply` drops the handle when it fails or closes.
    pub fn running(&self) -> bool {
        self.handle.is_some()
    }

    pub(super) fn mark_for_agents(&mut self) {
        self.for_agents = true;
    }

    /// Per tick: apply what the session reported.
    pub(super) fn tick(&mut self) {
        let events = self
            .handle
            .as_mut()
            .map(ChatHandle::drain)
            .unwrap_or_default();
        for event in events {
            self.dispatch_event(event);
        }
    }

    /// The session is over: drop it, and take `window.oxfer.chat` away with it so a page
    /// script's `send` throws instead of queueing text nothing will deliver.
    fn end_session(&mut self) {
        self.handle = None;
        #[cfg(target_arch = "wasm32")]
        {
            self.bridge = None;
        }
    }

    fn note(&mut self, text: impl Into<String>) {
        self.push_line(None, text.into());
    }

    fn push_line(&mut self, from: Option<String>, text: String) {
        if self.lines.len() >= CHAT_TRANSCRIPT {
            self.lines.pop_front();
        }
        self.lines.push_back(ChatLine { from, text });
    }

    /// Tell page scripts about `event`, then show it.
    fn dispatch_event(&mut self, event: ChatEvent) {
        #[cfg(target_arch = "wasm32")]
        emit_chat_event(&crate::chat::event_json(&event));
        self.apply(event);
    }

    fn apply(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::Ready {
                link,
                agent_link,
                you,
            } => {
                self.members = vec![you.clone()];
                self.you = Some(you);
                self.links = Some(RoomLinks {
                    link: BearerLink(link),
                    agent: BearerLink(agent_link),
                });
                self.status = ChatStatus::Open;
                self.note("Room open. Send the link to the person you want to talk to.");
            }
            ChatEvent::Connected { you, mut members } => {
                // The host's roster omits the newcomer; list it, as the host lists itself.
                if !members.contains(&you) {
                    members.push(you.clone());
                }
                self.you = Some(you);
                self.members = members;
                self.status = ChatStatus::Open;
                self.note("You're in.");
            }
            ChatEvent::Message { from, text, .. } => self.push_line(Some(from), text),
            ChatEvent::Joined { name } => {
                if !self.members.contains(&name) {
                    self.members.push(name.clone());
                }
                // An agent talking in turns leaves after each turn and rejoins for the next;
                // a rejoin with only notices since its leave just takes the leave back.
                let left = format!("{name} left");
                let rejoin = self
                    .lines
                    .iter()
                    .rev()
                    .take_while(|line| line.from.is_none())
                    .position(|line| line.text == left);
                match rejoin {
                    Some(back) => {
                        self.lines.remove(self.lines.len() - 1 - back);
                    }
                    None => self.note(format!("{name} joined")),
                }
            }
            ChatEvent::Left { name } => {
                self.members.retain(|m| *m != name);
                self.note(format!("{name} left"));
            }
            ChatEvent::Failed(message) => {
                self.status = ChatStatus::Failed(message);
                self.end_session();
            }
            ChatEvent::Closed => {
                self.status = ChatStatus::Closed;
                self.end_session();
                self.note("The room is closed.");
            }
        }
    }
}

/// Queue `text` for the room, from the input box or a page script; false when it is blank
/// or the queue is full. A full queue means the session is already 16 messages behind;
/// dropping a line beats blocking the UI.
fn queue_say(commands: &mpsc::Sender<ChatCommand>, text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && commands
            .try_send(ChatCommand::Say(text.to_string()))
            .is_ok()
}

/// Page-script access to the open room (`assets/chat-bridge.js`): events go out as DOM
/// events, and `window.oxfer.chat.send` queues text straight onto the session's commands.
#[cfg(target_arch = "wasm32")]
struct ChatBridge {
    _send: wasm_bindgen::closure::Closure<dyn FnMut(String) -> bool>,
    _leave: wasm_bindgen::closure::Closure<dyn FnMut()>,
}

#[cfg(target_arch = "wasm32")]
impl ChatBridge {
    fn install(handle: &ChatHandle) -> Self {
        let commands = handle.commands.clone();
        let send =
            wasm_bindgen::closure::Closure::new(move |text: String| queue_say(&commands, &text));
        let leave_token = handle.leave_token();
        let leave = wasm_bindgen::closure::Closure::new(move || leave_token.cancel());
        install_chat_sender(send.as_ref(), leave.as_ref());
        Self {
            _send: send,
            _leave: leave,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for ChatBridge {
    fn drop(&mut self) {
        remove_chat_sender();
    }
}

/// What "Copy for agent" puts on the clipboard: the agent link inside a short brief.
///
/// A bare link with a `cap=` token reads to a careful agent like a secret pasted by
/// mistake, so it asks what to do instead of opening it. The brief says the link is meant
/// for it, gives it a task, and shows a foreground command that needs no checkout: a
/// long-lived background process fed through a pipe is what permission checks refuse.
pub fn agent_prompt(link: &str) -> String {
    // The script and llms.txt sit next to the app page, so resolve them against it (a
    // sub-path deployment keeps working). Everything that reaches a shell is single-quoted,
    // since a path or query can carry shell syntax.
    let page = url::Url::parse(link)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let asset = |name: &str| {
        page.as_ref()
            .and_then(|page| page.join(name).ok())
            .map_or_else(|| format!("https://oxfer.app/{name}"), String::from)
    };
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let script = quote(&asset("oxfer-agent.mjs"));
    let llms = asset("llms.txt");
    let link = quote(link);
    format!(
        "Join this Oxfer chat room as my agent: say hello, then do what the people in the \
         room ask, such as replying or sending and receiving files. Keep listening until the \
         room closes or I tell you to stop. I'm giving you this link on purpose. It's a \
         private room invite, so use it only for this.\n\
         \n\
         Set up once, in an empty folder (needs Node 22 or later and Google Chrome):\n\
         \x20 curl -fsSLO {script} && npm init -y >/dev/null && npm i playwright@1.56.1 pngjs@7.0.0\n\
         \n\
         If you can run a background command, stay in the room. Start this once in the \
         background:\n\
         \x20 node oxfer-agent.mjs chat {link} --name <your name> --dir room\n\
         Then each turn is a foreground run that prints what is new, posts, and waits for a \
         reply (exit code 4 if nobody answered in time):\n\
         \x20 node oxfer-agent.mjs post room --say '<message>' --wait 300\n\
         Leave with: node oxfer-agent.mjs post room --leave\n\
         \n\
         Otherwise talk in turns. Each run joins, posts, waits for a reply, prints the room \
         and leaves:\n\
         \x20 node oxfer-agent.mjs chat {link} --name <your name> --say '<message>' --until . --wait 300\n\
         \n\
         Files, and everything else: {llms}\n"
    )
}

/// The session's wake callback for chat, from the shared notifier.
pub(super) fn wake_for(notifier: &super::Notifier) -> Wake {
    let notifier = notifier.clone();
    Arc::new(move || notifier.notify())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_test_chat_rejoin_takes_back_the_leave_notice() {
        let mut state = ChatSession::new(0, ChatRole::Host, None, false);
        let joined = |name: &str| ChatEvent::Joined { name: name.into() };
        let left = |name: &str| ChatEvent::Left { name: name.into() };
        state.apply(joined("oz"));
        state.apply(left("oz"));
        state.apply(left("bot"));
        state.apply(joined("oz"));
        let texts = |state: &ChatSession| {
            state
                .lines
                .iter()
                .map(|l| l.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&state), ["oz joined", "bot left"]);
        assert_eq!(state.members, ["oz"]);

        // A message after the leave keeps both notices: the gap was visible.
        state.apply(left("oz"));
        state.lines.push_back(ChatLine {
            from: Some("bot".into()),
            text: "hi".into(),
        });
        state.apply(joined("oz"));
        assert_eq!(texts(&state)[2..], ["oz left", "hi", "oz joined"]);
    }

    #[test]
    fn local_test_agent_prompt_points_at_llms_txt_and_keeps_the_link_whole() {
        let link = "https://chat-oxfer.example.dev/#chat&endpointabc&cap=00ff&agent";
        let prompt = agent_prompt(link);
        assert!(prompt.contains("https://chat-oxfer.example.dev/llms.txt"));
        assert!(prompt.contains(&format!("chat '{link}' --name")));
        assert!(prompt.contains("curl -fsSLO 'https://chat-oxfer.example.dev/oxfer-agent.mjs'"));
    }

    #[test]
    fn local_test_agent_prompt_keeps_shell_syntax_out_of_its_commands() {
        let link = "https://oxfer.app/it's?q=$(id)#chat&endpointabc&cap=00ff&agent";
        let prompt = agent_prompt(link);
        assert!(prompt.contains("curl -fsSLO 'https://oxfer.app/oxfer-agent.mjs' &&"));
        assert!(prompt.contains("chat 'https://oxfer.app/it'\\''s?q=$(id)#chat"));
        assert!(!prompt.contains("chat https://"));
    }

    #[test]
    fn local_test_agent_prompt_finds_its_files_next_to_a_sub_path_page() {
        let prompt =
            agent_prompt("https://example.dev/preview/index.html#chat&endpointabc&cap=00ff");
        assert!(prompt.contains("curl -fsSLO 'https://example.dev/preview/oxfer-agent.mjs' &&"));
        assert!(prompt.contains("everything else: https://example.dev/preview/llms.txt"));
    }

    #[test]
    fn local_test_chat_join_without_access_code_fails_without_a_session() {
        let notifier = super::super::Notifier::default();
        let session = ChatSession::join(
            3,
            RelayChoice::None,
            &FragmentParams::default(),
            wake_for(&notifier),
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.id, 3);
        assert!(!session.running());
        assert_eq!(
            snapshot.status,
            ChatStatus::Failed(super::super::text::MISSING_CAP.to_string())
        );
        assert_eq!(snapshot.leave_label(), "Back to home");
    }
}
