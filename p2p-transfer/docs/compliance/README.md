# Oxfer compliance records

Drafts of the assessments and records that
[`../compliance-plan.md`](../compliance-plan.md) calls for, with the status of
every plan item and the steps only the owner can take. Everything here is a
**DRAFT** until the operator fills the placeholders, dates and signs it. It is
not legal advice.

This directory is public (decision D5). Records here state facts and reasons.
Operational records (abuse log, incident log, correspondence, signed DPAs) stay
private: see [Private records](#private-records).

## Records

| File | What it is | Plan item |
| --- | --- | --- |
| [osa-illegal-content.md](osa-illegal-content.md) | UK Online Safety Act illegal content risk assessment | D1 |
| [osa-children-access.md](osa-children-access.md) | UK children's access assessment. Concludes that Oxfer is treated as likely to be accessed by children. | D2 |
| [osa-children-risk.md](osa-children-risk.md) | UK children's risk assessment, required by that conclusion. Needs an owner decision on measure PCU B4. | D2 (follow-on) |
| [esafety.md](esafety.md) | Australia: classification (DIS, not RES), DIS Standard risk assessment (Tier 3), Phase 2 code | D3 |
| [ropa.md](ropa.md) | GDPR Article 30 record, processor list, legitimate interest and DPIA screening, UK and Swiss representatives | D4, D7 |
| [transparency.md](transparency.md) | What exists per user or transfer, what authorities can obtain, how requests are handled | D5 |
| [incident-runbook.md](incident-runbook.md) | Incident scenarios, notification decision tree and deadlines, bundle verification | D6 |

Placeholders used throughout, the same as in the legal pages:
`[[OPERATOR_NAME]]`, `[[OPERATOR_ADDRESS]]`, `[[OPERATOR_REGISTRATION]]`,
`[[EFFECTIVE_DATE]]`, `[[RELAY_HOSTING_PROVIDER]]`, `[[RELAY_LOCATION]]`.
Lines marked *to be completed by the operator* need facts only the operator has.

## Status of every plan item

"Code" means the item is implemented in the named file; "Record" means it is
documented in this directory; "Owner" means it needs the owner (see
[Owner actions](#owner-actions)). Paths are relative to `p2p-transfer/`.

### A. Infrastructure

| # | Item | Status | Where |
| --- | --- | --- | --- |
| A1 | EU VPS for the relay | Owner | Owner action 3; runbook in `deploy/relay/README.md` |
| A2 | DNS for `relay.oxfer.app`, DNS-only | Owner | Owner action 3 |
| A3 | Official `iroh-relay` 1.1 with ACME, ports, no access log | Code, Owner | `deploy/relay/`; logging facts in [ropa.md](ropa.md#21-relay-logging-verified-in-the-relay-source) |
| A4 | STUN: Cloudflare only | Code | `src/webrtc.rs` `ICE_SERVERS` |
| A5 | Relay rate limits; open access except endpoint IDs on the server's denylist | Code | `deploy/relay/` configuration and `setup.sh` |
| A6 | Production builds use the relay | Code, Owner | `.github/workflows/oxfer-web.yml` passes `vars.P2P_RELAY_URL`; `build-web.sh`; owner sets the variable (owner action 3) |
| A7 | ICE server list without Google | Code | `src/webrtc.rs` |
| A8 | Several relays | Code | `src/node.rs` `RelayChoice::from_setting`, `parse_relay_list` |
| A9 | Diagnostics and docs for the custom relay | Code | `src/diagnostics.rs`, `src/node.rs` `relay_probe_urls`; `docs/diagnostics.md` and `docs/cloudflare-workers.md` to describe it |
| A10 | Uptime monitoring, including the WebSocket probe | Owner | Owner action 3, step 4 |
| A11 | Upgrade policy | Owner | `deploy/relay/README.md` |
| A12 | TURN only if measured | Not adopted | Issue #53; no TURN in the ICE list |

### B. Client and deployment hardening

| # | Item | Status | Where |
| --- | --- | --- | --- |
| B1 | CSP, report-only first | Code, Owner | `assets/_headers`, `assets/boot.js`, `package-cf-output.mjs` renders the relay into `connect-src`, `verify-deployment.mjs` asserts it; enforcing it is owner action 8 |
| B2 | `Permissions-Policy` | Code | `assets/_headers`, asserted by `verify-deployment.mjs` |
| B3 | `__p2p` debug handle only in debug builds or with `#dev` | Code | `src/webrtc.rs` `expose_debug_handle` |
| B4 | Supply chain: 2FA, branch protection, scoped and rotated token, bundle hashes | Code, Owner | Hashes: `.github/workflows/oxfer-web.yml` (run summary and artifact); account settings: owner actions 5 and 6 |
| B5 | Tell native senders their link contains their IP addresses | Code | `src/app.rs` share screen |
| B6 | Footer links to Privacy, Terms, Abuse, Source | Code | `src/app.rs` bottom bar on every screen (inline, or a Legal menu on narrow screens); a Report abuse link on the receive screens |

### C. Legal pages

| # | Item | Status | Where |
| --- | --- | --- | --- |
| C1 | Privacy notice | Code, Owner | `privacy.html`; placeholders: owner action 1 |
| C2 | Terms of use | Code, Owner | `terms.html` |
| C3 | Abuse, safety and law-enforcement page | Code, Owner | `abuse.html` |
| C4 | Clean paths `/privacy`, `/terms`, `/abuse` | Code | Workers static-assets HTML handling (no `_redirects`); checked by `verify-deployment.mjs` |
| C5 | Operator identity on each page | Owner | Decision D4, owner action 1; `build-web.sh` refuses to package while placeholders remain, and refuses filled pages unless `P2P_RELAY_URL` lists `https://relay.oxfer.app` |
| C6 | Honest "Technical details" relay sentence | Code | `src/app.rs` `show_how_it_works` (`RelayOperator`); only the browser app's text names Cloudflare, which serves the browser app and answers its STUN requests |

### D. Assessments and records

| # | Item | Status | Where |
| --- | --- | --- | --- |
| D1 | UK illegal content risk assessment | Record, Owner | [osa-illegal-content.md](osa-illegal-content.md) (multi-risk; measures for multi-risk services and the Crime and Policing Act 2026 duties included); sign at adoption |
| D2 | UK children's access assessment | Record, Owner | [osa-children-access.md](osa-children-access.md), [osa-children-risk.md](osa-children-risk.md); PCU B4 decision |
| D3 | Australia self-assessment | Record, Owner | [esafety.md](esafety.md) (the plan named it `esafety-dis.md`); Phase 2 section to complete |
| D4 | GDPR Article 30 record and DPIA screening | Record | [ropa.md](ropa.md) |
| D5 | Transparency and no-logs statement | Record, Code | [transparency.md](transparency.md); `privacy.html` section 3 and `abuse.html` section 9 |
| D6 | Incident runbook | Record | [incident-runbook.md](incident-runbook.md) |
| D7 | Processor list | Record, Owner | [ropa.md](ropa.md#4-recipients-and-processors-plan-item-d7); DPAs: owner action 4 |

### E. Registrations and market posture

| # | Item | Status | Where |
| --- | --- | --- | --- |
| E1 | Indonesia PSE registration | Owner | Owner action 9 |
| E2 | UK: no registration; answer Ofcom on time | Record | [incident-runbook.md](incident-runbook.md#6-request-from-an-authority-or-regulator), [transparency.md](transparency.md#4-how-requests-are-handled) |
| E3 | Markets not offered | Code | `terms.html` section 8 |
| E4 | Geoblocking lever documented, off by default | Code | `docs/cloudflare-workers.md` |
| E5 | Brazil: not directed at minors, no profiling | Code | `terms.html` section 3; `privacy.html` section 14.4 |

### F. Ongoing

| # | Item | Status | Where |
| --- | --- | --- | --- |
| F1 | Quarterly review | Owner | [Review cadence](#review-cadence) |
| F2 | Watch list | Owner | [Review cadence](#review-cadence) |
| F3 | Triggers that reopen everything | Record | [Review cadence](#review-cadence); [osa-illegal-content.md](osa-illegal-content.md#7-triggers-for-a-new-assessment-before-a-change) |

## Owner actions

Do them roughly in this order. Actions 1 and 3 are linked: the legal pages
describe the operator's own relay, so publish them (by filling the placeholders,
which unlocks the deploy guard) only when production builds use that relay.
`build-web.sh` enforces this: once the pages are filled, it refuses to package
them unless `P2P_RELAY_URL` lists `https://relay.oxfer.app`
([relay go-live guard](../cloudflare-workers.md#relay-go-live-guard)).
Otherwise the published privacy notice would describe a relay the production
build does not use. `build-web.sh` also refuses to package while any
placeholder remains, so no deploy succeeds with some placeholders filled and
others not. Prepare the fills in actions 1 and 3, then set the variable and
push them all in one commit (action 3, step 6).

### 1. Decide D4 and fill the placeholders

- **First, turn off Cloudflare Network Error Logging** for the `oxfer.app`
  zone. On 30 September 2026 responses carried Cloudflare's `nel` and
  `report-to` headers, which ask browsers to send reports of failed requests
  to `a.nel.cloudflare.com` [L17]. The privacy notice does not describe this,
  so it must be off before the placeholders are filled, and
  `verify-deployment.mjs` fails while either header is served
  ([ropa.md](ropa.md#22-network-error-logging)). Turn it off with the zone's
  **Network Error Logging** toggle in the dashboard, or with the zone setting
  `nel`:

  ```sh
  curl -sS -X PATCH "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/settings/nel" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    --data '{"value":{"enabled":false}}'
  curl -sI https://oxfer.app/ | grep -iE '^(nel|report-to):'   # expect no output
  ```

  The API call needs a token with *Zone Settings: Edit* on `oxfer.app`. The CI
  token from the Workers template (action 5) does not have it; use the
  dashboard or a short-lived token.
- Decide whether Oxfer is run by a natural person or a company. A company
  changes the e-commerce disclosures, brings Brazil's Marco Civil log rule into
  view ([ropa.md](ropa.md#10-decision-d4-and-this-record)), and decides whether
  the DSA's small-enterprise exemption from transparency reports applies
  ([transparency.md](transparency.md#6-figures)).
- Fill `[[OPERATOR_NAME]]`, `[[OPERATOR_ADDRESS]]`, `[[OPERATOR_REGISTRATION]]`
  (trade-register number and tax ID for a company; delete the line for a natural
  person), `[[EFFECTIVE_DATE]]`, `[[RELAY_HOSTING_PROVIDER]]` and
  `[[RELAY_LOCATION]]` in `privacy.html`, `terms.html`, `abuse.html` and every
  file in this directory. Keep the fills in a local commit and push it only in
  action 3, step 6, once `P2P_RELAY_URL` is set: pushed earlier, the deploy
  fails at one of the two guards above. List what is left with:

  ```sh
  grep -rnoE '\[\[[A-Z][A-Z0-9_]*\]\]' p2p-transfer/*.html p2p-transfer/docs/compliance
  ```

- Where a record names a person, a company operator writes the name of the
  individual it appoints, not the company's, as the legal pages' checklists
  already require for India and Singapore. Those places are ICU A2 (and the
  header's accountable individual), ICU A3 and ICU D12 in
  [osa-illegal-content.md](osa-illegal-content.md), including its statement
  of responsibilities in section 5.2; PCU A2 and PCU D13 in
  [osa-children-risk.md](osa-children-risk.md); who carried out the
  assessment in [esafety.md](esafety.md) section 5.1; and the "approved by"
  rows of every sign-off, which name the individual who signs for the company.
  The provider, controller, operator and owner rows keep the company's name.
- Confirm that Cloudflare's request-log retention (Logpull) is off for the
  `oxfer.app` zone, and leave it off: section 3 of the privacy notice says
  Cloudflare keeps no request log for the operator. Retention is off by
  default and Logpull exists only on the Enterprise plan [L27]. On that plan,
  check it with a token that has *Logs Read* and expect the flag to be
  `false`:

  ```sh
  curl -sS "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/logs/control/retention/flag" \
    -H "Authorization: Bearer $TOKEN"
  ```

- Complete the lines marked *to be completed by the operator*, then date and
  sign each record.
- Check section 4 of the privacy notice against the release build once, in a
  fresh browser profile with the developer tools' storage view open. Local
  storage stays empty until you pick a theme, then holds only `oxfer.theme.v1`.
  Oxfer sets no cookies; the only cookies are Cloudflare's security cookies,
  which the notice's section 6 describes, when those protections are on.
  Cache storage holds `oxfer-v3`. There is no IndexedDB database and no
  origin-private-file-system folder until you tick "Keep a copy here so I can
  continue later" and start saving; then both appear, named `oxfer-resume`.
  Looking for saved copies, exporting them and deleting them never create
  either. In a profile that used an older build, the legacy `app` and
  `egui_memory_ron` keys disappear after the first start.

### 2. Create the mailboxes

Create `privacy@oxfer.app`, `abuse@oxfer.app` and `ops@oxfer.app` with
Cloudflare Email Routing [L1]. `ops@oxfer.app` is the relay's Let's Encrypt
contact (`contact` in `deploy/relay/config.toml`); iroh-relay refuses to start
in Let's Encrypt mode without one, so create it before action 3.

1. Cloudflare dashboard, Email Routing, Destination addresses: add the mailbox
   you read and verify it from the email Cloudflare sends.
2. Routing rules: create `privacy`, `abuse` and `ops` on `oxfer.app`, each
   forwarding to that destination.
3. Send a test message to each address from an outside account and reply to it.
4. Record the mailbox provider in [ropa.md](ropa.md#4-recipients-and-processors-plan-item-d7)
   and file its DPA (action 4).

### 3. Provision the relay and switch production to it

1. Order the VPS and set it up with the runbook in
   [`deploy/relay/README.md`](../../deploy/relay/README.md) (plan items A1, A3,
   A5, A11). Use the kit's VPS variant with an EU-headquartered provider: the
   legal pages and these records describe it. Do not use the kit's Fly.io
   variant until the statements it would make false are changed: the
   processor and transfer (Fly.io, Inc. is a US company), address blocking and
   per-address limits (the relay sees Fly's proxy address, not the client's),
   and log retention (Fly's log pipeline, not a three-day host journal). The
   complete list of pages and records to change is "Before using Fly" in the
   relay runbook's
   ["Fly.io variant"](../../deploy/relay/README.md#flyio-variant) section;
   [ropa.md](ropa.md#1-configuration-covered) gives the reasons.
2. Cloudflare DNS: add `A` and `AAAA` records for `relay.oxfer.app`, proxy status
   **DNS only** (A2).
3. Check the relay from the app's Diags page with a local build that sets
   `P2P_RELAY_URL=https://relay.oxfer.app`, and never deploy it. While
   placeholders remain, `build-web.sh` packages only with
   `OXFER_ALLOW_PLACEHOLDERS=1`:
   `OXFER_ALLOW_PLACEHOLDERS=1 P2P_RELAY_URL=https://relay.oxfer.app npm run build`.
4. Monitoring (A10), alerting by email:
   - an external uptime monitor for `https://relay.oxfer.app/healthz`,
     expecting status 200 and the text `"ok"`, with certificate-expiry
     checking;
   - a WebSocket probe against `wss://relay.oxfer.app/relay` with the
     `iroh-relay-v1` subprotocol, the check the Diags page runs, from a monitor
     that supports it or a scheduled job running the `node -e` command in the
     relay runbook's
     ["First-boot verification"](../../deploy/relay/README.md#first-boot-verification).
5. Fill `[[RELAY_HOSTING_PROVIDER]]` and `[[RELAY_LOCATION]]`, in the same
   local commit as the fills of action 1. Do not push it yet.
6. GitHub repository, Settings, Secrets and variables, Actions, **Variables**:
   create `P2P_RELAY_URL` with value `https://relay.oxfer.app` [L2]. Then push
   the commit that fills every placeholder (action 1 and step 5). That push
   deploys: `build-web.sh` refuses to package while any placeholder remains,
   and once they are all filled it requires this variable. The deploy job
   then checks that the served `Content-Security-Policy-Report-Only` names the
   relay in `connect-src`. Until this push succeeds, production keeps the
   previous build.

### 4. Accept and file the DPAs

| Provider | What to do |
| --- | --- |
| Cloudflare | The DPA is part of the self-serve subscription agreement [L3]. Save a PDF of the current version (6.4, effective 3 April 2026) with the date. Ask Cloudflare whether it covers the public STUN service; record the answer in [ropa.md](ropa.md#4-recipients-and-processors-plan-item-d7). |
| VPS provider | Accept its DPA in the customer account (Hetzner: `accounts.hetzner.com/account/dpa` [L4]; OVHcloud attaches its DPA to the contract [L5]) and save a copy. |
| Mailbox provider | Accept its DPA and save a copy. |
| GitHub | No Oxfer user data goes to GitHub. Its DPA applies under the GitHub Customer Agreement [L6]; save a copy only if the account is on a plan it covers. |

File them in the private records, and re-check them at each quarterly review.

### 5. Secure the accounts (plan item B4)

1. GitHub: turn on two-factor authentication with an authenticator app or a
   security key [L7].
2. Cloudflare: turn on two-factor authentication [L8].
3. Branch protection on `main` [L9]: require status checks to pass before merging
   (the `ci` jobs), and turn on "Do not allow bypassing the above settings" if the
   rule should bind the owner too.
4. Cloudflare API token for CI: created from the "Edit Cloudflare Workers"
   template [L10], used only as the `CLOUDFLARE_API_TOKEN` secret.

### 6. Rotate the Cloudflare API token every year

Dashboard, My Profile, API Tokens, the token's menu, **Roll**; the old secret
stops working [L11]. Paste the new value into the GitHub Actions secret
`CLOUDFLARE_API_TOKEN` and run the `oxfer-web` workflow to confirm. Set a
calendar reminder. Roll at once in the scenario in
[incident-runbook.md](incident-runbook.md#1-compromised-build-or-deploy-pipeline-or-cloudflare-api-token).

### 7. Keep the bundle hashes

Each deploy records SHA-256 hashes in the run summary and the artifact
`oxfer-web-sha256-<commit>`. GitHub keeps them at most 90 days in a public
repository [L12]. Copy each production run's file to the private records or
attach it to a GitHub release. Verification steps:
[incident-runbook.md](incident-runbook.md#8-verifying-what-oxferapp-serves).

### 8. Enforce the Content-Security-Policy

1. Run the manual matrix with the browser console open and note every
   `Content-Security-Policy-Report-Only` violation: current Chrome, Firefox and
   Safari on desktop, Safari on iOS, Chrome on Android. On each: load the app;
   share and receive over a direct path and over the relay; each download route
   (`sink=fsa`, `sink=sw`, `sink=mem`); resume a saved copy; the Diags page; the
   theme page; the three legal pages; keep-screen-on during a transfer.
2. Fix or accept each violation.
3. Switch to enforcement with the steps in
   [cloudflare-workers.md, "Enforcing the policy"](../cloudflare-workers.md#enforcing-the-policy).
   They change three files in one commit: the header name in `assets/_headers`,
   the `CSP_REPORT_ONLY` constant in `package-cf-output.mjs` (which
   `verify-deployment.mjs` checks), and the assertion in
   `tests/package-cf-output.test.mjs` that no enforcing header exists, which
   otherwise fails CI. Run the tests and `bash build-web.sh`, then deploy.

### 9. Register in Indonesia (plan item E1)

- Law: Minister of Communication and Informatics Regulation 5 of 2020 on private
  scope electronic system operators. A foreign operator that provides services
  in Indonesia or whose system is used there must register, giving its identity,
  the identity of its management or person in charge, a tax identification
  number, and its number of Indonesian users and transaction value, with
  supporting documents translated into Indonesian by a sworn translator [L13].
- Portal: Komdigi's PSE registration site, `pse.komdigi.go.id` [L14]. Secondary
  guides disagree on whether foreign operators file there directly or through
  OSS-RBA, and on whether a local point of contact is expected [L15]. Follow the
  portal's current instructions.
- Stop and revisit decision D3 if registration requires an Indonesian entity or
  a local representative: D3 assumed a free registration without either.
- Expect a cost for sworn translation. Komdigi has been warning unregistered
  providers and blocking is the sanction [L16].

### 10. Other items that need the owner

- **PCU B4.** Choose an option in
  [osa-children-risk.md](osa-children-risk.md#6-decision-needed-pcu-b4).
- **Old Pages project.** `oxfer.pages.dev` still answered on 30 September 2026. It
  serves an earlier build that predates these changes, with no legal pages and
  none of the security headers. Delete the Pages project, or keep it deployed
  from the same build as production.
- **Zone settings that rewrite pages.** `assets/_headers` sends
  `Cache-Control: no-transform` for the legal pages (`/privacy`, `/terms` and
  `/abuse`) only, so Cloudflare Email Address Obfuscation and similar
  rewriting cannot alter them, including their `mailto:` links [L23]; the
  app's wasm and JavaScript keep Cloudflare's compression. Still check the
  `oxfer.app` zone settings once
  ([cloudflare-workers.md](../cloudflare-workers.md#zone-settings-that-rewrite-responses)):
  Email Address Obfuscation off, and no other feature that edits HTML.
  `verify-deployment.mjs` fails if a served file differs from the build.
- **NCMEC transfers.** The privacy notice relies on GDPR Art. 49(1)(d)
  (important reasons of public interest). Confirm that basis with counsel
  before the first report ([ropa.md](ropa.md#5-international-transfers)).
- **Australia Phase 2 code.** Complete [esafety.md](esafety.md#7-phase-2-dis-online-safety-code-class-1c-and-class-2-material).
- **EU e-Evidence.** The Regulation applies from 18 August 2026, and European
  Production and Preservation Orders are addressed directly to a designated
  establishment or legal representative of the provider [L18, Art. 7(1)]. The
  companion Directive had to be transposed by 18 February 2026; it requires
  providers established in the Union *with legal personality* to designate an
  establishment, by 18 August 2026 for those already offering services
  [L24, Art. 3(1)(a) and (6)]. Decision D4 therefore matters: check
  Romania's transposing law for what it asks of a company, or of a natural
  person, established in Romania, and register the designation if needed.
- **Acknowledgement template (ICU D5).** The abuse page says that Oxfer never
  tells the sender or any other user that a report was made or who made it
  (section 2, ICU D3), what the acknowledgement lists, and that a reporter can
  opt out of further messages (section 3, ICU D5 and D6), and
  [osa-illegal-content.md](osa-illegal-content.md#51-codes-measures-that-apply)
  records the three measures as adopted. Before adoption, write the
  acknowledgement template the page describes: the possible outcomes (no
  action; a block of the share or of addresses at the relay; referral to NCMEC
  or authorities; a reply explaining the decision), confirmation that the
  decision and any action will be sent, and the timeframe (15 days; 7 days for
  India). Keep it with the private records. The
  [incident runbook](incident-runbook.md#4-abuse-report) covers opt-outs.
- **India IT Rules timelines.** The abuse page (section 10) promises
  acknowledgement within 24 hours and resolution within 7 days
  [L28, r.3(2)(a)(i)], and says only that complaints about intimate or
  impersonating content are handled first. The rules as amended on
  10 February 2026 set two shorter windows [L28]: under r.3(2)(b), reasonable
  and practicable measures to remove or disable access within 2 hours of a
  complaint by the person shown, or by someone on their behalf, about content
  showing their private areas, nudity or sexual acts, or impersonating them
  (including morphed images); and under the proviso to r.3(2)(a)(i),
  resolution within 36 hours of a complaint asking for removal of content
  under r.3(1)(b), except its sub-clauses (i), (iv) and (xi). Decide whether
  to commit to both. If yes, state them in `abuse.html` section 10 and in the
  [incident runbook](incident-runbook.md) (section 4, step 10, and the India
  row of section 9.2). If no, record the reason in that India row, for
  example that Oxfer is not offered to India specifically (the position in
  the runbook's section 9.1).
- **Measures for a multi-risk service.** The illegal content risk assessment
  rates Oxfer multi-risk, which adds ICU A3, A5 to A7, C3 to C8 and D8. They
  are written into its section 5.2 for a one-person service; they need no
  setup beyond signing the record and holding the quarterly review below.
- **Shell history on the relay host.** Block commands typed in an
  interactive shell on the relay (`oxfer-relay-ban add ADDRESS DAYS`, edits to
  `/etc/oxfer-relay/denylist.txt`) stay in that account's shell history with no
  time limit, and `sudo` records them in the journal for three days
  ([relay README, What is logged](../../deploy/relay/README.md#what-is-logged)).
  The privacy notice (section 3) and abuse page (section 9.1) describe block
  lists only. Decide how to keep the two aligned: for example run block
  commands non-interactively from your own machine, change the host's shell
  history settings yourself, or add shell history to those sections. The kit
  deliberately does not change audit or history settings on your behalf.

## Review cadence

| When | What |
| --- | --- |
| Quarterly (F1) | Read the plan, the three legal pages and these records against the code; re-check the DPAs, the processor list and the Cloudflare UK share of requests; copy bundle hashes. From the abuse log, count reports by kind of harm against earlier quarters (ICU A5) and check the performance targets for reports and appeals (ICU C4, D8), and write the review note. |
| Every 12 months at most | UK children's access assessment, as the provider's policy (OSA s.36(3) requires yearly repeats only while a service is not treated as likely to be accessed by children, s.36(2)); UK illegal content and children's risk assessments; Australian assessment; yearly transparency figures; review what the abuse log keeps; re-read the records and Ofcom's current Codes and guidance (ICU A7). |
| Before a change | Any trigger in [osa-illegal-content.md](osa-illegal-content.md#7-triggers-for-a-new-assessment-before-a-change) or plan item F3: accounts, analytics, ads, payments or donations, a store-and-forward relay, messaging, or roughly 100,000 monthly visitors from one country. Redo the assessments and the [DPIA screening](ropa.md#8-dpia-screening) first. |
| When a regulator publishes | Ofcom risk profile or code changes; eSafety codes; EDPB guidance. |

Watch list (F2), as of 30 September 2026. Items without a source are carried
over from the plan.

- EU regulation on child sexual abuse (interpersonal communications services).
- Ofcom: its additional safety measures, consulted on in 2025 [L20]. From
  them, hash matching for intimate image abuse (ICU C14) is in the Codes from
  30 September 2026 [L21], and crisis response measures (ICU C15, C16) were
  laid in June 2026 [L25]; neither applies to Oxfer on its current ratings
  ([osa-illegal-content.md](osa-illegal-content.md#51-codes-measures-that-apply)),
  but both turn on risk levels that a review could change. Also notices under
  OSA section 121.
- UK: regulations under OSA s.20A(2)(f) or (3) on what an intimate image
  content report must contain; none identified on 30 September 2026 [L26].
- EU-U.S. Data Privacy Framework: Latombe's appeal to the Court of Justice
  (Case C-703/25 P) against the General Court's judgment upholding it [L22].
- India: DPDP Rules duties from mid-May 2027
  ([incident-runbook.md](incident-runbook.md#92-deadlines)); CERT-In directions.
- Australia: Phase 2 codes in force since 9 March 2026 ([esafety.md](esafety.md)).
- Brazil: ECA Digital enforcement; Marco Civil if D4 is a company.
- US state privacy and child-safety laws.

## Private records

Keep outside this public repository, in a private repository or encrypted
storage that only the operator can open:

- the abuse log (date and time, kind of report, endpoint ID or address
  blocked, outcome; never content and never a link's `cap=` part) and the
  relay denylist history;
- the quarterly review notes (ICU A5 counts, performance against targets) and
  the acknowledgement template;
- the incident log;
- correspondence with reporters, authorities, regulators and NCMEC, which
  alone holds a reported link and a reporter's contact details;
- signed or accepted DPAs, with dates and versions;
- bundle hash files for production deploys;
- filled-in versions of these records, if the operator prefers not to publish
  some facts (decision D5), with this directory keeping the public version.

The blocks in force also live on the relay host, outside this repository:
endpoint IDs in `/etc/oxfer-relay/denylist.txt` and IP bans in
`/etc/nftables.d/bans.nft`. The abuse log is the record from which they are
restored when the host is rebuilt
([incident-runbook.md](incident-runbook.md#2-compromised-relay-host)).

Keep each for as long as needed to show how a matter was handled, at least two
years after the end of the calendar year of the action for anything the
Australian DIS Standard requires [L19, s.38], and review what is kept every year.

## Sources

- [L1] Cloudflare, Email Routing rules and addresses: <https://developers.cloudflare.com/email-service/configuration/email-routing-addresses/>
- [L2] GitHub, store information in variables: <https://docs.github.com/actions/learn-github-actions/variables>
- [L3] Cloudflare Data Processing Addendum: <https://www.cloudflare.com/cloudflare-customer-dpa/>
- [L4] Hetzner, data privacy FAQ: <https://docs.hetzner.com/de/general/general-terms-and-conditions/data-privacy-faq/>
- [L5] OVHcloud, legal and privacy security: <https://www.ovhcloud.com/en/personal-data-protection/legal-privacy-security/>
- [L6] GitHub Data Protection Agreement: <https://github.com/customer-terms/github-data-protection-agreement>
- [L7] GitHub, about two-factor authentication: <https://docs.github.com/en/authentication/securing-your-account-with-two-factor-authentication-2fa/about-two-factor-authentication>
- [L8] Cloudflare, set up 2FA: <https://developers.cloudflare.com/learning-paths/application-security/account-security/set-up-2fa/>
- [L9] GitHub, managing a branch protection rule: <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/managing-a-branch-protection-rule>
- [L10] Cloudflare, create an API token: <https://developers.cloudflare.com/fundamentals/api/get-started/create-token/>
- [L11] Cloudflare, roll API tokens: <https://developers.cloudflare.com/fundamentals/api/how-to/roll-token/>
- [L12] GitHub, artifact and log retention: <https://docs.github.com/en/github/administering-a-repository/managing-repository-settings/configuring-the-retention-period-for-github-actions-artifacts-and-logs-in-your-repository>
- [L13] Komdigi legal database, Regulation 5 of 2020: <https://jdih.komdigi.go.id/produk_hukum/view/id/759/t/peraturan+menteri+komunikasi+dan+informatika+nomor+5+tahun+2020>
- [L14] Komdigi, PSE registration: <https://pse.komdigi.go.id/>
- [L15] XPND, PSE registration for foreign digital companies 2026 (secondary): <https://xpnd.co.id/guides/pse-registration-foreign-digital-companies-2026/>;
  Legal Indonesia, PSE registration (secondary): <https://legalindonesia.id/pse-registration-indonesia/>
- [L16] Digital Watch, Komdigi deadline for 25 providers (secondary): <https://dig.watch/updates/indonesia-gives-service-providers-deadline>
- [L17] Cloudflare, Network Error Logging: <https://developers.cloudflare.com/network-error-logging/>
- [L18] Regulation (EU) 2023/1543 (e-Evidence Regulation), EUR-Lex: <https://eur-lex.europa.eu/eli/reg/2023/1543/oj>; eucrim, e-Evidence Regulation and Directive (secondary): <https://eucrim.eu/news/e-evidence-regulation-and-directive-published/>
- [L19] DIS Standard 2024, F2024L00710: <https://www.legislation.gov.au/F2024L00710/asmade/text>
- [L20] Ofcom, consultation on additional safety measures: <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/online-safety-additional-safety-measures>
- [L21] Ofcom, statement on detecting intimate image abuse: <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/detecting-intimate-image-abuse/statement-detecting-intimate-image-abuse.pdf?v=418076>; the amendment adding ICU C14 (GOV.UK; in force at the end of 21 days from issue, and issued on 9 September 2026): <https://www.gov.uk/government/publications/online-safety-act-draft-amendments-to-the-illegal-content-codes-of-practice/draft-amendments-to-illegal-content-codes-of-practice-for-user-to-user-services>
- [L22] WilmerHale, Court of Justice to review the Data Privacy Framework (secondary): <https://www.wilmerhale.com/en/insights/blogs/wilmerhale-privacy-and-cybersecurity-law/20251201-european-court-of-justice-to-review-challenge-to-eu-us-data-privacy-framework>
- [L23] Cloudflare, Email Address Obfuscation (not applied with `Cache-Control: no-transform`): <https://developers.cloudflare.com/waf/tools/scrape-shield/email-address-obfuscation/>
- [L24] Directive (EU) 2023/1544 on designated establishments and legal representatives for gathering electronic evidence, EUR-Lex: <https://eur-lex.europa.eu/eli/dir/2023/1544/oj>
- [L25] Ofcom, draft amendments (No. 2) to the Illegal content Codes of Practice for user-to-user services (crisis response), GOV.UK: <https://www.gov.uk/government/publications/online-safety-act-draft-amendments-no2-to-the-illegal-content-codes-of-practice/draft-amendments-no-2-to-the-illegal-content-codes-of-practice-for-user-to-user-services>
- [L26] OSA s.20A (as inserted by the Crime and Policing Act 2026): <https://www.legislation.gov.uk/ukpga/2023/50/section/20A>
- [L27] Cloudflare, enabling log retention (Logpull; "By default, your HTTP request logs are not retained"): <https://developers.cloudflare.com/logs/logpull/enabling-log-retention/>; Logpull is available on the Enterprise plan: <https://developers.cloudflare.com/logs/logpull/>
- [L28] Information Technology (Intermediary Guidelines and Digital Media Ethics Code) Rules, 2021, as updated on 10 February 2026 (G.S.R. 120(E)), MeitY: <https://www.meity.gov.in/static/uploads/2026/02/550681ab908f8afb135b0ad42816a1c9.pdf>
- OSA s.36: <https://www.legislation.gov.uk/ukpga/2023/50/section/36>
