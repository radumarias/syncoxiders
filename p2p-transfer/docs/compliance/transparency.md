# Transparency and no-logs statement

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated below. |
| Operator | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]] |
| Contact for authorities | `abuse@oxfer.app` |
| Statement date | [[EFFECTIVE_DATE]] |

This statement describes what data about Oxfer's users exists, where, for how
long, and what the operator can and cannot provide in response to a request. It
matches the [privacy notice](../../privacy.html) (section 3) and the
[abuse page](../../abuse.html) (section 9) and adds the technical detail behind
them. The legal analysis is in the [ROPA](ropa.md). It describes the service with
production builds using the operator's relay (`relay.oxfer.app`); see section 5
for the interim configuration.

## 1. What exists per user or per transfer

Nothing held by the operator. In detail:

| Item | Exists? | Where and for how long |
| --- | --- | --- |
| Accounts, names, emails, phone numbers of users | No | Oxfer has no accounts. |
| Files, file names, file sizes | Not with the operator | On the sender's and recipient's devices. Transfers are end-to-end encrypted; the operator holds no keys. |
| Share links | Not with the operator | The link's fragment holds the endpoint ticket and the access capability; browsers do not send fragments to the web host. The operator sees a link only if someone reports it. |
| Which endpoint connected to which | Only while connected | The relay keeps connection state in memory for as long as a connection lasts. There is no access log. |
| IP addresses and endpoint IDs at the relay | Only while connected | The relay holds each connection's address and endpoint ID in memory while it lasts. With the shipped configuration it logs no client IP addresses, endpoint IDs or connection events. |
| Relay error log | Yes, without client addresses | The relay logs only its own operational errors (startup, certificate and task failures, and one line per malformed request on port 80), without client addresses. The host's journal deletes them after at most three days. |
| Relay host administration | Yes, briefly | The server's SSH service records the operator's own administrative logins, and the source addresses of failed login attempts. These concern the host, not relay users, and are deleted after at most three days. |
| Aggregate relay metrics | Yes | Counters such as total connections, in memory and bound to the relay host's localhost. No per-user data. |
| Relay TLS certificate | Public | Let's Encrypt publishes the certificate for `relay.oxfer.app` in public Certificate Transparency logs, as it does every certificate. It names the host only. |
| Website request logs | With Cloudflare, not the operator | Cloudflare processes requests to `oxfer.app` under its own log retention. The operator exports no logs; the dashboard shows aggregates. |
| STUN requests | With Cloudflare | `stun.cloudflare.com` sees the IP address and port of browsers that try a direct path. The operator receives nothing. |
| Abuse records | Yes, per report | Date, kind of report, the endpoint ID or address blocked, and the outcome; never content. Kept privately, reviewed yearly. |
| Relay blocks | Yes, while in force | Denied endpoint IDs in the relay configuration and banned IP addresses in the relay host's firewall, each traced to a report in the abuse log. |

## 2. What the operator can provide

- Confirmation that no data exists about a past transfer, share or user.
- The content of a report it received, and the actions it took.
- Prospective action from the moment of a valid request: blocking a share's
  endpoint ID or an IP address at the relay.

## 3. What the operator cannot provide

- The identity of any sender or recipient.
- Which IP addresses or endpoint IDs used the relay, or when. The relay logs
  neither.
- File contents, names or sizes, from any time.
- A list of shares or transfers, past or current.
- Decrypted traffic. The relay forwards encrypted QUIC packets whose keys only
  the two devices hold; direct WebRTC paths do not pass through the operator.

The relay software can be run with more verbose logging, which records client
IP addresses. The operator does not do so on the production relay, except
briefly while debugging a fault, after which the journal is cleared. The
relay's logging configuration is the `RUST_LOG` filter in
`deploy/relay/iroh-relay.service`, explained in the
[relay runbook](../../deploy/relay/README.md#what-is-logged); any change to
what the relay records by default will be reflected in the privacy notice and
in this statement before it takes effect.

## 4. How requests are handled

1. Requests arrive at `abuse@oxfer.app` or by post to the address in the
   header. Romanian authorities may write directly. Authorities elsewhere are
   asked to use mutual legal assistance or EU cooperation instruments, which reach
   the operator through Romanian authorities (abuse page, section 9.2).
2. Each request is checked for legal validity, scope and deadline and logged in
   the private abuse log.
3. The answer states what exists (section 1) and what was done. Deadlines set by
   the request are met; where more time is needed, an extension is asked for
   before the deadline.
4. Emergencies involving a risk to life are answered first, as far as the law
   allows.

Regimes that set their own deadlines, handled through the same channel:

| Request | Deadline | Source |
| --- | --- | --- |
| European Production Order (EU e-Evidence Regulation, from 18 August 2026) | 10 days; 8 hours in an emergency | [S1] |
| European Preservation Order | Preserve for 60 days, extendable by 30 | [S1] |
| EU orders to provide information under the Digital Services Act | As set in the order; the provider informs the authority of the effect given | [S2, Art. 10] |
| Ofcom information notice (UK) | As set in the notice | [S3, s.102] |
| eSafety removal notice for class 1 material (Australia) | 24 hours, or longer if allowed | [S4, s.109] |
| eSafety notice requiring risk assessment documents | As set in the notice | [S5, s.31] |

## 5. Interim configuration

Until the GitHub variable `P2P_RELAY_URL` is set, production builds use n0's
public relays and n0's address lookup at `dns.iroh.link`. In that configuration
n0, not the operator, sees relay connection metadata, and the operator cannot
block shares. n0 states that its public relays see source and destination IP
addresses, connection times and the amount of data, and cannot read the
encrypted traffic [S6]. This statement takes effect when production builds use
the operator's relay.

## 6. Figures

Published yearly, from the private abuse log, once the statement is adopted:

| Period | Reports received | Shares blocked | Addresses blocked | Referrals to NCMEC or authorities | Requests from authorities | Requests answered with "no data" |
| --- | --- | --- | --- | --- | --- | --- |
| *first period ending 12 months after [[EFFECTIVE_DATE]]* | | | | | | |

Whether the Digital Services Act's transparency reporting duty applies depends on
decision D4: micro and small enterprises are exempt [S2, Art. 15(2)]. The table
above is published either way.

## Sources

- [S1] eucrim, e-Evidence Regulation (EU) 2023/1543 and Directive (EU) 2023/1544 published (OJ L 191, 28.7.2023):
  <https://eucrim.eu/news/e-evidence-regulation-and-directive-published/>;
  EUR-Lex summary: <https://eur-lex.europa.eu/EN/legal-content/summary/electronic-evidence-in-criminal-proceedings.html>
- [S2] Regulation (EU) 2022/2065 (Digital Services Act): <https://eur-lex.europa.eu/legal-content/en/ALL/?uri=CELEX:32022R2065>
- [S3] Online Safety Act 2023 s.102: <https://www.legislation.gov.uk/ukpga/2023/50/section/102>
- [S4] Online Safety Act 2021 (Cth): <https://www.legislation.gov.au/C2021A00076/latest/text>
- [S5] DIS Standard 2024, F2024L00710: <https://www.legislation.gov.au/F2024L00710/asmade/text>
- [S6] iroh documentation, public relays: <https://docs.iroh.computer/iroh-services/relays/public>
