# Wave1 resume: acceptance evidence and remaining gaps

Task `st_01a097fd`; 2026-09-12. Read-only reconciliation, not a new verification gate or implementation approval.

## Basis and disposition

The inherited Darwin headless implementation is an accepted **scoped dependency baseline**, not completed A06/A07/A08 packets or full release acceptance. Do not repeat the completed directory, project, worktree, process-drain, publication, or typed-owner repairs. The blanket pending status in `WAVE1-remaining-boundaries.md:3-5` is historical. Its requirements still apply, but its implementation status predates the final composed receipts.

Authoritative requirements inspected: `/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md`, all 833 lines, particularly sections 4.2-4.4, 4.6, 6.1, 8, packets A06-A08 at lines 531-565, and sections 10-12. A passing node, a passing filter, a repaired defect, and full packet acceptance are different claims.

All source and historical evidence references below resolve under **`/Users/indo/code/project/orca-lite-wt/herdr-wave1`**. `E/` means `docs/evidence/paired-daemon/`; `S/` means `src-tauri/src/`; `T/` means `src-tauri/tests/`. Use that stable snapshot, not concurrently edited resume source, to interpret this report. The user supplied the byte-for-byte inheritance relationship; this investigation did not independently compare worktrees or validate new A09 changes. No tests, builds, daemons, PTYs, desktop actions, or independent gates were run. Only this report was written.

### Evidence precedence

1. `E/BATCH1-parent-final-verification.md` accepts the final composed scoped repairs and explicitly retains packet/platform/native/capability gates. Its 34-input before/after identity statement supersedes older composition descriptions; it is historical verification, not a new hash check here.
2. `E/A08-typed-owner-verification.md` and `E/A08-parent-typed-owner-verification.md` close the three residual generic owner error mappings. They supersede the typed-owner blocker in `E/A08-followthrough-verification.md`, not that report's external or fault-coverage limits.
3. `E/A08-followthrough-verification.md` supersedes the concrete path/prune/JoinError and transaction gaps in `E/A08-repair-batch-verification.md:91-101`; `E/A08-parent-partial-recovery-review.md` precisely limits prune/write recovery evidence.
4. `E/BATCH1-generation4-verification.md` closes early producer directory/project omissions; `E/BATCH1-crash-barrier-parent-verification.md` supersedes the nondeterministic interrupted-write evidence found by `E/BATCH1-generation4-parent-verification.md`.
5. `E/R3-reset-v2-parent-review.md` plus `E/R3-wave1-composition.md` close the R9/R3 **test determinism** repair. They expressly do not establish an eager-upload production transport fix.

## Inspected command/exit/artifact ledger

Commands in this ledger were executed by the historical owners, **not in this investigation**. Actual relevant log bodies were inspected here; monitor exits are attributed to the named report when the raw log does not contain an exit marker. Counts describe selector outputs, not independent scenario counts.

Common final environment, from `E/BATCH1-parent-final-verification.md:11-28`: CWD `herdr-wave1`, `CARGO_BUILD_JOBS=4`, `CARGO_TARGET_DIR=$PWD/src-tauri/target`, empty `RUSTC_WRAPPER`.

