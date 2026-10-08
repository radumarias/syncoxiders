// Chat room bridge between the wasm app and page scripts (agents, tests).
//
// The UI is a canvas, so this is the only machine-readable view of a room:
//   window.addEventListener('oxfer:chat', e => e.detail)   // {type: ready|connected|message|joined|left|failed|closed, ...}
//   window.oxfer.chat.send(text)                            // post a message to the room you are in
// Installed only while a room is open; removed when the page leaves it.

let sender = null;

export function installChatSender(callback) {
  sender = callback;
  const oxfer = (window.oxfer = window.oxfer || {});
  oxfer.chat = {
    version: 1,
    send(text) {
      if (!sender) throw new Error('not in a chat room');
      sender(String(text));
      return true;
    },
  };
}

export function removeChatSender() {
  sender = null;
  if (window.oxfer && window.oxfer.chat) delete window.oxfer.chat;
}

export function emitChatEvent(json) {
  let detail;
  try {
    detail = JSON.parse(json);
  } catch (_) {
    return;
  }
  window.dispatchEvent(new CustomEvent('oxfer:chat', { detail }));
}
