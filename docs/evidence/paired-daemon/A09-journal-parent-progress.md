# A09 journal contention parent repair

Status: PARTIAL. Actual list-read RED/GREEN, remaining contention cases open.

Parent owns remote/session_api.rs, test-only journal probes and
remote/journal_contention_tests.rs. Routed owner worker st_01a09869 was told
not to edit these files; changed adapter signatures require coordination.
Other source changes in this worktree belong to active producers.

## Observed regression and correction

The new fixture holds the actual journal state mutex at persist, before the
private atomic write, using a subscribed signal and bounded release channel.
A real authenticated session GET reaches journal.sessions while that mutex
is held. The gateway uses its own single-thread Tokio runtime. A separate
client runtime requests /health with a 500ms response deadline, so a blocked
gateway cannot prevent the watchdog from running.

Command:
`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib journal_writer_does_not_stall_session_http_executor -- --nocapture`

- A09-journal-contention-RED.log: compile succeeded, one test failed, exit101.
  Exact failure: journal mutex blocked unrelated HTTP executor, reqwest TimedOut.
  Writer joined, gateway joined, listener refused, root removed before failure.
- Moved session list/detail computation into existing run_blocking. Admission
  remains captured by the worker until it actually finishes. Read cancellation
  is checked before/after work, with outer revocation/deadline selection.
  No journal durability or locking policy was weakened.
- A09-journal-contention-GREEN.log: same selector, one passed, exit0;
  independent HTTP responded while the journal remained held; full cleanup.
- LSP errors on journal, new fixture and session_api: none in returned
  diagnostic results. A later patch hook reported server-cancelled LSP, not
  a Rust compiler error.

This demonstrates independent scheduler progress, not a 10-second deadline or
revocation test. These remain required along with held admission-slot proof.

## Pending mutation reproduction

Extended the fixture to POST a valid machine create shape for a nonexistent
project. It should reach real journal reconciliation before resolving that
project. Its test-only signal uses the same real mutex as list reads.

Command:
`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib journal_writer_does_not_stall_mutation_reconciliation -- --nocapture`

A09-journal-mutation-RED.log: exit101 from E0004, not runtime RED. Concurrent
routed-owner implementation added MachineSessionDetail before wiring exhaustive
matches in daemon/server.rs and daemon/client.rs. The parent notified that owner
and awaits its compile-boundary signal; it did not revert or weaken those matches.
Mutation reconcile production calls remain unchanged until actual RED is captured.

After owner enum wiring, A09-journal-mutation-parent-attempt2.log compiled but
did not reach the reconciliation probe. The fixture omitted mandatory explicit
null fields (worktree, inheritFromSessionId, cwdRelative); HTTP 400 occurred
before the intended seam. This is a fixture failure, not contention RED.
Its response assertion also ran before gateway cleanup. Parent added the
required fields and deferred response assertions until after cleanup.
The corrected test failed for the intended reason in
A09-journal-mutation-runtime-RED.log: the held reconciliation mutex prevented
an unrelated /health response (reqwest TimedOut). The writer and gateway
joined, listener refused, and fixture root was removed before failure.

Parent moved both mutation reconciliation calls into a shared run_blocking
helper, preserving domain errors and checking cancellation after each wait
and before final response publication. The spawned mutation task retains
the admission guard while its blocking worker drains.

A09-journal-combined-GREEN.log: selector journal_writer_does_not_stall,
two passed, exit0. Both list and mutation requests allowed independent HTTP
progress under the held journal lock; both fixtures fully cleaned up.

Two additional tests now revoke the authenticated device only after the
request reaches the held mutex. They require HTTP401 before writer release,
the admission permit still held, and reacquisition of all permits after
worker drainage. Their subscribed, bounded execution is recorded separately
as A09-journal-revocation-GREEN.log: four tests passed, exit0. Both revoked
requests returned 401 while the writer still held the mutex; both retained
admission and released it only after worker drainage.

The expanded six-test suite adds real production read (10s) and mutation
(40s) deadline expiry, without injected shortened deadlines or fixed sleeps.
It awaits HTTP504 before releasing the writer and requires the same retained
admission and final drainage. Its independent 45s response watchdog and 60s
writer-release watchdog bound failure. A09-journal-deadline-GREEN.log records
six passed, exit0, 40.14s. Both actual deadlines returned 504 before writer
release, preserved the held permit, then drained successfully. Parent read
the emitted signatures and test results. The empty supervisor
/tmp/herdr-journal-follow.Y50Fdx was removed with rmdir.

The related real-HTTP/PTY regression target machine_sessions passed one test,
exit0, in A09-journal-session-regression.log with actual shell/PID/CWD,
idempotent replay, conflicts, roots and explicit close cleanup.
Parent subsequently composed routed HTTP adapters and reran all six contention
tests plus the real session test successfully. See A10-owner-parent-composition.md:
last-owner retirement still prematurely exits its in-process handover fixture
and cannot be accepted from exit0 alone.

## Isolation and unfinished work

All commands used existing private src-tauri/target, jobs2, debug0, incremental0,
empty RUSTC_WRAPPER, explicit normal Cargo/Rustup homes. Each run set private HOME,
FERRYX_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR, XDG and TMPDIR under a
fresh /tmp supervisor outside Git before library initialization.

Supervisors:
- /tmp/herdr-journal-red.2YwQRZ: all contents observed empty after fixture cleanup;
  parent removed empty directories with rmdir successfully.
- /tmp/herdr-journal-green.LnJpSH: seven empty directories observed after exit0.
- /tmp/herdr-journal-follow.YS1xI6: seven empty directories observed after build
  failure; no runtime fixture executed.
- /tmp/herdr-journal-follow.5QB0Bl: actual mutation RED, empty supervisor removed.
- /tmp/herdr-journal-follow.ZbDQx8: combined GREEN, empty supervisor removed.
- /tmp/herdr-journal-follow.uYRBWY: revocation GREEN, empty supervisor removed.
- /tmp/herdr-journal-follow.6DZtvU: parent PTY drain GREEN, empty supervisor removed.

No canonical daemon, user desktop, remote host, commit or release operation.
Final related tests/build and aggregate verification await the composed source.
The full journal-contention todo is not complete. New untracked source files
do not appear in ordinary git diff; source inspection and explicit paths are
required rather than assuming an empty diff proves no edit.