| Ref | Exact historical command | Exit and inspected artifact |
| --- | --- | --- |
| F1 | `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture` | Exit 0 per final parent monitor `mon_D13KETKJMKJTV9VJ` / `bash_313`; `E/BATCH1-parent-final-remote.log`: 254 passed, including actual R3, directory, project, worktree authority, typed-owner and crash scenarios. |
| F2 | `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_catalog_persistence --test machine_worktrees --test worktree_safety --test machine_worktree_legacy_bounds --test machine_worktree_transports --test relay_pairing_generation_regression -- --nocapture` | Exit 0 per same monitor; `E/BATCH1-parent-final-integration.log`: catalog 4, worktrees 2, safety 9, legacy 2, transports 2 outer tests, relay 6. Child selectors are not extra independent proofs. |
| F3 | `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree:: -- --nocapture` | Exit 0 per same monitor; `E/BATCH1-parent-final-worktree.log`: 44 passed, real byte-fidelity, bounded Git, all seven hook modes, explicit JoinError child wait. |
| F4 | `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay` and `git --no-pager diff --check` | Exit 0 reported in `E/BATCH1-parent-final-verification.md:15-33`; build artifact named there is `E/BATCH1-parent-final-build.log`. Build outcome is report-attributed here, not independently rerun or used as native proof. |
| O1 | `src-tauri/target/debug/deps/ferryx_lib-a8f661fd9c0ae1d6 --exact remote::workspace_api::worktrees::authority_tests::typed_owner_repair_private_uds_and_native_adapter --nocapture` | `E/A08-parent-typed-owner.log`: 1 passed; exit 0 / `PARENT_TYPED_OWNER_EXIT=0` in `E/A08-parent-typed-owner-verification.md`, monitor `mon_7TJQV7ATMWDVHVDR` / `bash_312`. |
| P1 | `src-tauri/target/debug/deps/ferryx_lib-a8f661fd9c0ae1d6 followthrough_prune_partial_replays_after_restart --nocapture` | `E/A08-parent-partial-prune.log`: 1 passed; exit 0 / `PARENT_PRUNE_EXIT=0` in `E/A08-parent-partial-recovery-review.md`, monitor `mon_8XBB07RGE4KCWCA0`. |
| P2 | `src-tauri/target/debug/deps/ferryx_lib-a8f661fd9c0ae1d6 followthrough_write_failures_non_head_and_prunable_preview --nocapture` | `E/A08-parent-write-recovery.log`: 1 passed; same monitor, `PARENT_WRITE_RECOVERY_EXIT=0`. |
| G1 | `bun docs/evidence/paired-daemon/BATCH1-generation4-verification-artifacts/runtime-hardened.mjs` | Exit 0 in `E/BATCH1-generation4-parent-verification.md`, monitor `mon_K1BAQ5DFA5R34FZ3`; actual `E/BATCH1-generation4-parent-runtime.log` contains directory direct/relay equality, project replay/restart, busy PTY, non-destructive unregister and cleanup. |
| G2 | `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::workspace_api_tests -- --nocapture` | `E/BATCH1-crash-barrier-parent-tests.log`: 11 passed; exit 0 in `E/BATCH1-crash-barrier-parent-verification.md`, monitor `mon_C2M2N8JAP6B4FDPG`. Corrected retained-stdin crash barrier, not the earlier racy generation4 run. |
| H1 | `CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=$PWD/src-tauri/target RUSTC_WRAPPER= cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::server::tests::r3_http_boundary_contract -- --exact --nocapture` (CWD `herdr-batch1-http-repair`) | `E/R3-reset-v2-parent.log`: 1 passed, 24 wire + 18 full/stream checks; exit 0 / `PARENT_R3_V2_EXIT=0` in `E/R3-reset-v2-parent-review.md`, monitor `mon_X91199C7EBNTNTST`. Final composition independently includes the same test in F1. |

Additional inspected artifacts: `E/BATCH1-generation4-parent-lost-reply.log` records the actual intercepted relay 201, zero response bytes delivered, owner 83662 shutdown/reaped, owner 83728 replay of `project-2aa977be658643b6aa54dc4ee092b5ba`, revision 1, both exit 0, root removed. The enclosing aggregate exit 0 is recorded in `E/BATCH1-generation4-parent-verification.md` (`mon_QF9CDA8PS7WE1G93`). `E/BATCH1-repair-verification-artifacts/capacity.log` records seeded pending=10,000 and resultExpired=100,000 boundary children 6852/6853, each exit 0/reaped and roots removed; inspected `capacity.mjs` runs its sibling `capacity` executable under private paths. This is seeded-store admission/replay evidence, not 100,000 real mutations or worktree-specific stress.

## 1. A06 - authenticated directory API

**Disposition: scoped Darwin backend and real relay browsing proven; full packet not accepted. No demonstrated residual directory source defect in the inspected repair scope.**

Proven requirements:

