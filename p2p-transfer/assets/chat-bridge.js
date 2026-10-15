// Chat room bridge between the wasm app and page scripts (agents, tests).
//
// The UI is a canvas, so this is the only machine-readable view of a room:
//   window.addEventListener('oxfer:chat', e => e.detail)   // {type: ready|connected|message|joined|left|failed|closed, ...}
//   window.oxfer.chat.send(text)                            // post a message to the room you are in
//   window.oxfer.chat.leave()                               // leave now (a host closes the room), so
//                                                           // the others see it at once, not after a timeout
// Installed only while a room is open; removed when the page leaves it.
// Closing or navigating away from the tab leaves the room too (best effort, on
// `pagehide`), so a person who just closes the tab drops off the roster right away.

let sender = null;
let leaver = null;

export function installChatSender(send, leave) {
  sender = send;
  leaver = leave;
  const oxfer = (window.oxfer = window.oxfer || {});
  oxfer.chat = {
    version: 1,
    send(text) {
      if (!sender) throw new Error('not in a chat room');
      // False when the app did not take it: blank text, or its send queue is full.
      return Boolean(sender(String(text)));
    },
    leave() {
      if (leaver) leaver();
    },
  };
}

// `pagehide`, not `visibilitychange`: a phone that backgrounds the tab stays in the room.
window.addEventListener('pagehide', () => {
  if (leaver) leaver();
});

export function removeChatSender() {
  sender = null;
  leaver = null;
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
