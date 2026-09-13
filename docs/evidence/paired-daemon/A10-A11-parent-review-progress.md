# Parent review checkpoint: retirement, relay, receiver cleanup

Status: focused retirement and receiver follow-through accepted after parent
source review and independent execution. Full A10/A11/plan acceptance is open.

## Journal follow-up

A12 corrected the compiler error. A09-refresh-parent-RED2.log now captures
the actual failed OutcomeUnknown assertion, exit 101, one test run. Parent
fixed refresh_locked to retain previously established OutcomeUnknown only
when the durable record is still Pending. A durable completed record still
replaces that uncertainty; locally active Pending records are not converted.

A09-refresh-parent-GREEN.log captures seven tests passing, exit 0: the new
restart regression plus all six actual HTTP journal contention, revocation
and deadline cases. The supervisor /tmp/fx-journal-parent.s7Ybdf was removed
with rmdir. The behavioral RED supervisor /tmp/fx-journal-parent.FtNxJE was
also removed with rmdir. Journal/test/session adapter diagnostics have no
errors; handover diagnostics contain only inactive-platform hints.

Parent combined runtime verification bash_90 exited 0. Output
A10-A11-parent-combined.log records 29 passing tests: five handover, one
machine session scenario, eight relay, three receiver (including helper),
three input (including helper), and nine worktree safety tests.

Separate-process close used predecessor 54310, replacement 54346 and shell
54343; close, durable operation retrieval and replay returned successfully
before/after clean predecessor retirement. Receiver PIDs 55120 and 55129
were reaped, readers finished and roots removed in ordinary and injected
failure scenarios. Relay shell 55210 retained PID through actual suffix and
1 MiB history-gap replay; sibling 55269 survived the first close. The relay
failure scenario reaped both held session handles and joined runtime threads.
The earlier missing historical PID receipt remains outside these new proofs.

Parent inspected actual outputs and found only empty supervisor directories,
then removed /tmp/a10-owner-aggregate.d0LLBj with rmdir. Exact PID checks for
54310, 54346, 54343, 55120 and 55129 returned no rows. The receiver adjudication
is now accepted as NOT REPRODUCED with corrected fixture cleanup, not a
production repair.

Idle transport passed in 10.00 seconds (A10-parent-idle-check.log, bash_93,
exit 0). Cancellation guard and actual expired-ticket WS each passed one
test; headless ferryx-cli/ferryx-relay cargo check and cargo build both
completed successfully (A10-A11-parent-final-check.log, bash_94, exit 0).
Both supervisors /tmp/fx-journal-parent.gf6k82 and
/tmp/fx-journal-parent.aWx1n0 were removed with rmdir. Existing compiler
warnings remain in the complete logs; no diagnostics were suppressed.

These close the final-close retirement follow-through and receiver cleanup
adjudication. The relay follow-through's composed compiler blocker is resolved.
No full A10, A11 or plan completion follows from this batch alone. Forced
OS firewall exclusion, full Git-through-relay behavior, native paired client,
platform and final compatibility acceptance remain independent obligations.

## Initial review

The parent opened the retirement report and nine-file hash list, relay report
and runner, receiver report and runner, and the actual receiver fixture,
machine gateway and timer fixture, separate-process retirement fixture,
handover diff, session close/read adapter, journal implementation, journal
contention tests and full relay integration fixture.

The receiver fixture now catches only the scenario, performs bounded production
close outside that catch, and accepts its typed injected panic only after
reaping, reader completion, registry removal and root cleanup assertions.
No production repair is inferred from this NOT REPRODUCED investigation.
Its child runner still inherits unoverridden Ferryx environment variables;
parent acceptance must use a clean environment rather than repeat that runner.

The relay fixture adds actual shell-generated suffix and retained-history
overflow evidence. Its two-live-PTY injected failure reaches the normal
production cleanup path. Existing synthetic transport tests remain present
and are not evidence for actual Git mutations or OS firewall exclusion.
The reported missing Worktree argument was already being composed by the
active A12 owner when the parent read current source. No conflicting edit
was applied by the parent.

The journal review exposed a restart regression candidate: open converts
persisted Pending to OutcomeUnknown in memory, but refresh_locked replaces
that state with the unchanged durable Pending record before reconciliation.
The parent added
reopened_pending_operation_remains_outcome_unknown_after_refresh to the
existing journal contention test module. The first run did not reach its
assertion: current A12 machine_events.rs:70 fails E0277 because AcquireError
does not implement Into<String> for IpcError::internal.

Exact command:

`cargo test --locked --manifest-path src-tauri/Cargo.toml
--no-default-features --lib
reopened_pending_operation_remains_outcome_unknown_after_refresh
-- --nocapture`

Output is A09-refresh-parent-RED.log. Despite its filename this is a compiler
failure, NOT behavioral RED. Exit 101 is captured by parent monitor bash_82.
The command used env -i, private HOME/Ferryx/agent socket/XDG/TMP paths,
explicit Cargo/Rustup homes, existing private target, jobs 2, debug 0,
incremental 0 and empty wrapper. Empty supervisor
/tmp/fx-journal-parent.M0XPVF was removed with rmdir.

A12 owner st_01a098ac was notified of the exact compile error and parent
journal test ownership. Parent has not edited production journal code.
Receiver fixture and machine gateway LSP requests returned no diagnostics.
Independent runtime reruns of the three completed child scopes remain owed;
no task was marked completed from those reports.

Existing /tmp/a10-owner-parent.YmuzgZ and its historical missing cleanup
receipt remain unresolved and untouched. No canonical daemon, desktop,
commit, deployment or release action occurred. All source/evidence remains
uncommitted.
