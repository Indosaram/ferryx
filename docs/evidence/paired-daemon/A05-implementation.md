# A05 implementation handoff

## Final B1/B2 correction handoff (2026-09-12)

The corrected A05 candidate is ready for independent parent verification, not
accepted. This section supersedes earlier candidate limitations below; all prior
logs and reports, including the parent owner-restart proof, remain intact.

- B1: session-authority unregister dispatches supported `ssh:` IDs directly to
  existing ownership/session cleanup without touching the local catalog. Local
  unregister still propagates persistence errors; `daemon:` remains rejected.
  The regression seeds expired SSH ownership metadata, invokes the actual async
  unregister entry point, and checks ownership removal without contacting a host.
- B2: `cfg(test)` constructors without a config path own a fresh TempDir, retained
  in DaemonServer until drop. Catalog and associated session/SSH paths use that
  directory. Explicit-path restart and production persistence are unchanged.
  A child-only regression gives FERRYX_DATA_DIR a private corrupt canonical
  sentinel at `data/remote/machine-workspaces.v1.json`; two no-path constructors
  register the same ID against different roots independently, leave sentinel
  bytes intact, and remove both private directories on drop.
- Owner restart children now set FERRYX_RUNTIME_DIR explicitly. IPC checks Pong
  only for Ping and RegisterWorkspaceOk only for registration, rather than
  accepting either response for both requests.

### Exact commands and observed results

All commands ran serially from this worktree on macOS arm64 with
`CARGO_BUILD_JOBS=4` and
`CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target`.
Every command below uses `cargo`, followed by the listed arguments; each
`A05-final-*.log` also records its exact command on its first line.

Common arguments: `--locked --manifest-path src-tauri/Cargo.toml --no-default-features`.

| Cargo command (common arguments after test/check/build) | Evidence | Result |
| --- | --- | --- |
| test ... --lib a05_compatibility_tests -- --nocapture | A05-blockers-red.log | 101: SSH cleanup rejected; constructors shared runtime directory |
| test ... --lib a05_compatibility_tests -- --nocapture | A05-blockers-sentinel-red.log | 101: SSH cleanup rejected; constructor read actual private canonical sentinel |
| test ... --lib a05_compatibility_tests -- --nocapture | A05-blockers-green.log | 0: 3 passed, child 1 passed |
| test ... --test machine_catalog_persistence -- --nocapture | A05-final-runtime.log | 0: 4 passed |
| test ... --lib worktree::registry::tests -- --nocapture | A05-final-registry.log | 0: 7 passed |
| test ... --lib catalog_tests -- --nocapture | A05-final-sync.log | 0: 1 passed |
| test ... --lib test_server_unregister_workspace_is_idempotent_and_revokes_registration -- --nocapture | A05-final-local-unregister.log | 0: 1 passed |
| test ... --lib test_server_register_workspace_and_spawn_isolation -- --nocapture | A05-final-local-register.log | 0: 1 passed |
| test ... --lib test_server_spawn_cwd_validation -- --nocapture | A05-final-local-cwd.log | 0: 1 passed |
| test ... --lib a04_shared_services_tests -- --nocapture | A05-final-a04.log | 0: 4 passed |
| check ... --bin ferryx-cli --bin ferryx-relay | A05-final-check.log | 0 |
| build ... --bin ferryx-cli --bin ferryx-relay | A05-final-build.log | 0 |

Both RED runs preceded production correction. The first sentinel was one level
above the actual canonical remote directory; that test still exposed shared
constructor storage, but the second RED corrected the fixture location and
proved canonical catalog access directly. Neither used actual developer data.
The single GREEN blocker run passed; existing Local tests ran only after it.
The final serial suite exit receipt is 0. Compiler warnings remain visible.
LSP was requested before compilation on changed files: initial server diagnostics
returned no errors; subsequent fresh requests for all seven A05 source/test files
timed out at 3000ms. No clean final LSP result is claimed. Cargo compilation,
headless check/build, focused runtime tests, and `git diff --check` succeeded.
Linux/Windows/native UI and physical ENOSPC were not executed; actual rename
write failure and post-rename sync injection cover the agreed failure alternative.

### Final source/cleanup manifest

