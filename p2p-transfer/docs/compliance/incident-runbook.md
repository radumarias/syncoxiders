# Incident runbook

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated below. |
| Owner | [[OPERATOR_NAME]] |
| Version date | [[EFFECTIVE_DATE]] |

Steps for the incidents most likely to affect Oxfer, and the notification
decisions and deadlines that may follow. Records go in the private incident log
and abuse log (see [README](README.md#private-records)), never in this public
repository. What the operator holds is described in the
[transparency statement](transparency.md); the processing is in the
[ROPA](ropa.md).

First hour, for any incident: open an entry in the private incident log with the
time you became aware; that time starts the deadlines in section 9.

## 1. Compromised build or deploy pipeline, or Cloudflare API token

A tampered bundle served from `oxfer.app` is the one realistic way the
encryption claims become false: modified code could read files or share links in
the user's browser.

**Detection**

- `verify-deployment.mjs` fails in the deploy job, or when run by hand
  (section 8).
- A Worker version or deployment you did not make (`npx cf workers get oxfer`,
  or the Cloudflare dashboard).
- A commit on `main`, a workflow change, or an Actions run you did not start.
- Alerts from GitHub or Cloudflare about sign-ins or token use.

**Containment**

1. Roll the Cloudflare API token: dashboard, My Profile, API Tokens, the token's
   menu, Roll; the old secret stops working [K1]. Update the GitHub Actions secret
   `CLOUDFLARE_API_TOKEN`.
2. If GitHub is involved: revoke personal access tokens, SSH keys and OAuth app
   grants you do not recognise; review the account's security log; check branch
   protection on `main` is still on [K2].
3. Stop further deploys until the source is clean: disable the `oxfer-web`
   workflow in the Actions tab if needed.

**Recovery**

1. Identify the last known-good commit. Review every commit and workflow change
   since then.
2. Run the `oxfer-web` workflow on the known-good commit (Actions, oxfer-web, Run
   workflow). It rebuilds, deploys and runs `verify-deployment.mjs` against the
   three production origins.
3. Confirm with the hash check in section 8 from a machine outside the pipeline.
4. Users' browsers may keep a tampered service worker and its cache until they
   fetch the clean `sw.js`, which is served with `no-store` and re-checked by
   browsers on navigation. Until then a tampered worker can keep serving tampered
   files. Consider a temporary `Clear-Site-Data` response header after recovery;
   test it first, because clearing site storage also deletes users' saved
   resumable copies.

**Communication**: this is a likely personal data breach with high risk if the
tampered code could reach files or links. Go to section 9 at once.

## 2. Compromised relay host

**Detection**: unexpected processes, SSH logins or configuration changes;
provider abuse notice; uptime monitor showing a changed certificate.

**What an attacker gains**: connection metadata (IP addresses, endpoint IDs,
times, volume) for as long as they control the host, and the ability to drop or
delay traffic. Not file contents: the relay forwards encrypted traffic whose keys
only the two devices hold, and iroh connections authenticate each endpoint's key,
so the relay cannot impersonate a peer.

**Containment**

1. Take the host offline from the provider's console (browser transfers stop
   until the relay returns).
2. Keep a disk snapshot if the provider offers one, for analysis.
3. Rotate every SSH key that had access.

**Recovery**: build a fresh host from [`deploy/relay/`](../../deploy/relay/README.md),
point the DNS records at it, and let the relay obtain new certificates. Then
restore the blocks still in force from the private abuse log, not from the
compromised host: endpoint IDs into `/etc/oxfer-relay/denylist.txt` followed by
`setup.sh --denylist`, and addresses with `oxfer-relay-ban add` and their
remaining days (section 4, steps 7 and 8). Check it with the app's Diags page.

**Communication**: metadata may have been exposed; go to section 9.

## 3. Domain or DNS hijack

**Detection**: `oxfer.app` or `relay.oxfer.app` resolving to addresses you do not
control; `verify-deployment.mjs` failing; certificates for your names that you
did not request (certificate transparency logs); registrar notices.

**Containment**

1. Contact the registrar and Cloudflare support; restore the name servers and
   records; enable registrar lock.
2. Ask the issuing certificate authority to revoke certificates you did not
   request.
3. Rotate Cloudflare and registrar credentials.

**Recovery**: redeploy and verify as in section 1.

**Communication**: users who opened the app from a hijacked name may have run
untrusted code; treat as section 1.

## 4. Abuse report

Timelines are those promised on the [abuse page](../../abuse.html), section 3,
and for India section 10. The order in which reports are handled, and the
targets, are in section 5.2 of the
[illegal content risk assessment](osa-illegal-content.md#52-responsibilities-code-of-conduct-and-moderation-policies)
(ICU C4, C5).

1. **Log it** in the private abuse log: the date and time received and the
   kind of report. Later steps add the endpoint ID or address blocked (steps
   7 and 8) and the outcome (steps 10 and 11). The log holds nothing else
   (the code of conduct in section 5.2 and the list in
   [section 6](osa-illegal-content.md#6-step-4-records-reporting-and-review)
   of the illegal content risk assessment): the link part and the reporter's
   contact stay only in the correspondence. Never record the `cap=` part of a
   link, and never any content.
2. **Opt-out.** If the reporter asks for no further messages, in the report or
   later (for example by replying "No further messages"), note it with the
   correspondence and from then on send nothing about the report, not even the
   acknowledgement or the decision. Handle the report in the same way
   otherwise (abuse page, section 3; ICU D6).
3. **Acknowledge within 24 hours** (unless the reporter opted out), from the
   template kept with the private records. The acknowledgement lists the
   possible outcomes: no action; a block at the relay, of the share or of
   network addresses; a referral to NCMEC or other authorities; a reply that
   explains the decision. It confirms that the reporter will be told the
   decision and any action taken, and gives the timeframe: 15 days, or 7 days
   for a complaint from India (ICU D4, D5).
4. **Do not open or download the reported link**, and do not tell the person
   who shared it, or anyone else, that a report was made or who made it
   (abuse page, section 2).
5. **Read the share's endpoint ID offline.** The relay blocks a share by its
   endpoint ID, which is encoded in the link's ticket and cannot be read by eye.
   From `p2p-transfer/`, pass only the part of the link before `&cap=`:

   ```sh
   cargo run -q -p p2p-transfer --example ticket-endpoint-id -- 'endpoint…'
   ```

   It prints the endpoint ID as 64 hexadecimal digits, the form the relay's
   denylist accepts, and nothing else. It opens no connection and never prints
   the `cap=` part. It also accepts the whole link or its fragment, but the
   shell then keeps the capability in its history: to avoid that, pass `-`
   and paste the link on standard input.
6. **Decide**, in the order and within the targets of the prioritisation
   policy.
7. **To block a share,** add its endpoint ID to `/etc/oxfer-relay/denylist.txt`
   on the relay host and run `sh /opt/oxfer-relay/setup.sh --denylist`, which
   merges the list into `/etc/iroh-relay/config.toml` and restarts the relay.
   The restart briefly interrupts transfers running through it. The list is
   kept through relay restarts, upgrades and reruns of `setup.sh`
   ([relay runbook, "Abuse blocking"](../../deploy/relay/README.md#abuse-blocking)).
8. **To block an address,** for repeated or serious abuse only, use the relay
   host's firewall: `oxfer-relay-ban add ADDRESS` bans it for 30 days (another
   number of days can be given; every ban expires, and running the command
   again sets a new expiry), puts it in the nftables sets `banned_v4` or
   `banned_v6`, and writes it with its expiry to `/etc/nftables.d/bans.nft`,
   so it survives reboots, firewall reloads and reruns of `setup.sh`. Addresses
   can be blocked only there: the relay's denylist accepts endpoint IDs, not
   addresses
   ([relay runbook, "Abuse blocking"](../../deploy/relay/README.md#abuse-blocking)).
   Record the address in the abuse log, and the block with its expiry as the
   outcome.
9. **Intimate images shared without consent** (an intimate image content
   report, abuse page section 6): block the reported share as soon as
   reasonably practicable and no later than 48 hours after the report was
   received [K18, s.10(3A)], and block any other shares the reporter gives as
   carrying the same images. If the report lacks some of the declarations,
   act on it anyway and ask for the rest. If you conclude that the content is
   not an intimate image shared without consent, or that the reporter is
   neither the person shown nor acting for them, do not block under this rule:
   record the reasons, tell the reporter, and point to the expedited
   complaints procedure [K18, s.10(3B)].
10. **Answer** with the decision and any action taken, within 15 days, and
    record the outcome. A complaint from India is resolved within 7 days of
    receipt [K19, rule 3(2)(a)(i)], and one about content showing a person's
    private areas, nudity or sexual acts, or impersonating a person, is
    handled first (abuse page, section 10). Two shorter Indian windows are an
    open owner decision
    ([README, section 10](README.md#10-other-items-that-need-the-owner)):
    2 hours for such a complaint made by the person shown or on their behalf
    [K19, rule 3(2)(b)], and 36 hours for a request to remove content under
    rule 3(1)(b), except its sub-clauses (i), (iv) and (xi)
    [K19, proviso to rule 3(2)(a)(i)]. Until the owner decides, the abuse
    page promises neither.
11. **Appeals** (subject "Appeal"): review promptly and within 15 days; lift a
    wrong block and say so; record the outcome. The answer explains the
    decision but never says whether anyone made a report or who (abuse page,
    section 8).
12. **Expedited complaints** (subject "URGENT Complaint") from a person who
    made an intimate image content report, about that content or how the
    report was handled: handle before other messages and send the outcome
    within 48 hours. The Act requires this expedited procedure
    [K18, s.21(2A)]; the 48 hours is the abuse page's own promise
    (section 8).

## 5. Report of apparent child sexual abuse material

1. Do not open, download or ask for the material. Do not forward it.
2. Block the share's endpoint ID at the relay immediately (section 4, steps 5
   and 7), and any address involved that you know (section 4, step 8).
3. Report to NCMEC's CyberTipline [K3] as soon as reasonably possible. US law
   requires this of providers within its scope once they have actual knowledge,
   with no duty to search for material [K4]; the operator reports on the same
   basis. Send the report text and the endpoint ID; the operator holds no
   content. Registration as an electronic service provider (`espteam@ncmec.org`)
   is needed only to upload content with a report [K3].
4. Report to the Romanian police, and point the reporter to the national hotline
   in the INHOPE network (in Romania, Ora de Net, run by Salvați Copiii:
   <https://oradenet.ro/linia-de-raportare/>, as on the abuse page, section 5)
   [K5][K6].
5. Record the referrals (to whom and when) as the outcome in the abuse log.
   Keep the report itself, and what was sent, with the correspondence as
   evidence, access-restricted.
6. Before the first report, confirm with counsel the GDPR transfer basis for
   sending report data to NCMEC in the United States. The privacy notice relies
   on GDPR Art. 49(1)(d) ([ROPA](ropa.md#5-international-transfers), section 5).
   Send only what NCMEC needs to act: the report, the endpoint ID or link part
   before `&cap=`, and the reporter's contact only where needed.

## 6. Request from an authority or regulator

Follow section 4 of the [transparency statement](transparency.md). Record the
deadline on arrival. Ofcom has fined a file-sharing service for failing to
answer information requests [K7], so silence is the outcome to avoid: if the
answer is "we hold nothing", send that answer within the deadline.

## 7. Relay outage

**Detection**: the external monitors of plan item A10
([README](README.md#3-provision-the-relay-and-switch-production-to-it), owner
action 3): `https://relay.oxfer.app/healthz` answering 200 with `"ok"`, the
certificate's expiry, and a WebSocket probe to `wss://relay.oxfer.app/relay`
with the `iroh-relay-v1` subprotocol; users reporting that links do not
connect; the Diags page.

**Impact**: browsers cannot connect to each other without the relay; transfers
already on a direct WebRTC path continue. No personal data is lost.

**Recovery**: restart the service; if the host is unhealthy, rebuild from
[`deploy/relay/`](../../deploy/relay/README.md) and restore the blocks in force,
either by copying `/etc/oxfer-relay/denylist.txt` and
`/etc/nftables.d/bans.nft` from the old host if it is still readable and was
not compromised, or from the abuse log as in section 2. An outage alone is not
a personal data breach; log it in the incident log.

## 8. Verifying what `oxfer.app` serves

The deploy job records the SHA-256 of every served file (excluding `_headers`,
which Cloudflare applies rather than serves) in the run's summary and in the
artifact `oxfer-web-sha256-<commit>`, then runs `verify-deployment.mjs` against
`oxfer.42dev.workers.dev`, `www.oxfer.app` and `oxfer.app`
(`.github/workflows/oxfer-web.yml`).

By hand, against the recorded hashes (from `p2p-transfer/`, with the artifact's
`oxfer-web-sha256.txt` downloaded next to it):

```sh
while read -r hash path; do
  served=$(curl -fsSL "https://oxfer.app/${path}" | sha256sum | cut -d' ' -f1)
  [ "$served" = "$hash" ] && echo "ok   $path" || echo "DIFF $path"
done < oxfer-web-sha256.txt
```

`index.html` is served at `/`: a request for `/index.html` is redirected (307)
to `/`, as `/theme.html` is to `/theme` and `/privacy.html` to `/privacy`, and
`curl -L` follows these redirects, so the loop compares the served files.

Against a local build of the same commit, with the same relay setting as the
deployed build:

```sh
npm ci
P2P_RELAY_URL="<value of the GitHub variable>" npm run build
P2P_RELAY_URL="<value of the GitHub variable>" node verify-deployment.mjs https://oxfer.app/
```

This byte comparison only holds if the local toolchain reproduces CI's build
exactly. If it reports differences but the CI hashes match, rebuild in CI from
the known-good commit (section 1, recovery step 2) rather than trusting either
side.

GitHub keeps workflow artifacts and logs for at most 90 days in a public
repository [K8]. Copy each production run's `oxfer-web-sha256.txt` to the private
records, or attach it to a GitHub release (plan item B4), so older deployments
can still be checked.

## 9. Notification decisions and deadlines

### 9.1 Decision tree

1. **Is personal data involved?** (IP addresses, endpoint IDs, emails and
   reports, or the files and links in users' browsers through tampered code.)
   No: log the incident; stop here.
2. **Was it destroyed, lost, altered, disclosed or accessed without
   authorisation?** Yes: it is a personal data breach. Record it whatever
   happens next [K9, Art. 33(5)].
3. **Is it unlikely to result in a risk to people's rights and freedoms?** If so,
   record the reasons and stop. Otherwise notify ANSPDCP without undue delay and
   where feasible within 72 hours of becoming aware [K9, Art. 33(1)], using its
   online form [K10].
4. **Is it likely to result in a high risk?** If so, tell the people affected
   without undue delay [K9, Art. 34(1)]. Oxfer has no contact details for users,
   so use a public communication on `oxfer.app` and in this repository
   [K9, Art. 34(3)(c)].
5. **Are people in India affected, and is DPDP Rule 7 in force?** (Section 9.2.)
   If so, inform the Data Protection Board and the people affected as that rule
   requires, on a best-effort basis; the operator's position is that Oxfer is not
   offered to India specifically.
6. **Is it a cyber security incident of a kind CERT-In lists?** Report to CERT-In
   within 6 hours on a best-effort basis (section 9.2).

Usual outcomes, to be confirmed at the time: a tampered bundle (section 1) or a
domain hijack serving code (section 3) is a high-risk breach; a relay compromise
(section 2) exposes metadata and usually needs notification to ANSPDCP; an outage
(section 7) is not a breach.

### 9.2 Deadlines

| Who | When | What | Source |
| --- | --- | --- | --- |
| ANSPDCP (Romania, lead supervisory authority) | Without undue delay, where feasible within 72 hours of awareness | Personal data breach, unless unlikely to result in risk; online form | [K9, Art. 33], [K10] |
| People affected | Without undue delay | Breach likely to result in high risk; public communication where individual contact is disproportionate | [K9, Art. 34] |
| Data Protection Board of India and affected Data Principals | Board: without delay, then a detailed report within 72 hours; people affected: without delay | Any personal data breach, from when Rule 7 takes effect: eighteen months after the Rules' Gazette publication of 13 November 2025, so treat 13 May 2027 as the start | [K11], [K12] |
| CERT-In | Within 6 hours of noticing | Cyber incidents of the kinds in its directions, by email to `incident@cert-in.org.in`. Best effort: the directions address service providers and intermediaries generally, and their reach to providers outside India is not settled. | [K13] |
| NCMEC CyberTipline | As soon as reasonably possible after actual knowledge | Apparent child sexual abuse material | [K3], [K4] |
| Take-down after an intimate image content report (UK) | As soon as reasonably practicable, and no later than 48 hours after receiving the report | Block the reported share, and other shares identified as carrying the same or substantially the same content (section 4, step 9) | [K18, s.10(3A), (3B)] |
| Expedited complaint from a person who made an intimate image content report (UK) | Handled before other messages; outcome within 48 hours (the abuse page's promise; the Act requires an expedited procedure but sets no time) | Complaints about that content or how the report was handled (section 4, step 12) | [K18, s.21(2A)]; abuse page, section 8 |
| Complaints from users in India | Acknowledge within 24 hours; resolve within 7 days of receipt. Open owner decision ([README, section 10](README.md#10-other-items-that-need-the-owner)): 2 hours for a complaint by the person shown, or on their behalf, about content showing their private areas, nudity or sexual acts, or impersonating them; 36 hours for a request to remove content under rule 3(1)(b), except its sub-clauses (i), (iv) and (xi) | Grievances to the Grievance Officer (abuse page, section 10) | [K19, rule 3(2)(a)(i) and its proviso, rule 3(2)(b)] |
| Ofcom | As set in the notice | Answers to information notices | [K14] |
| eSafety Commissioner | 24 hours for a class 1 removal notice; as set for other notices | Removal notices and document requests | [K15] |
| EU issuing authority | 10 days, or 8 hours in an emergency; preservation for 60 days, extendable by 30 | European Production Orders and European Preservation Orders, sent directly to the provider, from 18 August 2026 | [K16] |

The Digital Services Act duty to notify suspicions of criminal offences
threatening life or safety applies to hosting services [K17, Art. 18]. Oxfer
stores no content, but the operator passes such information to the competent
authorities anyway.

## Sources

- [K1] Cloudflare, roll API tokens: <https://developers.cloudflare.com/fundamentals/api/how-to/roll-token/>
- [K2] GitHub, managing a branch protection rule: <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/managing-a-branch-protection-rule>
- [K3] NCMEC CyberTipline: <https://www.missingkids.org/gethelpnow/cybertipline>; report form: <https://report.cybertip.org/>
- [K4] 18 U.S.C. § 2258A: <https://www.law.cornell.edu/uscode/text/18/2258A>
- [K5] INHOPE: <https://inhope.org/>
- [K6] Ora de Net reporting line (Salvați Copiii): <https://oradenet.ro/linia-de-raportare/>
- [K7] Ofcom, fine on a file-sharing service for failing to answer information requests: <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/ofcom-fines-online-file-sharing-service-20000>
- [K8] GitHub, artifact and log retention: <https://docs.github.com/en/github/administering-a-repository/managing-repository-settings/configuring-the-retention-period-for-github-actions-artifacts-and-logs-in-your-repository>
- [K9] Regulation (EU) 2016/679 (GDPR): <https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=celex%3A32016R0679>
- [K10] ANSPDCP, breach notification form (Decision 128/2018): <https://www.dataprotection.ro/?page=pagina_formular_679>
- [K11] Press Information Bureau, DPDP Rules 2025 notified: <https://www.pib.gov.in/PressReleasePage.aspx?PRID=2190655&reg=48&lang=2>;
  <https://static.pib.gov.in/WriteReadData/specificdocs/documents/2025/nov/doc20251117695301.pdf>
- [K12] MediaNama, data breach reporting timeline of the DPDP Rules 2025 (secondary): <https://www.medianama.com/2025/11/223-data-breach-reporting-timeline-of-dpdp-rules-2025-explained/>
- [K13] CERT-In directions under s.70B(6), 28 April 2022: <https://www.cert-in.org.in/PDF/CERT-In_Directions_70B_28.04.2022.pdf>
- [K14] Online Safety Act 2023 s.102: <https://www.legislation.gov.uk/ukpga/2023/50/section/102>
- [K15] Online Safety Act 2021 (Cth) s.109: <https://www.legislation.gov.au/C2021A00076/latest/text>; DIS Standard s.31: <https://www.legislation.gov.au/F2024L00710/asmade/text>
- [K16] Regulation (EU) 2023/1543, Arts. 10 and 11 (EUR-Lex): <https://eur-lex.europa.eu/eli/reg/2023/1543/oj>; eucrim, e-Evidence Regulation and Directive (secondary): <https://eucrim.eu/news/e-evidence-regulation-and-directive-published/>
- [K17] Regulation (EU) 2022/2065 (Digital Services Act): <https://eur-lex.europa.eu/legal-content/en/ALL/?uri=CELEX:32022R2065>
- [K18] Online Safety Act 2023, as amended by the Crime and Policing Act 2026 from 29 June 2026: [s.10](https://www.legislation.gov.uk/ukpga/2023/50/section/10), [s.20A](https://www.legislation.gov.uk/ukpga/2023/50/section/20A), [s.21](https://www.legislation.gov.uk/ukpga/2023/50/section/21)
- [K19] Information Technology (Intermediary Guidelines and Digital Media Ethics Code) Rules, 2021, as updated on 10 February 2026 (G.S.R. 120(E)), MeitY: <https://www.meity.gov.in/static/uploads/2026/02/550681ab908f8afb135b0ad42816a1c9.pdf>
