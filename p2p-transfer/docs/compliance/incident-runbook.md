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
point the DNS records at it, and let the relay obtain new certificates. Check it
with the app's Diags page.

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

Timelines are those promised on the [abuse page](../../abuse.html), section 3.

1. Log it (date, kind, link part before `&cap=`, reporter contact if given).
2. Acknowledge within 24 hours. The acknowledgement lists the possible outcomes
   (no action, share blocked, address blocked, referral to authorities) and says
   whether the reporter will be told the decision.
3. Do not open or download the reported link.
4. Decide. To block a share, add the endpoint ID from the link to the relay's
   denylist (`access.denylist` in the relay configuration) and restart the relay;
   a restart briefly interrupts transfers running through it. To block an
   address, add a firewall rule or a relay denylist entry, with an expiry where
   possible.
5. Intimate images shared without consent: block within 48 hours.
6. Answer within 15 days with the decision; record it.
7. Appeals: review promptly, lift a wrong block, record it.

## 5. Report of apparent child sexual abuse material

1. Do not open, download or ask for the material. Do not forward it.
2. Block the share's endpoint ID at the relay immediately (section 4, step 4),
   and any address involved that you know.
3. Report to NCMEC's CyberTipline [K3] as soon as reasonably possible. US law
   requires this of providers within its scope once they have actual knowledge,
   with no duty to search for material [K4]; the operator reports on the same
   basis. Send the report text and the endpoint ID; the operator holds no
   content. Registration as an electronic service provider (`espteam@ncmec.org`)
   is needed only to upload content with a report [K3].
4. Report to the Romanian police, and point the reporter to the national hotline
   in the INHOPE network (in Romania, esc_ABUZ run by Salvați Copiii) [K5][K6].
5. Record what was reported, to whom and when. Keep the report itself as
   evidence, access-restricted.
6. Before the first report, confirm with counsel the GDPR transfer basis for
   sending report data to NCMEC in the United States ([ROPA](ropa.md), section 5).

## 6. Request from an authority or regulator

Follow section 4 of the [transparency statement](transparency.md). Record the
deadline on arrival. Ofcom has fined a file-sharing service for failing to
answer information requests [K7], so silence is the outcome to avoid: if the
answer is "we hold nothing", send that answer within the deadline.

## 7. Relay outage

**Detection**: external uptime monitor on `https://relay.oxfer.app/` and TCP 443
(plan item A10); users reporting that links do not connect; the Diags page.

**Impact**: browsers cannot connect to each other without the relay; transfers
already on a direct WebRTC path continue. No personal data is lost.

**Recovery**: restart the service; if the host is unhealthy, rebuild from
[`deploy/relay/`](../../deploy/relay/README.md). An outage alone is not a personal
data breach; log it in the incident log.

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

`index.html` is served at `/` as well as `/index.html`; the legal pages redirect
from `/privacy.html` to `/privacy`, which `curl -L` follows.

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
| Ofcom | As set in the notice | Answers to information notices | [K14] |
| eSafety Commissioner | 24 hours for a class 1 removal notice; as set for other notices | Removal notices and document requests | [K15] |
| EU issuing authority | 10 days, or 8 hours in an emergency | European Production Orders, from 18 August 2026 | [K16] |

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
- [K6] Ora de Net / esc_ABUZ (Salvați Copiii and the Romanian Police): <https://safernet.politiaromana.ro/>
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
- [K16] eucrim, e-Evidence Regulation and Directive: <https://eucrim.eu/news/e-evidence-regulation-and-directive-published/>
- [K17] Regulation (EU) 2022/2065 (Digital Services Act): <https://eur-lex.europa.eu/legal-content/en/ALL/?uri=CELEX:32022R2065>
