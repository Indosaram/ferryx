# Four-track final code review

Reviewer: st_01a09b19, completed 2026-09-13.
Integration baseline: integration/4track at 3cf3c259.
Scope: four committed track heads plus current uncommitted follow-up repairs.

## Current verdict: APPROVE for repaired code delta

Fresh delta reviewer `st_01a09b31` returned APPROVE on 2026-09-13 with no
concrete remaining blockers in P1/P2. The supervisor's independent focused
tests, Cargo check and diff check passed; see
`WATCHDOG_FINAL_LEAD_VERIFICATION.md`.

- P1 closed: `src-tauri/src/ipc/agents.rs:44-48` awaits daemon success before
  native mutation/event. Actual command/event tests cover rejection preserving
  Working, absence of premature Idle, and successful reset.
- P2 closed: `src-tauri/src/daemon/logging.rs:33-80` provides a filtered bounded
  file sink with blocking-context disk writes. `src-tauri/src/cli.rs` preserves
  primary service startup and completion independently of logging failures.
- The reviewer inspected the reports, final code and referenced transcripts.
  It did not rerun tests or operate the live daemon or GUI. Direct overfit/slop
  review identified no blocking test-quality issue.

This closes the original review's two code blockers, not the separate historical
acceptance failures. Commit/merge authorization is still pending. Combined-tree
documentation citations and integration verification remain to be completed.

## Original verdict: REJECT pending two watchdog fixes

This is a code-readiness verdict, separate from the previously recorded
historical acceptance failures. No additional merge blocker was identified
in the disk, documentation, or regression-test tracks in this bounded review.
Desktop GUI execution was not performed and is not a new gate; manual
procedures have been handed to the user as requested.

## P1: failed reset emits a premature native Idle event

`src-tauri/src/ipc/agents.rs:35-44` calls native reset before awaiting the
daemon request. `src-tauri/src/native_terminal/surface_host.rs:797-819` changes
retained native activity and emits an event. The frontend consumes that event
independently of the reset promise and can map Working/Waiting to Done.
Thus a rejected reset can change activity despite the repaired rejection path.
The frontend outcome tests mock the native subscription as a no-op and cannot
detect this command/event interaction.

Required correction: await successful daemon reset before explicit native
mutation/event. Capture a regression at the command/event boundary proving
rejection leaves prior activity and emits no reset-induced Idle event, and
that success still resets. Preserve frontend tests.

## P2: release-reason tracing has no daemon subscriber

`src-tauri/src/daemon/agent_state.rs:55-85` issues tracing info records, but
the headless entry path (`main.rs:35-44`, `cli.rs:471-518`) bypasses GUI tracing
initialization (`lib.rs:1824-1829`). `daemon/client.rs:530-570` pipes stderr
without draining it, so merely directing new logs to stderr is insufficient.

Required correction: provide a usable daemon logging destination, including
desktop-spawned headless execution, preserving stdout readiness semantics and
keeping blocking disk I/O off runtime workers. Verify actual session and
distinct manual/foreground reason records using an isolated test process.
Never launch against or modify the live daemon.

## Other review results and verification limits

- Disk cancellation/publication ordering, fresh loss previews, prunable
  record targeting, late scan events, and default IPC routing were reviewed;
  no additional blocking defect identified.
- Documentation source assertions were independently executed: 354 structural
  references and 16 token assertions passed. This does not prove prose truth.
  Citations must be checked again against the combined tree.
- Selection and duplicate tests assert meaningful regressions. The prospective
  duplicate RED contains the intended readiness assertion and cleanup records;
  current production/test SHA-256 values match the restoration proof.
- The latency baseline uses actual client transport with loopback gateways,
  not SSH, PTY, Rust gateway or rendering latency.
- `sa-tests/src-tauri/tests/zz_probe_bulk_selection.rs` is an unowned diagnostic
  probe without preservation assertions. Do not include it automatically.
- All four track heads are ancestors of integration/4track; their current
  tracked diffs pass diff checking.
- Rust/frontend suites, builds and GUI were not rerun by the reviewer.
  Existing executor and lead evidence was inspected, not relabeled as fresh
  execution. Runtime Windows/Linux and desktop limits remain disclosed.
- Minor unused cached-dirty prop plumbing, nested act calls and disk-test
  subprocess isolation were noted as nonblocking maintenance residue.

Original PID preservation, no-commit compliance, and missing original
pre-production RED cannot be recovered by these fixes. They do not justify
repeating unrelated green checks. Fix these two current defects, review their
delta, then integrate only after the pending commit/merge authorization.
