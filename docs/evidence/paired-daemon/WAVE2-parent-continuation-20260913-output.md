# Wave 2 parent continuation checkpoint

The full A01-A24 / AC01-AC12 objective is active and substantially incomplete.
The original approved plan remains at
`/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md`;
the isolated implementation worktree has no `.omo` directory.

## Latest independent verification

Parent output writer plus socket/budget/PTY/machine stream batch passed 14 tests.
See A10-output-parent-socket-review.md for source inspection, exact runtime
identities, logs and completed supervisor cleanup.

The subsequent parent command combined `a10_output_live_socket`,
`worktree_safety` and `pty_input_cancellation`, with `--test-threads=1`.
It exited 101 before tests: machine_operation_journal.rs:114 had malformed
`#[unix]`, producing E0658 and an unknown attribute error.
A10-live-reactor-parent.log preserves that failure. It is not behavioral RED
for live output or input. The empty `/tmp/a10-owner-parent.oKjIgV` supervisor
and its seven empty directories were removed with rmdir.
The journal owner was notified; no parent retry was started against the
same known broken source.

## Reviewed candidates awaiting parent runtime acceptance

- st_01a0986b delivered continuous 64 MiB actual PTY output through the exact
  HTTP-upgraded socket with a transparent real TCP Pending/Drop observer,
  sibling progress and original-PID replay-gap reconnection. Parent read both
  complete new files and the evidence report. The child run passed, but parent
  acceptance is pending the blocked command above. Parent added crate-level
  Unix gating and bounded exact-peer connection selection after coordinating
  those two fixture issues with the author. LSP returned no diagnostics.
- st_01a0989f delivered the pre-spawn entered-runtime check and a focused
  no-runtime test, preserving the synchronous missing-command test unchanged.
  Parent read the production diff, complete input tests and report.
  The child reported 12 related passes. Parent runtime acceptance remains
  pending, not inferred from the child or from compilation failure elsewhere.
  Without a runtime, the returned spawn error now identifies that missing
  prerequisite rather than executable resolution; this interpretation is explicit.

## Active disjoint work

- st_01a09869: actual separate-process final owner retirement and durable
  close-response delivery, now also authorized for the narrowly necessary
  session API/journal reconciliation repair. Earlier real progression reached
  predecessor 204 plus persisted completed close, but replacement operation
  lookup returned 404. Preserve six parent contention/deadline/revocation
  tests. No machine socket or PTY lifecycle ownership.
- st_01a098a6: dropped sole PTY output receiver cleanup adjudication and
  minimal lifecycle fix if reproduced. Owns terminal/pty.rs, session.rs only
  for lifecycle plus a new focused regression. Preserve completed reactor
  and Unix/Windows input changes. Historical failed session
  5455c4a5-6328-44bf-aa9b-a446ebbd3217 still lacks an exact PID/reaping receipt.
- st_01a098a9: A11 relay followthrough, restricted to relay source and fixtures.
  Finish identity agreement, two roots, actual lost durable reply reconciliation,
  replay, tickets, controls and cleanup. The previous relay child was evicted;
  it is not running in parallel with this successor.

Existing DAG `dag_72a630d3-51a8-4db5-9a50-c8c797d8db61` is scheduler-completed,
not accepted. A send to its A11 node was refused because the run was completed;
direct continuation of st_01a0985b was refused as evicted. No completed packet
was rerun from HEAD. Use a new phase or amend the existing definition for the
remaining genuinely dependency-ordered work after current composition.

A12 rich snapshots, revisions, metadata feeds, availability/Git invalidation,
snapshot-race/lag and mirror-isolation proofs remain unfinished. Its shared
service write phase must follow the active retirement owner, not conflict
with it. Aggregate verification remains paused until a stable composition.
A13-A24, platform composition, UI baseline failures, full code review and
compatibility/rollback are still open. No new todo was marked done here.

All work is uncommitted. Canonical daemons, user PTYs, desktop, other worktrees,
deployments and releases were not manipulated. Do not mark the goal complete.
