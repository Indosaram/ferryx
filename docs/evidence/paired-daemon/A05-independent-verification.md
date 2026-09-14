# A05 independent verification: corrected candidate

Date: 2026-09-12. Verifier: `st_01a09517`.
Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave1`.
Base and observed HEAD: `fe9011d5c5cf86263b7abf42df64984d00cf4749`.

## Current scoped verdict

**No blocking finding in the corrected A05 candidate. B1 and B2 are resolved by current source and independently passing narrow runtime tests.** Durable catalog behavior, actual isolated owner-process restart, fault fencing, permissions, unavailable-row preservation and compatibility have observable evidence. This is an A05-only verification recommendation, not parent acceptance, production rollout approval, or acceptance of A06-A24/AC01-AC12.

This section supersedes the historical verdict below. The earlier report and its original runtime logs are retained unchanged after this section; their B1/B2 findings describe the previous candidate, not the corrected source.

## Review and source identity

Read the complete approved 833-line plan, original A05 producer packet and parent corrections from `st_01a094fd` and `st_01a0950c`, `A05-parent-candidate-review.md`, the complete implementation handoff, current producer RED/GREEN/build/Local/A04 evidence and prior independent report. Read every actual changed source/test file in full: registry, workspace service, server, session service, remote module declaration, private catalog, and integration fixture. Reviewed the complete tracked diff against the specified base and the two new source/test files. Traced the SSH caller through GUI command, daemon client, IPC dispatch, session authority and ownership release; traced production CLI construction, gateway readiness, private writer and WorktreeManager probing.

All seven current source/test SHA-256 values matched `A05-final-manifest.log` both before and after independent execution. No source/test edits were made. Private catalog envelope/exposure creation in `remote/workspace_catalog.rs` is explicitly authorized; it reuses Availability and Epoch without duplicating the public Project response or editing machine_protocol.

## B1/B2 correction proof

- **B1 resolved:** `daemon/session_service.rs:462-475` retains the spawn serialization lock, recognizes supported `ssh:` IDs, and bypasses only the local catalog operation. The unchanged ownership/session loop at 477-500 still closes live owned sessions or releases expired ownership. Local catalog errors propagate; `daemon:` still reaches the rejecting local validator. The real caller at `ipc/project.rs:274-290` can therefore proceed to SSH inventory removal after daemon success. `daemon/client.rs:722-738` and server IPC dispatch preserve errors. The independent regression invoked the actual async unregister entry with expired SSH metadata, observed ownership removal, no catalog creation, and rejection of `daemon:desktop`. It contacts no SSH host; this is not a live SSH transport test.
- **B2 resolved:** `daemon/server.rs:889-900` allocates a fresh owned TempDir for no-config-path `cfg(test)` constructors. The same isolated directory backs the catalog, remote-session persistence and SSH store; `_catalog_fixture` at 774-775/1023-1024 retains its lifetime until server drop. Explicit-path construction and non-test production storage are unchanged. The child regression at 782-829 uses a private canonical corrupt sentinel under `FERRYX_DATA_DIR/remote`, constructs both `new()` and `new_with_paths(None,None)`, registers the same ID against distinct roots, verifies independent managers, leaves sentinel bytes unchanged, and verifies both temporary directories removed. It passed independently before any other unit target was run. This closes catalog isolation, not a blanket claim that every historical daemon test is safe to run against developer state.

## Independently executed commands

Producer `st_01a0950c` was completed at `2026-09-12T10:08:48.911Z` before execution. No producer/parent Cargo or catalog fixture was running in the assigned worktree. An unrelated existing rust-analyzer Cargo check in the main worktree was observed during inspection and was not touched. The four verifier commands ran serially, once each, on macOS arm64, with:

```sh
CARGO_BUILD_JOBS=4
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target
```

Each command uses `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features`, followed by the exact arguments below. Each complete log records its command and actual exit code and was read in full.

| Arguments | New independent evidence | Result |
| --- | --- | --- |
| `--lib a05_compatibility_tests -- --nocapture` | `A05-independent-final-blockers.log` | Exit 0; 3 passed, including a child entry that is a no-op in the parent; isolated child separately ran 1 passing test. |
| `--test machine_catalog_persistence -- --nocapture` | `A05-independent-final-runtime.log` | Exit 0; 4 passed, including the child-owner entry and three behavioral tests. |
| `--lib catalog_tests -- --nocapture` | `A05-independent-final-sync.log` | Exit 0; 1 passed. |
| `--lib worktree::registry::tests -- --nocapture` | `A05-independent-final-registry.log` | Exit 0; 7 passed. |

`git diff --check` also returned 0. Compiler warnings remain visible, including existing macOS unsafe blocks, unused imports/variables and unused helpers; none was suppressed. No independent build, LSP, native UI, Linux or Windows execution is claimed. Producer final CLI/relay check/build logs and exit receipts show 0, as do the three affected Local tests and four A04 regressions; these were read, not rerun by this verifier. No old prerequisite mutation or broad daemon suite was executed.

## Actual restart and failure-path observations

`tests/machine_catalog_persistence.rs:38-111` runs distinct owner processes, each constructing the real DaemonServer with explicit private paths and exercising its real IPC handler through platform-neutral Tokio duplex. It does not start the canonical listener or merely reconstruct services in a single PID.

1. Owner **90754** completed exact Ping/Pong readiness, received `register`, registered plain/Git roots through RegisterWorkspace/RegisterWorkspaceOk, emitted completion, exited 0 and was waited/reaped.
2. Only after that wait, owner **90944** constructed a fresh server from the same catalog, completed Ping/Pong, verified both restored registrations, exited 0 and was waited/reaped.
3. Owner **91013** reached readiness and exercised the injected parent failure. It was killed and reaped. This log includes `failed printing to stdout: Broken pipe (os error 32)` on that deliberately failed owner: closing its stdin/stdout before kill can let it reach its completion print. This is not the earlier normal-owner BrokenPipe failure; both normal owners exited 0. The injected path asserts its exact parent-side error and reaping, not a timing-dependent child exit reason.
4. The separate intentional IPC task panic (`injected IPC assertion after Pong`) was captured as a JoinError. Client and handler were joined and duplex endpoints closed before the test passed.

Readiness/completion subscribe/read exact `A05_READY` and `A05_DONE` signals with bounded deadlines and drain normal child output to EOF. IPC now matches each response to its request (`Ping => Pong`, `RegisterWorkspace => RegisterWorkspaceOk`, lines 18-22). No sleep/polling delay determines success. Children explicitly set HOME, FERRYX_DATA_DIR, FERRYX_RUNTIME_DIR, TMPDIR and XDG_RUNTIME_DIR. Git-init children are waited synchronously.

## A05 acceptance trace

| Requirement | Current mechanism and observed proof |
| --- | --- |
| Private durable commit precedes publication/success | `workspace_catalog.rs:53-64` reuses `auth.rs:916-966` for private directory/file, same-directory temporary file, file sync and rename, then checks Unix parent sync. Workspace register/machine-register/unregister (`workspace_service.rs:70-79,98-103,113-121`) persist before replacing catalog state or publishing/removing a registry manager. Failure stores an error fence and returns no success. Independent actual rename failure and post-rename sync injection passed; the latter preserves ambiguous disk candidate without false in-memory publication. |
| One daemon mutation gate | Registration, canonical machine registration and unregister lock `mutation_gate` at service lines 41,87,108. IPC delegates through run_blocking; session spawn takes the same gate inside its blocking admission closure (`session_service.rs:661`). Startup restore is single-threaded before service publication. No alternate current daemon registration writer was found; GUI registry is a separate frontend-side registry, followed by daemon IPC. No parking-lot registry guard crosses await/network I/O. |
| Canonical aliases and compatible IPC IDs | WorktreeManager canonicalizes roots and resolves nested Git roots. Machine register adopts a canonical-root ID under the gate or allocates `project-<uuid>`. Independent barrier-driven alias/root requests returned one ID, `project-7ab3db3d332e4366852466c78c630bf4`, with mirror exposure false. IPC normalized/idempotent regression passed unchanged revisions and explicit exposure promotion. Existing unchanged managers are not replaced. |
| Unavailable and invalid rows survive | Restore recomputes availability without mkdir or overwriting roots (`workspace_service.rs:17-30`). Independent missing-directory and directory-replaced-by-file scenarios retained rows, Missing/Invalid status and sentinel bytes. |
| Corrupt/newer catalog preservation | Strict load validates schema/version, IDs and absolute non-root paths; failure preserves original bytes and writes a private diagnostic quarantine byte-array copy (`workspace_catalog.rs:26-50`). Both malformed JSON and version 2 reject mutation without replacing original data. |
| Reserved identities | `registry.rs:18-21` rejects `ssh:` and desktop `daemon:` before local-path operations. Integration checks registration, manager lookup and unregister refusal. Session-level SSH cleanup dispatch does not weaken this guard. |
| Exposure/legacy compatibility | Private rows explicitly persist mirror exposure. Machine-only rows are withheld from legacy registry insertion and restoration. Local IPC can deliberately promote exposure. No new machine event stream or raw-path mirror broadcast is added. |
| Startup/headless | `cli.rs:485-514` creates a multi-thread Tokio runtime and calls `DaemonServer::new`. `server.rs:876-885` uses block_in_place; lines 910-920 join a dedicated catalog/fs/Git worker before gateway construction/readiness. Direct synchronous constructors wait for that worker. No AppHandle or catalog-triggered PTY creation is introduced. |
| Future capability boundary | `remote/server.rs:1967-1985` still returns an empty machine capability array; UDS exposes only existing machine pairing. A06-A12 HTTP, journal, worktree/session and event functionality is not enabled or claimed. |
| No replacement PTYs | Catalog restoration constructs managers only. The restart fixture sends no Spawn request and creates no PTYs. This proves registration restoration without terminal creation, not later packet live-terminal recovery or survival. |
| Private permissions | Independent runtime metadata assertions observed catalog mode 0600 and parent mode 0700. No physical ENOSPC or power-loss simulation is claimed; the agreed actual write-failure alternative is exercised. |

Historical original RED is a genuine `plain registration lost on restart` service-reconstruction failure (exit 101), not an OS-owner RED. Corrected distinct-owner GREEN now supplies the required real restart evidence. Producer B1/B2 RED logs preserve both the initial misplaced sentinel attempt and the corrected canonical-sentinel failure; current independent GREEN verifies the fixes. PermissionDenied fault injection and non-macOS execution remain unexecuted. Those boundaries do not turn this scoped A05 verdict into release acceptance.

## Original resources and teardown

Read-only process inspection before and after independent execution observed unchanged pre-existing application PIDs:

```text
493   /Applications/Ferryx.app/Contents/MacOS/ferryx
36170 /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon --handover-from /tmp/rorca-501/legacy-65513-1789049955335.sock
```

Neither was connected to, restarted, signaled or modified. No OS socket, network server, desktop or PTY was started by these independent A05 tests. Handover construction stores a path but the fixture never calls run_server. No canonical catalog was accessed by the no-path unit regression; its sentinel exists only within its private child root.

All paths below are beneath `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/`. In addition to log teardown receipts, the verifier directly checked filesystem absence and used signal 0 to verify owned PID absence after execution:

| Resource | Final observation |
| --- | --- |
| `.tmpphqmQ2`, including `.tmpkepdjQ` and `.tmpUyQ3v2` | Constructor owner/sentinel root and both private constructor directories absent. |
| `.tmpS25Ucd` | SSH compatibility root absent. |
| `.tmpdNyHAm` | Distinct-owner restart root absent. |
| `.tmpGpMhTX` | Service, alias, missing/invalid and corrupt/fault root absent. |
| `.tmpWLtE2j` | Normalization/exposure root absent. |
| `.tmp4LJArb` | Post-rename sync fault root absent. |
| PID 90361 | Constructor child exited successfully, waited/reaped, independently absent. |
| PIDs 90754 and 90944 | Register/restore owners exited 0, waited/reaped, independently absent. |
| PID 91013 | Injected failure owner killed/waited/reaped, independently absent. |
| IPC tasks/endpoints | Normal and injected paths joined/closed; no OS sockets. |

All four verifier Cargo invocations returned 0; final process inspection showed no Cargo/catalog fixture remaining. Registry-test temporary directories use existing TempDir RAII; their unnamed individual paths were not separately logged or independently checked. Source hashes remained unchanged. Verifier writes are limited to this report and the four new independent logs in the assigned evidence directory. No source edits, commits, staging, deployment or other-worktree modifications occurred.

**Conclusion:** current A05 has no blocking finding from this independent review. B1/B2 corrections and actual private owner restart are proven; parent retains the final acceptance decision.

---

# Historical A05 independent verification: changes required (superseded)

Date: 2026-09-12. Verifier: `st_01a09506`.
Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave1`.
Base and observed HEAD: `fe9011d5c5cf86263b7abf42df64984d00cf4749`.

