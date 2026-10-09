# Task 3 binding contract decision

Status: chosen implementation contract, not implemented or QA-verified by this decision.
Absolute artifact path: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/E/task-3-binding-contract-decision.md`.
This supersedes the earlier `.omo/evidence/local-pane-liveness-root-remediation/task-3-binding-contract-decision.md` conclusion that Task6 is required. No plan dependency changes are authorized.

## Chosen contract: correlate at local spawn ingress, bind before returning spawn

Do not learn the backend identity from an attach held event. Capture the existing local spawn request first and bind its real spawn result before returning it to the frontend. This is a private Task3 QA hook around baseline code, not a Task6 spawn protocol.

### Exact baseline seams inspected

- `ui/src/state/workspaceStore.ts::splitPane` (986 onwards) derives `sourceSession` from `tabId` and `targetLeafId`, creates frontend `localSessionId` and `newLeafId`, and calls `spawnDetailedForLogicalAction` with `inheritFromSessionId: sourceSession.backendSessionId`. It does not rebind the pane until the spawn promise resolves.
- `spawnDetailedForLogicalAction` (3275 onwards) generates `clientRequestId` once, puts it in `stableRequest`, and reuses that request on ambiguous transport retry.
- `src-tauri/src/ipc/terminal.rs::cmd_terminal_spawn` accepts `TerminalSpawnRequest.client_request_id` and `inherit_from_session_id`. The local branch resolves inherited CWD, then calls `daemon_client.spawn_terminal_with_startup(...)` (2358 onwards). The exact bind insertion point is immediately after the common `spawn_result` expression, before `let session_id = spawn_result.session_id.clone()` (2392 onwards): the command subsequently calls `host.mark_pending_startup`, `daemon_client.attach`, `start_managed_pump` and emits Started before returning its response. Binding only at the final response return would be too late for these producer side effects.
- `src-tauri/src/ipc/native_terminal.rs::hold_attach_handshake_barrier_qa` requires exact nonempty `target_backend_session_id`. Its call is before `begin_replay_request` (1049-1081). Leave that rule intact.
- `src-tauri/src/ipc/qa_barrier.rs` stores arms in a mutex-backed process-local map. `write_bound_ack` exists, but writing an ACK alone does not mutate that map or prove a live bind consumer. Startup arm-file rewrites are not a bind operation.

### Private operation selector and ambiguity rule

Runner subscribes before triggering the real split and installs a one-shot private reservation with `{runId, operationId, sourceBackendSessionId, workspaceId, worktree, deadline}`. The source ID must come from authoritative fixture/selection evidence, never a guessed new ID. It is a **source selector**, not `targetBackendSessionId` and never an attach target.

At `cmd_terminal_spawn` ingress, before any await, claim only a local spawn whose existing `request.inherit_from_session_id` equals that source and whose workspace/worktree match. Capture its nonempty existing `request.client_request_id` into the operation record. Initial pane spawn, tab creation without that inherit argument, remote spawn, and other source panes cannot claim it. The private fixture must establish a single authorized split action with no competing same-source spawn; a second distinct matching clientRequestId fails the operation as ambiguous, rather than choosing the first and continuing. Duplicate delivery of the captured clientRequestId is the same operation, not a new claim.

This selector does not universally distinguish a split from every possible restore that also inherits the same source. If isolated single-action provenance cannot be established, fail the gate explicitly; do not call it correlated. No new frontend identity or menu payload is assumed.

### Lifecycle order

1. Runner prearms the private operation reservation and dormant session barriers. Null target means **unbound/ineligible**, never hold-any-session. Subscribe to registration, operation-captured, binding, failure and held evidence before triggering.
2. Complete initial fixture readiness; identify/select the exact source through the authorized real surface. Trigger exactly one native split. `menu_split_*` has no identifying payload; correlation comes from the spawn request's existing inherit argument, not that menu event.
3. GUI-side spawn producer claims the reservation at ingress, associating QA operationId with the stable clientRequestId. Write operation-captured evidence with request/source provenance. Do not stop an arbitrary attach.
4. Run the existing spawn. On success, the same invocation associates captured clientRequestId with authoritative `spawn_result.session_id` and epoch. Place the private bind step **immediately after the common spawn-result expression and before mark_pending_startup, the spawn command's daemon attach/managed pump/Started event, and response return**, hence also before `REBIND_SESSION_BACKEND` can make this new pane natively attachable. This separates the spawn command's baseline stream attachment from the later native attach-handshake hook; do not hold the former while waiting for evidence produced only by the latter.
5. ac implements a checked process-local bind operation that validates run, operation, captured request, exact nonempty backend ID, barrier name and producer role; permits one immutable binding or identical duplicate, rejects conflicting rebinding; updates the live arm under its mutex and only then emits bound ACK. This operation is a required new private QA addition, not claimed present. The spawn hook must await checked bind completion for attach and presentation in the GUI process. Merely calling existing `write_bound_ack` is insufficient.
6. Any separately running producer that needs the same target must perform its own validated live bind and ACK its own PID/role before this spawn reply is released. A GUI singleton cannot bind daemon memory. ac/ab must implement explicit bounded private delivery to each required producer, or report that producer as blocked; never infer a daemon ACK from a GUI ACK. This split attach/presentation pair is GUI-owned and does not require changing daemon spawn wire DTOs.
7. Runner validates operation-bound evidence, exact target and producer ACKs. Only then may the spawn reply return. Runner must not wait for attach-held to authorize the bind: that recreates the cycle. The next native attach now matches the nonempty target, claims its barrier and emits held evidence before replay fencing. Other pane attaches remain unmatched.
8. For happy-path release, runner validates held provenance and releases; for attach-stall, it deliberately withholds release until the bounded producer deadline. Producer performs terminal settlement; no runner-authored success/failure receipt substitutes for it.

### Producer-owned shutdown and failure cleanup

The operation reservation/bind worker belongs to the GUI QA channel/command scope. It has a bounded deadline, cancellation and a tracked join handle, not a detached poller. Producer shutdown first closes admission, cancels outstanding reservations/binds/holds, awaits their terminal settlements and joins all owned workers, then uses checked receipt draining (`drain_and_verify_workers`) before channel deactivation. Receipt drain alone does not join an untracked bind or attach worker.

Spawn failure, bind failure/deadline, competing claim and shutdown each produce an explicit failure. If binding fails after a PTY was created but before the frontend received it, the spawn producer closes only that newly created task-owned backend through the existing lifecycle, awaits cleanup, and does not publish success. Same-clientRequestId retries use the recorded result/failure and must not create a replacement PTY. No guessed identity, production-daemon kill, unjoined worker, swallowed write error or fixture cleanup by runner assertion is allowed.

## Owner dispatch

- ac: one-shot request reservation, checked live binding, per-process ACK validation, tracked cancellation/join and checked drain.
- d1: keep attach strict-target; ensure no unmatched/unbound attach is held or attributed to this operation; keep gate before replay fencing and producer settlement on all exits.
- ab: source-selector prearm, subscribe-before-trigger, operation-captured then operation-bound/producer ACK order, then attach-held; remove startup `.arm.json` rewrite as live bind mechanism.
- `ipc/terminal.rs` owner/root: private ingress capture and post-spawn/pre-return hook. This is the additional Task3 integration seam; root must allocate ownership rather than silently editing another lane.

Peer notification is delegated to root as requested. This document is not an implementation-completion or peer-agreement claim.

## Acceptance unchanged

Task3 retains its approved dependencies; no Task3-to-Task6 cycle is added. The five-tuple/native presentation gates are unchanged: backend/request correlation cannot invent frontend pane identity, binding key, generation or incarnation. Missing fields remain explicit dependencies/failures.

The literal native screenshot, matching stage receipt and success/failure cleanup requirements remain open until actually executed. Headless exception evidence never closes that native gate. SIGSTOP plus classifier is Unix-only evidence, not cross-platform proof; Windows requires a verified portable fixture mechanism or explicit unsupported gate, never a fake stopped record.

Only this decision artifact was authored here. No product edits, diagnostics, tests, build or GUI QA were performed; the contract is grounded in the inspected baseline seams and awaits implementation and root acceptance.