`A05-final-manifest.log` contains SHA-256 values for all seven scoped source/test
files and independently checked absence of roots/PIDs from this turn's logs.
Changes in this correction are confined to server.rs, session_service.rs and the
existing A05 integration test, plus evidence. No other source scope was added.

Constructor owners 80490 and 81571 failed and were reaped with their private
roots removed; GREEN owner 83512 exited successfully and was reaped. Restart
owner 84593 registered, exited and was reaped before owner 84793 restored and
exited; injected-failure owner 84823 was killed only after exact Ping/Pong
readiness and reaped. All six PIDs independently report absent. IPC assertion
injection was captured with both tasks joined and duplex endpoints closed.
A05 opens no OS socket. A04 logs separately retain listener/socket/PTy and child
teardown receipts, including all injected failure paths. Git init children are
waited synchronously. No developer daemon, desktop or SSH host was contacted,
restarted or killed; no stage, commit or push occurred.

A06-A24, machine APIs/capabilities and the whole AC contract remain unaccepted.

## Corrected candidate (2026-09-12)

Parent review corrections are implemented. The earlier candidate and blocked
report below remain historical evidence, not descriptions of the corrected tree.

- IPC trims IDs consistently on register/unregister. Unchanged bindings do not
  replace managers or increment either revision; explicit machine-to-mirror
  exposure promotion still commits and publishes.
- Platform-neutral Tokio duplex transport exercises the actual daemon IPC
  handler; no unconditional UnixStream remains in this test target.
- Separate fixture owner processes register and restore plain/Git roots. Each
  constructs the real DaemonServer and establishes Ping/Pong before announcing
  readiness. This is process restart, not dropping a service in one process;
  it intentionally does not bind the canonical daemon listener or launch PTYs.
- IPC clients run as joined tasks, including a deliberately injected assertion
  after Pong. Deadlines abort and join tasks. Owner failure paths kill/wait/reap
  before assertions, including an injected failure immediately after readiness.
- Restore retains both a missing directory and a directory replaced by a file;
  the latter's sentinel bytes remain unchanged. Real rename failure and a
  thread-local test-only post-rename sync fault both fence mutations without
  false registry publication. The ambiguous disk candidate is not rolled back.
- Startup trace: cli::run_daemon_headless -> DaemonServer::new ->
  new_with_paths -> workspace service -> catalog load/WorktreeManager probes.
  `new` uses block_in_place on the production multi-thread Tokio runtime;
  catalog construction runs on a joined dedicated worker before gateway
  construction/readiness. Direct synchronous constructors also use that worker.
  No startup caller outside the authorized server file was edited.

### Correction commands and evidence

Run from the assigned worktree with `CARGO_BUILD_JOBS=4` and
`CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target`.
Every command below uses `--locked --manifest-path src-tauri/Cargo.toml
--no-default-features`; Cargo jobs ran serially.

| Command suffix | Evidence | Result |
| --- | --- | --- |
| `test ... --test machine_catalog_persistence normalized_registration_is_idempotent -- --nocapture` | A05-correction-red.log | Runtime RED, exit 101: normalized registration lost |
| `test ... --test machine_catalog_persistence -- --nocapture` | A05-correction-green.log | Fixture failure, exit 101: closing child stdout before libtest completion caused BrokenPipe; owner reaped and root removed |
| same full integration command after draining stdout to EOF | A05-corrected-green.log | 4 passed, exit 0 |
| `test ... --lib catalog_tests -- --nocapture` | A05-sync-failure.log | 1 passed, exit 0 |
| `test ... --lib worktree::registry::tests -- --nocapture` | A05-corrected-registry.log | 7 passed, exit 0 |
| `test ... --lib a04_shared_services_tests -- --nocapture` | A05-corrected-a04.log | 4 passed, exit 0 |
| `check ... --bin ferryx-cli --bin ferryx-relay` | A05-corrected-check.log | exit 0 |
| `build ... --bin ferryx-cli --bin ferryx-relay` | A05-corrected-build.log | exit 0 |