- Native, off-thread directory browsing with home/default/tilde, parent, outside-home scope; directories-only and accessible symlinks; canonical paths; hidden policy; spaces, quotes, Unicode and POSIX backslashes; malformed/duplicate query, invalid encoding/control/relative/oversized input refusal; no shell listing. `S/remote/filesystem.rs` was read in full: `query`, `resolve_directory`, `scan_after_resolution`, `directories`. F1 executes the native/HTTP tests; G1 records actual owning fixture home direct=relay=200 and invalid UTF-8=400. This closes the early lane's “relay not exercised” omission without claiming direct access was blocked by the OS.
- 1,000-entry bound, deterministic retained-name sort, 256 KiB byte limit and truncation. The old `E/BATCH1-A06-final-producer-report.md` byte-cap omission is **stale**: F1 logs `created=800 retained=473 json_bytes=261829 next_bytes=553`, and `E/BATCH1-generation4-verification.md` records the independent byte-cap case. Expired empty scan and cancellation after resolution are also exercised.
- Auth before probe, machine-Control only, service-less no-store 503, request-drop/revocation fences, worker-slot retention, bounded admission and responsive health during blocked auth. F1 logs fourth listing slot retained on disconnect, 16 blocked auth workers, 504 with slots retained until release, exact 18/18 drain, 401/403 precedence, listener/root cleanup. The source keeps permits inside workers rather than releasing them with the HTTP wrapper.
- The old CLI assertion “machine capabilities must remain empty” is stale: `E/BATCH1-generation4-verification.md` R5 records the corrected exact directory-only CLI capability test. Current `S/remote/server.rs:1982-2000` advertises only `directoryBrowseV1` for machine-Control with services.

Missing requirements / limits:

- Linux arbitrary-byte filenames on disk and Windows build/native drive-root/UNC/hidden-attribute/ACL behavior remain unexecuted. Darwin invalid-byte index output in A08 is not Linux directory-entry evidence. Windows branches exist in `filesystem.rs`; cfg presence is not compilation or runtime proof.
- 10,000 inspected-child and 10/sec burst-20 **exhaustion** are source-backed, not independently exhausted in the cited generation4/final receipts. Source explicitly contains both limits; do not call them absent implementation.
- Blocking OS filesystem calls cannot be forcibly interrupted. The 5s scan and 10s HTTP budget fence results, retaining slots until the OS call returns; they are not a hard wall-clock syscall termination guarantee.
- “Canceled old request cannot publish a later picker result” is only proven at the server request/worker boundary, not the host-generation/native picker surface. A14/A18 own client generation and cache adoption; A06 cannot be fully accepted from server cancellation alone (plan sections 7.1 and A18; early producer report explicitly delegates it).

Smallest remaining scopes: Q1 (two unexhausted backend budgets), Q4 (platform evidence), Q5 (native client/picker integration). Do not reimplement browsing or redo the completed byte-cap repair.

## 2. A07 - project registration/list/unregister

**Disposition: canonical project/journal/guarded-unregister dependency proven; initial producer omissions largely stale. Full events/capability/native/platform contract remains open.**

Proven requirements:

- Real 201 registration, same-request original 201/body replay, canonical alias/two-device existing 200 identity, digest conflict, root refusal, plain folder and nested unborn Git semantics, unavailable rows retained, revision checking and non-destructive 204 unregister. `S/remote/workspace_api.rs` was read in full (`path`, `project`, `list`, `mutate`, `operation`); F1/F2 exercise these paths. G1 logs permissionDenied and missing inventory rows, `PROJECT_BUSY` 409 for original shell 56659 at the registered root, explicit Close then absent/alreadyReaped, and idle unregister leaving disk intact. This is shared owner spawn after explicit mirror exposure, **not A09 machine session CRUD**.
- One daemon journal, private durable intent before side effect, device/request/digest identity, conservative unknown replay, catalog receipt recovery, seven-day compaction and retained tombstones. `S/remote/machine_operation_journal.rs` was read in full; `S/daemon/workspace_service.rs:30-36` opens that journal and recovers catalog receipts before service publication. F1 executes journal restart/device scope. The capacity artifact above closes “no capacity test” only at seeded admission boundaries.
- Actual owner process restart and zero-byte lost reply are no longer omitted: G1 and the lost-reply artifact above establish them. G2 and F1 establish all four afterCatalog/afterJournal register/unregister kill-and-replay windows. The original generation4 stdin-EOF race is superseded by G2: owner PIDs 18536/18879/18956/19223 were SIGKILL/waited before replay PIDs 18835/18927/19208/19266; no unexpected barrier release. Do not repeat R8 implementation.
- Routed auth/body deadline and extraction-envelope gaps are repaired: `S/remote/server.rs:2033-2046,2105-2117` and `S/remote/workspace_api.rs::admit_until/execute` carry admission through extraction/domain work; F1 contains incomplete-body, auth-slot, revocation and exact-envelope checks. The old “Git timeout maps to invalid” note is stale: `run_child` now bounds 65,536-byte pipes, uses min(5s, request deadline), kills/waits on failure, and returns TIMEOUT; F1 logs that real child scenario.
- Topology is refreshed after waiting for the shared gate; F1 logs enclosing-parent Git => 400/no rows and same-root Git => 201/current metadata. Catalog permissions/quarantine/write fencing are in F2, with actual catalog=600/parent=700 and owner restart receipts.

