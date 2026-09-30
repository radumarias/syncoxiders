# UK Online Safety Act: children's access assessment

| | |
| --- | --- |
| Status | **DRAFT.** Not adopted until dated and signed below. |
| Service | Oxfer: web app at `oxfer.app`, relay at `relay.oxfer.app`, desktop app built from this crate |
| Provider | [[OPERATOR_NAME]], [[OPERATOR_ADDRESS]] |
| Assessment date | [[EFFECTIVE_DATE]] |
| Next assessment due | Not more than one year after [[EFFECTIVE_DATE]] (provider's policy; see section 1) |
| Method | Ofcom's two-stage test in its *Children's access assessments guidance* [C1] |
| Conclusion | **Oxfer is treated as likely to be accessed by children** (section 4) |

This record is published in a public repository. It is not legal advice. The
service description is in section 2 of the
[illegal content risk assessment](osa-illegal-content.md).

## 1. Legal test

- A children's access assessment asks whether it is possible for children to
  access the service or part of it, and if so whether the child user condition
  is met [OSA s.35(1)].
- A provider may conclude that children cannot access the service only if age
  verification or age estimation is used with the result that children are not
  normally able to access it [OSA s.35(2)]. Ofcom's guidance requires that age
  assurance to be highly effective [C1][C3].
- The child user condition is met if there is a significant number of children
  who are users of the service, or the service is of a kind likely to attract a
  significant number of children. "Significant" includes a number that is
  significant in proportion to the total number of UK users, and the assessment
  is based on evidence about who actually uses the service, not who it is
  intended for [OSA s.35(3), (4)].
- A child is a person under 18 [OSA s.236].
- Ofcom advises providers to err on the side of caution and to conclude that the
  condition is not met only where evidence shows that neither limb is met
  [C1][C3][C4]. Factors for the second limb include whether the service provides
  benefits to children, whether its content or design appeals to children, and
  whether children are part of its commercial strategy [C1].
- A service whose provider does not carry out the assessment is treated as
  likely to be accessed by children [OSA s.37].
- Every children's access assessment must be recorded in writing
  [OSA s.36(7)]. The Act requires a new assessment at least every year, before
  a significant change, and in response to certain evidence only while a
  service is *not* treated as likely to be accessed by children
  [OSA s.36(2) to (4)]. Once a service is so treated, as Oxfer is (section 4),
  those repeat duties stop; they would apply again only if a later assessment
  concluded that the child user condition is not met. The yearly review in
  section 7 is therefore the provider's own policy, not a statutory deadline.

## 2. Stage 1: can children normally access Oxfer?

**Yes.**

Oxfer uses no age assurance of any kind: no age verification, no age
estimation, and no self-declaration. The terms of use set a minimum age of 13
and ask users below the local age of consent to have a parent's or guardian's
permission, but terms are not age assurance, and self-declaration would not be
highly effective age assurance either [C3]. Children can therefore open
`oxfer.app`, create a share and receive one.

## 3. Stage 2: is the child user condition met?

### 3.1 Evidence available

| Source | What it shows |
| --- | --- |
| Accounts or age data | None exist. Oxfer has no accounts and asks no one's age. |
| Analytics | None. Cloudflare's dashboard shows aggregate requests per country, with no age information. |
| Reports and complaints | Reports mentioning users under 18, from the private abuse log: *to be completed by the operator at adoption.* |
| Commercial strategy | None. Oxfer is free, has no advertising, and is not marketed to anyone. |
| External research on who uses browser file-transfer tools | None identified that would allow a conclusion about Oxfer. |

The evidence gap is stated as it is: the provider has no data about the age of
its users and, by design, collects none.

### 3.2 First limb: a significant number of children who are users

Not known. The terms admit users aged 13 to 17 (with parental permission where
local law requires it), so the service is open to a group that the Act counts as
children. Nothing in the available evidence shows that the number or proportion
of UK users under 18 is not significant.

### 3.3 Second limb: of a kind likely to attract children

| Ofcom factor [C1] | Oxfer |
| --- | --- |
| Benefits to children | Yes, as a general utility: sending photos, videos or schoolwork between devices is useful to anyone. |
| Content appealing to children | Oxfer has no content of its own. What a user receives is whatever someone sends them. |
| Design appealing to children | Neutral. A plain utility with no characters, games, rewards, streaks or social features. |
| Children in the commercial strategy | No. No marketing, no child-directed features. |

These factors are mixed. They do not show that the service is not of a kind
likely to attract a significant number of children.

## 4. Conclusion

It is possible for children to access Oxfer (section 2). The provider cannot
show that neither limb of the child user condition is met (section 3). Following
Ofcom's guidance to err on the side of caution [C1], the provider treats the
child user condition as met.

**Oxfer is treated as likely to be accessed by children from [[EFFECTIVE_DATE]].**

This is a precautionary conclusion drawn from the absence of evidence. It is not
a finding that children use Oxfer in any particular number.

## 5. Consequences

1. **Children's risk assessment.** Must be completed within three months of the
   date the service is first treated as likely to be accessed by children
   [OSA Sch. 3 para 5]. Draft: [osa-children-risk.md](osa-children-risk.md).
2. **Children's safety duties and the Protection of Children Code.** Measures
   recommended for every service likely to be accessed by children include an
   accountable individual (PCU A2), content moderation and swift action
   (PCU C1, C2), complaints and appeals (PCU D1, D2, D7, D9, D10), handling of
   complaints about non-compliance and manifestly unfounded complaints
   (PCU D13, D14) and terms of service (PCU G1, G3) [C2]. Because the terms
   do not prohibit every kind of primary priority content on the whole service
   (Code para 5.16), measure PCU B4 on highly effective age assurance is
   engaged. The options are set out
   in the [children's risk assessment](osa-children-risk.md#6-decision-needed-pcu-b4);
   choosing one is an owner decision.
3. **Illegal content Codes.** Measures ICU D3 and D5 apply to services likely to
   be accessed by children that are at medium or high risk of any kind of
   illegal harm; the [illegal content risk assessment](osa-illegal-content.md#51-codes-measures-that-apply)
   records them as adopted.

## 6. What would change the conclusion

- **Stage 1:** highly effective age assurance for UK users that keeps children
  out of the service. This would require collecting or checking age data for
  every UK user, which the current design avoids.
- **Stage 2:** reliable evidence that UK children do not use Oxfer in a
  significant number and that it is not of a kind likely to attract them, such
  as independent research on children's use of browser file-transfer tools. Any
  new collection of age data by Oxfer would itself be a significant change,
  reviewed under the [ROPA](ropa.md).
- Raising the minimum age in the terms to 18 would not by itself change the
  conclusion, because the test looks at actual users [OSA s.35(4)] and terms
  are not age assurance.
- Evidence pointing the other way, such as reports involving children, would
  confirm the conclusion and feed the children's risk assessment.

## 7. Review

As the provider's own policy (section 1), repeat this assessment not more than
one year after [[EFFECTIVE_DATE]], before any change listed in section 7 of the
[illegal content risk assessment](osa-illegal-content.md#7-triggers-for-a-new-assessment-before-a-change),
and when Ofcom revises its guidance.

## 8. Sign-off

| | |
| --- | --- |
| Assessed and approved by | [[OPERATOR_NAME]] |
| Date | [[EFFECTIVE_DATE]] |
| Next assessment due | Not more than one year after [[EFFECTIVE_DATE]] (provider's policy) |

## Sources

Statute (legislation.gov.uk): [OSA s.35](https://www.legislation.gov.uk/ukpga/2023/50/section/35),
[s.36](https://www.legislation.gov.uk/ukpga/2023/50/section/36),
[s.37](https://www.legislation.gov.uk/ukpga/2023/50/section/37),
[s.236](https://www.legislation.gov.uk/ukpga/2023/50/section/236),
[Schedule 3](https://www.legislation.gov.uk/ukpga/2023/50/schedule/3).

- [C1] Ofcom, *Children's access assessments guidance* (24 April 2025):
  <https://www.ofcom.org.uk/siteassets/resources/documents/consultations/category-1-10-weeks/statement-age-assurance-and-childrens-access/childrens-access-assessments-guidance.pdf?v=395679>;
  quick guide: <https://www.ofcom.org.uk/online-safety/illegal-and-harmful-content/quick-guide-to-childrens-access-assessments>
- [C2] Ofcom, *Protection of Children Code of Practice for user-to-user services* (GOV.UK):
  <https://assets.publishing.service.gov.uk/media/680a04f7532adcaaab3a2718/FINAL_-_Protection_of_Children_Code_of_Practice_for_user-to-user_services__2025_Parli_AC.pdf>
- [C3] Osborne Clarke, summary of Ofcom's age assurance and children's access statement (secondary):
  <https://www.osborneclarke.com/insights/uk-online-safety-act-ofcom-publishes-guidance-age-assurance-and-childrens-access>
- [C4] Tremau, key considerations for children's access assessments (secondary):
  <https://tremau.com/resources/two-days-left-key-considerations-for-the-online-safety-acts-childrens-access-assessment/>