Each command has a corresponding `.exit` receipt. Original RED and all earlier
logs remain untouched. Fresh LSP diagnostics returned no errors for all seven
changed source/test files after initial cancellation/timeouts. Compiler warnings
are retained, not suppressed. Windows/Linux execution was not performed on this
macOS arm64 workstation; platform-neutral transport is not a cross-platform
build claim. Physical ENOSPC is not claimed: the approved write-failure
alternative is exercised. Future machine APIs/events/capabilities remain off.

### Correction cleanup receipts

`A05-corrected-green.log` records owner PIDs 67467 (register) and 67666
(restore) exiting 0 and reaped, PID 67695 (injected failure) killed/reaped,
and joined/closed duplex endpoints on normal and assertion-failure paths.
Its roots `.tmps9MQMh`, `.tmpdbUWIC`, and `.tmp2d94ZS` have absent=true
receipts. `A05-sync-failure.log` records `.tmp7sI9ee` absent=true. The initial
BrokenPipe run records PID 65704 reaped and `.tmpeVCyjU` absent=true.
The A04 regression log includes its own child/PTY/socket/root teardown receipts.
No developer daemon or desktop was launched, restarted, or killed. Git init
children were synchronously waited. Source hashes and direct absence checks are
in `A05-corrected-manifest.log`. No staging/commit/push was performed.

A05 is a corrected candidate for independent parent acceptance, not acceptance
of A06-A24 or the whole AC contract.

## Earlier candidate (historical; superseded above)

The parent resolved the DTO ownership mismatch below. A scoped implementation and
runtime evidence now exist on `herdr-cloud-wave1`, based on accepted HEAD
`fe9011d5c5cf86263b7abf42df64984d00cf4749`. This is a candidate, not parent acceptance.

Implemented:

- Separate strict private version-1 `machine-workspaces.v1.json` envelope using
  existing `Epoch` and `Availability` values and explicit mirror exposure.
- Reuse of private atomic JSON writer, followed by checked parent-directory sync
  on Unix. Persistence failures fence subsequent service mutations and return an
  error before registry publication. A post-rename sync failure is an ambiguous
  disk outcome, not a published in-memory success.
- Startup restore before gateway service publication; unavailable roots remain
  rows without mkdir, overwrite, or replacement. Corrupt/newer original bytes
  remain untouched; diagnostic quarantine copies preserve the bytes as JSON.
- IPC IDs remain compatible; machine canonical-root registration allocates one
  `project-<uuid>` identity under the shared mutation gate. Machine-only rows are
  withheld from the mirror registry while future machine APIs remain disabled.
- Daemon unregister now propagates durable failure before session termination.
  Local registry validation rejects both `ssh:` and `daemon:` IDs.

### Exact commands and results

All Cargo commands used `CARGO_BUILD_JOBS=4` and
`CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target`,
from the assigned worktree; none ran concurrently.

1. `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence -- --nocapture`
   - `A05-red.log`: actual runtime failure, `plain registration lost on restart`,
     exit 101, before production edits. Fixture root removed on assertion failure.
   - `A05-compile-correction.log`: implementation compile error from a missing
     String-to-IpcError conversion, retained rather than relabeled as runtime RED.
   - `A05-service-green.log`: service restart and fault scenarios pass.
   - `A05-green.log`: expanded real IPC-handler path passes, 1 test, exit 0.
     Pong precedes registration; IPC task is joined and socket pair closed.
2. `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree::registry::tests -- --nocapture`
   - 7 passed in `A05-regressions.log`.
3. `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib a04_shared_services_tests -- --nocapture`
   - 4 passed, including isolated child-process/PTY/IPC/HTTP fixtures and cleanup
     failure injections, in `A05-regressions.log`.
4. `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`
   - exit 0, `A05-regressions.log`.
5. `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`
   - exit 0, `A05-build.log`.

Diagnostics eventually returned no errors for every changed source/test file
(the first parallel diagnostic wave was canceled/timed out). `git diff --check`
passed. Existing compiler warnings remain in the logs, including notifications,
macOS unsafe blocks, and unused native-terminal helpers; none were suppressed.

### Proof boundaries and review notes

