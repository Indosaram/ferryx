# A12 metadata parent verification

Status: real integration and UI wire checks passed; remaining Rust gates and
full scope audit remain open. This is not full A12 or full plan acceptance.

## Requirement-to-evidence checklist

- Owner-local inactive PTY title/CWD publication: parent read
  session_metadata_events.rs and machine_session_metadata.rs, then ran the
  real authenticated HTTP/WS/PTY regression successfully.
- Provider admission and isolation: parent read session_metadata_provider.rs,
  the canonical daemon report dispatch, and the provider process fixture.
  The runtime passed exact target, stale epoch, forged machine, unowned
  session, forged provider, and canonical report-socket OMO acceptance.
  Non-OMO runtime acceptance and platform matrices remain open.
- Durable metadata through exit: parent read journal update/save merge logic.
  The same runtime verified retained HTTP and exit-event title/CWD.
- Predecessor forwarding: parent read session_metadata_forward.rs and the
  actual handover fixture. The runtime disconnected and joined predecessor
  clients, observed partial/recovery boundaries and the retry floor, exhausted
  retries, and observed forwarders=0, owner_streams=0, subscriptions=0.
  Successor targets retained the original owner epoch; no successor PTY
  was created.
- Event revision and mirror isolation: parent read machine_events.rs.
  Runtime verified envelope revision=sequence, separate sessionRevision,
  ordered mirror barriers, subscribe-before-snapshot overlap, lag recovery,
  worktree events and external Git reconciliation.
- Snapshot race: parent read the deterministic real catalog commit barrier
  and stale-boundary/same-WS complete-recovery assertions. Independent execution
  has not reached the test because a sibling fixture prevents lib compilation.
- Wire compatibility: parent inspected the exact tracked delta: two optional
  Rust fields, two optional UI decoder fields, and one additive shared fixture.
  Original fixtures and equality assertions were not replaced.
  Parent UI invocation passed all 2,221 tests.
- Source provenance: all ten files in
  A12-session-metadata-reliability-source.sha256 matched parent current reads.
  This does not prove an exact historical delta for earlier shared-file edits.
  Full original child prompt/steering reconstruction and full bidirectional
  scope audit remain open; report/transcript claims alone are not acceptance.

## Parent commands and actual outcomes

Commands run through the inspected A12-session-metadata-run.sh isolation
wrapper, with unique labels:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test machine_session_metadata --test machine_events --test machine_sessions -- --nocapture
bun run --cwd ui test src/lib/pairedDaemonContracts.test.ts src/lib/pairedDaemonParity.test.ts
```

parent-combined.log: all three integration binaries passed in one invocation.
parent-ui.log: two test files, 2,221 passed. Both exit receipts are 0.
Both cleanup receipts are 0; parent independently confirmed their private
roots no longer exist. Existing compiler warnings were retained.

The next invocation, parent-unit-build, stopped with exit 101 before running
the deterministic snapshot test, machine_protocol tests, or headless CLI
build. Actual errors: E0599/E0282 in tests/support/machine_input_fixture.rs
at lines 39 and 42, using Response.json without that reqwest feature.
The active A10 owner st_01a09922 was notified; parent did not edit its file.
Its private root was removed and independently confirmed absent.
Resume these gates after the owner signals compile readiness; do not count
this compile failure as a behavioral RED or repeatedly retry unchanged source.

LSP found no errors in machine_events.rs or session_metadata_forward.rs.
No source edits were made by this parent during this verification.
No canonical daemon, user PTY, desktop, deployment, release or commit was used.
All current implementation work remains uncommitted in the isolated worktree.