## Verdict

**Do not accept the current candidate: two source-backed compatibility/isolation blockers remain.** The durable catalog and corrected isolated owner-process restart are present and independently pass. This is not the historical missing-DTO or provider/DNS blocker. Parent acceptance remains separate.

### B1. SSH project removal is now rejected before existing cleanup

- Changed `src-tauri/src/daemon/workspace_service.rs:107-110` makes unregister propagate `validate_workspace_id` failure. That validator deliberately rejects `ssh:` (`worktree/registry.rs:18-21`).
- Changed `daemon/session_service.rs:462-469` propagates this error before the session cleanup loop at lines 471-494.
- The existing real caller, `ipc/project.rs:274-288`, first awaits daemon unregister, then removes the SSH inventory entry. `daemon/client.rs:722-738` turns the daemon error into an IPC failure. Consequently ordinary SSH project removal fails and never reaches SSH inventory/session cleanup.
- At base `fe9011d5`, workspace-service unregister returned unit after the registry refused the local-path operation; session cleanup and the caller's SSH inventory removal still proceeded. The new error propagation changes that behavior.

Preserve rejection of remote IDs by **local-path registry operations**, but dispatch supported SSH unregister at the session-authority boundary without attempting a catalog mutation. Preserve durable errors for actual local catalog removals. Add a narrowly scoped SSH-unregister compatibility regression. This finding is established by complete caller tracing and baseline comparison, not a claimed live SSH reproduction; no SSH host was contacted.

