# Oxfer privacy, safety and legal compliance plan

Scope: the browser and native app in this crate as deployed at `oxfer.app`
from a Cloudflare assets-only Worker, operated from Romania, usable from any
country. This plan turns the September 2026 review into work items. It is an
engineering plan, not legal advice; the terms and privacy notice should get a
lawyer's read once traffic is more than testers.

Design facts the plan relies on (verified in code):

- No server code, no accounts, no analytics, no third-party scripts or fonts.
- Share links carry the ticket and a bearer capability in the URL fragment,
  which never reaches the web host. Every share uses a fresh key.
- File bytes flow peer to peer over DTLS (WebRTC) or over QUIC through an
  iroh relay. The relay sees ciphertext, endpoint IDs, IP addresses, timing
  and volume, never plaintext or the capability.
- Client-side storage is limited to a theme key, the service-worker app
  cache, and opt-in OPFS local copies that the user can delete in the app.
- Diagnostics are copied by the user; nothing is uploaded anywhere.

What is missing today, in one line each: no privacy notice, no terms, no
operator identity or abuse contact, production still uses n0's public relays
and DNS discovery, browser ICE still sends every user's IP to Google STUN, no
CSP, and no written risk assessments for the online-safety regimes that apply
to file-sharing services regardless of size.

## 0. Decisions needed from the owner

| # | Decision | Recommendation |
| --- | --- | --- |
| D1 | Relay hosting | Self-host `iroh-relay` on an EU VPS. Cloudflare Workers and Containers cannot host it (section 3). |
| D2 | UK Online Safety Act posture | Comply: write the two assessments, keep a reporting channel, answer Ofcom on time. Geoblocking the UK is the fallback lever, not the default. |
| D3 | Markets not offered | Do not localise, market or register in jurisdictions that require local registration or representatives for foreign online services. State this in the terms. Register in Indonesia only because it is free and enforced by blocking. |
| D4 | Operating entity | Decide whether Oxfer is run by a natural person or a company. A company changes e-commerce disclosures and brings Brazil's Marco Civil log rule into play. |
| D5 | Where assessments live | Keep the drafts under `docs/compliance/` in this repo, or move them to a private repo before filling in anything you would not want quoted back. |

## 1. Workstreams

### A. Infrastructure: relay, STUN, discovery

| # | Item | Where | Notes |
| --- | --- | --- | --- |
| A1 | Provision one EU VPS for the relay | Hetzner Falkenstein or Helsinki, Scaleway Paris, or OVH Gravelines | Smallest instance is enough. Hetzner includes 20 TB traffic per month. Accept the provider's DPA in the account settings and keep a copy. |
| A2 | DNS `relay.oxfer.app` A and AAAA records, DNS-only | Cloudflare DNS | Do not proxy this hostname. The relay needs long-lived WebSockets and UDP for QUIC address discovery, and proxying would add a US processor to the relay path. |
| A3 | Run the official `iroh-relay` 1.1 binary or container image | VPS, systemd | Config in section 4. ACME TLS is built in. Open TCP 80 and 443, UDP 7842. Bind metrics to localhost. Disable access logging; set journald retention to a few days. |
| A4 | Run a STUN-only server next to it | VPS, `coturn` with `stun-only` | iroh-relay 1.x no longer serves STUN, and browser WebRTC needs classic STUN. STUN is a few packets per session, negligible load. Open UDP and TCP 3478. |
| A5 | Rate limits and access control on the relay | relay config `[limits]` | Per-connection byte rate and connection-accept limits. Keep `access = "everyone"` because shares use fresh keys, so allowlists cannot work. |
| A6 | Point production builds at the relay | `.github/workflows/oxfer-web.yml`, `build-web.sh` | Export `P2P_RELAY_URL=https://relay.oxfer.app` before `trunk build`. `RelayChoice::from_env` in `src/node.rs` then selects `RelayChoice::Custom`, which uses `presets::Minimal`: no relay map from n0 and no publishing or lookup at `dns.iroh.link`. `tests/relay_wasm.rs` already tolerates the variable. |
| A7 | Replace the ICE server list | `src/webrtc.rs` `ICE_SERVERS` | `stun:stun.oxfer.app:3478` first, `stun:stun.cloudflare.com:3478` as redundancy under the existing Cloudflare DPA. Remove `stun.l.google.com`. |
| A8 | Support several relays for later regions | `src/node.rs` `RelayChoice::Custom` | Parse a comma-separated `P2P_RELAY_URL` into `RelayMap::from_iter`. Not needed on day one. A non-EU relay you operate is still your infrastructure; the provider's DPA and SCCs cover the location. |
| A9 | Update diagnostics and docs | `src/diagnostics.rs`, `docs/diagnostics.md`, `docs/cloudflare-workers.md` | The diagnostics probe already handles `RelayChoice::Custom`. Docs still describe the n0 preset. |
| A10 | Monitoring | External uptime probe | A WebSocket probe against `wss://relay.oxfer.app/relay` with the `iroh-relay-v1` subprotocol, the same check `assets/diagnostics.js` performs. |
| A11 | Upgrade policy | VPS | Unattended security upgrades for the OS. Relay upgrades follow the crate's iroh version; the protocol is versioned, so track `Cargo.lock`. |

