# Supervisor benchmark proposal: run admission, recovery, and owner triage

Status: proposed; no performance numbers or new runtime guarantees measured in this PR.

## What differs

| Surface | What it owns | What Ferryx should benchmark |
| --- | --- | --- |
| [Public Orca CI/release bots][orca-bots] | Turn CI events and schedules into focused triage work. CI triage distinguishes a code failure, a flaky test, and infrastructure; fixing is a separate, approval-gated step. The [trigger lane][orca-trigger] deduplicates source events and serializes admission. | Event relevance and suppression, first useful failure report, and whether the reported evidence is tied to the exact PR head. Reuse the [blocked-persistence test pattern][orca-durability], not Orca's ownership model. |
| [PirateTalk presentation / transport boundary][surface] | A chat surface can relay commands and observations; it does not own the lifecycle of a Ferryx goal. A replayed event is evidence to reconcile, not a reason to repeat an operation. | Disconnect a presentation adapter and let observations fall behind. Measure whether the view converges from durable state without creating another effect. Test replay correctness, not throughput across unlike products. |
| Ferryx Supervisor | Owns a durable goal, worker dispatch, observation and recovery across local and paired targets. [Admission commits before acknowledgement][admission]; [restart distinguishes ready, uncertain and already observed work][dispatcher]. [Receipts distinguish accepted, started, settled and uncertain][receipts]. | Preserve existing guarantees while measuring their paired-host and load boundaries. Do not flatten state into a bot-level “success,” introduce a second dispatcher, or re-run an ambiguous effect. |

The Orca comparison uses its open-source CI/release templates, not an unverified custom bot deployment. The transport column uses only Ferryx's public contract.

## Priority and pass criteria

| Priority | Exercise on an isolated temporary Hub DB and fake worker/transport | Proposed gate or metric |
| --- | --- | --- |
| P0 | Stall admission persistence; inject duplicate source events and an overlapping deadline. Kill the test Hub at three cuts: before claim, after claim before first byte, and after first byte before receipt. Then replay a late receipt, including a duplicate. | No worker traffic or acceptance before commit. One logical goal for identical source identity. Before claim: at most one dispatch after recovery, and exactly one when the target is healthy. After claim: no automatic second send on uncertain state. A valid late receipt reconciles once. Count admitted goals, claims, attempts, first bytes and unique terminal transitions separately. |
| P0 | Exercise the same fixture on local and paired paths; revoke the local target or change the paired holder generation during the ready/claim/delivery window. Make the old observer unavailable, then restore it. | No new effect from the stale holder and no blind retry on ambiguous delivery. Record time to explicit degraded/uncertain status and to observer convergence. Do not assume the [local first-byte guard][delivery] is the paired-host fence; verify the receiver-side acceptance and bytes separately. |
| P1 | Queue 100 and 1,000 synthetic wakes; return transient failures and restart the test Hub during backoff. Repeat with a live observer that temporarily stops consuming and repeat delivery of recent observation IDs. | Keep existing durable cursor correctness. Measure maximum attempts in a 10s window, peak queued/in-flight state, p50/p95 wake-to-decision, and time to drain after recovery. Check whether restarting resets retry pressure; the current [judge retry deadline state][wakes] is process-local. A snapshot or terminal report restored by replay must not trigger another worker effect. |
| P1 | Replay duplicate CI failures, superseded commit A after newer head B, a flaky test, and an infrastructure failure. Leave proposed fixes unapproved. | At most one actionable report per stable event key; no stale evidence reported as current and no write without approval. Record time from first relevant failure to a report with current SHA, first failing check, classification/evidence, and next owner action. Measure report duplication separately from duplicate input. |

## Run protocol

1. Use [existing recovery tests][recovery-tests] and the [durable wake cursor test][wake-tests] as baseline assertions. Add deterministic barrier/reopen cases before exercising longer runs. Do not kill a running user daemon or contact a live provider.
2. Record candidate/base SHA, OS, local vs. paired mock transport, concurrency, seed and fixture. Run the same seeds for both builds; for measurement use 5 warmups and 30 measured repetitions per scenario and load. Keep invariant violations and timeouts visible; do not average them away. Report p50/p95 for successful samples and timeout/violation counts over all samples.
3. Record these timestamps with a monotonic clock: event arrival, durable admission, claim, first byte, receipt, owner-visible update. Log synthetic IDs and counters only. Start with a baseline rather than inventing latency SLOs; the P0 safety assertions above are hard gates from the first run.
4. Land paired failover and persistence fault coverage first. Add bounded-retry and owner-report metrics next. Consider durable retry scheduling only if restart measurements show burst amplification. Keep UI or bot-template changes out of this benchmark proposal.

[orca-bots]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/shared/bots/bot-template-catalog/releases.ts#L5-L87
[orca-trigger]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/main/automations/automation-trigger-runs.ts#L102-L162
[orca-durability]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/main/automations/automation-worker-durability.test.ts#L40-L65
[surface]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/docs/supervisor/surface-separation-2026-11-11.md#L64-L107
[admission]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/admission.rs#L13-L25
[dispatcher]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/dispatcher.rs#L270-L364
[receipts]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/receipts.rs#L1-L31
[delivery]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/delivery.rs#L58-L74
[wakes]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/wakes.rs#L941-L966
[recovery-tests]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/tests/recovery.rs#L160-L270
[wake-tests]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/tests/wakes.rs#L807-L843