The A05 restart proof reconstructs the actual `DaemonServer` and services in one
test process; it does not restart an OS daemon PID. It registers through the real
IPC handler using a Unix socket pair, not through a canonical daemon socket.
The initial RED used the public service entry point; the final GREEN additionally
exercises IPC. Concurrent alias proof uses an exact Barrier, without sleeps.
Write failure is a real rename failure against a directory, not disk exhaustion.
Permissions were observed as catalog 0600 and directory 0700. Missing-root and
corrupt/newer preservation are asserted at runtime. No machine event stream is
introduced or claimed; A12 owns it. Machine-only registry/session admission and
HTTP projection remain for the disabled later packets.

Remaining acceptance gaps: OS-process restart proof, actual ENOSPC and
parent-directory-sync fault injection, explicit invalid-root restore proof, and
assertion-failure cleanup of the expanded IPC portion (its normal teardown is
verified). The synchronous startup constructor probes during construction;
registration over IPC runs off-thread, while direct service callers must supply
their blocking execution context. These limits are not full A05 acceptance.

### Source and cleanup manifest

Changed source: `src-tauri/src/worktree/registry.rs`,
`src-tauri/src/daemon/workspace_service.rs`, `src-tauri/src/daemon/server.rs`,
`src-tauri/src/daemon/session_service.rs`, `src-tauri/src/remote/mod.rs`.
New source: `src-tauri/src/remote/workspace_catalog.rs`.
New test: `src-tauri/tests/machine_catalog_persistence.rs`.
New evidence: the six A05 logs listed above plus `A05-build.log`; this document
was updated, preserving its historical blocked report below. Parent-owned
independent-verification, contract-correction, and wave-boundary files untouched.

A05 fixture roots in RED and both GREEN logs have `absent=true` receipts. No A05
daemon PID was started; Git init children exited synchronously. The final IPC
socket pair and handler task have explicit closed/joined receipts. The A04 log
records each fixture child reaped, private root removed, listener joined, socket
removed, and each started PTY reaped, including injected cleanup errors. No
developer daemon restart, desktop launch, staging, commit, push, or unrelated
process termination was performed. Only the assigned worktree was edited.

A06-A24 and the whole AC contract remain unaccepted.

---

## Historical blocked report (superseded by parent authorization)

Status: blocked before production edits; not an A05 candidate or acceptance.

## Inspected baseline

- Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave1`.
- Accepted HEAD: `fe9011d5c5cf86263b7abf42df64984d00cf4749`.
- Read the complete approved plan, including D02-D05 and section 4.3.
- Read `worktree/registry.rs`, `daemon/workspace_service.rs`, the server construction and service wiring, `remote/machine_protocol.rs`, and its complete `machine_protocol_lifecycle.rs` child module.
- The registry is in-memory. The workspace service serializes registration and unregister with a parking-lot gate; server construction creates a fresh registry.
- `remote/auth.rs::write_private_json` exists and is crate-visible, so the requested persistence writer can be reused.

## Blocking contract mismatch

The task requires reusing the accepted A02 strict catalog DTOs, explicitly forbids inventing parallel schemas, and excludes the machine protocol files from allowed source changes. This checkout has no catalog persistence DTO or exposure metadata DTO in those modules. `Project`/`Projects` describe response inventories; the lifecycle module ends with proxy descriptor/hash validation, not a catalog schema. They cannot express the required versioned durable envelope and exposure distinctions by themselves.

Parent action required: identify the intended existing catalog DTO location, or authorize adding the missing version-1 catalog/exposure contract within A05 (including its precise source ownership). No prerequisite mutations were redone.

## Verification and evidence status

No RED or GREEN runtime command was run: no production change can satisfy the stipulated DTO reuse boundary until that mismatch is resolved. No runtime logs, restart proof, permissions proof, or passing-build claims are supplied. A source inspection is not runtime RED. A06-A24 and the whole AC contract remain unaccepted.

## Source and cleanup manifest

- Production files changed: none.
- Test files changed: none.
- Evidence created: this document only.
- Pre-existing untracked files left untouched: `WAVE1-prerequisites.json`, `WAVE1-remaining-boundaries.md` in this evidence directory.
- Fixture PIDs started: none.
- Fixture roots, sockets, runtime directories, and homes created: none.
- Cargo processes started: none.
- Developer daemon, desktop, and other worktrees modified: none.
- No staging, commit, push, or process termination performed.