### B. Client and deployment hardening

| # | Item | Where | Notes |
| --- | --- | --- | --- |
| B1 | Content Security Policy, report-only first | `assets/_headers`, `verify-deployment.mjs` | Directives the code needs: `default-src 'self'`; `script-src 'self' 'wasm-unsafe-eval'`; `connect-src 'self' https://relay.oxfer.app wss://relay.oxfer.app`; `worker-src 'self' blob:` for `assets/resume-store.js`; `frame-src blob: 'self'` and `img-src 'self' data: blob:` for the download sinks; `object-src 'none'`; `base-uri 'self'`; `form-action 'none'`. Move the inline boot script out of `index.html` into `assets/boot.js` or add its hash. Run as `Content-Security-Policy-Report-Only` through one full manual test matrix, then enforce. Extend `verify-deployment.mjs` to assert the header. |
| B2 | `Permissions-Policy` | `assets/_headers` | Deny camera, microphone, geolocation, payment, usb. Keep `screen-wake-lock=(self)` for `assets/wake-lock.js`. HSTS is unnecessary: the `.app` TLD is HSTS-preloaded. |
| B3 | Remove the debug handle from release | `src/webrtc.rs` line that sets `__p2p` | Gate on `cfg!(debug_assertions)` or the `#dev` fragment. |
| B4 | Supply-chain protection | GitHub, Cloudflare | Two-factor auth on both accounts. Branch protection on `main` with required CI. Cloudflare API token limited to the Workers edit template, rotated yearly. Keep `verify-deployment.mjs` byte comparison in the deploy job. Publish the release bundle's SHA-256 in the GitHub release so anyone can compare with the served file. A tampered bundle is the one realistic way the encryption claims become false. |
| B5 | Native ticket disclosure | `src/app.rs` share screen, native only | Tell native senders their link contains their direct IP addresses in addition to the relay address. |
| B6 | Footer links | `src/app.rs` bottom bar | Privacy, Terms, Abuse and Source links, opened with `ui.hyperlink_to` on both targets. |

### C. Legal pages and in-app disclosures

| # | Item | Where | Notes |
| --- | --- | --- | --- |
| C1 | Privacy notice | `privacy.html`, copied by Trunk | Content in section 5. One global notice with a short regional annex. |
| C2 | Terms of use | `terms.html` | Content in section 6. |
| C3 | Abuse, safety and law-enforcement page | `abuse.html` | Content in section 7. One mailbox for everything. |
| C4 | Clean paths | `assets/_redirects` | Workers static assets honour `_redirects`. Map `/privacy`, `/terms`, `/abuse` to the files with 200 rewrites so the SPA fallback does not swallow them. Add the three files to `verify-deployment.mjs`. |
| C5 | Operator identity | footer of each page | Name, postal address or registered office, email. Required by Romanian Law 365/2002 art. 5 and DSA arts. 11 and 12 regardless of price. |
| C6 | Keep the in-app "Technical details" honest | `src/app.rs` `show_how_it_works` | Update the relay sentence once the self-hosted relay is live. |

