# UK Online Safety Act: illegal content risk assessment

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated and signed below. |
| Service | Oxfer: web app at `oxfer.app`, relay at `relay.oxfer.app`, desktop app built from this crate |
| Provider | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]] |
| Accountable individual (Codes measure ICU A2) | [[OPERATOR_NAME]] |
| Assessment date | [[EFFECTIVE_DATE]] |
| Next scheduled review | 12 months after [[EFFECTIVE_DATE]], or earlier on a trigger in section 7 |
| Method | Ofcom's four-step method in its *Risk Assessment Guidance and Risk Profiles* [R1] |

This record is published in a public repository. It sets out the provider's
assessment and reasons. It is not legal advice. Related records: the
[children's access assessment](osa-children-access.md), the
[children's risk assessment](osa-children-risk.md), the
[transparency statement](transparency.md) and the
[incident runbook](incident-runbook.md).

**Configuration assessed.** This assessment describes Oxfer with production
builds pointed at the operator's own relay (`P2P_RELAY_URL` set, see
[README](README.md#owner-actions)). Relay-level blocking, which several measures
below rely on, exists only in that configuration. Adopt this record when the
production build uses the operator's relay.

## 1. Legal frame

- A user-to-user service is an internet service by means of which content
  uploaded to or shared on the service by a user may be encountered by another
  user [OSA s.3(1)]. Oxfer lets a user send files that another user receives.
- A service is regulated if it has links with the United Kingdom: a significant
  number of UK users, the UK as a target market, or use in the UK with a material
  risk of significant harm [OSA s.4(5), (6)]. Oxfer does not target the UK and
  holds no user counts by country beyond Cloudflare's aggregate request
  statistics. The provider has decided to carry out this assessment and comply
  with the illegal content duties without taking a position on whether the
  s.4 test is met.
- The duty is to carry out a suitable and sufficient illegal content risk
  assessment, keep it up to date, and reassess before any significant change to
  the service's design or operation [OSA s.9]. The assessment must cover the
  matters in s.9(5), including the user base, the risk of each kind of priority
  illegal content, functionalities, and how design and operation reduce or
  increase risk.
- Every aspect of the assessment must be kept as a written record, together with
  the measures taken and any alternative measures [OSA s.23].
- The Codes apply regardless of whether the provider is in the United Kingdom
  [R3, para 2.3].

## 2. Step 1: the service and the kinds of illegal harm assessed

### 2.1 What Oxfer is

Oxfer sends files from one device to another. The facts below are taken from the
code in this crate (`src/node.rs`, `src/transfer.rs`, `src/webrtc.rs`,
`src/app.rs`) and the deployment files.

| Characteristic | Oxfer |
| --- | --- |
| Hosting of content | None. Files stay on the sender's device and stream to the recipient. The web host serves only the app itself. |
| Availability of content | Only while the sender keeps the share open and online. Closing the app or losing the connection ends the share. |
| How content is reached | A share link. Its fragment carries the share's endpoint ticket and a 128-bit bearer capability derived from a key generated for that share. Browsers do not send the fragment to the web host. |
| Who can receive | Anyone holding the complete link, for as long as the sender shares. A link can be forwarded outside Oxfer. The sender sees connected recipients and can stop sharing. |
| Discovery | None: no search, directory, feed, profiles, public pages, hashtags or recommender system. |
| Communication between users | None beyond the transfer. The recipient sees file names, sizes and checksums, then chooses where to save. There is no chat, comment or reaction feature and no in-app preview of received files. |
| Accounts and identity | None. Each share and each receiving session uses a new endpoint key. |
| Encryption | End to end between the two devices: DTLS on a direct WebRTC path, QUIC/TLS through the relay. The provider has no keys and never receives plaintext. |
| Relay | Forwards encrypted traffic and connection setup. Sees IP addresses, endpoint IDs, timing and volume while connected. Keeps no access log. |
| Scale and business model | Free, no advertising, no payments, no user analytics. |

### 2.2 Ofcom's risk factors

Ofcom's U2U Risk Profile lists risk factors that include the type of service,
the user base and particular functionalities [R1]. Oxfer's position on the
relevant ones:

| Risk factor group | Present on Oxfer? | Note |
| --- | --- | --- |
| Service type: file-storage and file-sharing | Treated as relevant | Ofcom's Codes define a file-storage and file-sharing service as one whose primary functionalities let users (a) store digital content on the cloud or dedicated servers and (b) share access to it through links [R3, section 5]. Oxfer meets (b) but not (a): nothing is stored on a server. Because the function overlaps, this assessment rates risk as if the file-sharing factor applied. Ofcom associates this service type with terrorism, image-based CSAM and intimate image abuse [R1]. |
| User identification (profiles, anonymous profiles) | No profiles | No accounts. Users are anonymous to the provider; this is recorded as a factor that limits attribution. |
| User networking (connections, groups, search for users) | No | Users cannot find or contact each other through Oxfer. |
| User communication (messaging, comments, livestreaming) | No in-app messaging | File names are the only free text a recipient sees. |
| Posting and sharing images, videos, files | Yes, one-to-one or one-to-few | Any file type and size. Large-file support is recorded as risk-increasing for collections of material. |
| Hyperlinks | Share links only | Links are ephemeral and bearer-based. File contents could contain links. |
| Recommender systems, search | No | |
| Encryption | End to end | Recorded as risk-increasing for detection: the provider cannot inspect content. |

### 2.3 Content communicated publicly or privately

Ofcom must consider, for this distinction, the number of UK individuals able to
access the content, restrictions on access, and how easily the content can be
forwarded [OSA s.232(2)], and has published guidance on it [R5].

- Number able to access: the people who hold a given link, usually one.
- Restrictions: access requires the unguessable bearer link; there is no
  directory and nothing is reachable without it.
- Forwarding: the link can be forwarded outside Oxfer while the sender shares.
  Recipients can re-send files they saved through other services.

The provider's view is that transfers are communicated privately, noting that
the forwarding factor points the other way. Section 5.3 lists a measure under
consideration that would strengthen the access restriction.

### 2.4 Kinds of illegal harm assessed

All kinds of priority illegal content in Ofcom's Table C [R3, section 5], as
updated for the two priority offences added by the 2025 amendment regulations
[R4]: Ofcom now
assesses encouraging or assisting suicide and serious self-harm together as
"suicide and self-harm", and cyberflashing as a separate kind [R6]. Other
(non-priority) illegal content is assessed as one group. Offences concerning
intellectual property are not "relevant offences" under the Act [OSA s.59(6)];
the terms prohibit infringement anyway.

### 2.5 User base

General audience, English only, no accounts, not marketed in the UK or
elsewhere. Minimum age 13 under the terms; the
[children's access assessment](osa-children-access.md) treats the service as
likely to be accessed by children. The UK share of users is estimated from
Cloudflare's aggregate requests per country (section 3). No other user data
exists.

## 3. Evidence used

Ofcom expects all providers to consider "core" inputs: the Risk Profiles, user
complaints and reports, relevant user data, and other information the provider
holds; "enhanced" inputs are expected from large services and from providers not
confident that core inputs are enough [R1].

| Input | Used | Result |
| --- | --- | --- |
| Ofcom U2U Risk Profile | Yes | Section 2.2. |
| Design and code review | Yes | Section 2.1. Reviewed against the crate at the commit deployed on [[EFFECTIVE_DATE]]. |
| User reports and complaints | Yes | Reports received through `abuse@oxfer.app` in the period, from the private abuse log: *to be completed by the operator at adoption (number and kinds only).* |
| User data | Limited | Oxfer collects none. Cloudflare's dashboard shows aggregate requests per country; the UK share at adoption: *to be completed by the operator.* |
| Incidents and law-enforcement contact | Yes | Any contact in the period, from the private abuse log: *to be completed by the operator.* |
| Enhanced inputs (product testing, external experts, user research) | No | Not expected for a service of this size and design. The evidence gap is stated: without user data or content visibility, the provider cannot measure actual misuse. Ratings below therefore rely on design factors. Where Oxfer's own function, private sending of images and videos, makes a severe harm plausible, the higher level is chosen. |

The service has no way to count users. Nothing indicates that it approaches
700,000 monthly active UK users, or the 7 million that define a "large service"
in the Codes [R3, section 5]; the Cloudflare figure is to be recorded at each
review.

## 4. Step 2: risk of each kind of illegal harm

### 4.1 Factors common to every kind

- **D1 No discoverability.** Content cannot be found by anyone who does not
  already hold a link. Oxfer does not connect strangers.
- **D2 No persistence.** Nothing is stored. A share lasts while the sender stays
  online; every share has a new key, so an old link stops working when the
  share ends.
- **D3 No amplification.** No feed, recommender, reposting, likes or comments.
  Onward spread depends on people forwarding links or saved files through other
  services.
- **D4 Live attribution.** A transfer needs the sender's device online at that
  moment, and the relay can deny a share's endpoint ID or an address from the
  moment a report arrives.
- **R1 No content visibility.** End-to-end encryption means the provider learns
  of illegal content only from reports. It cannot scan, sample or measure.
- **R2 No accounts.** Users are anonymous to the provider and cannot be banned as
  persons; only shares and IP addresses can be blocked.
- **R3 Any file, any size.** Suitable for moving large collections privately.
- **R4 Peer IP exposure.** On a direct path, each device usually sees the other's
  public IP address. This is disclosed in the privacy notice.

Risk levels follow Ofcom's scale of negligible, low, medium and high, combining
likelihood and impact [R1].

### 4.2 Ratings

| Kind of illegal harm | Likelihood on Oxfer | Impact | Risk level | Reasons |
| --- | --- | --- | --- | --- |
| Terrorism | Unlikely | Severe | Low | Ofcom lists file-sharing services as a risk factor for terrorism [R1]. The usual route is a link posted publicly for mass download; Oxfer links stop working when the sender goes offline (D2) and have no public surface (D1, D3). One-to-one sharing of material remains possible (R1). |
| CSEA: image-based CSAM | Possible | Severe | **Medium** | Private, encrypted, account-free transfer of large files (R1 to R3) can be used by people who already know each other to exchange CSAM. Ofcom treats file-sharing services as presenting particular risks of image-based CSAM [R7]. D1 to D3 remove the routes by which links are published for others to find and download at scale. Rated medium, not low, because the harm is severe and the provider cannot detect it. |
| CSEA: CSAM URLs | Unlikely | Severe | Low | No posting, messaging or directory where URLs are shown. URLs can only appear inside file contents or file names. |
| CSEA: grooming | Unlikely | Severe | Low | Oxfer offers no way to find or contact a child (D1); contact must already exist elsewhere. Images exchanged in a grooming relationship are covered by the CSAM and cyberflashing rows. |
| Suicide and self-harm | Unlikely | Severe | Low | No public dissemination (D1, D3). Private sending of material is possible. |
| Hate | Unlikely | Moderate | Low | The hate offences in Table C concern words, behaviour or material that is displayed, published, distributed or played [R3, Table C]; Oxfer has no audience beyond link holders (D1, D3). |
| Harassment, stalking, threats and abuse | Possible | Moderate | Low | A sender can put words in file names or send unwanted files, but only to someone who opens their link. A sender can learn a recipient's IP address on a direct path (R4). The recipient decides whether to open a link and to save. |
| Controlling or coercive behaviour | Unlikely | Moderate | Low | No monitoring, location or account features that could be used to control another person. |
| Drugs and psychoactive substances | Unlikely | Moderate | Low | No marketplace, listings or payments. |
| Firearms, knives and other weapons | Unlikely | Severe | Low | Files such as manuals or 3D-print designs can be sent privately; no dissemination features. |
| Unlawful immigration | Unlikely | Moderate | Low | No advertising, listings or contact features. |
| Human trafficking | Unlikely | Severe | Low | No advertising, profiles or contact features. |
| Sexual exploitation of adults | Unlikely | Severe | Low | No advertising, profiles or contact features. |
| Extreme pornography | Possible | Moderate | Low | Private sending of files is possible (R1, R3); no audience beyond link holders. |
| Intimate image abuse | Possible | Severe | **Medium** | Sharing an intimate photograph or film without consent is a priority offence (Sexual Offences Act 2003 s.66B, [R3, Table C]). Oxfer can carry such a transfer privately and cannot detect it (R1). Ofcom lists file-sharing as a risk factor for this harm [R1]. Blocking a reported share stops further receipt through the relay (D4). |
| Cyberflashing | Unlikely | Moderate | Low | A recipient must open a link and choose to save; Oxfer shows no preview, so an image is not displayed unsolicited inside Oxfer. |
| Proceeds of crime | Unlikely | Moderate | Low | No payments or value transfer. |
| Fraud and financial offences | Possible | Moderate | Low | A link can deliver a malicious or deceptive file. Oxfer shows file names only and the terms tell recipients to scan files. |
| Foreign interference | Unlikely | Moderate | Low | No public reach (D1, D3). |
| Animal cruelty | Unlikely | Moderate | Low | No public reach (D1, D3). |
| Other illegal content (for example malware, threatening communications) | Possible | Moderate | Low | As for fraud and harassment. |

Result: medium risk of image-based CSAM and of intimate image abuse; low for
every other kind. The service is not multi-risk, because it is at medium or high
risk of fewer than two kinds outside the CSEA sub-kinds [R3, para 5.6].

## 5. Step 3: measures

### 5.1 Codes measures that apply

Recommended measures for a service that is neither large nor multi-risk, is at
medium risk of at least one kind of harm, and is treated as likely to be accessed
by children (see the [children's access assessment](osa-children-access.md))
[R3, section 3]:

| Measure | Status | How Oxfer meets it |
| --- | --- | --- |
| ICU A2 Individual accountable | Adopted | [[OPERATOR_NAME]]. |
| ICU C1 Content moderation function to review suspected illegal content | Adopted, adapted | A person reviews every report. The provider cannot view content and does not open reported links (abuse page, section 2); it decides on the report, the link's endpoint ID and any other information supplied. |
| ICU C2 Swift take-down of illegal content | Adopted, adapted | "Take down" means denying the share's endpoint ID at the relay, which ends the share for new recipients in the browser, and blocking IP addresses for repeated or serious abuse. It cannot stop a transfer already running on a direct path, a direct native-to-native connection, or copies already saved. Timelines: abuse page, section 3. |
| ICU D1, D2 Complaints system, easy to find and use | Adopted | `abuse@oxfer.app` on the abuse page, which every legal page links to; the app footer links to it once plan item B6 ships. |
| ICU D3 Information before a complaint is submitted (services likely to be accessed by children at medium or high risk) | Open | The abuse page should say, before the reporting instructions, that the provider does not tell the sender or anyone else that a report was made, and cannot, because it holds no contact details for users. |
| ICU D4 Indicative timeframes | Adopted | Acknowledgement within 24 hours, answer within 15 days; 48 hours for intimate image abuse. |
| ICU D5 Possible outcomes stated in the acknowledgement (services likely to be accessed by children at medium or high risk) | Open | The acknowledgement template should list the possible outcomes (no action, share blocked, address blocked, referral to authorities) and confirm that the complainant will be told the decision. Keep the template with the private records. |
| ICU D6 Opt-out from communications after a complaint | Open | The abuse page should say that a complainant can ask not to receive further messages about a complaint. |
| ICU D7 Action on complaints about suspected illegal content | Adopted | Handled under C1 and C2. |
| ICU D9, D10 Appeals (services neither large nor multi-risk) | Adopted | Appeals against blocks, with the block lifted if wrong (abuse page, section 8). |
| ICU D11 Complaints about proactive technology | Not triggered | No proactive technology is used. |
| ICU D12 Other complaints, with a nominated individual | Adopted | Complaints about compliance, freedom of expression and privacy are accepted (terms, section 6) and handled by [[OPERATOR_NAME]]. |
| ICU D13 Manifestly unfounded complaints | Not used | There is no policy for disregarding complaints; every complaint is considered. |
| ICU G1, G3 Terms of service: substance, clarity | Adopted | Terms, section 5, state how users are protected, that no proactive technology is used, and how complaints work. |
| ICU H1 Removing accounts of proscribed organisations | Not applicable | No accounts. |

Measures for large or multi-risk services (ICU A1, A3 to A7, C3 to C8, D8, D14,
E1, J1 to J3) and those that depend on knowing a user's age or on a high risk of
grooming (ICU F1, F2) do not apply to Oxfer on the ratings above
[R3, section 3].

### 5.2 Measures in place beyond the Codes

- Design: D1 to D3 in section 4.1 are deliberate and are protected by the
  triggers in section 7.
- Relay limits on connection rate and per-client throughput (see
  [`deploy/relay/`](../../deploy/relay/README.md)).
- Cooperation: reports of apparent CSAM go to NCMEC and Romanian authorities;
  lawful requests are answered (see [incident runbook](incident-runbook.md) and
  [transparency statement](transparency.md)).
- Deployment integrity: CSP and hash comparison of the served bundle
  (`verify-deployment.mjs`), so the encryption described here is the encryption
  users actually receive.

### 5.3 Measures considered and not adopted

| Measure | Decision | Reasons |
| --- | --- | --- |
| ICU C9 Hash matching for CSAM | Not adopted | The measure applies to services at high risk of image-based CSAM that are large, have more than 700,000 monthly active UK users, or are file-storage and file-sharing services [R3, ICU C9.1]; Oxfer is rated medium and does not store content. It covers content communicated publicly [R3, ICU C9.2], and Oxfer transfers are communicated privately (section 2.3); the Act does not allow Codes to recommend proactive technology for privately communicated content [OSA Sch. 4 para 13(4)]. It is also not technically feasible: the provider never holds plaintext, hashes of files or keys. |
| Client-side scanning in the browser app | Not adopted | Would need a CSAM hash list on every user's device, which the provider cannot obtain or protect appropriately, and would change the privacy properties of the app for all users. |
| ICU C10 CSAM URL detection | Not applicable | Applies to large services or those with more than 700,000 UK users [R3, section 3]; Oxfer shows no user-posted URLs. |
| Accounts or identity checks for senders | Not adopted | Would add personal data about every user, remove the no-records property the privacy design depends on, and add little protection where sender and recipient already know each other. |
| Logging IP addresses or share IDs | Not adopted | Would create records of who sent to whom. Blocking at the relay works from the moment of a report without any log. |
| Single-recipient links (a share closes to new recipients after the first completes) | Under consideration | Would strengthen the private character of transfers (section 2.3) and limit forwarding. To be decided at the next review. |
| In-app safety notice for recipients (only open links from people you know; scan files) | Under consideration | Low cost. The terms already say this. |

## 6. Step 4: records, reporting and review

- **Records.** This document is the written record under s.23. Operational
  records are kept privately: the abuse log (date, kind of report, endpoint ID or
  address blocked, outcome, never content), relay denylist changes, and
  correspondence with authorities. See [README](README.md#private-records).
- **Reporting to Ofcom.** Ofcom can require information, including this record,
  by an information notice that sets the deadline [OSA s.102]. Ofcom has fined a
  file-sharing service for not answering such requests [R8]. Notices go to
  `abuse@oxfer.app`; handling is in the [incident runbook](incident-runbook.md).
- **Keeping it up to date.** The Act requires keeping the assessment up to
  date, including when Ofcom makes a significant change to a Risk Profile, and a
  new assessment before a significant change to the service [OSA s.9]. The
  provider reviews it at least every 12 months and before any change in
  section 7.

## 7. Triggers for a new assessment before a change

Any of these is a significant change to design or operation:

- storage of files on a server, or any store-and-forward relay;
- accounts, profiles, contact lists, search, a directory, public pages or links
  that work while the sender is offline;
- messaging, comments or previews of received files;
- analytics, advertising, payments or donations;
- dropping the operator's relay for another provider, or running relays in new
  regions;
- evidence of misuse in reports, or a UK share of traffic that suggests the
  700,000 threshold is in reach.

## 8. Sign-off

| | |
| --- | --- |
| Assessed and approved by | [[OPERATOR_NAME]] |
| Date | [[EFFECTIVE_DATE]] |
| Next review due | 12 months after [[EFFECTIVE_DATE]] |

## Sources

Statute (legislation.gov.uk): [OSA s.3](https://www.legislation.gov.uk/ukpga/2023/50/section/3),
[s.4](https://www.legislation.gov.uk/ukpga/2023/50/section/4),
[s.9](https://www.legislation.gov.uk/ukpga/2023/50/section/9),
[s.23](https://www.legislation.gov.uk/ukpga/2023/50/section/23),
[s.59](https://www.legislation.gov.uk/ukpga/2023/50/section/59),
[s.102](https://www.legislation.gov.uk/ukpga/2023/50/section/102),
[s.232](https://www.legislation.gov.uk/ukpga/2023/50/section/232),
[Schedule 4](https://www.legislation.gov.uk/ukpga/2023/50/schedule/4).

- [R1] Ofcom, *Risk Assessment Guidance and Risk Profiles*:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/risk-assessment-guidance-and-risk-profiles.pdf?v=419933>
- [R3] Ofcom, *Illegal content Codes of Practice for user-to-user services*
  (text as laid, GOV.UK):
  <https://assets.publishing.service.gov.uk/media/6762c7dfff2c870561bde79f/draft-illegal-content-codes-of-practice-for-user-to-user-services.pdf>;
  version in force from 17 March 2025:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/illegal-content-codes-of-practice-for-user-to-user-services-24-feb.pdf?v=391889>
- [R4] The Online Safety Act 2023 (Priority Offences) (Amendment) Regulations 2025:
  <https://www.legislation.gov.uk/ukdsi/2025/9780348275698>
- [R5] Ofcom, *Guidance on content communicated 'publicly' and 'privately'*:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/guidance-on-content-communicated-publicly-and-privately-under-the-online-safety-act.pdf?v=388093>
- [R6] Ofcom, *Statement: New priority offences - serious self-harm and cyberflashing* (June 2026):
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/statement-new-priority-offences-serious-self-harm-and-cyberflashing>
- [R7] Ofcom, enforcement programme on file-sharing and file-storage services and CSAM:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/enforcement-programme-into-measures-being-taken-by-file-sharing-and-file-storage-services-to-prevent-users-from-encountering-or-sharing-child-sexual-abuse-material-csam>
- [R8] Ofcom, fine on a file-sharing service for failing to answer information requests:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/ofcom-fines-online-file-sharing-service-20000>
- Ofcom, online safety regulatory documents and guidance (current versions, including record-keeping guidance):
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/online-safety-regulatory-documents>
