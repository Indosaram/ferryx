# External owner-report adapters

These patches migrate external producers to the Ferryx hub report gate. They
are deployment artifacts, not installed adapters. A Ferryx binary upgrade alone
does not apply them to separately managed producer source directories.

Apply only after the installed hub exposes `report`, `report_status`, and
`report_mutate`, plus `report_adopt` for legacy Autopilot cards.
An older hub rejects these requests; there is deliberately no direct Slack
fallback. Preserve concurrent producer changes by checking each patch first:

```sh
git -C "$AUTOPILOT_SOURCE" apply --check "$FERRYX_SOURCE/scripts/hub-report-adapters/autopilot.patch"
git -C "$CONTENT_INTEL_SOURCE" apply --check "$FERRYX_SOURCE/scripts/hub-report-adapters/content-intel.patch"
```

After deployment approval, apply each checked patch using the same command
without `--check`. If a check fails, rebase the patch onto the current producer
source; do not restore or overwrite the source. Autopilot need not be a Git
repository for `git apply` to check and apply its patch.

Both adapters persist intent before submission, retain pending delivery IDs,
and record a Slack reference only after a delivered receipt. Repeated state
does not create a second report. Pending reports resume on subsequent producer
runs. Content attachments require the parent report receipt. After a lost reply
or restart, the producer resubmits the same mutation identity and uses the hub's
stored receipt; only a hub-confirmed uncertain outcome is recorded as uncertain.
A `pending` mutation reply with `progress` (`in_flight` or `recovering`) and
`blocked_by` equal to the request's own identity means the hub is still settling
that identity: the producer keeps its claim and asks again with the same identity.
The hub never repeats an attachment whose completion is unknown. An unknown card
update is resolved by reading its mutation-specific block IDs from the original
Slack message, including thread replies. No later edit is sent until that original
update is observed; elapsed time alone never authorizes a replay.

Card edits are desired-state generations. An earlier card edit that is still
`pending`, `in_flight` or `recovering` does not stop the producer from submitting
the newer desired card. The earlier intent keeps its own identity and truthful
status and is re-queried on later runs. The hub settles it as `delivered`
(observed on Slack) or `superseded`. `superseded` means a newer generation replaced
an unconfirmed intent. It does not prove the older state was never displayed and
does not report it as delivered.
Admission (`pending`) is never recorded as delivered. Only a card edit can be
superseded; attachments keep the no-repeat uncertainty rule above, and a
`superseded` attachment reply is rejected as a protocol error, never counted as
uploaded. These rules give two different guarantees. Display convergence is
eventual, provided Slack becomes available and delayed older writes eventually
finish. Periodic reconciliation also revisits confirmed cards. At dispatch,
buttons are checked against the current desired mutation identity; a stale
identity is rejected. An action already dispatched before a newer target is
accepted is not revoked. Delivery is recorded only after a successful Slack
update response or exact read-back, never merely from accepting an intent.

Autopilot processing acknowledgments edit the existing card through the hub instead
of posting another message, and saved approval buttons survive process restarts.
Card edits and content attachments use the parent's hub receipt and recheck its
current reporting policy. Neither adapter owns Slack credentials for these calls.
Legacy Autopilot card edits are retained until the hub verifies the original Slack
message and its bot ownership through `report_adopt`. Successful adoption resumes
the stored edits in order; unavailable adoption leaves them held. Mutation identities
survive retries and restarts, and uncertain outcomes block later edits rather than
being blindly replayed. The patch
preserves the source collector login recovery and stale-card cleanup.

Run the isolated producer-entry regression on a permitted build host:

```sh
bun test scripts/hub-report-adapters/autopilot-redraft.test.ts
```

The patches also ship producer-side regressions for the mutation contract (run
inside each patched source tree):

```sh
(cd "$AUTOPILOT_SOURCE" && bun test src/report-card.test.ts)
(cd "$CONTENT_INTEL_SOURCE" && bun test tests/unit/report-outbox.test.ts tests/unit/publish-ready-report-gate.test.ts)
```

It executes source extracted from the patch, with isolated state and recorded
platform/model seams. It covers restricted and eligible drafts and redrafts,
plus transient outage suppression and persistent-error deduplication. No live
Slack request, browser action, or production database is used.
Permanent bid restrictions (`PREFERRED_ONLY`, `PROJECT_CLOSED`) close the old
card and mark the item skipped. Repeated clicks neither bid nor report again;
recoverable form failures retain the retry path.

Verification receipts live under the report-gate-20261030 evidence directory.
The adapter tests passed on Windows, including real constructor equality and
offline/restart behavior. Content candidate and baseline share three existing
Windows failures; see the C2 report for exact limits. Installation, actual Slack
delivery, and daemon handover remain separately approval-gated.
