# Paired browser diagnostics

1. Open `https://oxfer.pages.dev/diags` on the first device, or press **Diags**
   in the bottom bar from any app screen. Wait for local checks and press
   **Start peer test**. Keep this tab open.
2. Copy **peer test link** and open it on the second device. This link contains
   an ephemeral diagnostic endpoint address, not a file access code. The
   diagnostic endpoint serves no files and closes when the first device stops
   the test or leaves the page.
3. Press **Test peer connection** on the second device. Wait for the ping
   result; the first device should show one successful incoming ping.
4. On **both** devices, press **Copy report** and share the reports together.
   The common session ID allows their results to be correlated. Do not share a
   real file transfer link.

The reports include the app revision, browser user agent, time, secure-context
and API availability, WebSocket handshakes against configured relays, an iroh
relay registration test, the peer ping result and a short session ID. They do
not automatically upload data or include tickets, capabilities, files, IP
addresses, or arbitrary terminal logs. Review the text before sharing; the
browser user agent and time can still be identifying.

An opened WebSocket only establishes reachability for that relay. A completed
peer ping tests an iroh connection and a bidirectional stream over a
diagnostics-only protocol, **not** a real file transfer, file picker, storage
sink, WebRTC ICE negotiation, or the original sender's share. A failed peer
ping may need additional browser console/network evidence.