Missing requirements / limits:

- External projectRegistered/projectRemoved/availability events and client reconciliation are not proved by internal state or the legacy event WebSocket sentinel. Current project `mutate` persists/reconciles outcomes but is not an A12 machine event implementation. `machineWorkspaceV1` remains unadvertised. Preserve the A12 dependency rather than marking all section 4.3 requirements complete.
- Native Add Project/reference adoption, desktop restart/visibility, deployed-relay compatibility, OS-blocked direct access, Linux/Windows, logging and rollback remain outside these backend receipts.
- Seeded capacity is not retention/capacity stress under real mutation load; exotic credential-bearing Git URL syntax is not exhaustively exercised. No source defect is established by those limits. Cross-process held-flock exhaustion, power-loss/storage-controller durability, and exceptional double-wait failure are explicitly not proved by the reports.

Smallest remaining scopes: Q4-Q7 (platform/client/events/release). A07's completed canonicalization, busy guard, project crash barrier, timeout and body-envelope implementation should not be reopened. Any additional stress or OS-failure gate must be named as evidence expansion, not relabeled missing production code.

## 3. A08 - worktree list/create/preview/delete packet

**Disposition: named worktree backend repairs proven, including final typed owner errors; full packet/release acceptance still withheld.**

Proven requirements:

- Existing worktree route extended rather than duplicated; rich root/managed/external/prunable listing, normal deletion preview, HEAD/default create, plain/unborn refusal without a partial managed parent, canonical managed identity and derived branch/path, wrong wsId/options/root/symlink refusal, dirty/locked/busy/unmerged protections, explicit branch retention/deletion and partial replay. Full `S/remote/workspace_api/worktrees.rs` was inspected; F1/F2 plus `E/A08-typed-owner-verification.md:49-62` map these to actual behavior.
- Real owner HTTP/relay/private UDS worktree lifecycle is **not still missing**. F2 records owner 91200, private UDS, host-qualified relay create/list/status/delete, branch choices and lost reply `99038616-589b-48d0-be3d-d63f76e5e713`: forwarded_reply_bytes=0, replay_equal=true, journal_equal=true, actual_target_count=1. Listener/runtime joins, owner exit 0/wait and root absence are recorded. That transport fixture explicitly spawns no PTY; the separate busy fixture's original PTY 94842 prints its actual worktree CWD and is explicitly closed/reaped.
- Successful Git stdout now rejects invalid UTF-8; quoted legacy paths are decoded without trimming path bytes; dirty check uses NUL records. `S/worktree/git.rs:590-747`, `S/worktree/manager.rs:530-550`; F3 records invalid native Git stdout rejection and the real unusual-path scenario. The old lossy-success/legacy-path repair todo is stale. Lossy diagnostic text on failed commands is not used as a successful path value.
- Typed `WORKTREE_BUSY`, `WORKTREE_LOCKED`, and `WORKTREE_REMOVED_BRANCH_RETAINED` are **closed**, not pending. `S/remote/workspace_api/worktrees.rs::owner_mutation`, `S/ipc/error.rs:100-128`, and `S/ipc/worktree.rs:13-18` preserve them. O1 logs exact UDS/native equality, original PID 81984/session/CWD, actual branch `git branch -D -- orca/typed/partial` exit 1 and nested cause. The final F1 run repeats with PID 89810. Internal committed removal revision/event occurs once; locked/busy refusal leaves targets and emits none (parent typed-owner report). This is native adapter conversion, **not actual Tauri invocation**.

