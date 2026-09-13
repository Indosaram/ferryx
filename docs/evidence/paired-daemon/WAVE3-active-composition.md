# Native host inventory composition checkpoint

The approved A01-A24 and AC01-AC12 plan remains the objective. Overall status:
NOT COMPLETE. No capability, test count, or child completion closes that goal.

## Current disjoint implementation owners

- `st_01a098c3`, A13 inventory core: paired_host inventory, private-file helper,
  tests and module declarations. Native HostView carries hostId, relayOrigin,
  machineId, displayLabel, grantScope, canonical-string generation, authStatus
  and online; it excludes the bearer by construction.
- `st_01a098c9`, A13 desktop state: remoteHostStore, new pairedHostInventory
  adapter/tests and minimal native bootstrap. Browser/mobile persistence must
  retain its existing behavior. Desktop migration removes only explicitly
  host-scoped legacy credentials after native durable verification.
- `st_01a098d0`, A13 native integration: new paired_host service, IPC handlers,
  additive daemon protocol/client/server and command registration. It consumes
  the core inventory, rather than creating a second credential authority.
  Proxy capability remains disabled until A15/A16 actually work.
- `st_01a098cc`, remaining A09 journal seams: second spawn reconciliation and
  machine socket validation, with real held-writer RED evidence required.
  Owns journal-access portions of session_service and contention fixtures.
  Remote server socket edits require exact-section coordination.

Native and frontend workers must agree command names and payloads before
integration; proposals alone are not implemented commands. All workers retain
foreign changes and avoid canonical state, user desktop, commits and releases.

## Parent findings and verification

R9's existing legacy response regression is repaired and independently passes
the actual HTTP boundary test. See R9-parent-boundary-current.log and
R9-eager-upload-parent-review.md, including the earlier invocation failure.
The R9 todo is closed as an investigated transport limitation, not a guarantee
that concurrent oversized upload failures always deliver an HTTP response.

A12 workspace worker st_01a098ac delivered six changed files. Parent read all
six actual files, including their complete worktree transaction paths, and
the service delta. machine_events, workspace_watcher and event-test hashes
match the worker report. Parent independently executed the reported combined
catalog, events, worktree, transport and prunable tests followed by headless
build as bash_105: ten top-level tests passed and build exited 0. Output:
A12-parent-combined-current.log. Parent inspected actual event, PTY reap,
relay Git/UDS and listener cleanup receipts, then removed the empty supervisor
/tmp/fx-a12-parent.eN8Ijq with rmdir. Five production files returned no LSP
errors. This validates the current scoped scenarios, not all A12 behavior.

Parent added parent_watch_promoted_to_project_observes_nested_file in
workspace_watcher.rs, observing actual notify paths after a parent watch
becomes a project. Its first run did not reach the assertion: active A13
integration lacked its service module and exhaustive request matches.
A12-parent-watch-RED.log is compiler exit 101, NOT behavioral RED. The empty
supervisor /tmp/fx-watch-parent.w2CjYC was removed. The owning A13 worker was
notified; the later compiled execution is recorded below.

A13 core delivered its report with final tests blocked by the earlier A09
fixture compiler errors. A09 worker corrected those accesses with a narrow
test-only accessor and delivered second-spawn RED/GREEN, but still owes
the real socket contention evidence. Parent read its new fixture and
production offload; parent composed execution is recorded below. Neither worker's
completion is accepted as full packet completion.

Parent independently read all five A13 desktop files and both inventory core
source/test files. Native integration has since added read_verified and
generation_snapshot to inventory.rs; its final hash is not assumed to match
the earlier core report.

Parent ran the six-file UI gate using the plan's Bun script, then the UI
build, in monitor bash_112:

```sh
bun run --cwd ui test src/state/remoteHostStore.native.test.ts src/state/remoteHostStore.test.ts src/lib/pairedHostInventory.test.ts src/remote/RemoteRouting.test.tsx src/remote/MobileHostDrawer.test.tsx src/remote/zeroConfigSecurityProbe.test.tsx
bun run --cwd ui build
```

Observed output: six test files, 60 tests passed; tsc passed; Vite 6.4.3
transformed 1884 modules and built in 2.22 seconds; A13_UI_PARENT_EXIT=0.
Fresh parent LSP on pairedHostInventory.ts and remoteHostStore.ts returned no
errors. These are fixture/compile checks, not real native IPC or desktop QA.

Parent read in-progress native service.rs and notified its owner of missing
machine Control permission validation and synchronous constructor join/
blocking-worker policy concerns. Parent read the final service correction:
machine scope requires Control, and lazy inventory initialization runs through
run_blocking without joining a thread in the constructor.

