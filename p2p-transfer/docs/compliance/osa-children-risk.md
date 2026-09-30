# UK Online Safety Act: children's risk assessment

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated and signed below. Section 6 needs an owner decision first. |
| Service | Oxfer: web app at `oxfer.app`, relay at `relay.oxfer.app`, desktop app built from this crate |
| Provider | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]] |
| Why this exists | The [children's access assessment](osa-children-access.md) treats Oxfer as likely to be accessed by children. A children's risk assessment is then due within three months [OSA Sch. 3 para 5]. |
| Assessment date | [[EFFECTIVE_DATE]] |
| Method | Ofcom's four-step method in its *Children's Risk Assessment Guidance and Children's Risk Profiles* [K1] |

This record is published in a public repository. It is not legal advice. The
service description and the design factors D1 to D4 and R1 to R4 are in
sections 2 and 4.1 of the [illegal content risk assessment](osa-illegal-content.md).

## 1. Legal frame

- The assessment covers the user base including children in different age
  groups, the risk of children encountering each kind of primary priority
  content, priority content and non-designated content harmful to children,
  the functionalities that affect that risk, and how design and operation
  reduce or increase it; it must be kept up to date and redone before a
  significant change [OSA s.11].
- Primary priority content: pornographic content, and content that encourages,
  promotes or gives instructions for suicide, deliberate self-injury or an
  eating disorder [OSA s.61].
- Priority content: abusive or hateful content targeting protected
  characteristics, bullying content, content encouraging or instructing serious
  violence, graphic depictions of serious violence or injury to people or
  animals, dangerous stunts and challenges, and content encouraging the
  self-administration of harmful substances [OSA s.62].
- Ofcom's guidance asks providers to assess children by age group (0 to 5, 6 to
  9, 10 to 12, 13 to 17) and assign each kind of content a risk level of
  negligible, low, medium or high [K1].

## 2. Step 1: children who may use Oxfer

- The terms set a minimum age of 13. The group most likely to use Oxfer is
  13 to 17. Users under 13 are not permitted but are not prevented.