### B2. Existing no-path unit constructors now read/write the canonical catalog

- `daemon/server.rs:807-825` chooses `canonical_identity_dir()/machine-workspaces.v1.json` when `config_path` is absent, including `cfg(test)` builds. The test-specific gateway branch at lines 855-862 does not isolate this newly added catalog.
- Existing tests in this same file call `DaemonServer::new()` and register temporary roots under repeated IDs: for example lines 2425-2431, 2452-2465, 2499-2503, 2565-2569 and 2831-2835. These previously operated on a fresh in-memory registry. Now they share persistent daemon state and can conflict with each other, retain stale roots between runs, or modify the user's canonical catalog.
- `remote/auth.rs:32-48` resolves that location from the actual `FERRYX_DATA_DIR` or user HOME; it is not a per-test temporary location. Even an externally isolated shared HOME leaves concurrent test instances with independent in-memory catalogs and repeated IDs writing the same file.

Make the no-path test constructor own an isolated catalog lifetime, or migrate the affected callers to explicit fixture-owned paths with cleanup. Do not run the existing broad daemon unit suite against the current constructor. I did not execute those unsafe tests or touch the canonical catalog to demonstrate the issue. The focused A05 target uses explicit paths and does not have this problem.

## Material reviewed and source identity

Read the full approved 833-line plan, original A05 scope and correction packet in completed producer task `st_01a094fd`, `A05-parent-candidate-review.md`, the full implementation handoff, all A05 producer/parent logs, and the historical independent report. Read every actual changed source file in full: registry, workspace service, server, session service, remote module registration, new catalog module, and integration test. Reviewed the complete diff against the specified base. Traced private persistence, WorktreeManager canonicalization, production CLI startup, capabilities, GUI project registration/removal, daemon client removal and handover construction.

