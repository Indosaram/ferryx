# Q4 Linux readiness repair candidate

Task st_01a0984c; 2026-09-13. Independent private-snapshot repair investigation.
**Causality confirmed; minimal test-only candidate passes. Parent composition
and verification remain required.** This does not supersede other platform,
native or full-plan exclusions in Q4-linux-frozen-verification.md.

## Failing-first evidence and exact cause

The original, preserved Q4-linux-remote.log records 251 passed / 3 failed,
exit 101; Q4-linux-integration.log records catalog 3 passed / 1 failed, exit
101. No baseline failure log was overwritten and none was retried unchanged.

Before modifying source, Q4-linux-readiness-capture.py directly executed the
same frozen Linux helper binaries with the same `--exact <helper> --nocapture`
argv and inherited `RUST_TEST_THREADS=1`, private HOME/runtime/data/session/XDG/
temp paths. Complete stdout bytes are retained in the four `.stdout.bin`
artifacts, stderr separately; Q4-linux-readiness-capture.log contains command,
PID, byte representation, marker observation and exit. No strace was installed;
the harness owns and captures the actual pipe instead of tracing another task.

Exact first marker lines (each ends in byte 0a):

```
test catalog_owner_process ... A05_READY
test remote::workspace_api::worktrees::authority_tests::interrupted_worktree_owner_child ... A08_GIT_COMPLETE_BEFORE_PUBLICATION
test remote::workspace_api::worktrees::authority_tests::interrupted_worktree_owner_child ... A08_RECONCILED_UNKNOWN_NO_REPEAT listener_joined=true
test remote::workspace_api_tests::r12_crash_owner ... R12_WINDOW
```

The parent catalog/project parsers require exact line equality; worktree
parsers require the sentinel at line start. Serial libtest writes its test
name and ` ... ` without a newline before the helper's first println. The
sentinel is present, but the strict framing contract is violated.

| Captured child | PID | Result and correlation |
| --- | --- | --- |
| catalog owner | 313243 | prefixed A05_READY observed; harness sent register; A05_DONE and exit 0 |
| worktree publication barrier | 313265 | prefixed pre-publication marker observed; explicit SIGKILL, exit -9 |
| same worktree replay | 313295 | prefixed reconciled marker observed; real unknown/no-repeat assertions pass, exit 0 |
| project afterCatalog barrier | 313313 | prefixed R12_WINDOW observed; explicit SIGKILL, exit -9 |

The capture harness searches raw bytes only to diagnose their placement; it is
not a replacement acceptance test or proposed permissive parser. It retains
stdin across crash kill/wait, waits for stdout markers with bounded async
reads, explicitly waits each child, and removes its private roots. These bytes
explain the active-Git test's successful replay exit but failed marker, the
worktree publication timeout, project barrier timeout and catalog timeout.

## Minimal candidate

Q4-linux-readiness-candidate.patch changes only three existing test files:

- `src-tauri/tests/machine_catalog_persistence.rs`: prefix A05_READY emission
  with newline. A05_DONE already follows the readiness newline and is unchanged.
- `src-tauri/src/remote/workspace_api/worktree_authority_tests.rs`: prefix the
  first publication and replay sentinels with newline.
- `src-tauri/src/remote/workspace_api_tests.rs`: prefix crash-window and both
  replay-result sentinels with newline.

Six machine-consumed marker emissions plus three explanatory comments change.
No parser/assertion, timeout, kill/reap, ownership, request identity, Git,
catalog/journal or production behavior changes. No forced multi-thread libtest
setting, loose contains/ends_with check, sleep, or retry masks the defect.
The leading delimiter makes each first signal a line regardless of libtest's
pending prefix. Parent strict matching and all substantive assertions remain.

The local private snapshot was edited using apply_patch. The resulting minimal
patch passed `git apply --check` and was applied only to the retained remote
private snapshot. Source before/after SHA-256 values are in
Q4-linux-readiness-source-hashes.json. Original capture identity remains the
6,827-file Q4-linux-snapshot-manifest.json (SHA-256
f5c4eab343d595b88c83b31a29412f8ad3c4acc3008951283fb8bee71bc8b4bf).
Final full comparison permits exactly these three test files plus the already
disclosed generated Linux schema delta from the prior build. The schema delta
is not part of this candidate patch. Neither herdr-wave1 nor resumed source
was edited; no production credentials or canonical daemons were accessed.

## Single-run Linux verification

The same real omaki x86_64 Linux host/toolchain and retained private target
were used. Q4-linux-readiness-environment.json retains RUST_TEST_THREADS=1,
jobs=3, empty wrapper, debug=0, incremental=0 and explicit Cargo/Rustup homes.
All runtime paths are private and established before library initialization.
LSP was attempted before compilation; rust-analyzer remains unavailable
(Q4-linux-readiness-diagnostics.log). Existing compiler warnings remain visible.

Each command below ran **once after the candidate**, under background monitor
313715, which waited/reaped each Cargo child. Monitor completion was subscribed
through Linux pidfd with a bounded wait, not sleeps or duplicate invocations.
Exact argv/PIDs/exits are in Q4-linux-readiness-results.json.

| Command | Exit | Evidence |
| --- | --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture` | 0; 254 passed, 0 failed, 674 filtered | Q4-linux-readiness-green-remote.log |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence -- --nocapture` | 0; 4 passed, 0 failed | Q4-linux-readiness-green-catalog.log |
| `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay` | 0 | Q4-linux-readiness-green-build.log |

Actual affected real-surface cases now complete:

- Active-Git owner crash: original owner killed/waited, fixture-owned surviving
  Git group explicitly killed and hook EOF observed; replay owner exits 0 with
  unknown/no-repeat assertions intact. This remains harness orphan cleanup,
  not a new automatic parent-death guarantee.
- After-Git/before-publication crash: original killed/waited and replay exits
  0; original same-request/no-repeat behavior asserted by the existing helper.
- Project crash windows: afterCatalog and afterJournal, each register and
  unregister, run all eight original/replay children. Original owners SIGKILL;
  replay owners exit 0; registration same device/request gives 201 revision 1,
  unregister replay 204, folders retained and temporary roots removed.
- Catalog owner: register PID 315692 and restore PID 315710 exit 0;
  post-readiness injected failure PID 315727 is killed/reaped. Real Ping/Pong,
  registration persistence, restored rows and injected IPC cleanup remain tested.

Expected panic output in injected-cleanup tests is preserved; harness results
are green, not inferred from absence of panic text. Helper entry tests returning
without child environment are not counted as independent scenario coverage.

## Cleanup and delivery

Q4-linux-readiness-cleanup.log verifies all 30 extracted logged process IDs
absent, no task-owned live process, explicit `/tmp/a08-typed-owner-oTLahw` absent,
and complete readiness-qa removal, including generated fixture credentials.
Tests retain their listener/PTY/child wait receipts. Source, target, raw stdout,
logs and scripts remain only under the two assigned private staging roots;
evidence copies are Q4-linux-prefixed files under resumed docs/evidence.
Prior failure logs and the original NOT ACCEPTED report remain unchanged.

The earlier provider HTTP 400 remains orchestration history only; no API or
tool failure was relabeled compiler RED. No deployment, installed GUI launch,
privileged change or Git commit occurred. This candidate resolves the four
demonstrated Linux fixture framing failures; it grants no implementation,
native or full-plan acceptance beyond the recorded private candidate run.
