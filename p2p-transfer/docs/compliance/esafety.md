# Australia: Online Safety Act 2021 codes and standards self-assessment

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated and signed below. Section 7 is incomplete. |
| Service | Oxfer: web app at `oxfer.app`, relay at `relay.oxfer.app`, desktop app built from this crate |
| Provider | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]] |
| Assessment date | [[EFFECTIVE_DATE]] |
| Result | Designated internet service (DIS); Tier 3 under the DIS Standard for class 1A and 1B material |

This record is published in a public repository. It is not legal advice. The
service description is in section 2 of the
[illegal content risk assessment](osa-illegal-content.md).

## 1. Summary

- Oxfer is best classed as a **designated internet service**, not a relevant
  electronic service (section 3).
- Within the *Online Safety (Designated Internet Services—Class 1A and Class 1B
  Material) Industry Standard 2024* ("DIS Standard"), Oxfer falls in none of the
  named categories, so it must carry out a risk assessment and determine its own
  tier (section 4).
- The risk assessment in section 5 finds a **low** risk, which makes Oxfer a
  **Tier 3** DIS [A2, s.7(8)]. Tier 3 obligations are sections 31, 33 and 37,
  plus record-keeping under section 38 (section 6).
- The Phase 2 *Designated Internet Services Online Safety Code (Class 1C and
  Class 2 Material)* has applied since 9 March 2026 and needs its own risk
  assessment (section 7).

The plan assumed "DIS Tier 3". That holds, with two corrections: the tier comes
from a documented risk assessment rather than a pre-assessment, and the Phase 2
code adds a second assessment.

## 2. Legal frame

- **Relevant electronic service (RES):** a service that enables end-users to
  communicate with other end-users by email, instant messaging, SMS, MMS or a
  chat service, or to play online games with other end-users, or a service
  specified in legislative rules [A1, s.13A(1)].
- **Designated internet service (DIS):** a service that allows end-users to
  access material using an internet carriage service, or delivers material by
  means of one, other than a social media service, a RES, an on-demand program
  service or an exempt service [A1, s.14(1)].
- Both industry standards apply to a service wherever it is provided from, but
  only so far as it is provided to end-users in Australia [A2, s.5(1)]
  [A3, s.5(1)].
- Class 1A material covers child sexual exploitation material, pro-terror
  material and extreme crime and violence material; class 1B covers crime and
  violence material and drug-related material [A3, s.6] [A4].

## 3. RES or DIS?

| Question | Answer |
| --- | --- |
| Email, SMS or MMS? | No. |
| Instant messaging or chat between end-users? | No. Oxfer has no way to send messages. The recipient sees file names, sizes and checksums of the files offered and chooses whether to save them. |
| Online games? | No. |
| Specified in legislative rules as a RES? | None identified. |
| Access to or delivery of material over the internet? | Yes: files are delivered from one end-user to another over the internet. |

**Conclusion: DIS.** The classification matters. Had Oxfer been a RES whose
predominant purpose is to let an end-user communicate with another end-user, it
would be a pre-assessed "communication relevant electronic service", and most of
the RES Standard's compliance measures would apply [A3, s.6 and s.12]. The
provider's view is that transferring files, with no messaging, is not
communication "by means of" email, instant messaging, SMS, MMS or chat within
s.13A. This is reconsidered if Oxfer ever adds messaging (section 9).

## 4. Category within the DIS Standard

| DIS Standard category [A2, s.6] | Oxfer? | Reason |
| --- | --- | --- |
| End-user managed hosting service: primarily designed or adapted to enable end-users to store or manage material; examples include online file storage, photo storage and other online media hosting, including where it allows sharing | No | Oxfer stores nothing. Files stay on the sender's device and stream to the recipient while the sender is online. |
| General purpose DIS (information for business, education, government and similar purposes, or a web browser) | No | Not an information service. |
| Enterprise DIS (account holder is an organisation using it for its activities) | No | No accounts or enterprise customers. |
| Classified DIS, high impact DIS, high impact generative AI DIS, model distribution platform | No | Oxfer offers no classified or high impact material, no generative AI and no models. |