Missing requirements / limits:

- A prunable/missing checkout receives explicit `422 WORKTREE_PREVIEW_UNSUPPORTED`, not the full rich preview promised for ordinary targets. `worktrees.rs::read` and P2 prove the conservative refusal; this must not be described as complete prunable-preview metadata. The final accepted repair explicitly retained this limitation. Completing rich preview for absent checkouts is a separately bounded contract-completion scope, not grounds to repeat safe deletion.
- External A12 events, actual Tauri/native desktop operation, Linux/Windows, full logging and forced-relay network exclusion remain open. Internal `WorktreeCommittedChange` and cache revision invalidation are not the authenticated event contract.
- Non-HEAD HEAD~1 is proven as a real Git side effect and as a recovered 201 under storage failure, not a separate fault-free first HTTP 201. Local/legacy prune fault wire coverage remains open. See items 5-7 for exact boundaries.

Smallest remaining scopes: Q2 (named missing worktree proof seams), Q3 (rich prunable preview only if completing the full metadata contract), Q4-Q7. No repair scope remains for the three typed-owner mappings.

## 4. R9 - early HTTP rejection resets (R3 artifacts)

**Disposition: deterministic contract-test repair closed; eager-upload reset causality/production behavior unresolved, not “transport fixed.”**

Proven requirements:

- The original failure is real and retained. `E/R3-reset-parent.log:120-138` reports three passing cases then `hyper::Error(BodyWrite, Os { code: 54, kind: ConnectionReset ... })`, test FAILED (0 passed/1 failed). `E/R3-reset-repair.md` final-v2 section records the failed first repair and confines the replacement to the test.
- H1 and F1 pass actual wire refusal with exact statuses/error envelopes/no-store: unauthorized requests send no body; authenticated requests send 65,537 bytes (limit+1) and cease uploading before reading refusal. Full original 65,537 and 2,097,153 byte bodies also reach the real router using test-only full/unknown-length body injection. `S/remote/server.rs:2410-2555` was inspected; middleware supplies bytes only, not auth or synthetic rejection. Socket errors still fail. This preserves real extraction and ordering tests without an eager-upload race.
- `E/R3-reset-repair.md` records behavioral mutation sensitivity: truncating only unknown-length input to 65,536 yields 400 versus expected 413, exit 101; auth-order mutation separately fails. These are report-attributed mutation receipts, not new mutation runs here.
- `E/R3-wave1-composition.md` says only three approved test regions were composed, no production handler changes. Final F1 includes the complete wire/full/stream scenario and cleanup. The old “needs composition/aggregate” todo is stale.

Missing requirements / limits:

- The full 2 MiB body is **not uploaded end-to-end on the wire** in the staged test. Full-payload extraction uses fixture injection. No evidence proves concurrent eager upload always receives the early 401/403/413 without a client BodyWrite failure. Fresh-client experiment passed; no independent fresh-client reset receipt exists; pooling causality is unresolved (`E/R3-reset-v2-parent-review.md:49-55`).
- This does not demonstrate a production transport defect or require a speculative server body-drain change. If full eager-client rejection delivery is an acceptance requirement, Q8 is the isolated investigation/proof scope. Do not reopen R9's completed deterministic test repair or accept resets as successful HTTP responses.

## 5. A08 process containment

**Disposition: ordinary and named injected Darwin runner cleanup accepted. Stronger OS-failure, platform, and automatic parent-death guarantees remain unproved.**

Proven requirements:

