# Paired browser diagnostics

1. Open `https://oxfer.app/diags` on the first device, or press **Diags**
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
and API availability, WebSocket handshakes against this build's relays, an
iroh relay registration test, the peer ping result and a short session ID. They
do not automatically upload data or include tickets, capabilities, files, IP
addresses, or arbitrary terminal logs. Review the text before sharing; the
browser user agent and time can still be identifying.

An opened WebSocket only establishes reachability for that relay. A completed
peer ping tests an iroh connection and a bidirectional stream over a
diagnostics-only protocol, **not** a real file transfer, file picker, storage
sink, WebRTC ICE negotiation, or the original sender's share. A failed peer
ping may need additional browser console/network evidence.

Diagnostics are for technical problems. To report what someone sent, use the
**Report abuse** link on the receive screen, which opens the **Abuse &
safety** page (`/abuse`, also in the bottom bar). Do not put a transfer link
or file names in a diagnostics report to report content.

## What the browser stores

When someone asks what `oxfer.app` left in their browser, or needs a clean
start, these are all the items the web app keeps (Chromium: DevTools →
**Application**; Firefox: DevTools → **Storage**):

| Item | Written | Removed by |
| --- | --- | --- |
| localStorage `oxfer.theme.v1` (`rusty` or `clean`) | Only when the user picks a theme in the **Theme** menu, or once at start-up to carry over a non-default theme the user picked in an older version | Clearing site data; start-up also removes a default `clean` value that an older version wrote without a choice |
| Service worker `sw.js` and Cache Storage `oxfer-v3` (the app shell, fetched network-first) | On a visit without `#dev` | `#dev` ([below](#console-handle-__p2p)), a new cache name in `assets/sw.js`, clearing site data |
| IndexedDB database and origin-private (OPFS) directory, both `oxfer-resume` | Only when the receiver ticks **Keep a copy here so I can continue later** and saving starts: both are created then. Looking for saved copies at start-up, **Download file** and deleting a copy never create either | Each copy: **Delete saved copy** → **Delete permanently** under **Saved files in this browser**. The database and directory themselves stay, empty, after the last copy is deleted: clearing site data removes them |

Oxfer sets no cookies and writes no other localStorage keys. Cloudflare may
set its own security cookies, such as `__cf_bm`, when its bot protection is
active (privacy notice, section 6).

Older versions also let eframe write its `app` key and `egui_memory_ron`; the
current build writes neither and removes both at start-up, after carrying
over the theme. Older versions also created an empty `oxfer-resume` database
at start-up while looking for saved copies, so a browser that ran one may
still show it. The current build leaves that database alone (deleting it
could race a save in another tab); clearing site data removes it.

Running diagnostics adds nothing to this list. The desktop app is different:
it keeps eframe's normal settings file on disk, with the theme, the save
folder and window state.

## Relay probes

Which relays a build uses is fixed at compile time by `P2P_RELAY_URL`
([docs/cloudflare-workers.md](cloudflare-workers.md#relay-selection-p2p_relay_url)).
The report shows the choice in two places.

**`Relay <url>: <result>` lines.** `assets/diagnostics.js` opens one WebSocket
per URL, offers the `iroh-relay-v1` and `iroh-relay-v2` subprotocols, and waits
up to 6 seconds. The result is `WebSocket opened`, `WebSocket failed (browser
hides the reason)`, `WebSocket closed before opening`, `WebSocket timed out` or
`WebSocket could not be created`. `relay_probe_urls` in `src/node.rs` builds
the list:

| Build | Probed URLs |
| --- | --- |
| `P2P_RELAY_URL` unset (n0's public relays) | n0's relays in iroh's relay-map order, `wss://aps1-1.relay.n0.iroh.link/relay`, `wss://euc1-1.relay.n0.iroh.link/relay`, `wss://use1-1.relay.n0.iroh.link/relay` and `wss://usw1-1.relay.n0.iroh.link/relay`, then the dotted spelling `wss://euc1-1.relay.n0.iroh.link./relay` |
| `P2P_RELAY_URL` set | One `wss://host[:port]/relay` per configured relay, in the configured order with duplicates dropped; `ws://` for an `http://` test relay. For example, `https://relay.oxfer.app` is probed as `wss://relay.oxfer.app/relay`. |

A custom relay's probe URL is rebuilt from its scheme, host and port only, so
credentials, a path or a query never reach a copied report.

**`Iroh endpoint (...)` line.** A real iroh endpoint starts with this build's
relay settings. `registered with relay` means it came online through one of
them within 15 seconds. The label names the configuration:

| Label | Build |
| --- | --- |
| `browser, DNS dots removed` | n0's relays, with the trailing DNS dots removed from their hostnames |
| `browser, configured relays` | the relays in `P2P_RELAY_URL`; no n0 relay and no `dns.iroh.link` lookup |
| `browser, no relay` | no relay (tests only; production builds never use it) |

If a build meant for `relay.oxfer.app` still shows the n0 probe list and the
`DNS dots removed` label, it was built without the variable, or with a value
the app rejected. Rejected values are logged in the browser console as
`P2P_RELAY_URL is not a valid relay list (...)`. `build-web.sh`, which makes
every deployed build, refuses such values before building, including ones
with an invisible character such as a byte-order mark, so a rejected value
points to a build made another way (`trunk build` or `trunk serve`).
**Technical details** on the home screen says the same thing in words: "a
relay operated by Oxfer" when every configured relay is `relay.oxfer.app`,
"a relay this build was configured to use" for any other `P2P_RELAY_URL`,
or "n0.computer's public iroh relays".

### n0 builds: trailing DNS dots

Browser file transfers on n0 builds use the n0 relay preset with trailing DNS
dots removed from its relay hostnames. WebKit cannot open the dotted WebSocket
URLs, even though it opens the equivalent no-dot URLs. Incoming tickets from
older senders are normalized locally before dialing, without changing their
peer identity or file access code. Native clients keep the upstream n0 preset.
If registration still fails, diagnostics also tries the dotted preset and
reports it as `Iroh endpoint (legacy dotted DNS)`. That extra check can take
another 15 seconds. On custom-relay builds, and whenever the first
registration succeeds, the line reads `not run (browser relay succeeded or a
custom relay is configured)`.

The page's Content-Security-Policy interacts with the dotted checks. On n0
builds, `connect-src` allows `https://*.iroh.link` and `wss://*.iroh.link`.
A hostname ending in a dot does not match those sources (checked in Chromium).
While the policy is report-only, both dotted checks still run, and each
dotted connection adds a `[Report Only] Refused to connect to ...` console
message:

- the dotted probe, always: `wss://euc1-1.relay.n0.iroh.link./relay`;
- the legacy dotted check, only when the first registration failed: its
  HTTPS latency probes of `https://<relay>.relay.n0.iroh.link./ping` for
  `euc1-1`, `use1-1`, `usw1-1` and `aps1-1`, repeated while it runs (three
  rounds in a Chromium test, so 12 messages), and a relay connection to
  `wss://<relay>.relay.n0.iroh.link./relay` if a probe answers.

Once the policy is enforced, the dotted probe and the legacy dotted check
report a failure whatever the network does. Custom-relay builds run neither
check.

## Direct WebRTC path and STUN

Browsers use a single STUN server, `stun:stun.cloudflare.com:3478`, run by
Cloudflare. No TURN server is configured. If a NATed browser cannot reach that
server over UDP, it gathers no server-reflexive (`srflx`) candidates and a
direct data channel may not open. The transfer then continues over the iroh
relay. The paired diagnostics do not test ICE; use the `WebRTC perf` lines
below.

The desktop app uses neither WebRTC nor this STUN server, and is not served
by Cloudflare, so its **Technical details** text mentions only the relay
(Oxfer's, n0.computer's, or one the build was configured to use, without
naming who runs it) or says the build has no relay. It says instead that its
share links contain the device's IP addresses.

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

### Console handle `__p2p`

For hands-on inspection in the browser console, the newest
`RTCPeerConnection` is published as the page global `__p2p`, for example
`await __p2p.getStats()`. This happens only in a debug build (`trunk serve`),
or when the page's URL fragment carries the `dev` flag at the moment the peer
connection is created. A release build opened without `dev` never sets it.

- Sender: open `https://oxfer.app/#dev`, then share. Links shared from that
  page carry `dev` too.
- Receiver: append `&dev` to the share link's fragment, as in
  `…#<ticket>&cap=<code>&dev`. The app keeps `dev` when it removes the ticket
  and access code from the address bar.

`#dev` also unregisters the service worker and clears the app caches on load.
The service-worker download sink (`sink=sw`) is therefore not available in a
`#dev` session. Measure that route without `dev`, and without `__p2p`. `#dev`
leaves localStorage and the `oxfer-resume` copies alone.
