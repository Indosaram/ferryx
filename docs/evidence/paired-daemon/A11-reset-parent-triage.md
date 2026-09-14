# Parent relay reset triage

Status: scoped repair delivered; parent combined runtime acceptance pending.
Full goal is not complete.

## Inspected evidence

Parent read `A11-relay-reset-repair.md`, actual authority validation and four
awaiting socket callers, and the real PTY fixture. The worker's
`A13-issuer-repeat-A11-reset-final.log` contains eight passes in one run,
`stream.log` contains one pass, and `build.log` has a completed headless build.
Cleanup markers are present. These logs do not verify later concurrent edits.

The read-only authority check now waits off-runtime with a shared ten-second
lock deadline instead of treating ordinary try-lock failure as loss of
authority. Global controller mutex retention remains a separate A10 concern;
the saturation worker must preserve this validation correction.

No scoped before/after delta or worker source-hash receipt exists. The worker
explicitly confirmed that absence. HEAD diff contains inherited work and
cannot establish this lane's exact delta. Current parent read hashes are:

- session_service.rs:
  d7a718d7ec5231e22af9d3f4dfa3e4e1e5f82edbc854c20f0697663ad42687ff
- remote/server.rs:
  04b634bbabc0aa8c48c54b63573569310f9c73f538d1b6b09cbe414707c0a1ba
- tests/relay_pairing_generation_regression.rs:
  3844f0bc46222f87851e8ca69a62c132a620e9c6f293ebce03fed3d4387a2260

These identify current reads, not the historical GREEN source.

## Six broader failures and ownership

The actual security log records 271 passed and six failed.

- Three machine-protocol equality failures: metadata added title/agentType
  nulls to old round-trip output. Existing metadata worker `st_01a098de`
  owns compatibility repair and Rust/TypeScript parity proof, preserving
  original equality assertions.
- The pending-input test waits on a fake backend that production input no
  longer calls. `st_01a09922` owns replacing that disconnected observation
  with actual WS/PTY cancellation coverage, not dropping its assertions.
- Worktree failure publication returns no event when the test expects one.
  Parent verified its subscriber is the dedicated worktree channel, not the
  metadata broadcaster. The worker withdrew its metadata-interference theory.
  New `st_01a09926` owns durable publication/receipt ordering diagnosis and
  repair. In particular, inspect journal.catalog_receipt after the fixture
  obstructs the journal path but before catalog persistence.
- `r12_unreadable_root` synchronously called registration on a Tokio task;
  registration now performs Git probing with a blocking-thread-only bridge.
  Parent moved initial fixture setup/registration through ipc::run_blocking.
  Existing PermissionDenied and replacement Invalid assertions are unchanged.
  LSP is clean. Focused run mon_4JYDHHYFRGGJCKFH / bash_140 completed:
  `A07-unreadable-fixture-parent.log` records one passed test, zero failures,
  exit 0, and R12 fixture removal. Parent removed the empty supervisor
  `/tmp/fx-a07-fixture.Tl6Gih` and all nine subdirectories with rmdir.
  This scoped fixture repair is closed; full A07 remains open.

The temporary V05 UI QA config was independently confirmed absent.
No canonical daemon, desktop, user PTY, commit, release or deployment occurred.
All work remains uncommitted. Final combined verification follows stable
producer composition; no repeated checks against known compile gaps.