- Request-scoped Git work runs off-thread with argument arrays, a child budget min(30s, remaining request budget), 262,144 bytes per pipe, cancellation/revocation observation, group/job ownership and explicit immediate-child wait. Full `S/worktree/git.rs`, `git/unix.rs`, `git/windows.rs`, `git/drain.rs` were inspected.
- Unix process group established before exec, checked against child PID, unreaped leader retained during group signaling, event-based member drain, no signaling of unrelated enumerated PIDs. Darwin zombie-only EPERM handling is narrow, not ignored globally. F3 logs seven real multigeneration modes: cancel/revoke/deadline/output/success/failure/injected; each worker/descendant joined, sibling waited, root removed, errors empty. The injected mode records Git 5740, hook 5741, intermediate 5779, leaf 5780, owned group distinct from test group. “Descendant cleanup unexecuted” is stale.
- Drain-worker JoinError no longer returns before explicit child wait. `bounded_output` collects cleanup/fallback/observer errors and waits before returning them. F3 shows injected `A08_DRAIN_WORKER_FAILURE`, PID 2461 `explicit_wait=Ok(ExitStatus(...0))`, root removed, original error returned. Service-less post-readiness failure cleanup is also proven in F2: Git 91179/hook 91186, request joined, direct Git reaped, listener/root removed. Those old source/fixture repair todos are stale.
- F1 request-drop proof records PID 85041 active_child_reaped=true/worker_joined=true/root removed. Shortened real child deadlines and actual output overflow execute; counts alone are not the evidence.

Missing requirements / limits:

- JoinError test runs already-exiting `git --version`; it is not a blocked-descendant drain-worker-panic termination test. Actual prepare/attach, signal, observer and OS wait failures and full native 30s/40s plus cleanup exhaustion are not injected. Source has 5s drain/observer/wait bounds and a finite 40s exit observer; source is not proof those exceptional outcomes clean every resource.
- Active-owner crash fixture explicitly observes a surviving Git PID/group, kills that group itself and waits hook EOF before replay. F1 records owner 86173, Git group 86992, hook 87290 and replay owner 87749. `S/remote/workspace_api/worktree_authority_tests.rs::interrupted_fixture` confirms harness kill. This proves conservative replay **after fixture cleanup**, not automatic production orphan containment. Unix group ownership alone is not parent-death cleanup.
- Linux pidfd/proc drain and Windows suspended-create/job-assign/resume/job-drain are source-only here; Darwin builds exclude Windows implementation. No sandbox guarantee against deliberate setsid/setpgid escape is part of these accepted receipts or added by this reconciliation.

Smallest remaining scopes: Q9 (only exceptional runner proof), Q4 (platform evidence). Automatic orphan containment is not silently added as a new repair requirement; if a product guarantee is wanted it requires a separately explicit lifecycle scope.

## 6. A08 transaction ownership/recovery

**Disposition: single authority, gate races, publication fencing and conservative/recoverable outcomes proven at the documented seams. No demonstrated residual source defect in the repaired transaction scope.**

Proven requirements:

- One service owns catalog/journal and per-workspace gates; local spawn acquires the same workspace gate before resolving/spawning (`S/daemon/workspace_service.rs:14-124`, `S/daemon/session_service.rs:647-662`). `worktrees.rs::owner_mutation/legacy/mutate_worktree` use that authority. F1 shows local owner create + seven HTTP mutations occupy eight slots, ninth=429, queued revoke=401x7, exactly one target; queued root replacement=400 and managed-child replacement=409 preserve outside sentinel.
- Delete blocks spawn through publication; after deletion, spawn refuses the missing target. Inspected `worktree_authority_tests.rs:146-199`; F1 records the exact gate/barrier outcome. Old queued-child/delete-versus-spawn todos are stale.
- Intent is durable before Git; only New executes; completed/unknown request replay never repeats Git. Catalog outcome receipt is persisted before revision/event publication, then journal completion. Failed/aborted/revoked publication after Git yields unknown and no committed catalog event. `worktrees.rs::mutate_worktree` and journal `begin/mark_unknown/recover_catalog` implement this; F1 logs both cancel and revoke target-preserved/catalog-unchanged/unknown/drained paths.
- Revisions include committed observations and external changes; F2 logs post-commit change, unchanged owner reload equality, and offline clean change causing stale first DELETE. Do not reopen the durable-observation repair.
- Actual direct/relay lost replies and real after-Git/active-Git owner interruption yield original result or conservative unknown/no-repeat as applicable. F2's relay artifact and F1's killed-owner identities establish worktree-specific evidence; A07 project test counts are not substituted.
- Local worktree mutations remain excluded from blind resend (`S/daemon/client.rs:2418-2428`; `E/A08-typed-owner-verification.md` records exact no-resend command exit 0). A transport error is not permission to recreate the target.

