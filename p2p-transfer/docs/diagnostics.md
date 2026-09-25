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

Browser file transfers use the n0 relay preset with trailing DNS dots removed
from its relay hostnames. WebKit cannot open the dotted WebSocket URLs, even
though it opens the equivalent no-dot URLs. Incoming tickets from older
senders are normalized locally before dialing, without changing their peer
identity or file access code. Native clients keep the upstream n0 preset.
If registration still fails, diagnostics compares the legacy dotted preset;
the extra check can take another 15 seconds.

An opened WebSocket only establishes reachability for that relay. A completed
peer ping tests an iroh connection and a bidirectional stream over a
diagnostics-only protocol, **not** a real file transfer, file picker, storage
sink, WebRTC ICE negotiation, or the original sender's share. A failed peer
ping may need additional browser console/network evidence.

## Slow WebRTC transfers

While a file transfer is active, both browsers write a `WebRTC perf` line to
**Terminal Output** every five seconds. Copy several lines from each side after
the transfer has been running for at least ten seconds, plus the sender's
`serving file … frame limit` line and any `sender waited`, `frame send`, `source
read`, `receiver sink write`, or `receiver control credit send` lines. These
statistics are local; they are not sent to a diagnostics server. They contain
candidate *types*, not ICE addresses, share links, file names or capabilities.
Review the copied output before sharing, since the terminal also contains
other log messages.

`pairTx`/`pairRx` are selected-ICE-pair bytes per second, if the browser
exposes them; `queuedTx` counts bytes accepted by the browser's data-channel
send queue, while `deliveredRx` counts data-channel messages delivered to the
page. Neither is proof of a saved file. `buffered` is the current send-queue
size; `drainWait` is time spent awaiting WebRTC backpressure during the
sampling interval. `rtt` and `availableUp` are browser estimates and may be
unavailable (`-`). `unknown` means the browser did not identify the selected
ICE candidate pair; it must not be assumed to mean direct.

For an A/B test of SCTP message size, open the **sender's app page** with
`?dcframe=64` before selecting a file, then generate a fresh share link for
the receiver. Valid diagnostic caps are 16, 32, 64, 128, or 256 KiB; the
browser's smaller negotiated limit always wins. With no query parameter,
the existing browser-advertised size applies. Compare identical files,
receivers, networks, and save modes, and check `maxFrameTx` to confirm the
actual frame size. The public parameter belongs on the sender's page, **not**
in the receiver's private transfer link.
