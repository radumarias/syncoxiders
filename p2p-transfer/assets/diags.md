# Oxfer diagnostics

> `https://oxfer.app/diags` checks whether this browser can reach Oxfer's relays and
> another device. It is not a file link and serves no files. Like the rest of Oxfer,
> the page is a WebAssembly program drawn on a canvas, so it needs a real browser.

## What it checks

- Local checks run on load: app revision, secure context, whether the browser
  reports being online, and whether WebRTC and origin-private storage are available.
- WebSocket handshakes against the configured relays, and an iroh relay
  registration test.
- An optional peer test between two devices.

## Peer test between two devices

1. Open `https://oxfer.app/diags` on the first device, or press **Diags** in the
   bottom bar of any app screen. Wait for the local checks, press
   **Start peer test**, and keep the tab open.
2. Copy the **peer test link** and open it on the second device. The link holds an
   ephemeral diagnostic endpoint address, not a file access code. The endpoint
   serves no files and closes when the first device stops the test or leaves.
3. Press **Test peer connection** on the second device. The first device should
   show one successful incoming ping.
4. Press **Copy report** on both devices and share the two reports together. They
   share a session ID so their results can be matched.

## What a report contains

The app revision, browser user agent, time, secure-context and API availability,
relay handshake results, the peer ping result, and a short session ID. Reports
are never uploaded automatically, and they contain no tickets, capabilities,
files, IP addresses, or terminal logs. Review the text before sharing: the user
agent and time can still identify you.

Sending and receiving files: https://oxfer.app/index.md
Full guide: https://github.com/radumarias/syncoxiders/blob/main/p2p-transfer/docs/diagnostics.md