All seven actual source/test SHA-256 values independently matched `A05-corrected-manifest.log` after execution. No source changed during this verification. The private catalog envelope/exposure types are authorized in `remote/workspace_catalog.rs`; existing Availability/Epoch are reused. No machine-protocol edit or duplicate public Project response was required.

## Independently executed runtime

Producer status was `completed` at 09:49:54Z before execution. Preflight showed no Cargo/catalog fixture process. Commands ran serially from the assigned worktree on macOS arm64, using the private warm target and four build jobs:

```sh
CARGO_BUILD_JOBS=4 \
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target \
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --test machine_catalog_persistence -- --nocapture

CARGO_BUILD_JOBS=4 \
CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target \
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --lib catalog_tests -- --nocapture
```

| Independent evidence | Result |
| --- | --- |
| `A05-independent-runtime.log` (read in full) | Exit 0; 4 passed, 0 failed. Three behavioral tests plus the child-owner entry test, which is a no-op without its child environment. |
| `A05-independent-sync.log` (read in full) | Exit 0; 1 passed, 0 failed; post-rename sync failure leaves candidate on disk, no in-memory publication, later mutation fenced. |
| `git diff --check` | No output/errors. |

The restart test is a real process restart: owner PID **71096** constructs the actual DaemonServer, completes IPC Ping/Pong, receives the register command, registers plain/Git roots through the actual handler over Tokio duplex, completes and exits 0. Only after wait/reap does PID **71266** construct a fresh server from the same catalog, complete Ping/Pong, verify both restored registrations, and exit 0. This is not service drop/reconstruction in one PID. No canonical listener, desktop or network server is launched.

