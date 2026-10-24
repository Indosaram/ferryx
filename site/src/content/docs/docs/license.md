---
title: License FAQ
description: Which uses the Ferryx License 1.0 allows free, which need a Team plan, and which it forbids, with the LICENSE clause behind each answer.
---

Ferryx is source-available under the [Ferryx License 1.0](https://github.com/Indosaram/ferryx/blob/main/LICENSE). This page maps common situations to the clause of the license that decides them. It quotes the license; it does not interpret it for your situation.

:::caution[Not legal advice]
This page is not legal advice. The [`LICENSE`](https://github.com/Indosaram/ferryx/blob/main/LICENSE) file is the only binding text. If your case is unclear, read the full license or ask a lawyer.
:::

## Key definitions

- **Small Company.** "A company that, counted together with every organization that controls it, is controlled by it, or is under common control with it, has fewer than 50 employees and contractors and less than USD 5,000,000 in annual revenue."
- **Use on behalf of a company.** "'Use on behalf of a company' includes use by its employees and contractors for its work."

## Ferryx License 1.0 (current releases)

| Case | Allowed? | Clause |
| :--- | :--- | :--- |
| Personal use, including a personal side project that earns money | Yes, free | Free Use 1: "personal use by an individual, including personal side projects that earn money" |
| Teaching, studying, or academic research at an accredited school or university | Yes, free | Free Use 2 |
| Work on an OSI-licensed open-source project, not paid for by a company that is not a Small Company | Yes, free | Free Use 3 |
| Use by a non-profit organization | Yes, free | Free Use 4 |
| Internal work use at a Small Company (under 50 people and under USD 5M revenue) | Yes, free | Free Use 5: "any use by or on behalf of a Small Company" |
| Work use at a larger company | Needs a Team plan or higher for each person who uses it | Commercial Use: "requires an active paid subscription to the Ferryx Team plan or a higher plan, covering each person in that company who uses the software" |
| Work use at a larger company before the Team plan opens, and for 90 days after | Yes, free | Transition Period |
| Consulting or contracting: using Ferryx for a client's work | Follows the client's status: free for a Small Company client, Team plan for a larger one | Commercial Use: use on behalf of a company "includes use by its employees and contractors for its work" |
| Running your own `ferryx-relay` server, for any company size | Yes, free | Self-Hosted Relay: "Anyone, including a company that is not a Small Company, may run the Ferryx relay server" |
| Redistributing unmodified or modified copies free of charge for non-commercial purposes | Yes, with the license and a modification notice included | Distribution; Notices |
| Selling copies, or bundling Ferryx into a product you sell | No | Distribution: "only free of charge and only for non-commercial purposes" |
| Hosting Ferryx for others as a hosted or managed service | No | Distribution: "You may not offer the software, or a modified version of it, to third parties as a hosted or managed service." |
| Removing license notices or working around a license key check | No | Distribution |
| Files in an `ee` directory or marked as Ferryx Commercial License | Not covered; needs a license key | Commercial Features |

See [Pricing & Plans](/docs/pricing/) for the Team plan and its status.

## Earlier releases (SUL-1.0)

The license says: "These terms apply only to versions of the software released with these terms. Each version released earlier remains available under the license it was released with." Releases published before the move to the Ferryx License shipped under the Sustainable Use License 1.0 (SUL-1.0), whose limitation clause reads: "You may use or modify the software only for your own internal business purposes or for non-commercial or personal use. You may distribute the software or provide it to others only if you do so free of charge for non-commercial purposes."

| Case, on an earlier SUL-1.0 release | What the SUL-1.0 clause says |
| :--- | :--- |
| Personal use | Allowed: "non-commercial or personal use" |
| Internal company use, any company size | Allowed: "your own internal business purposes" |
| Consulting for a client | Not named; the clause covers only "your own internal business purposes" |
| Redistribution | Only "free of charge for non-commercial purposes" |
| Bundling into a product you sell | Not allowed: distribution must be "free of charge for non-commercial purposes" |
| Hosting for others | Only "free of charge for non-commercial purposes" |

A release keeps the license it shipped with; check the `LICENSE` file inside the release you use.
