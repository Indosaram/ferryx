# Wave2 continuing repair ownership

Overall status: NOT COMPLETE. This is coordination, not packet acceptance.
The approved A01-A24 and AC01-AC12 plan remains binding.

## Current source and scheduler

Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.
The parent surveyed its inherited dirty source. Plain git status failed because
the provisioned Ghostty submodule is a symlink; status with
`--ignore-submodules=all` succeeded. No dependency link was changed.

Existing run: `dag_72a630d3-51a8-4db5-9a50-c8c797d8db61`.
A12's completed child `st_01a0985a` explicitly reports PARTIAL / NOT READY.
Its report lists rich snapshots, worktree payloads, metadata feeds, watcher,
availability, snapshot-race and lag proofs as unfinished. Its shared-file
write phase has ended; its candidate event changes must survive later edits.
A11 relay work remains its own file domain. Aggregate worker `st_01a09868`
was steered to read-only gap review while producers change source. No executable
aggregate or readiness verdict can be based on that moving tree.

## Independent active repairs

- `st_01a09869`, `a10-routed-owner-repair`: original session owner epoch,
  machine scope and singular controller authority across actual handover.
  Owns daemon routing/protocol/session authority, terminal/service.rs and
  necessary remote server/backend/state wiring, plus new handover tests.
  Preserves A12 event additions. Does not own output_hub.rs, session.rs,
  pty.rs, dependencies or relay files.
- `st_01a0983c`, resumed existing PTY worker: complete portable cancellable
  input. Owns terminal/session.rs, terminal/pty.rs, new platform adapter and
  focused tests. Any dependency/lock change first needs parent coordination;
  global Cargo caches and shared dependency links cannot be mutated.
  Windows execution uses separate private staging, not Q4's retained root.
- `st_01a0986b`, `a10-output-budget-hub`: additive machine byte-bounded
  subscription in terminal/output_hub.rs and new queue module/tests only.
  Existing broadcast and SessionAttachment contracts remain compatible.
  Actual socket integration follows routed-owner wiring, not a concurrent
  edit of server.rs. Hub proof alone does not close the stalled-WS requirement.
- `st_01a0984d`: Windows platform failure repairs in its retained private
  Q4 snapshot only; reviewed clipboard repair already has scoped acceptance.

Attempting to continue old A09 worker `st_01a097fc` returned evicted residency.
The new routed-owner task is a fresh repair using its existing source, not a
repeat of A09 CRUD implementation or a duplicate live task.

## PTY evidence inspected by parent

Read `A10-pty-input-repair.md`, the actual cancellation fixture, current
session input implementation and `A10-pty-input-final.log`.
The final log records original PID32647, actual kernel saturation at1022
bytes, receipt of1023 bytes after cancellation plus a fresh sentinel, and
child reap/output drain/root cleanup; input selector exit0.

The selector's two passes include its no-environment child helper, so there is
one independent saturation scenario. This is future-drop cancellation evidence,
not controller-generation socket evidence. Current files are changing in the
portable continuation; historical hashes do not establish the new candidate.
The parent sent review concerns about bounded child control waits, EOF handling
and post-close input admission to the owning worker. No portable, controlled
deadline or full A10 acceptance is claimed.

## Next composition boundary

Collect producer source and focused runtime evidence, preserve all prior failures,
then integrate byte-bounded subscription into the actual owner/socket path and
complete A12's remaining event contract under exclusive shared-file ownership.
Execute one aggregate against a stable source manifest, then split discovered
repairs by genuinely disjoint file domains. Do not insert independent approval
nodes after each producer.

R9 boundary verification, journal contention, composed Linux/Windows checks,
A13-A24, native/manual coexistence, forced-relay complete workflow, compatibility,
rollback and final code review remain open. Canonical daemons, user desktop,
foreign worktrees, commits, deployment and release builds remain untouched.
All completion notifications are pending asynchronous inputs, not PASS signals.