A13 native worker submitted 14 passing tests, real in-process DaemonServer/
UDS plus two gateways and relay, and headless check/build. Parent read the
actual service, IPC commands, client exchange and complete native fixture.
Its native fixture manually expires issuer Ready state before repeated PIN
issuance; that does not prove ordinary repeated issuer CLI behavior.
Separate-process restart and paused-network stale-adoption proof were absent.
The worker is resumed to complete those two native gaps. Parent traced the
actual issuer server call: it invokes generate_scoped_pairing directly,
without the fixture's explicit expiration. Independent worker st_01a098ec
now owns the repeat-issuance repair/proof and A03 capability fixture update:
PairingCoordinator sections of relay_client.rs, PairingState only if needed,
and daemon/a03_owner_cli_fixture.rs. It may not edit paired-host service or
shared server/client/daemon protocol files. Required proof uses actual owner
CLI and relay, not only a direct coordinator call.

A13 native gap worker subsequently delivered 17 passing entries, including
paused authenticated capability responses and separate daemon test-binary
process restart. Parent read complete process_tests.rs and the gated HTTP
test; three owned children cover pair, restart/readback/authentication and
injected action failure. Parent combined native/watch verification is now
running as bash_126 in A13-native-A12-watch-parent-composed.log; its result
is pending, not inferred from worker counts.

Issuer worker stopped at compiled behavioral RED without production change.
Parent resumed it explicitly to implement the focused coordinator fix and
prove repeated actual owner CLI issuance without manual expiration.

Parent paired_host rerun bash_122 did not execute tests: concurrent metadata
schema additions left one Session initializer missing agent_type/title.
A13-native-parent-current.log is compiler exit101, not behavioral evidence.
Metadata owner was notified; /tmp/fx-a13-native-parent.nTkcn8 was removed.
The worker's erroneous ferryx --help GUI entry attempt is excluded from QA;
no further GUI/binary-help launch is permitted.

Metadata lane st_01a098de first delivered a real HTTP/WS/PTY failing test,
not production publication. Parent read the complete test and RED log:
the inactive owned shell emitted the title control after cd, then the
machine WS timed out waiting for sessionMetadataChanged. Subsequent metadata/
revision/mirror assertions were not reached. Parent independently reproduced
the same failure as bash_119: A12-session-metadata-parent-RED.log records
one failed test, exit101, 6.29 seconds, real PTY marker and complete cleanup.
The empty supervisor /tmp/fx-metadata-parent.IdmS6B was removed with rmdir.

The lane is resumed with expanded write ownership: metadata-specific server
agent hooks and owner routing; metadata-only protocol/client additions;
Session title/agent schema; journal metadata persistence and exit-record
merge; session-lifetime output consumer. Native integration st_01a098d0
retains pairing-specific sections in those files and was notified. Both use
targeted patches, no whole-file rewrite/formatting. Watcher/machine_events
remain with cancellation lane st_01a098e5. Revision composition changes
must be coordinated through parent. No production GREEN is yet claimed.

Metadata worker next delivered owner-local production GREEN with title/CWD
parser, provider admission, exact-owner IPC routing, retained Session fields,
and journal exit merging. Parent read both new production modules. Its report
explicitly leaves predecessor live events and canonical agent-report socket
acceptance unverified; the direct provider-admission test is not socket proof.
The worker is resumed to finish those paths.

Parent approved A12-session-metadata-revision-proposal.md: envelope revision
equals broadcaster sequence; snapshot sequence/revision capture the cursor
before construction; projectRevision/sessionRevision retain domain values
and snapshot payload revisions remain unchanged. Metadata worker owns only
publication/revision/boundary fields in machine_events.rs, preserving the
completed cancellation/select/permit logic. Successor forwarding must retain
original targets and owner epochs, with bounded lifecycle and mirror redaction.

Parent compiled A13-A09-A12-parent-composed.log: all ten core inventory tests
passed; the actual watch promotion test failed with observed=false, exit101.
The initial journal selector used the filename rather than module name and
selected no journal tests; no journal PASS is inferred from this run.
Supervisor /tmp/fx-native-parent.jyzTSo was empty and removed with rmdir.

Parent fixed workspace_watcher.rs by tracking a path-to-recursive-mode map.
Recursive root registration wins over nonrecursive parent registration; mode
changes explicitly unwatch/re-watch. The actual nested file event now arrives.
Parent also added a real WS/PTY test in journal_contention_tests.rs proving
terminal attachment, exact owner target, kernel PTY echo and independent HTTP
while the actual journal writer holds its mutex. Detach preserves that PTY;
explicit cleanup closes it afterward. No socket production change was needed.

