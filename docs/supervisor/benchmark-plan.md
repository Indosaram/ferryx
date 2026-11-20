# Supervisor benchmark proposal: run admission, recovery, and owner triage

Status: proposed; no performance numbers or new runtime guarantees measured in this PR.

## What differs

| Surface | What it owns | What Ferryx should benchmark |
| --- | --- | --- |
| [Public Orca CI/release bots][orca-bots] | Turn CI events and schedules into focused triage work. The CI template asks for a code/flaky/infrastructure distinction and explicit approval before proposing to act on a fix; the template is not a runtime authorization boundary. The [trigger lane][orca-trigger] deduplicates source events and serializes admission. | Event relevance and suppression, first useful failure report, and whether the reported evidence is tied to the exact PR head. Reuse the [blocked-persistence test pattern][orca-durability], not Orca's ownership model. |
| [PirateTalk presentation / transport boundary][surface] | A chat surface can relay commands and observations; it does not own the lifecycle of a Ferryx goal. A replayed event is evidence to reconcile, not a reason to repeat an operation. | Disconnect a presentation adapter and let observations fall behind. Measure whether the view converges from durable state without creating another effect. Test replay correctness, not throughput across unlike products. |
| Ferryx Supervisor | Owns a durable goal, worker dispatch, observation and recovery across local and paired targets. [Admission commits before acknowledgement][admission]; [restart distinguishes ready, uncertain and already observed work][dispatcher]. [Receipts distinguish accepted, started, settled and uncertain][receipts]. | Preserve existing guarantees while measuring their paired-host and load boundaries. Do not flatten state into a bot-level “success,” introduce a second dispatcher, or re-run an ambiguous effect. |

The Orca comparison uses its open-source CI/release templates, not an unverified custom bot deployment. The transport column uses only Ferryx's public contract.

## Priority and pass criteria

| Priority | Exercise on an isolated temporary Hub DB and fake worker/transport | Proposed gate or metric |
| --- | --- | --- |
| P0 | Stall admission persistence and inject duplicate events for the same source identity. Separately, race two wakes for the same goal and phase. Kill the test Hub at three cuts: after intent commit before claim, after claim before first byte, and after first byte before receipt. Replay a valid late receipt twice. | No worker traffic or acceptance before commit. One logical goal for identical source identity. Before claim: exactly one dispatch after recovery in a healthy single-target fixture. After claim: no automatic second send on uncertain state. For the still-current attempt, the first matching late Start receipt reconciles the uncertain run; its duplicate changes no state. Neither starts another worker effect. The same-goal/phase wake race records one decision/action and no duplicate worker effect, even if more than one handler runs. Count admitted goals, claims, attempts, first bytes and unique terminal transitions separately. |
| P0 | Exercise local owner pause/cancel/correction before first byte. For paired targets, separately partition the receiver before the holder fence is delivered and delay an old request until after the receiver persists a newer fence (also reopen the receiver after persisting it). Drop and later restore the observer. | Local: zero first bytes for the revoked action once the [local check][delivery] sees revocation. Paired: the new holder waits while its fence cannot be delivered; explicitly record any already-admitted old-holder work. After the newer fence is persisted, [receiver-side rejection][paired-fence] of a delayed old request must produce zero new worker bytes. Neither path blindly retries unknown effects. Record time to observable degraded/uncertain and to convergence. |
| P1 | Queue 100 and 1,000 synthetic wakes; return transient failures and restart the test Hub during backoff. Repeat with a live observer that temporarily stops consuming and repeat delivery of recent observation IDs. | Keep existing durable cursor correctness. Measure maximum attempts in a 10s window, peak queued/in-flight state, p50/p95 wake-to-decision, and time to drain after recovery. Check whether restarting resets retry pressure; the current [judge retry deadline state][wakes] is process-local. A snapshot or terminal report restored by replay must not trigger another worker effect. |
| P1 | Replay duplicate CI failures, superseded commit A after newer head B, a flaky test, and an infrastructure failure. Leave proposed fixes unapproved. | At most one actionable report per stable event key; no stale evidence reported as current and no write without approval. Record time from first relevant failure to a report with current SHA, first failing check, classification/evidence, and next owner action. Measure report duplication separately from duplicate input. |

## Run protocol

1. Use [existing recovery tests][recovery-tests] and the [durable wake cursor test][wake-tests] as baseline assertions. Add deterministic barrier/reopen cases before exercising longer runs. Do not kill a running user daemon or contact a live provider.
2. Record candidate/base SHA, OS, local vs. paired mock transport, concurrency, seed and fixture. Run the same seeds for both builds; for measurement use 5 warmups and 30 measured repetitions per scenario and load. Keep invariant violations and timeouts visible; do not average them away. Report p50/p95 for successful samples and timeout/violation counts over all samples.
3. Use a single monotonic harness observer for end-to-end timings across process or host restarts. For internal phases, subtract only readings from the same process-local clock. Capture event arrival, durable admission, claim, first byte, receipt and owner-visible update; mark unobservable phases and timeouts explicitly, rather than fabricating a timestamp. Log synthetic IDs and counters only. Start with a baseline rather than inventing latency SLOs; the P0 safety assertions above are hard gates from the first run.
4. Land paired failover and persistence fault coverage first. Add bounded-retry and owner-report metrics next. Consider durable retry scheduling only if restart measurements show burst amplification. Keep UI or bot-template changes out of this benchmark proposal.

[orca-bots]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/shared/bots/bot-template-catalog/releases.ts#L5-L87
[orca-trigger]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/main/automations/automation-trigger-runs.ts#L102-L162
[orca-durability]: https://github.com/stablyai/orca/blob/74452b55440204f4f375b31bbaf24406ee7cab3f/src/main/automations/automation-worker-durability.test.ts#L40-L65
[surface]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/docs/supervisor/surface-separation-2026-11-11.md#L64-L107
[admission]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/admission.rs#L13-L25
[dispatcher]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/dispatcher.rs#L270-L364
[receipts]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/receipts.rs#L1-L31
[delivery]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/delivery.rs#L58-L74
[paired-fence]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/src-tauri/src/agent_control/fence.rs#L189-L234
[wakes]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/wakes.rs#L941-L966
[recovery-tests]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/tests/recovery.rs#L160-L270
[wake-tests]: https://github.com/Indosaram/ferryx/blob/a10d3d5baa620e30d93c475bd9bb39ec3aea3d4c/crates/ferryx-hub/src/supervisor/tests/wakes.rs#L807-L843
