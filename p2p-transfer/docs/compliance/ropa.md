# GDPR Article 30 record of processing activities

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated and signed below. |
| Controller | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]], [[OPERATOR_REGISTRATION]] |
| Contact | `privacy@oxfer.app` |
| Data protection officer | None designated (section 9.4) |
| Representative in the EU | Not applicable: the controller is established in Romania |
| Lead supervisory authority | ANSPDCP (Romania) |
| Record date | [[EFFECTIVE_DATE]] |

This record is published in a public repository. It is not legal advice. The
public-facing version is the [privacy notice](../../privacy.html); this record
adds the controller's reasoning, the processor list (plan item D7) and the DPIA
screening.

**Why a record is kept.** The exemption for organisations with fewer than 250
employees does not apply where processing is not occasional [GDPR Art. 30(5)].
The relay processes connection data continuously, so the record is kept.

## 1. Configuration covered

| | Target (this record) | Interim, until `P2P_RELAY_URL` is set |
| --- | --- | --- |
| Relay | The operator's `iroh-relay` at `relay.oxfer.app`, on a VPS from [[RELAY_HOSTING_PROVIDER]] in [[RELAY_LOCATION]] | n0's public relays (`*.relay.n0.iroh.link`) |
| Address lookup | None: the custom relay build uses iroh's minimal preset | Browsers publish and resolve endpoint records at `dns.iroh.link` |
| STUN | `stun:stun.cloudflare.com:3478` only (`src/webrtc.rs`) | Same |