None applies, so Oxfer must carry out a risk assessment and determine its risk
profile [A2, s.7(1), (6), (7)].

If eSafety took the view that Oxfer is an end-user managed hosting service
because it lets users share files, the service would be exempt from the risk
assessment but subject to the end-user managed hosting obligations in item 5 of
section 12 of the DIS Standard [A2, s.12]. These include systems to detect and
remove known child sexual abuse material; a provider need not use a system or
technology that is not technically feasible or would require new decryption
capability in an end-to-end encrypted service, but must then take appropriate
alternative action [A2, s.20(2) to (5)]. The provider's view is that the storage
element of the definition is not met.

## 5. Risk assessment under sections 7 and 8

### 5.1 Plan and methodology

This record is the written plan and methodology required by s.8(1). It follows
the matters in s.8(5) and the forward-looking analysis in s.8(4). It was carried
out by [[OPERATOR_NAME]], who designed and operates the service (s.8(3)).

### 5.2 Matters taken into account (s.8(5))

| Matter | Oxfer |
| --- | --- |
| (a) Predominant purpose | Sending files directly from one device to another. |
| (b) Functionality, including posting or sharing | Users share files through a link. No public posting. |
| (c) How material is created or contributed | Users send their own files; Oxfer creates no material. |
| (d) Chat, messaging or other communications functionality | None. |
| (e) Availability of material to end-users in Australia | Only to holders of a given link, only while the sender stays online. No search, directory, feed or public pages. |
| (f) Terms of use | Prohibit child sexual abuse material, terrorist content, other illegal content, malware and harassment ([terms](../../terms.html), section 4). |
| (g) Terms on which the provider acquires content | Not applicable; the provider acquires no content. |
| (h) Ages of end-users | Unknown; minimum age 13 in the terms; no age data (see the [children's access assessment](osa-children-access.md)). |
| (i) Forward-looking analysis | Section 5.3. |
| (j) Safety by design guidance | Ofcom's Risk Profiles were used; the design factors are in section 4.1 of the [illegal content risk assessment](osa-illegal-content.md). eSafety's own safety-by-design material is to be checked at the first review. |
| (k) to (m) Material generated by AI | Oxfer has no AI features. |

### 5.3 Forward-looking analysis (s.8(4))

Expected changes: production builds move to the operator's own relay; a desktop
build may be distributed; use may grow. None of these adds storage, discovery,
messaging or public reach, so none is expected to raise the risk of class 1A or
1B material being accessed, distributed or stored. Changes that would are listed
in section 9.

### 5.4 Risk and determination

| Risk under s.7(1) | Assessment |
| --- | --- |
| Class 1A or 1B material generated using the service | Low. Oxfer generates nothing. |
| Accessed by, or distributed to, end-users in Australia | Low. Oxfer can carry such material privately between people who already share a link, and the provider cannot detect it because transfers are end-to-end encrypted. It offers no way to find material, no public reach, no persistence after the sender goes offline, and no amplification. |
| Stored on the service | None. Oxfer stores no files. |

**Determination: low risk, Tier 3** [A2, s.7(8), item 3]. Reasons are recorded
here as required by s.9.

## 6. Obligations of a Tier 3 DIS and how Oxfer meets them

A Tier 3 DIS must comply with sections 31, 33 and 37 [A2, s.12(1), items 1 and 4],
and every DIS with section 38 [A2, s.38(1)].

| Provision | Obligation | How met |
| --- | --- | --- |
| s.7(5) | No material change unless reassessed or the change does not increase risk | Triggers in section 9. |
| s.31 | Give eSafety, on written notice, the risk profile determination, the risk assessment record and methodology, within the period set | This file. Notices go to `abuse@oxfer.app` ([abuse page](../../abuse.html), section 9.3). |
| s.33 | Notify eSafety of a new or removed feature unless it will not significantly increase the risk of the service being used to generate high impact material | Oxfer has no generative features; adding one requires notification. |
| s.37 | eSafety may extend reporting deadlines on application | Used if a notice's deadline cannot be met. |
| s.38 | Keep records of actions taken to comply for at least 2 years after the end of the calendar year of the action | This record and the private abuse log (see [README](README.md#private-records)). |

Beyond Tier 3, Oxfer voluntarily keeps a report channel, blocks reported shares
at its relay, and reports apparent child sexual abuse material (see the
[incident runbook](incident-runbook.md)).

## 7. Phase 2: DIS Online Safety Code (Class 1C and Class 2 Material)

eSafety registered the Phase 2 DIS code on 9 September 2025 and it took effect on
9 March 2026 [A5] [A6]. It deals with material unsuitable for children, including
pornography, self-harm material and high-impact violence, and sorts services
into tiers by the risk that Australian children encounter such material; lower
risk services carry fewer obligations once they document a risk assessment [A6].

The code text itself [A7] was not reviewed for this draft. Before adoption:

1. Read Schedule 6 [A7] and identify Oxfer's category and the risk assessment it
   requires.
2. Record the assessment here, reusing the reasoning in the
   [children's risk assessment](osa-children-risk.md), which rates every kind of
   content harmful to children as low.
3. Record the resulting tier and any obligations.

## 8. Other duties under the Act

- **Class 1 removal notices.** eSafety may require a DIS provider to take all
  reasonable steps to remove class 1 material within 24 hours [A1, s.109]. Oxfer
  holds no material; the reasonable step available is blocking the share at the
  relay, then replying within the deadline.
- **Basic Online Safety Expectations.** The Act sets expectations for DIS
  providers and lets eSafety require reports [A1, Part 4]. Requests are answered
  through `abuse@oxfer.app`.

## 9. Review triggers

Reassess before any of: file storage or store-and-forward delivery; messaging or
chat (which would reopen section 3); accounts, search, public pages or feeds;
generative AI features; evidence from reports of class 1A or 1B material; or
growth that makes Australian use material. Otherwise review with the UK
assessments every 12 months.

## 10. Sign-off

| | |
| --- | --- |
| Assessed and approved by | [[OPERATOR_NAME]] |
| Date | [[EFFECTIVE_DATE]] |

## Sources

- [A1] *Online Safety Act 2021* (Cth), compilation of 12 September 2026, ss.13A, 14, 109 and Part 4:
  <https://www.legislation.gov.au/C2021A00076/latest/text>
- [A2] *Online Safety (Designated Internet Services—Class 1A and Class 1B Material) Industry Standard 2024*, F2024L00710:
  <https://www.legislation.gov.au/F2024L00710/asmade/text>
  (PDF: <https://www.legislation.gov.au/F2024L00710/asmade/2024-06-21/text/original/pdf>)
- [A3] *Online Safety (Relevant Electronic Services—Class 1A and Class 1B Material) Industry Standard 2024*, F2024L00711:
  <https://www.legislation.gov.au/F2024L00711/asmade>
  (PDF: <https://www.legislation.gov.au/F2024L00711/asmade/2024-06-21/text/original/pdf>)
- [A4] eSafety, fact sheet on the registration of the DIS Standard:
  <https://www.esafety.gov.au/sites/default/files/2024-06/Fact-sheet-registration-DIS-Standard.pdf>
- [A5] eSafety, Register of Online Safety Codes and Standards:
  <https://www.esafety.gov.au/industry/codes/register-online-industry-codes-standards>
- [A6] Digital Policy Alert, DIS Online Safety Code (Class 1C and Class 2 Material) enters into force (secondary):
  <https://digitalpolicyalert.org/event/33342-designated-internet-services-online-safety-code-class-1c-and-class-2-material-enters-into-force>;
  Baker McKenzie, Phase 2 codes registered (secondary):
  <https://connectontech.bakermckenzie.com/australia-phase-2-online-safety-codes-registered-by-esafety-commissioner/>
- [A7] Schedule 6, *Designated Internet Services Online Safety Code (Class 1C and Class 2 Material)*:
  <https://www.esafety.gov.au/sites/default/files/2025-09/Schedule-6-Designated-Internet-Services-Online-Safety-Code-Class-1C-and-Class-2-Material.pdf>