Missing requirements / limits:

- Crash during active Git does not establish automatic orphan cleanup (item 5). Prune/write-failure “restart” labels are fresh in-process owner loads, not shutdown/restart of original process (item 7). They should not be inflated to end-to-end TCP/crash coverage.
- A09 must continue using the sole journal/authority while adding session ownership; this report is not acceptance of its concurrently edited implementation. External machine events and native operation reconciliation remain their later packet scopes.

Smallest remaining scopes: Q2 for exact additional transaction proof seams; Q6 for external event integration. No duplicate transaction owner, new journal, or repeated gate/publication implementation is justified.

## 7. A08 remaining failure proofs

**Disposition: historical named failure gaps mostly closed; residuals are narrowly enumerated evidence/contract limits.**

| Historical todo | Current evidence-backed disposition | Remaining scope |
| --- | --- | --- |
| Successful native-byte/legacy-path fidelity | Closed by strict success decoding and non-trimming quoted parser; F3 and inspected Git/manager source. Darwin Git-index invalid byte differs from actual Linux invalid disk name. | Linux disk case Q4 only. |
| Prune failure erased into 204 | Closed. `S/worktree/manager.rs:580-636` now returns `WorktreeRemovedPruneFailed`; machine maps it to stored partial 409, legacy to redacted partial, Local to typed nested cause. P1 actual request `0839b7dd-60ea-42c6-8297-01216670c910` replays identical 409. Full test body inspected at `worktree_authority_tests.rs:7-71`: real private config corruption makes Git prune fail, then config is restored. | Local/legacy fault injection over their wires not executed; Q2. No repeat of prune propagation implementation. |
| Catalog/journal persistence failure after real Git | Closed at handler/service/fresh-load seam. P2 catalog request `7c8dd557-22af-4627-82ad-cc5668dbf484`: 409 unknown, no committed event; journal request `9ad06648-8cc7-443c-87e8-c199e1ff5cb5`: initial 503 after durable catalog event, fresh owner recovers 201. Actual destination directory causes atomic-write failure, not mocked return. | True original-process restart/TCP fault injection absent for these two cases; Q2 only if requiring that stronger seam. |
| Explicit non-HEAD | Real HEAD~1 side effects verified at `b78db68af2c8a0a3e08a6b9a2c9fa16c02b21f89` in both P2 cases, including recovered 201. | Separate fault-free initial HTTP 201 + exact HEAD is not covered; Q2. Not absent baseRef implementation. |
| Prunable preview | Closed as conservative refusal: P2 and source return 422, never fabricated clean state. | Rich absent-checkout preview contract not implemented by that refusal; Q3, distinct from Q2 proof-only work. |
| Active-Git crash, queued child swap, delete/spawn race | Closed at exact described barriers in F1; original owner killed/waited, conservative unknown/no-repeat; sentinel and missing-target protections observed. | Automatic orphan containment not established or newly required; item 5. |
| Busy/locked/branch-partial owner response becomes INTERNAL_ERROR | Closed by O1 and final F1, exact wire/native error equality, nested Git exit and original PTY evidence. | Actual Tauri invocation only (Q5). Do not rerun this implementation repair. |
| Ordinary/injected descendants; exceptional service-less cleanup; JoinError explicit wait | Closed at executed Darwin seams in F1-F3. | OS-failure/blocked-descendant exceptional test limits Q9; platform Q4. |

The fresh-load distinction is directly visible in `worktree_authority_tests.rs:57-62,126-138`: additional DaemonServer objects are constructed while the original `owner` still exists. P1/P2 test names/log words such as `restart=true` or `restart_no_repeat=true` do not override that mechanism. The stronger killed-process fixtures are separate and must remain separately cited.

## Disjoint actionable scopes for the lead

These are the smallest remaining scopes; none authorizes implementation in this task. Recommendation: retain the accepted backend baseline and assign only the missing seam or explicit later packet below. Do not create a new Wave1 implementation cycle based on stale reports.