In the interim configuration, n0 receives users' IP addresses, endpoint IDs,
connection times and traffic volume. n0 describes its public relays as suitable
for development and hobby use only, states that they see this connection
metadata, and reserves the right to block abuse [T6]. There is no contract with
n0. The privacy notice describes the target configuration, so it should be
published when the production build uses the operator's relay
(see [README](README.md#owner-actions)).

**Hosting variant.** The relay kit ([`deploy/relay/`](../../deploy/relay/README.md))
has a VPS variant, set up by `setup.sh`, and a Fly.io variant
(`deploy/relay/fly/`). This record, the privacy notice, the terms, the abuse
page and the [transparency statement](transparency.md) describe the VPS
variant, with an EU-headquartered provider and the server in the EU. On the
Fly.io variant several of their statements would be false, so these must change
before it is used. The complete list of pages and records to change is
"Before using Fly" in the relay runbook's
["Fly.io variant"](../../deploy/relay/README.md#flyio-variant) section; the
reasons, and the sections each one affects, are:

- **Processor and transfer.** Fly.io, Inc. is a United States company, and
  its edge proxies, which can be outside the EU, receive every client
  connection. Record it as a processor with its DPA and transfer basis (its
  Data Privacy Framework certification, if the list shows one, or the Standard
  Contractual Clauses) in sections 4 and 5, and in section 8 of the privacy
  notice [T2][T21].
- **Address blocking.** Behind Fly's proxy the relay sees the proxy's address,
  not the client's, and there is no host firewall, so IP addresses cannot be
  blocked or rate-limited per client. Change the P3 row of section 2, section
  3.2 and section 6 here; section 3 of the privacy notice; sections 5.2, 7.1
  and 7.3 of the terms; the summary and sections 3, 4 and 7 of the abuse page;
  sections 1 and 2 of the transparency statement; and measure ICU C2 and the
  "Configuration assessed" note in the
  [illegal content risk assessment](osa-illegal-content.md#51-codes-measures-that-apply).
- **Logs.** The relay's output goes to Fly's log pipeline, whose log search
  keeps logs for 7 days [T22], not to a host journal capped at three days, and
  the Fly image runs no SSH service. Change the P3 row of section 2, sections
  2.1, 6 and 7 here, section 3 of the privacy notice, section 9.1 of the abuse
  page and section 1 of the transparency statement.

## 2. Processing activities

| # | Activity and purpose | Data subjects | Personal data | Legal basis | Recipients | Retention |
| --- | --- | --- | --- | --- | --- | --- |
| P1 | Serving the app shell and legal pages from Cloudflare; protecting the site | Visitors | IP address, user agent, request line without the URL fragment, time, TLS and connection data; security cookies such as `__cf_bm` when Cloudflare bot protection is active; Network Error Logging reports until the owner turns them off (section 2.2) | Art. 6(1)(f), LIA 3.1 | Cloudflare, Inc. (processor) | No request log is kept for the operator: Cloudflare's request-log retention (Logpull) is off by default and stays off [T24] ([README](README.md#owner-actions), action 1). The dashboard shows aggregates, and details of requests Cloudflare's security features block for the period the plan sets (24 hours on the Free and Pro plans). Cloudflare keeps its own operational and security logs under its privacy policy [T3]. The operator exports no logs. |
| P2 | STUN, so browsers can find a direct path | Senders and recipients using the browser app | Public IP address and port, time | Art. 6(1)(b) | Cloudflare, Inc. (see section 4 on its role) | Cloudflare's policy. The operator receives nothing. |
| P3 | Relay: authenticating endpoints, carrying connection setup, forwarding encrypted traffic when no direct path works | Senders and recipients | IP address and port, endpoint ID (a public key new for each share and each receiving session), connection times, traffic volume. The relay forwards ciphertext it cannot read. | Art. 6(1)(b); rate limits and blocking: Art. 6(1)(f), LIA 3.2 | [[RELAY_HOSTING_PROVIDER]] (processor, infrastructure only) | Held in memory only while connected: the relay's key cache is turned off, so endpoint IDs are not kept after a connection ends (section 2.1). No access log: with the shipped configuration the relay logs no client IP addresses, endpoint IDs or connection events. Its own operational errors are logged without client addresses and deleted by journald after at most 3 days. The host firewall's rate-limit sets hold a source address for at most 60 seconds. Aggregate metrics contain no personal data. Blocked endpoint IDs and IP addresses: while the block is in force (section 6). |
| P4 | Peer connection: exchanging addresses (ICE candidates; direct addresses in native tickets) and transferring the files | Senders and recipients | IP addresses and candidates, endpoint IDs, file names, sizes, checksums, file contents | Art. 6(1)(b) | The other party to the transfer, chosen by the user who shares the link. The operator receives none of it. | Held by the peers for the session; saved files are under the recipient's control. |
| P5 | Handling abuse reports, complaints, appeals and requests from authorities and regulators | Reporters, people reported (usually only as an endpoint ID or IP address), staff of authorities | Email address, name if given, message, reported link or endpoint ID, IP addresses blocked, decision and outcome, correspondence | Art. 6(1)(c) where a law requires the handling (for example the points of contact in DSA Arts. 11 and 12); otherwise Art. 6(1)(f), LIA 3.3 | Cloudflare Email Routing and the mailbox provider (processors); competent authorities; NCMEC for apparent child sexual abuse material (section 5) | As long as needed to handle the matter and show how it was handled; reviewed at least yearly. |
| P6 | Handling data protection requests | People who write to `privacy@oxfer.app` | Email address, request, identity details if needed | Art. 6(1)(c) (GDPR Arts. 12 to 22) | Cloudflare Email Routing and the mailbox provider (processors) | As long as needed to show the request was handled; reviewed at least yearly. |
| P7 | Browser-local storage: the theme (`oxfer.theme.v1` in local storage, written only when the user picks a theme, or once to carry over a theme picked in an earlier version), the service worker's app-shell cache, and opt-in resumable copies (IndexedDB database and OPFS directory, both `oxfer-resume`, created only when the user turns on "Keep a copy" and starts saving; listing, exporting and deleting saved copies never create them). No cookies and no other local-storage keys; the legacy `app` and `egui_memory_ron` keys are migrated and removed at start-up. | Users of the browser app | The chosen theme; opt-in received file data with names, sizes, checksums and progress | Not received by the operator; storage relies on the strictly-necessary exemption for storage in the user's device [T9] | None | Until the user clears site data or deletes the saved copy |
| P8 | Diagnostics page | Users who run it | Build label, user agent, feature checks, relay reachability results | Art. 6(1)(b) | None, unless the user sends the report (then P5 or P6) | Not stored |

Build and deployment (GitHub repository and Actions, Cloudflare API) process the
operator's own account data and no user data, so they are not listed as an
activity.

### 2.1 Relay logging, verified in the relay source

The `iroh-relay` 1.1.0 binary has no access-log option. It logs through
`tracing`, and its only log control is the `EnvFilter` read from `RUST_LOG`
[T7]. At `info`, each connection runs inside a span that records the peer's
socket address, so every connection error prints that IP address and port; at
`warn`, each failed or probed connection still writes a line. The
[relay runbook](../../deploy/relay/README.md#what-is-logged) therefore ships a
`RUST_LOG` filter in `deploy/relay/iroh-relay.service` that keeps startup
failures, ACME and certificate errors and relay task failures, and turns the
per-connection targets off. With that configuration:

- the relay logs no client IP addresses, endpoint IDs or connection events;
- operational errors are logged without client addresses. The only
  per-request line is one per malformed HTTP request on port 80, with the
  parser error and no address;
- the relay's key cache is turned off (`key_cache_capacity = 0` in
  `deploy/relay/config.toml`). Left at its default, it would keep up to
  1,048,576 endpoint IDs from relayed traffic in memory with no time limit,
  after their connections end. With it off, endpoint IDs are held in memory
  only while connected;
- the systemd journal keeps entries at most three days
  (`MaxRetentionSec=3day`, rotated daily with `MaxFileSec=1day`). `setup.sh`
  removes rsyslog and deletes the log files it had written, so no other
  general log store exists;
- `sshd` records the source address and user of SSH logins and failed login
  attempts in the journal, and in the login records: failed attempts in
  `/var/log/btmp`, sessions in `/var/log/wtmp`. The kit rotates `btmp` and
  `wtmp` daily and keeps at most three days, and `lastlog` (and `lastlog2`
  where present) keeps no persistent record. These records concern the
  operator's own administration of the host, not relay users. Block commands
  (section 6) leave no shell history on the host;
- metrics are aggregate counters in memory, bound to `127.0.0.1`;
- the firewall has no logging rules. Its rate-limit sets hold a source address
  for at most 60 seconds; its ban sets hold only addresses the operator bans,
  each with an expiry (section 6);
- the relay's TLS certificate for `relay.oxfer.app` is published by Let's
  Encrypt in public Certificate Transparency logs, as every public certificate
  is. It names the host only.

The runbook allows a more verbose `RUST_LOG` only briefly while debugging a
fault, followed by clearing the journal.

### 2.2 Network Error Logging

On 30 September 2026, responses from `oxfer.app` carried Cloudflare's
`report-to` and `nel` headers with `success_fraction` 0.0, which ask browsers to
send reports of failed requests to Cloudflare's collector
(`a.nel.cloudflare.com`). Cloudflare lets a zone administrator turn this off
with the zone setting `nel` [T5]. Decision: the owner turns it off before the
legal pages are published ([README](README.md#owner-actions), action 1), and
`verify-deployment.mjs` fails while the headers are still served. The privacy
notice therefore does not describe it.

## 3. Legitimate interest assessments

### 3.1 P1, serving and protecting the site

- **Purpose:** deliver the app and keep the site available against attacks and
  abuse.
- **Necessity:** a web host must process the requester's IP address to answer.
  The operator adds no analytics and exports no logs.
- **Balancing:** data is limited to what an HTTP request carries; the URL
  fragment holding the share capability never reaches the host; users expect a
  website to see their IP address. Interests do not override users' rights.

### 3.2 P3, rate limits and blocking at the relay

- **Purpose:** keep the relay available and stop abuse reported to the operator.
- **Necessity:** rate limits act per connection in memory. Blocking uses only an
  endpoint ID (which identifies a single share) or an IP address, from a report.
- **Balancing:** no profile or history is built. Address blocks can affect other
  people behind the same address, so they are used for repeated or serious abuse,
  set to expire (normally after 30 days), and reviewed on request (abuse page,
  section 4). Rate limits hold a source address for at most 60 seconds.
- **Variant:** address blocks and per-address rate limits need the host firewall
  of the VPS variant; the Fly.io variant has neither (section 1).

### 3.3 P5, handling reports where no law requires it

- **Purpose:** protect people from harm through the service, answer complaints,
  cooperate with authorities.
- **Necessity:** only the report, the identifiers needed to act, and the outcome
  are kept.
- **Balancing:** reporters choose what to send and may stay anonymous; reported
  people appear only as endpoint IDs or IP addresses. Records are private and
  reviewed yearly.

## 4. Recipients and processors (plan item D7)

| Recipient | Role | What | Contract and link | Location and transfer basis |
| --- | --- | --- | --- | --- |
| Cloudflare, Inc. | Processor | P1 hosting and CDN; Email Routing for P5 and P6 | Cloudflare Data Processing Addendum, v6.4 effective 3 April 2026, incorporated into the self-serve subscription agreement [T1]; sub-processors: <https://www.cloudflare.com/gdpr/subprocessors/> | Global network; United States: EU-U.S. Data Privacy Framework certification and Standard Contractual Clauses in the DPA [T1][T2] |
| Cloudflare, Inc. | Not confirmed | P2 public STUN at `stun.cloudflare.com`, used by browsers without any account or credential [T4] | Whether the DPA covers anonymous use of the public STUN service is not confirmed. Until Cloudflare confirms, this record treats Cloudflare as a separate recipient acting under its own privacy policy [T3]. | Anycast, global |
| [[RELAY_HOSTING_PROVIDER]] | Processor | P3 infrastructure | Provider's DPA, accepted in the account and filed privately. Examples: OVHcloud attaches its DPA to its contracts [T10]; Hetzner customers conclude the DPA in the account at `accounts.hetzner.com/account/dpa` [T11]. | [[RELAY_LOCATION]], EU, with an EU-headquartered provider (the VPS variant, section 1). No transfer; any non-EU sub-processor is covered by the provider's own safeguards. The Fly.io variant would change this row (section 1). |
| Mailbox provider (to be named when chosen) | Processor | P5, P6 | Provider's DPA, filed privately | To be recorded |
| GitHub, Inc. | Not a processor of user data | Source code and CI for the operator's own account | The GitHub Data Protection Agreement applies under the GitHub Customer Agreement [T12]; no Oxfer user data goes to GitHub | Not applicable |
| The other party to a transfer | Recipient chosen by the user | P4 | None | Wherever that user is; the data flows directly between the users' devices at their request |
| Competent authorities; NCMEC | Recipients case by case | P5 | Legal process; reporting under the abuse page | Section 5 |
| n0 (interim only) | Recipient, no contract | P3 in the interim configuration (section 1) | None | United States and other regions; no transfer mechanism identified. Ends when `P2P_RELAY_URL` is set. |

## 5. International transfers

- **Cloudflare:** relies on the EU-U.S. Data Privacy Framework (with the UK
  Extension and the Swiss-U.S. framework) and Standard Contractual Clauses in its
  DPA [T1][T2]. The General Court upheld the Framework's adequacy decision on
  3 September 2025 (Case T-553/23, *Latombe*); an appeal to the Court of Justice
  is pending [T8]. Watch item in the [README](README.md#review-cadence).
- **Relay:** in the EU, with an EU-headquartered provider; no transfer. This
  assumes the VPS variant; the Fly.io variant needs Fly.io, Inc.'s transfer
  basis recorded first (section 1).
- **NCMEC (United States):** reports of apparent child sexual abuse material
  contain the report, the link or endpoint ID in it and, only where needed,
  how to reach the reporter; never content (the operator holds none). No
  adequacy decision covers NCMEC: the Data Privacy Framework covers only
  certified organisations [T2], and NCMEC is not one. The privacy notice
  (section 8) relies on the derogation for transfers necessary for important
  reasons of public interest [T15, Art. 49(1)(d)], an interest that Union or
  Member State law must recognise [T15, Art. 49(4)]; the notice points to
  Directive 2011/93/EU on combating the sexual abuse and sexual exploitation of
  children [T23]. Confirm this basis with counsel before the first report; the
  [incident runbook](incident-runbook.md) limits what is sent.
- **Other peer:** data goes directly between the users' devices at their
  request; the operator does not transfer it.

## 6. Retention summary

| Data | Where | Retention |
| --- | --- | --- |
| HTTP request logs | Cloudflare | No request log kept for the operator: Cloudflare's request-log retention (Logpull) is off by default and stays off; security-event details for the period the plan sets (24 hours on Free and Pro); Cloudflare's own operational and security logs under its privacy policy [T3]; the operator exports none |
| Relay connection state (IP addresses, endpoint IDs) | Relay memory; key cache off | While connected |
| Rate-limit state (source addresses of new connections) | Relay host firewall sets | At most 60 seconds |
| Relay operational error log (no client addresses) | Relay host journald | At most 3 days |
| SSH login records (operator's logins; source addresses of failed attempts) | Relay host journald, and `/var/log/wtmp` and `/var/log/btmp` | At most 3 days (the login files are rotated daily); `lastlog` and `lastlog2` keep no persistent record |
| Relay TLS certificate (host name only) | Public Certificate Transparency logs | Permanent, public |
| Relay aggregate metrics | Relay host, localhost only | Not personal data |
| Denied endpoint IDs | `/etc/oxfer-relay/denylist.txt` on the relay host, merged into the relay's configuration by `setup.sh` | While the block is in force; kept through relay restarts, upgrades and reruns of `setup.sh`; recorded in the abuse log |
| Banned IP addresses | `/etc/nftables.d/bans.nft` on the relay host, each with an expiry (normally 30 days), loaded into the firewall | Until the expiry or until lifted; kept through reboots, firewall reloads and reruns of `setup.sh`; recorded in the abuse log |
| Abuse log and correspondence | Private records | As long as needed; reviewed at least yearly |
| Browser-local items | User's browser | Until the user clears them |

## 7. Security measures (Art. 32)

- End-to-end encryption of every transfer (DTLS on WebRTC paths; QUIC/TLS through
  the relay); the operator holds no keys.
- Bearer capability in the URL fragment, never sent to the web host; a new key
  per share and per receiving session.
- No accounts, analytics or third-party scripts; Content-Security-Policy (report
  only until the manual test matrix passes), `Permissions-Policy`,
  `X-Frame-Options`, `Referrer-Policy: no-referrer`, `nosniff` (`assets/_headers`).
- Deployment integrity: CI builds from `main`, records SHA-256 hashes of every
  served file, deploys with a scoped Cloudflare token, then compares every served
  file with the build (`verify-deployment.mjs`,
  `.github/workflows/oxfer-web.yml`).
- Account security: two-factor authentication on GitHub and Cloudflare, branch
  protection with required CI, yearly token rotation (owner actions in the
  [README](README.md#owner-actions)).
- Relay host: SSH keys only, firewall limited to the relay's ports, unattended
  security updates, metrics on localhost, journal and login records capped at
  three days, relay key cache off (relay runbook).
- Incident handling: [incident runbook](incident-runbook.md).

## 8. DPIA screening

### 8.1 EDPB-endorsed criteria (WP248 rev.01)

The EDPB endorsed the Article 29 Working Party's DPIA guidelines, which list nine
criteria; processing meeting two or more usually needs a DPIA [T13].

| Criterion | Met? | Reason |
| --- | --- | --- |
| Evaluation or scoring | No | No profiling. |
| Automated decisions with legal or similar effect | No | Blocks are decided by a person on a report. |
| Systematic monitoring | No | The relay forwards traffic; it does not observe behaviour and keeps no history. |
| Sensitive or highly personal data | No, for the operator | Files may be highly personal, but the operator only forwards ciphertext it cannot read. |
| Large scale | Not currently | To be re-checked at each review against traffic volume. |
| Matching or combining datasets | No | |
| Vulnerable data subjects | Possibly | Children may use Oxfer ([children's access assessment](osa-children-access.md)); nothing is collected about them. |
| Innovative technology | No | Relays, STUN and WebRTC are established techniques. |
| Prevents exercising a right or using a service | No | |

### 8.2 ANSPDCP list (Decision 174/2018)

None of the seven cases applies [T14]. The closest, large-scale or systematic
processing of traffic or location data, applies only when the processing is not
necessary for a service the data subject asked for; relaying a user's own
transfer is necessary for that service.

### 8.3 Conclusion

At most one criterion is possibly met, and no national-list case applies. A DPIA
is not required. Screen again before any change in section 7 of the
[illegal content risk assessment](osa-illegal-content.md#7-triggers-for-a-new-assessment-before-a-change),
or if relay traffic becomes large scale.

## 9. Representatives and data protection officer

### 9.1 EU

Not applicable. Article 27 concerns controllers not established in the Union
[T15, Art. 27].

### 9.2 United Kingdom (UK GDPR Art. 27)

A controller outside the UK needs a UK representative where UK GDPR Art. 3(2)
applies, unless the processing is occasional, excludes large-scale special
category data and is unlikely to result in a risk [T16]. Art. 3(2) turns on
whether goods or services are offered to people in the UK. The EDPB reads the
equivalent EU provision as requiring intentional targeting, not mere
accessibility [T17], and the ICO's territorial scope material follows the same
structure [T18].

Oxfer is offered from Romania under EU law, in English, free, with no UK
marketing, pricing or domain, and the terms say so (terms, section 8). The
controller's position is that Art. 3(2) UK GDPR is not engaged and no UK
representative is required. Two points are recorded neutrally:

- The operator complies with the UK Online Safety Act by choice (decision D2).
  That Act uses a different test, based on UK users and risk [OSA s.4], and does
  not decide the UK GDPR question.
- If Art. 3(2) were engaged, the occasional-processing exemption would be hard to
  rely on, because relay processing is continuous. Revisit if UK traffic becomes
  material or the service is promoted in the UK.

### 9.3 Switzerland (FADP Art. 14)

A private controller abroad needs a Swiss representative only if its processing
of data of people in Switzerland is connected with offering goods or services
there or monitoring behaviour there, and is extensive, regular and high risk;
the conditions are cumulative [T19]. The high-risk condition is not met on the
screening in section 8, so no representative is required.

### 9.4 Data protection officer

Not required: the core activities do not consist of large-scale regular and
systematic monitoring, or large-scale processing of special categories or
criminal-offence data [T15, Art. 37(1)].

## 10. Decision D4 and this record

If the operator is a company rather than a natural person, record its name,
registered office and registration number in the header. A company operating an
internet application in an organised, professional way and for economic purposes
must keep application access logs for six months under Brazil's Marco Civil
da Internet [T20, Art. 15]. Oxfer is free and non-commercial, but if the
operator becomes a company this point is to be reviewed with counsel before
Brazilian traffic becomes material, because it conflicts with the relay's
no-access-log design.

## 11. Review and sign-off

Reviewed with the other records at the cadence in the
[README](README.md#review-cadence).

| | |
| --- | --- |
| Approved by | [[OPERATOR_NAME]] |
| Date | [[EFFECTIVE_DATE]] |

## Sources

- [T1] Cloudflare Data Processing Addendum: <https://www.cloudflare.com/cloudflare-customer-dpa/>
- [T2] Data Privacy Framework List: <https://www.dataprivacyframework.gov/list>
- [T3] Cloudflare Privacy Policy: <https://www.cloudflare.com/policies/privacy/>
- [T4] Cloudflare Realtime TURN service (lists `stun.cloudflare.com:3478`): <https://developers.cloudflare.com/realtime/turn/>
- [T5] Cloudflare Network Error Logging: <https://developers.cloudflare.com/network-error-logging/>
- [T6] iroh documentation, public relays: <https://docs.iroh.computer/iroh-services/relays/public>
- [T7] `iroh-relay` 1.1.0 source (`src/main.rs`, `src/server/http_server.rs`, `src/server.rs`, `src/key_cache.rs`, `src/defaults.rs`) and `tracing-subscriber` 0.3 `EnvFilter::from_default_env`, as fetched into the workspace's Cargo registry; the relay kit in `deploy/relay/` (`config.toml`, `setup.sh`, `nftables.conf`).
- [T8] General Court, Case T-553/23 *Latombe v Commission*: <https://infocuria.curia.europa.eu/tabs/redirect/juris/liste.jsf?num=T-553%2F23>;
  WilmerHale on the appeal (secondary): <https://www.wilmerhale.com/en/insights/blogs/wilmerhale-privacy-and-cybersecurity-law/20251201-european-court-of-justice-to-review-challenge-to-eu-us-data-privacy-framework>
- [T9] Directive 2002/58/EC, Art. 5(3): <https://eur-lex.europa.eu/eli/dir/2002/58/oj?locale=en>;
  EDPB Guidelines 2/2023 on the technical scope of Art. 5(3): <https://www.edpb.europa.eu/system/files/2024-10/edpb_guidelines_202302_technical_scope_art_53_eprivacydirective_v2_en_0.pdf>
- [T10] OVHcloud, legal and privacy security: <https://www.ovhcloud.com/en/personal-data-protection/legal-privacy-security/>
- [T11] Hetzner, data privacy FAQ: <https://docs.hetzner.com/de/general/general-terms-and-conditions/data-privacy-faq/>
- [T12] GitHub Data Protection Agreement: <https://github.com/customer-terms/github-data-protection-agreement>
- [T13] EDPB, endorsed WP29 guidelines (WP248 rev.01): <https://www.edpb.europa.eu/endorsed-wp29-guidelines_en>;
  WP248 rev.01: <https://ec.europa.eu/newsroom/article29/item-detail.cfm?item_id=611236>
- [T14] ANSPDCP Decision 174/2018: <https://www.dataprotection.ro/servlet/ViewDocument?id=1556>
- [T15] Regulation (EU) 2016/679 (GDPR): <https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=celex%3A32016R0679>
- [T16] UK GDPR Art. 27: <https://www.legislation.gov.uk/eur/2016/679/article/27>
- [T17] EDPB Guidelines 3/2018 on territorial scope: <https://www.edpb.europa.eu/sites/default/files/files/file1/edpb_guidelines_3_2018_territorial_scope_after_public_consultation_en_1.pdf>
- [T18] ICO, territorial scope of the UK GDPR: <https://ico.org.uk/media2/migrated/4031113/ic-327905-y2y5-knowledge-hub-territorial-scope.pdf>
- [T19] FDPIC, representatives under Art. 14 FADP: <https://www.edoeb.admin.ch/en/representatives-in-accordance-with-article-14-fadp>;
  FADP: <https://www.fedlex.admin.ch/eli/cc/2022/491/en>
- [T20] Lei 12.965/2014 (Marco Civil da Internet): <https://www2.camara.leg.br/legin/fed/lei/2014/lei-12965-23-abril-2014-778630-publicacaooriginal-143980-pl.html>
- [T21] Fly.io, compliance documents (DPA): <https://fly.io/documents/>; Fly.io, Data Privacy Framework privacy policy: <https://fly.io/legal/data-privacy-framework/>
- [T22] Fly.io, logging overview (log search keeps logs for 7 days): <https://docs.fly.io/monitoring/logging-overview/>
- [T23] Directive 2011/93/EU on combating the sexual abuse and sexual exploitation of children and child pornography: <https://eur-lex.europa.eu/eli/dir/2011/93/oj>
- [T24] Cloudflare, enabling log retention (Logpull; "By default, your HTTP request logs are not retained"): <https://developers.cloudflare.com/logs/logpull/enabling-log-retention/>
- OSA s.4: <https://www.legislation.gov.uk/ukpga/2023/50/section/4>
