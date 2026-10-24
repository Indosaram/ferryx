# Ferryx license transition

Status: **in effect** from the first release after 2026-10-24. `LICENSE` is now the Ferryx License 1.0.
Payment launch is still deferred.

## What changed

| Item | Sustainable Use License (before) | Ferryx License (now) |
|---|---|---|
| Large company work use | Free ("internal business purposes") | Needs Team or higher, after the transition period |
| Free use | Personal, non-commercial, any internal business use | Personal, education, open-source work, non-profits, companies under 50 people **and** USD 5M revenue |
| Self-hosted relay | Free | Free for everyone, stated explicitly |
| `ee/` code | n/a | Separate commercial license, unlocked by a license key |
| Hosting as a service, bypassing key checks | Not addressed | Forbidden |
| Earlier releases | n/a | Stay under SUL-1.0 for good |

**Transition period:** large companies may keep using Ferryx for work free of charge until the Team
plan is on sale and for 90 days after that. While payment is deferred, nobody has to pay.

## Known risks the maintainer accepted

- **No legal review.** The text was written without a lawyer. Have one review it before the first
  paid Team subscription.
- **Outside contributors were not asked.** The relicense went ahead without consent from:
  - Sungmin Kim <sungmin.t@maths.study>, commit `05fad139` (2026-09-03, font measurement fix).
    Getting consent later, or rewriting that change from scratch, removes the risk.
  - `동치미 <npopo@dongchimiui-Macmini.local>`, commits `d8c01db9`, `d9c25581`, `df0d32eb`
    (2026-09-14), unless this is the maintainer's own machine (unconfirmed).

## Still to do

1. Turn the CLA check on: create the private repository `Indosaram/ferryx-cla-signatures` and add a
   fine-grained PAT with contents read/write on that repository only as the `CLA_SIGNATURES_TOKEN`
   secret. Signatures live outside this repository because `scripts/release-workflow-policy.mjs`
   forbids `contents: write` here.
2. Publish an announcement: why, who stays free, the transition period, and that self-hosted relays
   stay free. Existing users and early supporters get 50% off their first year.
3. When payment opens: ship Team member invite/removal and seat management, then start the 90-day
   clock.
