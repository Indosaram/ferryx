# Task 3 binding resume status

Date: 2026-10-03
Worktree: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`
Decision: `E/task-3-binding-contract-decision.md` (read in full)

## Decision accepted; implementation state is not complete

The accepted ordering is source reservation before split, then spawn-ingress operation capture, checked live binding and per-producer bound ACKs before spawn response/attach, followed by the existing `attach-handshake.held` evidence. The attach-held event is downstream evidence, never the source of the backend ID. Source backend ID must come from authoritative fixture/selection evidence; absence is a typed failure. No frontend identity may be invented. No aliases or arm-file rewrite as a live bind mechanism are permitted.

## Current script mismatch

- `scripts/qa/pane-liveness.mjs` currently calls `bindBackendSession` from the `split-create` receipt and from `attach-handshake.held`.
- `scripts/lib/qa-scenarios/common-harness.mjs::bindBackendSession` rewrites `<name>.arm.json`; this is not a live producer bind.
- No source backend selector/reservation, operation-captured event, or pre-attach per-producer bind ACK flow is wired in these scripts.
- `fixture-setup` validates session IDs but does not currently establish which existing source session is the authorized split source.

These are known mismatches, not completed binding behavior. Binding edits are paused until the exact reservation JSON/core API is coordinated with canonical peer `st_01a100ac`.

## Fixture source mapping audit

The root decision identifies the actual selected source as `sourceSession.backendSessionId` at the existing split selection seam, passed as `inheritFromSessionId`. The `fixture-setup` emitter in `src-tauri/src/ipc/qa_barrier.rs::emit_fixture_setup_records` emits `sessionId` (first record), `sessions`, and `fixtureKind: "real-isolated-pty-fixtures"`; session records carry kind/backend ID/ownership evidence but do not identify the actual selected source. The runner's four-kind fixture validation only groups by `kind`. It is not safe to infer `created` or any other kind as selected. The runner integration needs actual selection evidence carrying the existing source backend ID; if unavailable, fail typed as blocked rather than guess or synthesize frontend identity.

## Producer API evidence and coordination status

Inspected `src-tauri/src/ipc/qa_barrier.rs`: `write_bound_ack(name, session_id, operation_id)` writes `<name>.bound-ack.json` only. It does not claim a reservation or mutate a process-local live arm. Direct contact attempts to canonical peers `st_01a100ac`, `st_01a100d3`, and `st_01a100d1` all failed: `task_send` reports no resident session and `task_output` reports no task in this session, including cross-session lookup. The root relay target `st_01a100ab` was also not addressable. No reservation/capture/bind API signature or peer agreement is confirmed. The root must relay the runner contract to those assigned owners; do not infer agreement or invent API field/event names beyond the root decision's fields and concepts.

## Runner schema requested from assigned owners

The runner integration requires confirmation of the decision's one-shot reservation `{ runId, operationId, sourceBackendSessionId, workspaceId, worktree, deadline }`; source selection evidence; subscription API/order for captured, bound, failure, and held events; spawn claim selector using existing `inherit_from_session_id` and `client_request_id`; operation-captured payload; checked bind API after real `spawn_result.session_id`; per-process ACK schema including exact barrier, backend ID, producer component, PID and role; and bounded cancellation/idempotency semantics. Required order: source selection -> reservation/subscription -> trigger -> capture -> actual spawn result -> checked bind + each producer ACK -> spawn response/attach -> attach-held. The current emitter/API does not yet implement or confirm this runner contract.

## Preserved constraints

- `targetRole` remains only a process role; backend IDs only use `targetBackendSessionId`.
- Existing validation gates and the five-tuple/native acceptance gates remain unchanged.
- Do not add aliases, guessed IDs, dynamic `.arm.json` bind rewrites, or attach-held target discovery.
- Cleanup must not kill adopted/replayed PTYs. No Rust edits, GUI actions, tests, builds, or remote executions were performed.

## Existing script hashes (unchanged this resume)

| Script | SHA-256 |
| --- | --- |
| `scripts/qa/pane-liveness.mjs` | `2d4c0080e656610c5ee0ffbd04db75084ced68d726d0af7f99ec9037096e486c` |
| `scripts/qa/pane-liveness.test.mjs` | `d160a1db443106895c9668b522e41c561a8752d6ebfbe57b418ce8663bb865f5` |
| `scripts/qa/pane-liveness-vitest.config.mjs` | `a8709872b371397000497aaea5b7bfea5ee19414d240c61ae4f850ec44a779a9` |
| `scripts/lib/qa-scenarios/common-harness.mjs` | `1ae202c36f538c5a44ad100424e3d52a286d716a1dac759f4f3f616a8569ca08` |
| `scripts/lib/qa-scenarios/native-driver.mjs` | `60f4e23514b1c653f3a1385d3b731c8624ee2756f9e3d759094f4ecaf587f161` |
| `scripts/lib/qa-scenarios/diagnostic-classifier.mjs` | `cdee82b2cba410f1cf8753c9802e03c06e61ffaf1a0acf2b8cfe0bea1340c6da` |

These hashes identify the current scripts, which still contain the mismatched binding flow above; they do not indicate the accepted binding repair is implemented or ready for execution.