### D. Assessments and records

| # | Item | Where | Notes |
| --- | --- | --- | --- |
| D1 | UK OSA illegal-content risk assessment | `docs/compliance/osa-illegal-content.md` | Outline in section 8. Ofcom's small-service tool produces the skeleton. Dated and signed by the operator. |
| D2 | UK OSA children's access assessment | `docs/compliance/osa-children-access.md` | General-audience tool, no child-directed content, no discovery, no feed. Record the conclusion and revisit yearly. |
| D3 | Australia DIS Standard tier self-assessment | `docs/compliance/esafety-dis.md` | Tier 3 low-risk profile: terms prohibit class 1A and 1B material, report channel exists, eSafety notices are answered. |
| D4 | GDPR Article 30 record and DPIA screening | `docs/compliance/ropa.md` | Processing activities: web hosting logs at Cloudflare, relay and STUN connection metadata on own infrastructure, peer address exchange. Screening note: no high-risk criteria, so no DPIA. |
| D5 | Transparency and no-logs statement | `docs/compliance/transparency.md` and a section in `privacy.html` | What exists: nothing per user or per transfer. What a lawful request can obtain: nothing retroactively. |
| D6 | Incident runbook | `docs/compliance/incident-runbook.md` | Primary scenario: compromised build or deploy pipeline. Steps: revoke tokens, redeploy a verified build, compare hashes, notify. Deadlines: GDPR 72 hours to ANSPDCP; India DPDP 72 hours to the Board from May 2027; CERT-In 6 hours on a best-effort basis. |
| D7 | Processor list | inside `ropa.md` | Cloudflare (hosting, STUN), VPS provider (relay, STUN), GitHub (build). Link to each DPA. |

### E. Registrations and market posture

| # | Item | Notes |
| --- | --- | --- |
| E1 | Indonesia PSE registration | Free, through the OSS-RBA portal, foreign private PSE form. The sanction for not registering is nationwide blocking. |
| E2 | UK | No registration. Be able to answer an Ofcom information request within its deadline; the first fine on a small file-sharing service was for silence, not for content. |
| E3 | Markets not offered | Terms state that Oxfer is offered from Romania under EU law, is not localised, marketed or supported in jurisdictions that require local registration or a local representative for foreign online services, and that users are responsible for local rules on encryption tools. Authorities reach the operator through Romanian legal process. |
| E4 | Geoblocking lever | Document a Cloudflare WAF custom rule per country in `docs/cloudflare-workers.md`. Available on the free plan. Off by default. |
| E5 | Brazil | Terms say the service is not directed to minors and runs no profiling or advertising. Appointing a Brazilian representative is deferred until Brazilian traffic is material. |

### F. Ongoing

| # | Item | Cadence |
| --- | --- | --- |
| F1 | Review this plan, the notice and the assessments | Quarterly, and after any architecture change |
| F2 | Watch list | EU CSAM regulation (interpersonal communications services; trilogue ongoing), UK OSA section 121 notices, India DPDP duties from 14 May 2027, US state privacy and child-safety laws, Brazil ECA Digital enforcement |
| F3 | Triggers that reopen everything | Accounts, analytics, ads, payments or donations, a store-and-forward relay, or more than roughly 100k monthly visitors from any single country with threshold-based rules |

## 2. Sequence

| Phase | Items | Time |
| --- | --- | --- |
| 0. Quick fixes | A7 (with Cloudflare STUN only until A4 exists), B3, B4, C5 draft | Days |
| 1. Relay | A1 to A6, A9, A10, then redeploy | 1 to 2 weeks |
| 2. Pages | C1 to C4, B6, C6 | 1 week, parallel with phase 1 |
| 3. Hardening and records | B1 report-only then enforce, B2, B5, D1 to D7, E1 | 2 weeks |
| 4. Steady state | F1 to F3, A8 when a second region is justified | Ongoing |

## 3. Relay hosting options

Why Cloudflare Workers and Containers do not fit:

- `iroh-relay` is a native Rust server. It cannot compile to a Worker.
- Containers only receive traffic through a Worker as HTTP or WebSocket. They
  cannot accept inbound UDP, so QUIC address discovery for native clients is
  impossible there.
- Containers sleep after ten minutes by default, and Cloudflare picks their
  location. A relay needs long-lived connections and a known region.
- Containers need the Workers Paid plan, and relayed bytes would be billed as
  compute time plus egress after the included allowance. A VPS with included
  traffic is cheaper and simpler.
- Cloudflare would also terminate TLS for the relay hop, which puts a US
  processor on the relay path again. Encryption stays end to end, but the
  jurisdictional gain of self-hosting would be lost.

| Option | UDP for QAD | Region control | Long-lived WebSockets | Cost order | Data-protection position | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| EU VPS with `iroh-relay` and `coturn` (Hetzner, Scaleway, OVH) | Yes | Yes | Yes | About EUR 4 to 8 per month, traffic included | You are the controller, EU provider under a DPA | Recommended |
| n0 managed relays | Yes | Yes | Yes | Quote from n0 | Needs a DPA with n0 and confirmation of regions | Acceptable if you prefer not to operate a server |
| Fly.io machine in an EU region (fra, ams, cdg, arn) | Yes, with a dedicated IPv4 | Yes, one machine per relay hostname | Yes, raw TCP passthrough on 443 | About USD 5 to 8 per month plus USD 0.02 per GB egress | US company (Fly.io Inc.), EU region, pre-signed DPA | Viable second choice; conditions in section 4 |
| Cloudflare Workers or Containers | No | No | Limited | USD 5 plan plus usage | US processor on the relay hop | Not viable |
| n0 public relays (today) | Yes | No | Yes | Free | No contract; hobby-use terms; US and Asia servers | Replace |

Browser clients only ever use `wss://` on port 443 against the relay. UDP
7842 matters for the native app's address discovery and can be omitted until
native builds are distributed.

## 4. Relay runbook

Config for `iroh-relay` (adjust paths and contact address):

```toml
# /etc/iroh-relay/config.toml
http_bind_addr = "0.0.0.0:80"
https_bind_addr = "0.0.0.0:443"
enable_quic_addr_discovery = true
quic_bind_addr = "0.0.0.0:7842"
access = "everyone"
enable_metrics = true
metrics_bind_addr = "127.0.0.1:9090"

[tls]
cert_mode = "LetsEncrypt"
hostname = "relay.oxfer.app"
contact = "ops@oxfer.app"
prod_tls = true
cert_dir = "/var/lib/iroh-relay/certs"

[limits]
accept_conn_limit = 100
accept_conn_burst = 200

[limits.client_rx]
bytes_per_second = 26214400
max_burst_bytes = 52428800
```

Check the key names against the `iroh-relay` version pinned by the
workspace before deploying; the server config schema is versioned with the
crate.

STUN-only `coturn`:

```
# /etc/turnserver.conf
stun-only
listening-port=3478
no-cli
no-tls
no-dtls
```

Firewall: allow TCP 80, 443 and 3478, UDP 3478 and 7842, plus SSH from your
addresses. Everything else closed.

Logging: run both services under systemd with `StandardOutput=journal`, set
`SystemMaxUse=200M` and `MaxRetentionSec=3day` in `journald.conf`, and do
not enable any access log. The relay's metrics are aggregate counters and
stay on localhost.

Client side: `P2P_RELAY_URL=https://relay.oxfer.app`. The diagnostics page
derives `wss://relay.oxfer.app/relay` from it.

### Fly.io variant

Fly.io can host the relay. Verified against Fly's networking and pricing
documentation in September 2026. Eight conditions, none of which apply to a
plain VPS:

1. **One machine per relay hostname.** Both peers of a transfer must reach
   the same relay process. Fly's anycast and autoscaling would split them, so
   set `auto_stop_machines = "off"`, `auto_start_machines = false`,
   `min_machines_running = 1`, one region, one machine. A second region is a
   second app with its own hostname.