| Scope | Work boundary / owner | Observable completion; exclusions |
| --- | --- | --- |
| Q1 - A06 budget evidence | Directory tests only; `S/remote/filesystem_tests.rs`, existing limiter seams | Deterministic proof of 10,000 inspected-child cutoff and 10/sec burst-20 admission without timing luck. Existing 1,000-entry/256 KiB/cancellation implementation stays intact. |
| Q2 - A08 missing transaction/partial seams | Worktree fixtures only (`worktree_authority_tests.rs`, dedicated HTTP/UDS/legacy integration fixtures); coordinate shared test-file edits | Fault-free non-HEAD initial HTTP 201/exact HEAD; real Local/legacy prune failure wire outcomes; if required, isolated owner-process restart and TCP publication/storage fault cases with identical device/request replay and no duplicate target. Preserve already accepted fresh-load evidence; do not rewrite production transactions absent a new failure. |
| Q3 - prunable rich preview | `worktrees.rs::read`, preview DTO/tests only | Complete plan section 4.4 metadata for missing/prunable checkout without claiming it is clean or enabling unsafe delete, or retain explicit unsupported behavior as an explicit shipped-capability exclusion. Current 422 is safe but not full rich-preview acceptance. Keep separate from Q2's proof-only changes. |
| Q4 - platform gates | Linux/Windows fixture/build environments; no speculative Darwin production edits | Linux real arbitrary-byte directory/worktree names and process drain; Windows compilation plus drive/UNC/hidden/ACL/job lifecycle. Native production target is macOS desktop + Linux daemon; Darwin success is insufficient. Preserve unexecuted environment requirements until actual receipts exist. |
| Q5 - native client/UI gates | Existing A14/A18/A16/A21 and V05 ownership, not Wave1 service rewrites | Actual Tauri invocation, native picker host-generation cancellation/adoption, Local/SSH coexistence and safe worktree workflow. UDS/adapter equality is already proven, not actual command/menu/rendering execution. |
| Q6 - events/capability handoff | A12 and later feature-gate owner; serialize edits in shared service/router paths, respect active A09 producer | Authenticated project/worktree event projection, snapshot/revision ordering, invalidation/reconciliation; advertise only completed dependent capabilities. Current directory-only advertisement must not be widened by a scoped repair verdict. |
| Q7 - release environment/logging/compatibility | A23/A24/operator acceptance | Forced relay with OS-blocked direct gateway, deployed-relay compatibility, production log redaction/budgets, full AC09 and native/Linux workflow, rollback rehearsal. Existing loopback relay traffic/legacy WS is not this gate. No canonical daemon or developer PTY may be used as a fixture. |
| Q8 - optional eager-upload rejection investigation | HTTP client/wire fixture only first; R9 owner | Establish an actual reproducible eager-upload reset and its client/server cause if full eager-upload delivery is required; only then scope a production fix. No fresh-client reset evidence currently supports a pooling diagnosis. Do not weaken auth ordering, retry mutations, accept resets as responses, or claim R3 v2 fixed production transport. |
| Q9 - exceptional runner proof | `worktree/git.rs`/platform-owner fault seams and owned fixtures only | Actual attach/signal/observer/wait failures, blocked-descendant drain-worker panic, and complete deadline exhaustion with retained ownership/cleanup receipts. Existing ordinary/injected success is accepted. Parent-death automatic containment or deliberate group-escape sandboxing is a separate product guarantee, not implied by this scope. |

### Final acceptance boundary

The concrete old typed-owner, successful-byte fidelity, prune outcome, JoinError-before-wait, queued-child/spawn race, durable observation, body extraction and interrupted-project barrier repairs are not outstanding implementation. Full A06/A07/A08 acceptance still requires the explicitly missing metadata/client/event/environment evidence above or an explicit capability exclusion. `E/BATCH1-parent-final-verification.md:57-68` says precisely that the repaired composed Darwin baseline may seed Wave2, while V05, Linux/Windows, actual desktop invocation, forced-relay network exclusion, production logging and final rollback/review remain. This report preserves that boundary and does not accept A09 or any later packet by inheritance.