- No age data exists (see the [children's access assessment](osa-children-access.md#31-evidence-available)).
- How a child meets content on Oxfer: only by opening a link that someone sent
  them through another channel, choosing to receive, saving the file and opening
  it outside Oxfer. Oxfer shows the file names and sizes first and has no
  in-app preview, feed, search or recommender (D1, D3).

## 3. Step 2: risk to children

| Kind of content | Risk level | Reasons |
| --- | --- | --- |
| Pornographic content (PPC) | Low | Can be sent to a child who opens a link, for example between peers. No child meets it by browsing, and nothing is displayed without the child choosing to save and open a file. |
| Suicide, self-harm and eating disorder content (PPC) | Low | Same route. No public reach, no recommender that could repeat exposure. |
| Abuse and hate, bullying (PC) | Low | Bullying can use files (for example humiliating images) or file names as messages, but only towards someone who opens the sender's link. Onward sharing of saved files happens on other services. |
| Violent content, including animal violence (PC) | Low | Same route as pornographic content. |
| Harmful substances, dangerous stunts and challenges (PC) | Low | Same route; no discovery or trends on Oxfer. |
| Non-designated content | Low | No kind identified beyond those above. |

The evidence limits in the illegal content risk assessment apply here too: the
provider sees no content and has no age data, so the ratings rest on design
factors and on reports.

Result: low risk for every kind. The service is not multi-risk (children).

## 4. Step 3: measures

Protection of Children Code measures that apply to a service likely to be
accessed by children that is neither large nor multi-risk [K2, section 3]:

| Measure | Status | Note |
| --- | --- | --- |
| PCU A2 Accountable individual | Adopted | [[OPERATOR_NAME]]. |
| PCU C1 Content moderation function | Adopted, adapted | As ICU C1 in the illegal content risk assessment. |
| PCU C2 Swift action against content harmful to children | Adopted, adapted, with a technical feasibility record | The only action available is blocking a reported share at the relay (take-down for new recipients) or an address. The provider cannot see content, so it cannot apply content-level controls such as blurring, warnings or down-ranking; this is recorded under PCU C2.12 [K2]. |
| PCU D1, D2 Complaints | Adopted | `abuse@oxfer.app`. |
| PCU D7 Action on complaints about content harmful to children | Adopted | Handled under C1 and C2. |
| PCU D9, D10 Appeals | Adopted | Abuse page, section 8. |
| PCU D13 Complaints about non-compliance with the children's safety duties or the reporting duty | Adopted | Such complaints go to `abuse@oxfer.app` (terms, section 6; abuse page, section 8). The nominated individual is [[OPERATOR_NAME]], who handles them as ICU D12 in the illegal content risk assessment, within the 15-day timeframe on the abuse page [K2, PCU D13.3, D13.4]. |
| PCU D14 Manifestly unfounded complaints | Not used | There is no policy for disregarding complaints, so none is disregarded; every complaint is considered [K2, PCU D14.2]. |
| PCU G1, G3 Terms of service | Open | The terms must say how children are protected from each kind of primary priority and priority content, depending on the decision in section 6. |
| PCU B4 Highly effective age assurance where primary priority content is not prohibited | **Decision needed** | Section 6. |

Measures for large or multi-risk (children) services, or for services at medium
or high risk of specific kinds of content, do not apply on the ratings above.

## 5. Why PCU B4 is engaged

- A kind of primary priority content is "prohibited" only if the terms prohibit
  it on all child-accessible parts of the service for all users; otherwise it is
  "allowed" [K2, para 5.16].
- Oxfer's terms prohibit illegal content, including child sexual abuse material
  and non-consensual intimate images, but not lawful adult pornography between
  adults, and not every kind of content that promotes self-harm or eating
  disorders. Those kinds are therefore allowed.
- PCU B4 applies to a service likely to be accessed by children where a kind of
  primary priority content is allowed, or where all are prohibited but it is not
  currently technically feasible to take down content found in breach
  [K2, PCU B4.1]. It recommends highly effective age assurance to target the
  content or access controls in PCU C2.3(b) and C2.4 at children [K2, PCU B4.2].

## 6. Decision needed: PCU B4

| Option | Effect | Trade-off |
| --- | --- | --- |
| A. Prohibit all four kinds of primary priority content in the terms, for all users, and act on reports by blocking the share | PCU B4.1(b)(i) no longer applies. Blocking at the relay is a take-down for browser shares. | Adults could no longer lawfully use Oxfer for adult pornography under the terms. A block cannot reach a direct native-to-native connection, which may bring PCU B4.1(b)(ii) back into question for desktop builds. |
| B. Keep the current terms and record an alternative approach | The provider records why B4 is not followed and how the duties are met otherwise [OSA s.23]. | Relies on the provider's reasoning (no content visibility, low risk, recipient-initiated access) being accepted. |
| C. Highly effective age assurance for UK users | Follows B4 as written. | Requires collecting or checking age data for UK users, which the privacy design avoids, and a third-party age assurance provider. |
| D. Withhold the service from the UK | Stops serving UK users. Whether any duty remains then depends on the UK links test [OSA s.4]. | The fallback lever under decision D2 in the [plan](../compliance-plan.md), not the default. |

The draft takes no option. The owner records the choice and reasons here before
signing, and the terms and this record are then aligned.

## 7. Step 4: records and review

Kept with the illegal content records (see [README](README.md#private-records)).
Reviewed with the children's access assessment at least every 12 months, when
Ofcom changes a Children's Risk Profile, and before any change listed in section
7 of the [illegal content risk assessment](osa-illegal-content.md#7-triggers-for-a-new-assessment-before-a-change).

## 8. Sign-off

| | |
| --- | --- |
| Option chosen in section 6 | *to be completed by the operator* |
| Assessed and approved by | [[OPERATOR_NAME]] |
| Date | [[EFFECTIVE_DATE]] |

## Sources

Statute (legislation.gov.uk): [OSA s.4](https://www.legislation.gov.uk/ukpga/2023/50/section/4),
[s.11](https://www.legislation.gov.uk/ukpga/2023/50/section/11),
[s.23](https://www.legislation.gov.uk/ukpga/2023/50/section/23),
[s.61](https://www.legislation.gov.uk/ukpga/2023/50/section/61),
[s.62](https://www.legislation.gov.uk/ukpga/2023/50/section/62),
[Schedule 3](https://www.legislation.gov.uk/ukpga/2023/50/schedule/3).

- [K1] Ofcom, *Children's Risk Assessment Guidance and Children's Risk Profiles* (24 April 2025):
  <https://www.ofcom.org.uk/siteassets/resources/documents/consultations/category-1-10-weeks/statement-protecting-children-from-harms-online/main-document/childrens-risk-assessment-guidance-and-childrens-risk-profiles.pdf?v=396653>
- [K2] Ofcom, *Protection of Children Code of Practice for user-to-user services* (GOV.UK):
  <https://assets.publishing.service.gov.uk/media/680a04f7532adcaaab3a2718/FINAL_-_Protection_of_Children_Code_of_Practice_for_user-to-user_services__2025_Parli_AC.pdf>
- Ofcom, quick guide to the children's safety codes:
  <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/quick-guide-to-childrens-safety-codes>