2. **Dedicated IPv4, USD 2 per month.** Shared IPv4 only carries Fly-terminated
   HTTP and TLS. Raw TCP on 443 and any UDP need a dedicated address.
3. **TLS stays in the relay.** Expose 443 with no handlers so `iroh-relay` runs
   ACME itself; its rustls-acme implementation uses the TLS-ALPN challenge, so
   port 80 is not needed for certificates. Persist `cert_dir` on a small
   volume, or every redeploy asks Let's Encrypt for a new certificate.
4. **Client addresses.** With raw passthrough the relay sees Fly's proxy
   address, not the client's. Harmless here: the relay logs nothing and limits
   per connection. Do not enable the PROXY protocol handler; `iroh-relay` does
   not parse it.
5. **UDP for QUIC address discovery, native clients only.** Fly preserves the
   UDP source address, so discovery works, but the socket must bind to the
   `fly-global-services` address and internal and external ports must match.
   `quic_bind_addr` takes an IP, so an entrypoint script resolves
   `fly-global-services` and writes the config before start. Skip until native
   builds ship; browsers never use UDP against the relay.
6. **STUN.** Run coturn as a second small app with its own dedicated IPv4, or
   use Cloudflare STUN alone. One STUN server is enough for WebRTC.
7. **Egress is metered from the first byte**, USD 0.02 per GB in Europe and
   North America. A relayed 5 GB transfer costs about ten cents. A VPS with
   included traffic has no such line item.
8. **Bucharest is not a Fly region.** Nearest are Frankfurt, Amsterdam, Paris
   and Stockholm. The relay's data-protection position is a US processor with
   EU servers under Fly's pre-signed DPA, the same shape as Cloudflare.

```toml
# fly.toml
app = "oxfer-relay-eu"
primary_region = "fra"

[build]
dockerfile = "Dockerfile"   # FROM n0computer/iroh-relay:v1.1.0, COPY config.toml

[mounts]
source = "relay_certs"
destination = "/var/lib/iroh-relay"

[[services]]
protocol = "tcp"
internal_port = 443
auto_stop_machines = "off"
auto_start_machines = false
min_machines_running = 1
  [[services.ports]]
  port = 443              # no handlers: the relay terminates TLS and runs ACME

# Add only when native clients need address discovery:
# [[services]]
# protocol = "udp"
# internal_port = 7842
#   [[services.ports]]
#   port = 7842
```

The image `n0computer/iroh-relay` is published on Docker Hub with tags
matching iroh releases. Use the tag that matches the workspace's iroh
version or a newer 1.x.

## 5. Privacy notice: content checklist

Identity and contact of the operator. Supervisory authority: ANSPDCP, with
the ICO for UK users.

What is processed, by whom, and why:

| Data | Who sees it | Purpose | Retention |
| --- | --- | --- | --- |
| IP address, user agent, request metadata for the app shell | Cloudflare, as processor | Serving and protecting the site | Cloudflare's own edge-log retention; none by the operator |
| IP addresses, endpoint IDs, timing and volume of relayed connections | The operator's relay and STUN servers | Establishing and, if needed, relaying encrypted transfers | Not logged; aggregate counters only |
| IP address in STUN binding requests | Operator's STUN server, Cloudflare STUN as backup | NAT traversal | Not logged |
| Peer IP addresses exchanged through ICE | The other party to the transfer | Direct connection | Held by the peer's browser for the session |
| Theme preference, service-worker app cache | The user's browser only | Convenience, offline shell | Until cleared |
| Opt-in local copies: file bytes, names, sizes, hashes | The user's browser only, OPFS and IndexedDB | Resumable receives | Until the user deletes them |
| Diagnostics report: build label, user agent, timestamp, reachability results | Nobody unless the user shares it | Support | Not stored |

Legal basis: legitimate interest and performance of the requested service.
Transfers: Cloudflare under SCCs; relay in the EU. Rights: access, erasure
and so on, with the honest note that there is nothing to retrieve. Share
links are bearer tokens; anyone holding a complete link can download while
the sender keeps sharing. No sale of data, no advertising, no profiling.

