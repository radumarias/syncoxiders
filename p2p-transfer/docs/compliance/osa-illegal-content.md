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
[README](README.md#owner-actions)), run on a VPS with the deployment kit's VPS
variant ([`deploy/relay/`](../../deploy/relay/README.md)). Relay-level blocking,
which several measures below rely on, exists only in that configuration; on the
kit's Fly.io variant the relay cannot block addresses (section 7). Adopt this
record when the production build uses the operator's relay.

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
- The safety duties include using proportionate systems and processes to
  minimise how long priority illegal content is present and to take down
  illegal content swiftly once alerted to it [OSA s.10(3)], and terms of
  service that say how individuals are protected [OSA s.10(5), (8)].
- Since 29 June 2026, the Crime and Policing Act 2026 has added duties about
  intimate images [CPA 2026 ss.100, 101; SI 2026/689, reg. 2(1)(j)]:
  - **Take-down within 48 hours.** A duty to operate proportionate systems and
    processes designed to take down content that is the subject of an
    *intimate image content report*, and any other content the provider
    identifies as the same or substantially the same, "as soon as reasonably
    practicable, and no later than 48 hours, after the provider receives the
    report" [OSA s.10(3A)]. It does not apply where the provider considers that
    the content is not intimate image content, or that the reporter is neither
    its subject nor acting for them [OSA s.10(3B)].
  - **Terms of service** must address that duty [OSA s.10(5)(b)].
  - **Reporting.** Users and affected persons must be able to make such a
    report easily. The report declares that the content is intimate image
    content, that the reporter is its subject or acts for them, and that it is
    made in good faith and true to the best of the reporter's knowledge and
    belief; it identifies the content and gives contact details
    [OSA s.20A(1), (2)]. No regulations adding requirements under s.20A(2)(f)
    or (3) were identified on 30 September 2026.
  - **Expedited complaints.** A person who made such a report must have an
    expedited complaints procedure for complaints about that content
    [OSA s.21(2A)].
  - Section 5.3 records how Oxfer meets each of them.
- Every aspect of the assessment must be kept as a written record
  [OSA s.23(2)], with the Codes measures taken [OSA s.23(3)] and, where
  alternative measures are used, the Codes measures not taken, the alternatives,
  and how they comply [OSA s.23(4)]; for alternatives to the duties in
  s.10(2), (3) or (3A), the record also covers each area listed in s.10(4)
  [OSA s.23(5)].
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
the forwarding factor points the other way. Section 5.5 lists a measure under
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
  moment, and from the moment a report arrives the relay can deny a share's
  endpoint ID and the relay host's firewall can block an address.
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
| CSEA (overall: every offence in Schedule 6 to the Act) | Possible | Severe | **Medium** | The Codes rate each kind of harm for its offences taken together [R3, para 5.4]. Image-based CSAM, CSAM URLs and grooming are subsets of the CSEA offences [R3, Table C, rows 2 to 2C], so CSEA overall is at least as high as the highest of the three rows below: medium, from image-based CSAM. |
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

Result: medium risk of CSEA (overall, from image-based CSAM) and of intimate
image abuse; low for every other kind.

**Oxfer is a multi-risk service.** A service is multi-risk if it is at medium
or high risk of two or more kinds of illegal harm in Table C, "excluding the
kinds of illegal harm set out in rows 2A, 2B and 2C" [R3, para 5.6]. That
exclusion stops the three CSEA sub-kinds being counted on top of CSEA itself; it
does not exclude CSEA (row 2). Oxfer is at medium risk of CSEA and of intimate
image abuse, two kinds, so it is multi-risk. It is not a large service
(section 3). Section 5.1 therefore applies the measures for multi-risk services.

## 5. Step 3: measures

### 5.1 Codes measures that apply

Recommended measures for a service that is multi-risk but not large, is at
medium risk of at least one kind of harm, and is treated as likely to be
accessed by children (see the [children's access assessment](osa-children-access.md))
[R3, section 3]. Each measure's application paragraph was checked in the text of
the Codes [R3]. Statuses: *Adopted*, with how; *Not applicable*, with the
reason; *Not used* for the optional exception in ICU D13. *Adapted* marks a
measure that Oxfer follows as far as its design allows: it has no access to
content, so it acts on the report and the information supplied (ICU C1, C2);
the row says how. This record lists no alternative measures under s.23(4).

| Measure | Status | How Oxfer meets it |
| --- | --- | --- |
| ICU A2 Individual accountable | Adopted | [[OPERATOR_NAME]]. |
| ICU A3 Written statements of responsibilities (large or multi-risk services) | Adopted | The provider has one senior manager, [[OPERATOR_NAME]], who takes every decision about the risks of illegal harm. The statement of responsibilities is in section 5.2. |
| ICU A5 Tracking evidence of new and increasing illegal harm (large or multi-risk services) | Adopted | At each quarterly review the operator counts reports, complaints and referrals from law enforcement by kind of harm from the private abuse log, compares them with earlier quarters as the baseline, and records any new kind or unusual increase in the review note. Reports are the only evidence Oxfer has (factor R1 in section 4.1). A new kind of harm or an unusual increase is a trigger for a new assessment (section 7). The operator is also the most senior governance body. |
| ICU A6 Code of conduct (large or multi-risk services) | Adopted | Section 5.2. |
| ICU A7 Compliance training (large or multi-risk services) | Adopted | Everyone involved in the design or operational management of Oxfer (now only the operator) reads this record, the [incident runbook](incident-runbook.md), the abuse page and the terms before working on the service, and again at each yearly review with any change Ofcom has made to the Codes or its guidance. |
| ICU C1 Content moderation function to review suspected illegal content | Adopted, adapted | A person reviews every report. The provider cannot view content and does not open reported links (abuse page, section 2). It decides on the report, the share's endpoint ID and any other information supplied. It reads the endpoint ID from the link offline, with the tool in the incident runbook (section 4), without opening the link. |
| ICU C2 Swift take-down of illegal content | Adopted, adapted | "Take down" means denying the share's endpoint ID at the relay, which ends the share for new recipients in the browser, and blocking IP addresses at the relay host's firewall for repeated or serious abuse, normally for 30 days. Both kinds of block are kept in files on the relay host, so they survive relay restarts, reboots and upgrades. A block cannot stop a transfer already running on a direct path, a direct native-to-native connection, or copies already saved. Timelines: abuse page, section 3, and the targets in section 5.2. |
| ICU C3 Internal content policies (large or multi-risk services) | Adopted | Section 5.2. |
| ICU C4 Performance targets (large or multi-risk services) | Adopted | Section 5.2. |
| ICU C5 Prioritisation (large or multi-risk services) | Adopted | Section 5.2, applied in the [incident runbook](incident-runbook.md), section 4. |
| ICU C6 Resourcing (large or multi-risk services) | Adopted | Section 5.2. |
| ICU C7 Training and materials for people working in content moderation (large or multi-risk services) | Adopted | The operator, the only person who moderates, works from this record, the incident runbook and Ofcom's *Illegal Content Judgements Guidance* [R11]. When a report concerns a kind of harm the operator has not handled before, the operator reads the Guidance's chapter on that kind before deciding (ICU C7.3(b)). |
| ICU C8 Materials for volunteers (large or multi-risk services) | Not applicable | No volunteers moderate content. Anyone who ever does gets the ICU C7 materials first. |
| ICU D1, D2 Complaints system, easy to find and use | Adopted | `abuse@oxfer.app` on the abuse page. The app's bottom bar links to the abuse page on every screen (as a link on wide screens, in its Legal menu on narrow ones), the receive screens have a Report abuse link to it, and every legal page links to it. |
| ICU D3 Information before a complaint is submitted (services likely to be accessed by children at medium or high risk) | Adopted | Abuse page, section 2, before the list of what to include in a report: Oxfer never tells the person who shared the content, or any other user, that a report was made or who made it. The only exception is a referral to NCMEC or authorities, which can include the report and, where needed, how to reach the reporter. An answer to an appeal does not say whether anyone made a report or who made it (ICU D3.2(b)). |
| ICU D4 Indicative timeframes | Adopted | Acknowledgement within 24 hours, giving the timeframe for a decision: 15 days, or 7 days for a complaint from India (abuse page, sections 3 and 10). Taking down content after an intimate image content report is a statutory maximum of 48 hours, not an indicative timeframe (section 5.3). No acknowledgement goes to a complainant who has opted out (ICU D4.3). |
| ICU D5 Possible outcomes stated in the acknowledgement (services likely to be accessed by children at medium or high risk) | Adopted | Abuse page, section 3: the acknowledgement lists the possible outcomes (no action; a block at the relay, of the share or of network addresses; a referral to NCMEC or other authorities; a reply that explains the decision) and confirms that the complainant will be told the decision and any action taken. The operator writes the acknowledgement template before adoption and keeps it with the private records ([README](README.md#10-other-items-that-need-the-owner)). |
| ICU D6 Opt-out from communications after a complaint | Adopted | Abuse page, section 3: a complainant can ask, when complaining or at any time later, to receive no further messages about it; the complaint is still handled in the same way. Incident runbook, section 4. |
| ICU D7 Action on complaints about suspected illegal content | Adopted | Handled under C1 and C2, in the order set by the prioritisation policy (C5). |
| ICU D8 Appeals: determination (large or multi-risk services) | Adopted | Appeals against blocks are determined promptly and within 15 days (abuse page, sections 3 and 8), against the targets in section 5.2, which the quarterly review monitors. In ordering appeals the operator has regard to how serious the block is (an address block can affect other people behind the address, so it comes before a share block) and to past errors. No content identification technology is used (ICU D8.4(b)). |
| ICU D10 Appeals: action after the decision | Adopted | A wrong block is lifted and the appellant told (abuse page, section 8). A pattern of wrong blocks leads to a change in the incident runbook. |
| ICU D11 Complaints about proactive technology | Not applicable | No proactive technology is used. |
| ICU D12 Other complaints, with a nominated individual | Adopted | Complaints about compliance, freedom of expression and privacy are accepted (terms, section 6; abuse page, section 8) and handled by [[OPERATOR_NAME]]. |
| ICU D13 Manifestly unfounded complaints | Not used | There is no policy for disregarding complaints; every complaint is considered. |
| ICU G1, G3 Terms of service: substance, clarity | Adopted | Terms, section 5, state how users are protected, separately for terrorism, CSEA and other illegal content, that no proactive technology is used, and (section 5.5) how intimate image content reports are handled, as s.10(5)(b) requires. Section 6 covers complaints, including the expedited procedure. |
| ICU H1 Removing accounts of proscribed organisations | Not applicable | No accounts. |

ICU D9 (appeals for services that are neither large nor multi-risk) no longer
applies; ICU D8 replaces it.

These measures do not apply on the ratings above [R3, section 3]:

- measures for large services: ICU A1; A4 (large and multi-risk); C10 (large
  services at medium or high risk of CSAM URLs, or services with more than
  700,000 monthly active UK users at high risk of them); D14 (large services
  at medium or high risk of fraud); J1 to J3;
- ICU C9 and C14, hash matching: section 5.5;
- ICU E1, safety metrics when testing recommender systems on the platform:
  Oxfer has no recommender system;
- ICU F1 and F2, which need an existing means of determining users' ages and
  a high risk of grooming (or a large service at medium risk): Oxfer holds no
  age data and is at low risk of grooming;
- ICU G2, for Category 1 services;
- the crisis response measures ICU C15 and C16, laid before Parliament in June
  2026 [R10]: they would apply only to large services at medium risk, or any
  service at high risk, of terrorism, hate, harassment, stalking, threats and
  abuse, or foreign interference. Oxfer is at low risk of each.

### 5.2 Responsibilities, code of conduct and moderation policies

These are the documents that measures ICU A3, A6 and C3 to C6 call for, kept
short to fit a service run by one person. Reports and their handling are
recorded in the private abuse log ([README](README.md#private-records)).

**Statement of responsibilities (ICU A3).** [[OPERATOR_NAME]] is the provider's
only senior manager and the individual accountable under ICU A2. There is no
board or other governance body. [[OPERATOR_NAME]] is responsible for:

- the illegal content, children's access and children's risk assessments, and
  their reviews;
- the terms of use, the privacy notice and the abuse page;
- handling every report, complaint and appeal, and the blocks at the relay;
- referrals to NCMEC and authorities, and answers to Ofcom, eSafety and other
  regulators and authorities;
- the private records;
- the security of the build, the deployment and the relay host.

**Code of conduct (ICU A6).** Everyone working for the provider:

1. never opens, downloads or asks for reported content, and never forwards it;
2. never tells the person who shared content, or any other user, that a report
   was made or who made it;
3. records in the abuse log only what section 6 lists: never content, and never
   the `cap=` part of a link;
4. keeps no records about users beyond those the privacy notice describes, and
   raises the relay's logging only briefly while debugging a fault, as the
   [relay runbook](../../deploy/relay/README.md#what-is-logged) allows;
5. acts on reports in the order and within the targets below, and refers
   apparent CSAM without delay;
6. keeps reports and correspondence confidential and treats reporters, people
   reported and authorities fairly.

**Internal content policies (ICU C3).** What is not allowed: terms of use,
sections 4 and 5, which do not permit illegal content. How that is enforced:
[incident runbook](incident-runbook.md), sections 4 and 5, with the targets and
the order below. Both are reviewed at each yearly review, and earlier when
ICU A5 evidence shows a new kind of harm or an unusual increase.

**Performance targets (ICU C4 and D8).** Measured from the dates in the abuse
log at each quarterly review. Every missed target and every appeal upheld
because a decision was wrong is recorded with its cause, and the runbook is
changed where the cause was in the process.

| Action | Time target | Accuracy target |
| --- | --- | --- |
| Acknowledge a report or complaint (unless the sender opted out) | 24 hours | |
| Block a share reported for apparent CSAM | Without delay, before other reports | No block lifted on appeal because a report was misread or the wrong endpoint ID was blocked |
| Block a share after an intimate image content report | As soon as reasonably practicable, never later than 48 hours after receipt (statutory, section 5.3) | As above |
| Decide an expedited complaint from a person who made an intimate image content report | 48 hours | |
| Decide any other report or complaint and tell the reporter | 15 days; 7 days for a complaint from India | As above |
| Decide an appeal | Promptly, and within 15 days | No appeal upheld because the original decision was wrong |

**Prioritisation (ICU C5).** Reports are reviewed in this order:

1. apparent CSAM, and any threat to someone's life or safety;
2. intimate image content reports and expedited complaints about them;
3. other priority illegal content (terrorism first), and requests from
   authorities with a deadline;
4. other reports, complaints and appeals (appeals against address blocks
   first, because an address block can affect other people).

Within each group, a share that may still be live comes first, because it can
still reach more people (ICU C5.2(a)). Reports from authorities, NCMEC, hotlines
and other expert bodies are treated as more likely to concern illegal content
(ICU C5.2(c)). The order also reflects severity, including harm to children
(ICU C5.2(b)).

**Resourcing (ICU C6).** One person, the operator, handles every report, in
English or Romanian; UK users write in English. The operator checks
`abuse@oxfer.app` at least once a day, which the 24-hour acknowledgement
depends on, including during absences, or arranges cover by a person briefed
under ICU A7. If a surge of reports (for example after a news event) makes
the targets unreachable, blocks come first and answers follow, and the surge
is recorded as ICU A5 evidence and treated as a trigger for review
(section 7).

### 5.3 Duties added by the Crime and Policing Act 2026

In force since 29 June 2026 (section 1).

| Duty | How Oxfer meets it |
| --- | --- |
| Take down reported intimate image content within 48 hours [OSA s.10(3A)(a)] | On an intimate image content report, the operator blocks the reported share at the relay as soon as reasonably practicable and no later than 48 hours after receiving the report (abuse page, sections 3 and 6; [incident runbook](incident-runbook.md), section 4). The block ends the share for new recipients in the browser, with the limits stated under ICU C2. |
| Also take down content identified as the same or substantially the same [OSA s.10(3A)(b)] | The provider cannot see files, so it can identify other shares of the same images only from what it is told, such as other links the reporter gives. It blocks them within the same time (abuse page, section 6). Every share has a new endpoint ID, so a new share of the same images needs a new report. |
| Exception [OSA s.10(3B)] | If the operator concludes that the content is not intimate image content, or that the reporter is neither the person shown nor acting for them, it does not take the share down under this duty. It records its reasons, tells the reporter, and points to the expedited complaints procedure (terms, section 5.5). A report that lacks some of the s.20A(2) items is still acted on, and the operator may ask for the rest (abuse page, section 6). |
| Terms of service address s.10(3A) [OSA s.10(5)(b)] | Terms, section 5.5. |
| Easy reporting [OSA s.20A] | Abuse page, section 6, asks for the declarations and details listed in s.20A(2)(a) to (e) and for no images. Reports go to `abuse@oxfer.app` with URGENT in the subject; the abuse page is linked from every screen (ICU D1, D2 above). |
| Expedited complaints procedure [OSA s.21(2A)] | Abuse page, section 8, and terms, section 6: a complaint from a person who made an intimate image content report, about that content or the handling of the report, sent with URGENT Complaint in the subject, is handled before other messages, with the outcome within 48 hours. |

No alternative measures are used for these duties, so the additional record
that s.23(5) requires for alternative measures does not arise.

### 5.4 Measures in place beyond the Codes

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

### 5.5 Measures considered and not adopted

| Measure | Decision | Reasons |
| --- | --- | --- |
| ICU C9 Hash matching for CSAM | Not applicable; not adopted | The measure applies to a service that "(a) is at high risk of image-based CSAM, and: (i) has more than 700,000 monthly active United Kingdom users … or (ii) is a file-storage and file-sharing service; or (b) is a large service and is at medium or high risk of image-based CSAM" [R3, ICU C9.1]. Oxfer is at medium risk, is not large and does not store content. The measure covers content communicated publicly [R3, ICU C9.2], and Oxfer transfers are communicated privately (section 2.3); the Act does not allow Codes to recommend proactive technology for privately communicated content [OSA Sch. 4 para 13(4)]. It is also not technically feasible: the provider never holds plaintext, hashes of files or keys. |
| ICU C14 Hash matching for intimate image abuse (in the Codes from 30 September 2026) | Not applicable; not adopted | The measure applies to a service that lets users share photographs, videos or visual images and "(a) is at high risk of intimate image abuse, and: (i) the principal purpose of the service is the hosting or dissemination of regulated pornographic content; (ii) has more than 700,000 monthly active United Kingdom users …; or (iii) is a file-storage and file-sharing service; or (b) is a large service and is at medium or high risk of intimate image abuse" [R9, ICU C14.1]. Oxfer is at medium risk and not large. The measure covers only content communicated publicly [R9, ICU C14.2], and the feasibility points under ICU C9 apply equally. |
| Client-side scanning in the browser app | Not adopted | Would need a CSAM hash list on every user's device, which the provider cannot obtain or protect appropriately, and would change the privacy properties of the app for all users. |
| ICU C10 CSAM URL detection | Not applicable | Applies to large services at medium or high risk of CSAM URLs, and to services with more than 700,000 monthly active UK users at high risk of them [R3, section 3]. Oxfer is at low risk and shows no user-posted URLs. |
| Accounts or identity checks for senders | Not adopted | Would add personal data about every user, remove the no-records property the privacy design depends on, and add little protection where sender and recipient already know each other. |
| Logging IP addresses or share IDs | Not adopted | Would create records of who sent to whom. Blocking at the relay works from the moment of a report without any log. |
| Single-recipient links (a share closes to new recipients after the first completes) | Under consideration | Would strengthen the private character of transfers (section 2.3) and limit forwarding. To be decided at the next review. |
| In-app safety notice for recipients (only open links from people you know; scan files) | Under consideration | Low cost. The terms already say this. |

## 6. Step 4: records, reporting and review

- **Records.** This document is the written record under s.23(2) and (3): the
  assessment, and the Codes measures taken. It records no alternative
  measures (s.23(4)). Operational records are kept privately: the abuse log
  (date, kind of report, endpoint ID or address blocked, outcome, never
  content), relay denylist changes, the quarterly review notes (ICU A5 and the
  targets in section 5.2), and correspondence with authorities. See
  [README](README.md#private-records).
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
- moving the relay to a platform where it cannot see client addresses, such
  as the relay kit's Fly.io variant, which removes blocking by address (ICU C2);
- evidence of misuse in reports, including a new kind of harm or an unusual
  increase found under ICU A5, or a UK share of traffic that suggests the
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
[s.10](https://www.legislation.gov.uk/ukpga/2023/50/section/10),
[s.20A](https://www.legislation.gov.uk/ukpga/2023/50/section/20A),
[s.21](https://www.legislation.gov.uk/ukpga/2023/50/section/21),
[s.23](https://www.legislation.gov.uk/ukpga/2023/50/section/23),
[s.59](https://www.legislation.gov.uk/ukpga/2023/50/section/59),
[s.102](https://www.legislation.gov.uk/ukpga/2023/50/section/102),
[s.232](https://www.legislation.gov.uk/ukpga/2023/50/section/232),
[Schedule 4](https://www.legislation.gov.uk/ukpga/2023/50/schedule/4);
Crime and Policing Act 2026 [s.100](https://www.legislation.gov.uk/ukpga/2026/20/section/100)
and [s.101](https://www.legislation.gov.uk/ukpga/2026/20/section/101), in force
on 29 June 2026 under the
[Crime and Policing Act 2026 (Commencement No.1 and Saving Provision) Regulations 2026 (SI 2026/689), reg. 2(1)(j)](https://www.legislation.gov.uk/uksi/2026/689/regulation/2/made).
The section texts on legislation.gov.uk show the amendments and their dates.

- [R1] Ofcom, *Risk Assessment Guidance and Risk Profiles*:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/risk-assessment-guidance-and-risk-profiles.pdf?v=419933>
- [R3] Ofcom, *Illegal content Codes of Practice for user-to-user services*
  (text as laid, GOV.UK), from which the measure texts above were checked:
  <https://assets.publishing.service.gov.uk/media/6762c7dfff2c870561bde79f/draft-illegal-content-codes-of-practice-for-user-to-user-services.pdf>;
  version in force from 17 March 2025:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/illegal-content-codes-of-practice-for-user-to-user-services-24-feb.pdf?v=391889>
- [R4] The Online Safety Act 2023 (Priority Offences) (Amendment) Regulations 2025,
  SI 2025/1352, made 18 December 2025 and in force on the 21st day after
  (8 January 2026, reg. 1(2)):
  <https://www.legislation.gov.uk/uksi/2025/1352/contents/made>
- [R5] Ofcom, *Guidance on content communicated 'publicly' and 'privately'*:
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/guidance-on-content-communicated-publicly-and-privately-under-the-online-safety-act.pdf?v=388093>
- [R6] Ofcom, *Statement: New priority offences - serious self-harm and cyberflashing* (June 2026):
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/statement-new-priority-offences-serious-self-harm-and-cyberflashing>
- [R7] Ofcom, enforcement programme on file-sharing and file-storage services and CSAM:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/enforcement-programme-into-measures-being-taken-by-file-sharing-and-file-storage-services-to-prevent-users-from-encountering-or-sharing-child-sexual-abuse-material-csam>
- [R8] Ofcom, fine on a file-sharing service for failing to answer information requests:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/ofcom-fines-online-file-sharing-service-20000>
- [R9] Ofcom, *Amendments to the Illegal content Codes of Practice for
  user-to-user services* adding ICU C14 (text as laid, GOV.UK; submitted
  15 May 2026, in force at the end of 21 days beginning with the day Ofcom
  issues them, para 1.5; issued on 9 September 2026, so in force from
  30 September 2026):
  <https://www.gov.uk/government/publications/online-safety-act-draft-amendments-to-the-illegal-content-codes-of-practice/draft-amendments-to-illegal-content-codes-of-practice-for-user-to-user-services>;
  Ofcom, *Statement: Detecting intimate image abuse*:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/statement-detecting-intimate-image-abuse>
- [R10] Ofcom, *Draft amendments (No. 2) to the Illegal content Codes of
  Practice for user-to-user services* (crisis response, ICU C15 and C16; laid
  18 June 2026, GOV.UK):
  <https://www.gov.uk/government/publications/online-safety-act-draft-amendments-no2-to-the-illegal-content-codes-of-practice/draft-amendments-no-2-to-the-illegal-content-codes-of-practice-for-user-to-user-services>
- [R11] Ofcom, *Illegal Content Judgements Guidance* (current version):
  <https://www.ofcom.org.uk/siteassets/resources/documents/online-safety/information-for-industry/illegal-harms/illegal-content-judgements-guidance-icjg.pdf>
- Ofcom, online safety regulatory documents and guidance (current versions, including record-keeping guidance):
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/online-safety-regulatory-documents>