Correct combined command:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --nocapture machine_operation_journal::contention_tests parent_watch_promoted_to_project_observes_nested_file
```

A12-watch-A09-journal-parent-GREEN.log: 11 passed, exit0, 40.21 seconds.
Includes actual 10/40-second read/mutation deadlines, revocation, second-spawn
admission and spawn-guard retention, no post-cancellation PTY/journal intent,
restart uncertainty, real WS/PTY contention and watch promotion. All fixture
cleanup signatures are present; /tmp/fx-watch-journal.HtoOHr was removed with
rmdir. Parent LSP on both changed files returned no errors.

New disjoint worker st_01a098e5 owns machine_events.rs and a crate-private
machine_event_cancellation_tests.rs for watch-refresh cancellation/deadline
and retained-worker capacity. Parent finished the verified watch-mode change
and transferred targeted watcher cancellation edits to this lane, preserving
the parent promotion regression.

Cancellation worker submitted two actual WS RED/GREEN tests, existing event
integration and unchanged promotion regression. Parent read all three source
files and the full cancellation fixture: close/read-timeout is selected while
refresh waits on the real catalog mutex; two permits remain in blocked
watchers until drainage, and three reconnects cannot start extra workers.
Parent LSP on machine_events.rs/workspace_watcher.rs returned no errors.

Parent independent batch bash_124 did not reach any test: new concurrent
a13_issuer_repeat_fixture.rs used reqwest RequestBuilder.json without the
project enabling that feature. A12-watch-parent-audit.log records four compile
errors and exit101. The issuer owner was notified to use the existing explicit
JSON body convention. Supervisor /tmp/fx-watch-audit.t9ycs6 was removed with
rmdir. That attempt was superseded by bash_126 below.

Parent bash_126 completed: A13-native-A12-watch-parent-composed.log records
20 unit tests, one machine_events integration test, and headless CLI/relay
check all passing; exit0. Includes two retained catalog workers/reconnect
capacity, delayed-network stale adoption and separate daemon child processes.
All fixture cleanup receipts are present. Supervisor
/tmp/fx-native-watch-final.Vw720N was already absent at parent rmdir; a
separate existence check confirmed absence. Cancellation repair is closed.

Parent issuer first run selected zero tests and is not test evidence.
Corrected A13-issuer-repeat-parent-exact.log records 18 tests passing plus
the child fixture result. Actual CLI repeatedly issued/redeemed machine
Control grants without manual expiration, stale PIN redemption refused,
mirror restriction preserved. Fresh CLI build passed. Child 72601 and CLI
processes reaped; both parent roots removed. Repeated-issuer todo is closed.

Issuer worker also disclosed real relay PTY ResetWithoutClosingHandshake in
relay_pairing_generation_regression. Full relay acceptance stays open;
st_01a09914 owns tunnel-only repair and full runtime verification, preserving
the verified coordinator changes.

Successor metadata worker submitted live forwarding, WS redial, canonical
agent socket and revision separation. Parent read forwarding/provider/event
production code and ran all three metadata/lifecycle/event targets as
bash_131. The first machine_events target FAILED in 0.87 seconds, exit101,
before metadata/lifecycle executed. A12-metadata-parent-aggregate.log records
failure and fixture cleanup; /tmp/fx-metadata-audit.v2H78c removed.
The worker is resumed to fix event reliability and prove actual owner IPC
disconnect/reconnect and forwarder drainage; isolated PASS is insufficient.

Parent source review concerns to resolve before full A12 acceptance:

- Recursive watch promotion is repaired with real RED/GREEN above; Linux/
  Windows behavior remains part of the full platform gate.
- Watch refresh cancellation passed parent runtime verification before the
  later successor metadata composition; retain it in the final regression gate.
- New rich local registration probes execute under catalog and mutation locks.
  Their interaction with concurrent event watch refresh and local registration
  needs focused inspection; the existing unrelated-workspace fence test does
  not hold these new Git probes.
- Owner-validated title/CWD/provider feeds and session revision composition
  remain unimplemented by this workspace-only lane. They cannot be inferred
  from sessionStarted or preserved PTY object identity.

These are explicitly open concerns, not claimed reproduced defects or fixes.
Full A12 remains open even if the current combined run passes. One final
aggregate verification follows stable implementation composition; no per-node
approval gate is added to the implementation batch.