Regional annex, one or two lines each: California (no sale, no tracking,
Do Not Track not applicable), Brazil (LGPD contact channel), India (grievance
contact and timelines), Singapore (data protection officer contact), Japan
(list of third-party transmission destinations: relay, STUN, Cloudflare),
Switzerland and UK (rights mirror GDPR).

## 6. Terms of use: content checklist

- Operator identity, governing law Romania, mandatory consumer-law carve-outs.
- Service description: peer-to-peer transfer, sender must stay online, the
  operator cannot see, recover, or selectively revoke content.
- Acceptable use: no CSAM, no intimate images without consent, no terrorist
  content, no malware, no copyright infringement, no harassment.
- Abuse handling: what the operator can do (block addresses at the relay,
  cooperate with lawful requests) and cannot do (inspect or delete content).
  This is the repeat-infringer policy for US purposes.
- Age: not for users under 13; users under 16 in the EU need parental consent
  where national law requires it.
- Market posture from E3 and the user's responsibility for local rules on
  encryption tools.
- Sanctions notice: hosting providers apply their own sanctions rules.
- No warranty, limitation of liability within what Romanian consumer law
  allows, changes to the terms, contact.

## 7. Abuse, safety and law-enforcement page: content checklist

- One mailbox, monitored, with an expected acknowledgement within 24 hours
  and resolution or answer within 15 days. Those figures satisfy India's IT
  Rules and are reasonable everywhere else.
- Non-consensual intimate imagery: response within 48 hours, matching the US
  TAKE IT DOWN Act even if Oxfer is not a covered platform.
- CSAM: reports with actual knowledge are forwarded to NCMEC and to Romanian
  authorities; the operator holds no content to remove.
- What the operator holds: nothing per user or per transfer. What legal
  process can obtain: nothing retroactively. Requests go through Romanian
  authorities and MLAT.
- Named grievance officer for India, which can be the operator.
- Ofcom and eSafety contact acknowledgement: information requests are
  answered within their deadlines.

## 8. UK OSA illegal-content risk assessment: outline

1. Service description: one-to-one transfers via unguessable bearer links;
   no search, feed, directory, profiles, comments or public URLs; content is
   never stored or visible to the operator; the sender must stay online.
2. User base: general audience, English only, no UK targeting; estimated UK
   share from Cloudflare aggregate analytics.
3. Risk by priority offence category, each rated with reasons. The structural
   arguments: no discoverability, no persistence, no amplification, sender
   attribution to a live network connection, and no capacity for the
   operator to scan content. Grooming and CSAM distribution risk is limited
   to parties who already share a link out of band.
4. Measures in place: terms, abuse channel with defined timelines, relay
   address blocking, cooperation with lawful requests, no anonymity beyond
   what the relay path provides.
5. Measures considered and not adopted, with reasons: hash matching is not
   possible without access to plaintext; account gating would defeat the
   privacy design and add personal data.
6. Review date and owner.

## 9. Code changes summary

| File | Change |
| --- | --- |
| `src/webrtc.rs` | New `ICE_SERVERS`; gate the `__p2p` handle |
| `src/node.rs` | Optional: comma-separated relay list in `RelayChoice::Custom` |
| `src/app.rs` | Footer links; native ticket IP note; updated relay sentence |
| `src/diagnostics.rs` | No change required; verify probe against the custom relay |
| `assets/_headers` | CSP report-only, then enforced; Permissions-Policy |
| `assets/_redirects` | Clean paths for the legal pages |
| `index.html` | Boot script moved to `assets/boot.js` or hashed; copy-file entries for the new pages and `_redirects` |
| `privacy.html`, `terms.html`, `abuse.html` | New static pages |
| `build-web.sh`, `.github/workflows/oxfer-web.yml` | `P2P_RELAY_URL` export; add the new pages and headers to verification |
| `verify-deployment.mjs` | Assert CSP and Permissions-Policy; check the new pages |
| `docs/diagnostics.md`, `docs/cloudflare-workers.md` | Relay and geoblocking documentation |
| `docs/compliance/*.md` | Assessments, records, runbook |