Failure-path owner PID **71323** reaches the same readiness boundary, receives the deliberate parent-side failure, is SIGKILLed and reaped. The separate injected IPC task panic occurs at the expected assertion after Pong, is captured as a task failure, and client/handler endpoints are joined/closed before the test passes. The panic text in the log is intentional, not a hidden failing test. Readiness/completion use exact `A05_READY`/`A05_DONE` signals, bounded deadlines and EOF drain, not sleeps. The fixture's IPC matcher accepts Pong or RegisterWorkspaceOk for each response rather than matching each request individually; the actual server dispatch is correct and restore additionally verifies persisted registrations.

## Observed A05 mechanisms and boundaries

| Requirement | Evidence and assessment |
| --- | --- |
| Private atomic commit before publish/success | `workspace_catalog.rs:53-64` reuses `auth.rs:916-966`: owner-only directory/file, same-directory temporary file, file sync and rename, followed by an additional checked Unix parent sync. `workspace_service.rs:70-79,98-103,113-121` commits before state/registry publication or success. Failure stores an error fence. Independent real rename failure and injected post-rename fault both passed. No physical ENOSPC or power-loss simulation claimed. |
| One mutation gate | Register, machine-register and unregister take the same service mutex (`workspace_service.rs:41,87,108`); spawn uses it (`session_service.rs:658`). Current daemon registration writers route through this service; restore is single-threaded before service publication. GUI `ipc/project.rs` uses a distinct GUI registry, then daemon IPC, not an alternate daemon catalog writer. |
| Canonical alias identity | `register_machine` probes through WorktreeManager, finds canonical root and inserts UUID identity under the gate. Independent barrier-driven symlink/root concurrent requests returned `project-2388f88b4d7444dd9f15728af7ff191e`; no duplicate alias identity, mirror exposure false. |
| IPC normalization/idempotency | Independent regression passed trimmed ID lookup, unchanged registry/catalog revisions and explicit machine-to-mirror exposure promotion. Existing manager is not replaced on unchanged binding. |
| Unavailable roots | Missing root stays Missing without mkdir; Git directory replaced by a file stays Invalid with sentinel bytes unchanged. Independent runtime passed both; usable roots alone become registry managers. |
| Corrupt/newer preservation | Strict envelope load preserves original bytes, writes a private diagnostic byte-array quarantine copy and returns a persistent mutation error. Both malformed JSON and version 2 were independently exercised without overwrite. |
| Reserved IDs | Registry validator rejects both `ssh:` and `daemon:`. Independent fixture checks registration/manager refusal and unregister false. This correct local-path guard must not be confused with the broken higher-level SSH cleanup in B1. |
| Exposure/legacy projection | Catalog persists explicit mirror exposure; machine-only rows are withheld from the legacy registry during insertion/restore. IPC can explicitly promote exposure. No new raw-path machine event broadcaster is introduced. |
| Future capabilities | `remote/server.rs:1967-1985` still returns empty machine capability array; UDS advertises only existing machine pairing. A06-A12 HTTP/session/event functionality is not enabled or claimed. |
| Startup/headless | `cli.rs:485-514` builds a multi-thread Tokio runtime, invokes `DaemonServer::new`; `server.rs:796-837` uses block_in_place and a joined worker for catalog/fs/Git restoration before gateway construction/readiness. No AppHandle or PTY spawn is introduced. Direct synchronous constructors still block their caller while joining; production CLI takes the block_in_place branch. |
| No replacement PTYs | Catalog restore only builds managers. The process fixture creates no PTYs and sends no Spawn request; it does not claim live-session survival. Existing SSH restore is separate. No process replacement can be inferred from a catalog row. |

