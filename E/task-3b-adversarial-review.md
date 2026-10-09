# Task 3 Unit 3-B Adversarial Re-Review (Revision 3 Bounded Delta)

- **Date:** 2026-10-03
- **Target Worktree:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`
- **Scoped sources (hashes verified):**
  - `src-tauri/src/ipc/native_terminal.rs`: `2f9dbf0889a46d77f1473d39bc47f13c9d5645e8b072bc6058ece280a07e0172`
  - `src-tauri/src/native_terminal/surface_host.rs`: `e06f83cb6e608440cebf2acb9f48657a3aae7b1f4d7fa1c886502219727caa5a`
- **Review boundary:** Bounded source-only delta review. No build/test/GUI/remote command was run; no product source was edited.
- **Verdict:** **NEEDS-FIX: runner target-binding contract mismatch.** Core worker sealing/binding and five-tuple remain dependencies, not passes.

## Delta scorecard

| Check | Source-only assessment |
|---|---|
| Nested `block_on` | No `block_on` occurrence found in the two scoped Rust files. This is a source inspection result, not compile evidence. |
| Attach command test | Test spawns `cmd_native_terminal_attach` with test-managed app state and subscribes to held event before advancing paused time. It covers an explicitly targeted arm, not the runner's unbound prearm. No test execution was performed. |
| Async receipt writes | Relevant paths delegate persistence to async channel helpers / `spawn_blocking`; this review does not claim runtime completion or error behavior beyond the source. |
| Target binding | **Needs fix.** Runner prearms `attach-handshake` without `targetBackendSessionId`, expects the held event to discover authoritative session ID, then binds downstream presentation. Backend returns on missing target after scheduling a binding-failed receipt, so the expected discovery hold cannot be reached under this contract. |
| Missing-target presentation | Source schedules a binding-failed receipt when target is absent. This consumes the operation's receipt path rather than waiting for runner's dynamic bind. Confirm that runner contract before treating as final; current `pane-liveness.mjs` binds presentation only after the attach hold yields an ID. |
| `ac` channel core | **Active source dependency.** Worker sealing and claim/channel behavior in `qa_barrier.rs` are outside this bounded Unit 3-B delta; no final pass asserted. |
| Five-tuple | **Open dependency.** `frontendSessionId`, `paneIdentity`, and `bindingKey` remain unwired; not fabricated and not waived. |

## Remaining Unit 3-B finding: runner prearm and target requirement conflict

The runner's flow in `scripts/qa/pane-liveness.mjs` prearms `attach-handshake` before a backend session exists, awaits the held event, derives the authoritative backend session ID from that event, and binds the downstream `presentation` barrier afterward. The current `hold_attach_handshake_barrier_qa` path requires a nonempty `target_backend_session_id`; when absent, it schedules an `attach_handshake_binding_failed` receipt and returns `None` instead of holding the attaching session. Those two source contracts conflict: the runner needs the held event as the ID-discovery source, while the native producer currently requires that ID before it will hold.

The same missing-target failure path exists for presentation. Since presentation is dynamically bound after attach discovery, an unbound prearm must not produce a terminal binding-failed receipt before that bind can occur. This assessment is based on reading the runner and producer source; it is not a runtime test result.

### Required correction

- For `attach-handshake`, permit the unbound prearmed barrier to hold the actual attaching session and report that session as the authoritative ID; the target cannot be required before discovery.
- For `presentation`, do not emit a terminal binding-failed receipt for the expected unbound state. Ignore/defer interception until the runner binds the actual target, or implement an explicit typed not-yet-bound state that the runner does not mistake for a terminal receipt.
- Add/retain coverage for the runner's unbound arm contract, not only the test case that supplies `targetBackendSessionId` directly.

## Test coverage boundary

`attach_handshake_timeout_leaves_prior_live_session_tasks_intact` now drives the command through `cmd_native_terminal_attach`, manages app state in the test, subscribes to the held event before virtual-time advancement, and checks a live abort handle. However, the reviewed test configures the barrier with a target session ID. It therefore does not exercise the runner's unbound prearm / session-discovery path that currently conflicts with the producer check. The test source is not evidence that the test passed; it was not run per request.

## Dependencies and verdict

- **Unit 3-B:** NEEDS-FIX until the target-binding producer behavior agrees with the runner's active prearm/bind sequence and the unbound path has test coverage.
- **Author `ac` core:** pending worker sealing/channel primitives remain an active source dependency; no final integration pass claimed.
- **Five-tuple:** remains OPEN until `frontendSessionId`, `paneIdentity`, and `bindingKey` are actually wired. No final acceptance claim.
- **Verification:** source-only; no build, test, GUI, or remote execution. This report is written uncommitted.
