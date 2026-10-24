---
title: "Ferryx Is Changing Its License: What It Means for You"
description: "Ferryx moves from SUL-1.0 to the Ferryx License. Individuals and small companies stay free; larger companies need Team for work use after a grace period."
---

Starting with the next release, Ferryx ships under the [Ferryx License](https://github.com/Indosaram/ferryx/blob/main/LICENSE) instead of the Sustainable Use License (SUL-1.0). For almost everyone reading this, nothing changes: Ferryx stays free to download, free to use, and the source stays public.

## Who stays free

You can keep using Ferryx at no cost if you are:

- an individual, including for side projects that make money;
- a student, teacher, or academic researcher;
- working on an open-source project (the Ferryx License itself is source-available);
- a non-profit; or
- a company with fewer than 50 employees and contractors **and** less than USD 5M in annual revenue.

Self-hosted relays stay free for everyone, large companies included. The "perpetually free" promise on the [pricing page](/docs/pricing/) still holds.

## What changes

A company over either limit (50 people or USD 5M in revenue) will need a Team subscription for the people who use Ferryx for work. Under SUL-1.0, any company could use Ferryx internally for free; that is the one thing this change ends.

It does not happen today. Paid plans are not on sale yet, and the license includes a transition period: larger companies can keep using Ferryx for work free of charge until the Team plan opens **and for 90 days after that**. We will announce the date the Team plan opens well in advance.

## What does not change

- **Releases you already have stay under SUL-1.0.** A license change cannot take back rights you were already given, and we would not try.
- **Local features are never locked for individuals.** Workspaces, worktrees, panes, agents, and the daemon all keep working without an account.
- **The source stays public.** You can read, build, and modify it, as before.

## Why we are doing this

Ferryx is built by a very small team. Companies that run it across their engineering organizations get real value from it, and a Team subscription is how that value can pay for continued development. Drawing the line at company size keeps Ferryx free for the people it was built for while asking organizations that can afford it to contribute.

## Contributing

New pull requests now ask you to sign a short [Contributor License Agreement](https://github.com/Indosaram/ferryx/blob/main/CLA.md). You keep the copyright in your work; the CLA lets us keep Ferryx's license consistent in the future. See [CONTRIBUTING.md](https://github.com/Indosaram/ferryx/blob/main/CONTRIBUTING.md).

## FAQ

### Do I need to do anything now?

No. If you are an individual or work at a small company, nothing changes. If you are at a larger company, you have until 90 days after the Team plan opens.

### How much will Team cost?

The planned launch price is USD 12 per seat per month, with a minimum of 3 seats. See the [pricing page](/docs/pricing/) for current details.

### Is Ferryx open source or source-available?

No. Ferryx is source-available: the source is public, but the Ferryx License is not an OSI-approved open-source license, and neither was SUL-1.0.

### Can I keep using an older release under SUL-1.0?

Yes. Each release stays under the license it shipped with.