The no-replacement result here is construction/path evidence plus a fixture that creates no terminals, not a terminal-expiry/recovery test from later packets. PermissionDenied is represented in source but was not independently fault-injected. Machine-only rows survive subsequent service reload through the catalog, but the distinct-owner restart assertions specifically cover IPC-exposed plain/Git roots.

## Producer evidence versus independent evidence

Read corrected registry log (7 passed), A04 regression log (4 passed, real shared IPC/HTTP ownership and cleanup), CLI/relay check/build logs (exit receipts 0), and all historical A05 logs. These are producer executions, not commands rerun by this verifier. No A04 prerequisite mutation was rerun. Compiler warnings remain visible (unused imports/variables, existing macOS unsafe blocks and unused helpers); none was suppressed.

Original `A05-red.log` is a genuine failing service-reconstruction assertion, `plain registration lost on restart`, exit 101. It was not an OS-owner restart RED. The corrected distinct-owner GREEN above closes the runtime restart proof, not the historical RED's process-boundary limitation. `A05-correction-red.log` records the real normalized-ID failure. The intermediate `A05-correction-green.log` correctly remains a failure (BrokenPipe, owner exit 101); current code drains owner stdout through EOF and the independent run passed on its first invocation.

No independent LSP/build/native UI/Linux/Windows run is claimed. This is a read-only verification task with two exact focused runtime runs, not broad suite expansion. Platform-neutral duplex replaces unconditional UnixStream; the symlink alias portion remains Unix-gated. Parent's broader acceptance and later release/platform gates remain open.

## Original resources and teardown

Existing application resources were not opened, connected to, signaled or modified. Read-only process inspection observed the same pre-existing desktop PID **493** and daemon PID **36170** described in the historical report:

```text
493   /Applications/Ferryx.app/Contents/MacOS/ferryx
36170 /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon --handover-from /tmp/rorca-501/legacy-65513-1789049955335.sock
```

The A05 children use a private HOME/data/TMPDIR and explicit constructor config/auth paths. `XDG_RUNTIME_DIR` is set by the fixture, but Ferryx actually resolves `FERRYX_RUNTIME_DIR`; that variable is not set by this fixture. No runtime socket operation occurs: HandoverManager::new only stores the computed socket path, and `run_server` is never invoked. Thus the canonical path is not bound/read/removed. Sockets: none; transport: in-memory duplex. Git init children synchronously exit via status().

Full private root prefix: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/`.

| Resource | Teardown observed and independently checked |
| --- | --- |
| Owner root `.tmpuv5MFR` | Removed; filesystem absence confirmed. |
| Service/fault root `.tmpRuX4Vd` | Removed; filesystem absence confirmed. |
| Normalization root `.tmpSYLJv1` | Removed; filesystem absence confirmed. |
| Sync-fault root `.tmpLPr0dI` | Removed; filesystem absence confirmed. |
| PID 71096 / 71266 | Both exit 0, wait/reaped; kill(pid, 0) subsequently reports absent. |
| PID 71323 | Injected failure, SIGKILL/wait/reaped; subsequent absence confirmed. |
| IPC tasks/endpoints | Joined and closed on normal and injected assertion paths. |

Both verifier Cargo invocations returned 0. No owned Cargo/fixture process remains. A later process listing showed an unrelated `maho-ffi` Cargo build; it was not this target and was not touched. No source/test edits, staging, commits, desktop launch, network deployment, canonical daemon restart, or other-worktree modifications occurred. Verifier outputs are this report and its two complete runtime logs in the assigned evidence directory.

**Scoped conclusion:** durable persistence, private permissions, alias atomicity, unavailable/corrupt preservation and actual isolated process restart have concrete passing evidence. A05 is nevertheless not ready for acceptance until SSH unregister compatibility and canonical-catalog unit-test isolation are corrected. No new feature or source fix was made by this verifier.
