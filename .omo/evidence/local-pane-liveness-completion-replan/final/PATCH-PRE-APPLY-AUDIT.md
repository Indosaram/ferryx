# Patch pre-apply audit — three stacked artifacts, attacked before a host window

Authored 2026-10-05. Adversarial, **read-only** audit. This audit deliberately looks for what is
*wrong*; a patch that survives it has been attacked, not blessed.

| | |
|---|---|
| Worktree audited | `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation` |
| Branch | `work/local-pane-liveness-completion-foundation` |
| HEAD verified | `b1f249f4a710d82dedb314fd42dcc7f90316c8fd` (`git rev-parse HEAD`) |
| Tree verified clean | `git status --porcelain` → empty, before and after this audit |
| Method | `git` inspection, `grep`/`read`, and `git apply`/`git apply --check` on **scratch copies only** |

**Nothing was compiled.** No `cargo`, no `bun`, no `tsc`, no `vitest`, no `vite`, no GUI, no daemon,
no remote host, no scenario was invoked. **Nothing was applied to the worktree and nothing was
committed.** The only file this audit created is this one; `.omo/` is gitignored
(`.gitignore:21`), which is why the tree is still reported clean.

## 0. Artifacts, apply order, and the observed `git apply` result

Apply order is fixed as stated in the brief and matches `F2-REMAINING.md §1`: **F2-1 → F2-REMAINING →
SPLIT-FLOW-DIAGNOSTICS**, all `git apply -p1` at repository root.

Scratch procedure used (`git archive b1f249f4 | tar -x -C <scratch>`, then `git apply -p1` in order):

| # | Artifact | `git apply --check` | `git apply` | Result |
|---|---|---|---|---|
| 1 | `E/final/F2-1-suspend-windows.patch` (25 221 B, 7 files) | **exit 0** | **exit 0** | all 7 files "Applied … cleanly" |
| 2 | `E/final/F2-REMAINING.patch` (15 036 B, 6 files) | **exit 0** | **exit 0** | all 6 files "Applied … cleanly" |
| 3 | `E/split-flow-diagnostics/SPLIT-FLOW-DIAGNOSTICS.patch` (10 809 B, 3 files) | **exit 0** | **exit 0** | all 3 files "Applied … cleanly"; `ipc/terminal.rs` hunks 1–5 applied **at offset +14 lines** (the F2-REMAINING additions above them) |

**Full sequence: `--check` exit 0 / apply exit 0 for all three, no fuzz, no rejects, no conflict
markers.** The +14 offset on patch 3 is exactly the expected consequence of patch 2 having added 14
lines to `ipc/terminal.rs` above patch 3's first hunk; it is not drift.

Additional structural facts verified:

- All three patches are ASCII, LF-only (`grep -c $'\r'` → 0 matches each); `file` reports
  "unified diff output text, ASCII text".
- Every `---`/`+++` path resolves in the tree; all are `a/src-tauri/...` / `b/src-tauri/...` or
  `a/ui/...` / `b/ui/...` and match files that exist at `b1f249f4`.
- Patch stats reproduced with `git apply --numstat`, and they match the READMEs where claimed
  (patch 3: `ipc/terminal.rs` +56/−0, `App.tsx` +22/−3, `workspaceStore.ts` +21/−3 = **+99/−6**,
  exactly the "total +99/−6" claimed in its README §0).
- `sha256sum` of patches 1 and 2 reproduce the README's declared digests byte-for-byte
  (`7dc9366e…b725cb`, `e8070bdb…2a1e7f`).
- After the full sequence, every intended edit appears **exactly once**: `install_ownership_verifier`
  ×3 (def/export/call), `forget_transferred_owner` ×2 (def/call), `FERRYX_QA_SPLIT_` ×8 (the eight
  documented literals), `nativeSnapshotDeadlineFired` ×9 (1 + 8), `switchDebug("split.…")` ×10
  (6 in `App.tsx`, 4 in `workspaceStore.ts`). No duplicated or contradictory hunks.
- No `deny(warnings)` / `-D warnings` / `[lints]` anywhere in the tree, so a new warning cannot fail
  the host build. (The F2-1 README's quoted build logs show warnings — `SpawnOwnerGuard is never
  constructed` — and that build still succeeded, consistent with this.)

## 1. Findings

Severity: **blocker** = must not apply · **major** = real defect or unintended behaviour change ·
**minor** = incomplete/incorrect claim or narrow hazard · **note** = informational, may still matter
to the host operator.

| # | patch | file:line | class | severity | what is wrong | evidence |
|---|---|---|---|---|---|---|
| F1 | F2-1 | `daemon/session_service.rs:1742-1757` (claim) vs `:1859-1868` (cache) | behaviour | **major** | The new `retain_spawn_owner` claim is taken **before** the idempotency-cache replay and before the machine `previous`-record replay. During the `prepare→commit` handover window (status `Prepared`, `is_draining()==false`) a *retry of an already-created spawn* — same `clientRequestId` — now fails with `SpawnError::Other("Daemon handover is in progress and does not accept new sessions")` instead of returning the cached session id. The F2-2 finding is about a spawn *mid-flight*; an idempotent replay is not a new session, so this exceeds the finding's requirement. | scratch `session_service.rs`: claim at 1742-1757; `prune_dead_spawn_ownership(now)` + `spawn_idempotency_cache.lock()` + `return Ok(entry.session_id.clone())` at 1857-1868; machine `previous?` replay at 1812-1820. Pre-patch the same retry returned `Ok(cached_id)`. |
| F2 | F2-1 | `daemon/session_service.rs:1742` + `daemon/handover.rs:656,708,762` | behaviour | **major** | `retain_spawn_owner()` had **zero production callers** before this patch (only `handover.rs`'s `#[cfg(test)] mod spawn_owner_tests`). This patch is the first thing that ever increments `spawn_owners`, so it **activates three previously inert gates**: `prepare_handover`, `commit_handover_v4` and `commit_handover_v5` now return `HANDOVER_BUSY` whenever any spawn is in flight. Any spawn that can block for a long time — notably `spawn_remote`, which waits on a relay tunnel — now blocks handover for its whole duration. Deliberate per F2-2, but it is a new availability coupling and a behaviour change, not merely "wire up an unused guard". | `grep retain_spawn_owner\|spawn_owners` → production call sites: none before the patch; def `handover.rs:574`, reads `:656/:708/:762`, all six callers inside `#[cfg(test)]`. |
| F3 | F2-1 | `daemon/session_service.rs:1557-1559` + `F2-1-SUSPEND-WINDOWS-REGRESSION.md §5.3` + `F2-2-SPAWN-OWNER-GUARD.md §5` | claim | minor | Both READMEs say the guard is held "for the whole registered-session lifecycle" / "so a handover cannot commit over a spawn that is still attaching". The diff holds it only for the duration of `handle_spawn`/`spawn_remote`. The subsequent attach handshake (`cmd_terminal_spawn_operation` / the attach tuple) is a **separate request**, so the stated purpose — covering a spawn that is still *attaching* — is only partly achieved. | Patch context shows `spawn_owner` bound at 1742 and dropped when `handle_spawn` returns; `spawn_remote`'s `_spawn_owner: Option<&SpawnOwnerGuard>` is likewise call-scoped. Attach is driven by `ipc/terminal.rs::cmd_terminal_spawn_operation`, a different IPC entry point. |
| F4 | F2-1 | `daemon/session_service.rs:1747` vs `daemon/handover.rs:656` and `daemon/server.rs:4233` | hazard | minor | `retain_spawn_owner()` acquires `HandoverManager::status.write()` (a `parking_lot::RwLock`) on the **async runtime thread**, and `SpawnOwnerGuard::drop` acquires the same write lock. `prepare_handover` — called **directly on the serve loop**, not via `run_blocking` — holds that write lock across `UnixListener::bind`, `fs::remove_file`, `fs::set_permissions` and `terminal_service.list_sessions()`. A runtime worker can therefore block on a lock held across blocking I/O. Pre-existing lock discipline, but this patch puts a new blocking acquirer on the spawn path. | `handover.rs:652-654` (`let mut status_guard = self.status.write();` before the bind/permissions/`list_sessions` calls); `handover.rs:454-459` (`Drop` takes `status.write()`); `server.rs:4233` calls `prepare_handover` inside the `Ok(DaemonRequest::PrepareHandover)` arm with no `run_blocking`. |
| F5 | F2-1 | `daemon/server.rs:2076-2087` | hazard | minor | The ownership verifier is installed into a **process-global `OnceLock`** inside `DaemonServer::new()`, and `DaemonServer::new*()` has ~40 call sites, every one of them in a `#[cfg(test)]` module of `server.rs`. On Windows the **first** server constructed in a test binary becomes the ownership authority for every later one (and `suspension/windows.rs`'s `OWNERSHIP` ledger is likewise process-global), so a later test's own session is refused as `NotOwned`. Windows-test-only; production constructs one daemon per process. The `let _ = set(...)` also silently keeps a *stale* verifier if a daemon is ever re-constructed in one process — fail-closed (refuses rather than mis-signals), but silent. | `suspension/windows.rs`: `static OWNERSHIP_VERIFIER: OnceLock<…>`, `install_ownership_verifier` uses `let _ = …set(…)`; `grep DaemonServer::new` → 40+ sites, all `#[cfg(test)]`. |
| F6 | F2-1 | `terminal/qa_liveness.rs:499-520` vs `suspension.rs:42-52` | behaviour | minor | The receipt now carries `stop_observed`/`guarantee`, but the QA evidence payload the harness reads (`suspension_receipt_payload`, whose `verifiedActuationReceipt` field the frozen runner asserts) surfaces **neither** field. The plan's Windows suspend/resume contract tests therefore cannot observe the weaker Windows guarantee through the artifact. The README's claim that the patch "keeps the receipt honest" is true of the type, not of the evidence path. | `qa_liveness.rs:499-520` `json!({ … "verifiedActuationReceipt": owned.verified_actuation_receipt, … })` — `OwnedStopOutcome` (`qa_liveness.rs:463-474`) has no `stop_observed`/`guarantee` member; the patch does not touch this file's payload. |
| F7 | F2-1 | `F2-1-SUSPEND-WINDOWS-REGRESSION.md §6` | claim | minor | Its construction-site enumeration names only **one** `SuspensionReceiptWire` literal (`session.rs:1516`) and omits the production one at `session.rs:562` (`set_suspension_receipt`). The patch *does* update that site, so the code is complete — the enumeration is incomplete, which is precisely the list a host operator would use to spot a missed site. | `grep 'SuspensionReceiptWire {'` → exactly two literal sites: `session.rs:562` and `session.rs:1507` (pre-patch numbering); both are updated by patch 1. |
| F8 | F2-1 | `terminal/suspension/windows.rs` `install_ownership_verifier` closure | note | note | The verifier captures `Arc<TerminalService>` inside a `'static` `OnceLock`, so the service is never dropped (intended for a long-lived daemon, but it is a permanent retain). Separately, the closure literal relies on rustc inferring `u32`/`&str` parameters from the higher-ranked `impl Fn(u32, &str) -> bool` bound — expected to work, but it is exactly the kind of inference a compiler must confirm. | Patch adds `let ownership = Arc::clone(&terminal_service); crate::terminal::install_ownership_verifier(move |pid, incarnation| { … })`. |
| F9 | F2-REMAINING | `ipc/terminal.rs:1873-1884` and `terminal/qa_liveness.rs:448-460` | behaviour | note | Both new `run_blocking` wrappers map **every** `Err` to `None`, i.e. "no control observed yet" / "no evidence yet". `run_blocking` can only fail with a `JoinError` (panic/abort), so a panic in the synchronous read — which previously propagated — is now silently absorbed. The direction is safe (never an armed control, never a pass), but it is a behaviour change and it hides a panic. | `ipc/mod.rs:45-53`: `spawn_blocking(operation).await.map_err(|error| IpcError::internal(format!("blocking task failed: {error}")))?`; the closure body is `Ok(read_control_in(…))`, which is infallible. |
| F10 | F2-REMAINING | `F2-REMAINING.md §1` | claim | note | The six "sha256" values for the resulting files are 16-hex-character **prefixes**, not sha256 digests (a sha256 is 64 hex chars). I recomputed sha256 on my own scratch after applying patches 1+2: **all six prefixes match exactly** — `ab0837a42a4d9f79…`, `e0b2ae2ee89f1125…`, `53cf4dcaad668e34…`, `5e35c5f50331bf63…`, `66ef429564c3a564…`, `09af4f04562c14fc…`. Substantiated; the label is imprecise. | `shasum -a 256` over the six files in a 1+2 scratch. |
| F11 | F2-REMAINING | `ui/src/lib/paneLiveness.ts:246-252` | behaviour | note | The race now resolves a discriminated `{ snapshot }` on both arms and sets `nativeSnapshotDeadlineFired = observed.snapshot === null`. When the `invoke` arm wins with a genuine `null` payload, the flag reads `true` even though the deadline did **not** fire — looser than its own doc ("the native snapshot IPC lost the fixed 100 ms race"). The 100 ms value, the `?? sessionId` fallbacks and `classifyPaneLiveness` are untouched, so the **verdict logic is unchanged**; only the flag's meaning is approximate. | Patch: `nativeSnapshot = observed.snapshot; nativeSnapshotDeadlineFired = observed.snapshot === null;`; `return { verdict: classifyPaneLiveness(snapshot), nativeSnapshotDeadlineFired }`. |
| F12 | F2-REMAINING | READMEs vs tree | claim | note | **Positive finding, checked because it was the likeliest silent breakage.** The claim that existing UI tests keep passing holds: `paneDebugInfo.test.ts` and `TerminalSplitView.test.tsx` assert with `toHaveProperty` / `toMatchObject`, never exact object equality, so the added `nativeSnapshotDeadlineFired` key cannot fail them; `observePaneLivenessAsync` has exactly one consumer in `ui/src` (`paneDebugInfo.ts:10`); no file under `scripts/` parses the debug-info JSON. | `grep formatPaneDebugInfo` → `paneDebugInfo.test.ts:19,37,58`, `TerminalSplitView.test.tsx:992,1005,1040` (sync formatter) — all shape-tolerant; `grep nativeSnapshotDeadlineFired scripts/` → none. |
| F13 | SPLIT-FLOW | `split-flow-diagnostics/README.md §6` | claim | minor | The apply instructions say `git rev-parse HEAD  # expect 89a363a0`, but the declared stack applies this patch **third**, on top of F2-1+F2-REMAINING at `b1f249f4`. `89a363a0` *is* an ancestor of `b1f249f4` and `git diff 89a363a0..b1f249f4` over the three touched files is **empty**, so following the instruction would not corrupt anything — but an operator following it literally would check out the wrong revision, and the README never states the patch's position in the stack or that its base is the *pre-patch* content of files patch 2 also edits. | `git merge-base 89a363a0 b1f249f4` = `89a363a0`; `git diff --name-only 89a363a0..b1f249f4 -- <the 3 files>` → empty; patch 3 applies at `b1f249f4` only after patch 2 (offset +14). |
| F14 | SPLIT-FLOW | `src-tauri/Cargo.toml:58` + `ipc/qa_barrier.rs:1047,1456` | note | note | The README prescribes `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` as the first real check. The tree itself documents that **this exact command has historically failed with `E0275`** (recursion-limit overflow proving `Sync` through the WGPU object graph) *while `cargo check` passes*, and that the workarounds were structural. If the host hits `E0275`, that is a pre-existing trap, not a patch defect — and the README should say so, naming `cargo check --features local-split-qa` as the discriminating first gate. The README's parenthetical that `native-terminal` is a default feature **is** correct. | `qa_barrier.rs:1047-1049`: "That derivation is deep enough to overflow the default recursion limit, which fails `cargo build --features local-split-qa` with `E0275` while `cargo check` passes."; `qa_barrier.rs:1456` repeats it; `Cargo.toml:53` `default = ["native-terminal"]`, `:58` `local-split-qa = []`. |
| F15 | SPLIT-FLOW | `ui/src/App.tsx`, `ui/src/state/workspaceStore.ts` | behaviour | note | **Positive finding.** The TS additions are pure logging: all three `handleSplitActive` guards and all three `splitPane` guards keep **byte-identical conditions**, only their bodies gained a `switchDebug` call; `switchDebug` is inert unless `import.meta.env.DEV \|\| VITE_SWITCH_DEBUG === "1"` and is forced `false` for `MODE === "test"`. None of the ten new event names is in `RELEASE_PERSISTED_INPUT_EVENTS`, so a release build persists nothing new. Residual: the event-name and `reason` string literals still ship in the production bundle as dead code — the only "QA surface" left, and it is inert. | `switchDebug.ts:60-64` `shouldTrace = enabled \|\| (allowReleasePersisted && isReleasePersistedInputEvent(event))`; the 20-entry allowlist is all `terminal.surface.*` / render / presentation names; `resolveSwitchDebugEnabled` returns false for `MODE === "test"`. Rust side is `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`, so a normal build contains none of it. |

**Counts: blocker 0 · major 2 · minor 6 · note 7 · total 15.**

No blocker was found. Nothing in the three patches prevents a clean application, and no missed
construction/call site was found (see §2).

## 2. Per-patch compile-risk list

Every changed type, field, signature and constructor the patches touch, with the sites enumerated by
grep and whether the patch covers them. This is the section the host operator should rely on.

### 2.1 `F2-1-suspend-windows.patch`

| Changed item | Kind | Sites enumerated | Count | Covered |
|---|---|---|---|---|
| `ActuationReceipt` + `stop_observed: bool` + `guarantee: StopGuarantee` | struct, **two new required fields** | Literal constructions: `suspension.rs:118` (`Ownership::stop`), `session.rs:270` (`SuspensionReceiptWire::receipt`), `service.rs:663` (`mod preparation_tests`); plus one added by this patch in `suspension/windows.rs` | 3 pre-existing + 1 new = **4** | **YES 4/4** |
| `StopGuarantee` | new enum | Definition `suspension.rs:52`; re-export `terminal/mod.rs:55`; uses: `suspension.rs` (4), `windows.rs` (4), `session.rs` (3), `service.rs:663` (1), `mod.rs` (1) | new | **YES** (re-export added so `super::super::StopGuarantee` resolves from `service.rs`/`session.rs` tests) |
| `SuspensionReceiptWire` + `stop_observed` with `#[serde(default)]` | struct, new field | Literal constructions: `session.rs:562` (`set_suspension_receipt`), `session.rs:1507` (test) | **2** | **YES 2/2** |
| `terminal/mod.rs` re-export line | `pub use` list | `StopGuarantee` inserted; new `#[cfg(windows)] pub use suspension::windows::install_ownership_verifier;` | — | **YES** |
| `suspension.rs` `mod windows` → `pub mod windows` | visibility | Its three consumers `windows::stop_for_owned_suspension` / `classify_stop_source` / `resume_owned` (all `pub(super)`, called from `suspension.rs:162/171/180`) | 3 call sites | **YES** (unchanged, still visible) |
| `session.rs` `mod windows_suspend` → `pub(crate) mod windows_suspend` | visibility | `suspend_process` / `resume_process`: pre-existing callers `session.rs:1070/1077`; new callers in `suspension/windows.rs` (`actuate_stop`, `actuate_resume`) via `crate::terminal::session::windows_suspend::` | **4** | **YES** |
| `install_ownership_verifier` | new fn | Definition `suspension/windows.rs`; re-export `terminal/mod.rs`; call `daemon/server.rs:2078` | **1 call** | **YES 1/1** |
| `retain_spawn_owner` | existing fn, **first production call** | Production call sites before the patch: **0** (six callers, all in `handover.rs`'s `#[cfg(test)] mod spawn_owner_tests`); added: 1 | **1** | **YES 1/1** |
| `spawn_remote` + `_spawn_owner: Option<&super::handover::SpawnOwnerGuard>` | signature change | Definition `session_service.rs:1555`; call sites: **1** (`session_service.rs:1899`) | **1** | **YES 1/1** |
| `SpawnOwnerGuard` (now referenced from `session_service.rs`) | `pub(crate)` type | Path `super::handover::SpawnOwnerGuard` valid from `daemon::session_service`; `retain_spawn_owner` takes `self: &Arc<Self>` and `handover_manager: Weak<HandoverManager>` → `.upgrade()` | — | **YES** |
| `SuspensionError::UnsupportedPlatform` | variant retained | Still constructed at `suspension.rs`'s `#[cfg(not(any(unix, windows)))]` arm, so it is not dead after the Windows stub is deleted | 1 | **YES** |
| Verifier closure body | new code | `TerminalService::list_sessions() -> Vec<String>` (`service.rs:609`), `get_session(&str) -> Option<Arc<PtySession>>` (`service.rs:616`), `PtySession::pid() -> Option<u32>` (`session.rs:203`), `incarnation() -> Option<&str>` (`session.rs:553`), `TerminalService::pty_manager() -> &Arc<PtyManager>` (`service.rs:174`) | all exist | **YES** |
| New `#[cfg(windows)]` tests in `suspension/windows.rs` | new tests | `WindowsOwnership`, `OwnershipVerifier`, `platform_failure`, `accepts`, `target` — all module-local; `stop_for_owned_suspension`/`classify_stop_source`/`resume_owned` no longer referenced by the removed stub test | 6 new tests, 1 removed | **YES** |
| `src-tauri/tests/**` | external callers | grep `ActuationReceipt\|SuspensionReceiptWire\|spawn_remote\|retain_spawn_owner\|stop_observed\|StopGuarantee` under `src-tauri/tests` | **0** | n/a — no site to miss |

### 2.2 `F2-REMAINING.patch`

| Changed item | Kind | Sites enumerated | Count | Covered |
|---|---|---|---|---|
| `PtyManager::forget_transferred_owner` | **new** `#[cfg(unix)] pub fn` | Call sites: `daemon/server.rs:2926` | **1** | **YES 1/1** — and the call site is provably inside a `#[cfg(unix)]` region: the same block already calls `expect_transferred_owner`, itself declared `#[cfg(unix)]` at `pty.rs:1007`, with no local attribute. **This is the single most likely place for a Windows-only compile break, and it is clean.** |
| `read_control` → renamed `read_control_in` + new async `read_control` | rename + new wrapper | All call sites inside `mod qa_split_producers`: async watchers `2576, 3040, 3112` and the `3231/3232` `&&` pair → **5** awaited `read_control` calls; synchronous unit tests `3474, 3486, 3498, 3880, 3895` → **5** `read_control_in` calls | **9** | **YES 9/9** (verified in the patched scratch: 5 awaited + 5 `read_control_in`, no stale reference remains; the only other occurrence is a prose comment at `3866`) |
| `read_frame_submission` | unchanged fn, now invoked through `run_blocking` | Production caller: `qa_liveness.rs:449` → `454`; tests use the distinct `read_frame_submission_in` (`1086, 1090, 1093, 1095, 1099`) and are untouched | **1** | **YES** (still used, so no dead-code warning) |
| `observePaneLivenessAsync` return `LivenessStageVerdict` → `PaneLivenessObservation` | signature change | Consumers in `ui/src`: `paneDebugInfo.ts:10` | **1** | **YES 1/1** — no test calls it; `grep observePaneLivenessAsync ui/**/*.ts*` → import, call, definition only |
| `PaneLivenessSnapshot` + `nativeSnapshotDeadlineFired?: boolean` | optional field | Literal constructions / consumers: `TerminalSplitView.test.tsx:992,1005,1040` and `paneDebugInfo.test.ts` (sync `formatPaneDebugInfo`), `paneLiveness.test.ts` (sync `observePaneLiveness`) | several | **YES** — field is optional; all assertions are `toHaveProperty`/`toMatchObject` |
| `formatPaneDebugInfoAsync` JSON + `nativeSnapshotDeadlineFired` key | additive output | Consumer: `paneDebugInfo.test.ts:58` | **1** | **YES** — shape-tolerant assertions |
| `Ok(DaemonRequest::AbortHandover)` → `run_blocking` | conversion | `Arc<HandoverManager>` moved into a `move` closure; `HandoverManager` is already moved into `run_blocking` for `commit_handover` at `server.rs:4356`, so `Send + Sync + 'static` is already established in-tree | — | **YES** |
| `crate::ipc::IpcError::internal` | bound `impl std::fmt::Display` | `ipc/error.rs:255`; `String: Display` → `.map_err(IpcError::internal)` on `Result<(), String>` | — | **YES** |
| `crate::ipc::run_blocking` bounds | generic | `T: Send + 'static`, `F: FnOnce() -> Result<T, IpcError> + Send + 'static` (`ipc/mod.rs:45-48`). `T = ()` / `Option<Value>` / `Option<u64>`; `F` captures only `Arc<QaBarrierChannel>`, `String`, `PathBuf`, `Arc<HandoverManager>` | — | **YES** |
| `Send` obligation on the watcher futures | async | `await_frame_evidence` and the `qa_split_producers` watchers already held `&mut`/`&` values across `.await` (they awaited `ticker.tick()` / `sleep()` inside the same loops), so their futures were **already** `Send`-constrained; the added `run_blocking(...).await` introduces no new captured non-`Send` state (the wrappers clone into owned data) | — | **YES — no new `Send` requirement is created** |

### 2.3 `SPLIT-FLOW-DIAGNOSTICS.patch`

| Changed item | Kind | Sites enumerated | Count | Covered |
|---|---|---|---|---|
| 8 × `eprintln!` in `ipc/terminal.rs` | new statements, `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]` | Every interpolated identifier is already in scope at its site: `identity.request_id`, `identity.origin_epoch`, `request.workspace_id`, `cwd.display()`, `prepared.identity.request_id`, `result.session_id`, `result.epoch` | **8** | **YES 8/8** (verified in the patched scratch; `identity` is borrowed by `format_args!` before it is moved into `PreparedLocalSplit`, and `prepared` is a `&PreparedLocalSplit` — no move conflict) |
| `switchDebug(...)` in `ui/src/App.tsx` (6) / `ui/src/state/workspaceStore.ts` (4) | new calls | Import present at `App.tsx:152` and `workspaceStore.ts:18`; signature `switchDebug(event: string, details?: Record<string, unknown>): SwitchDebugEntry \| null` matches all 10 call shapes; `direction`, `activeTab.kind`, `collectLeafIds` already in scope | **10** | **YES 10/10** |
| `feature = "local-split-qa"` / `"native-terminal"` | feature gate | `src-tauri/Cargo.toml:58` `local-split-qa = []`; `:53` `default = ["native-terminal"]` | — | **YES** (declared; the README's "native-terminal is a default feature" is correct) |
| `handleSplitActive` dependency array | hooks | Unchanged (`[reportRuntimeError, splitPane]`); the added calls are module-level imports, so no new dependency is required | — | **YES** |

### 2.4 Cross-patch interaction

- `daemon/server.rs` (patches 1 and 2): patch 1's hunk lands at `@@ -2073,6 +2073,15 @@`, patch 2's at
  `@@ -2921,8 +2921,12 @@` and `@@ -4376,7 +4380,13 @@`. Disjoint. **No textual conflict.**
- `ipc/terminal.rs` (patches 2 and 3): patch 2 edits `mod qa_split_producers` (≈1812-3910); patch 3
  edits `cmd_terminal_spawn_operation`/`cmd_terminal_spawn` (≈3914+). Disjoint, and patch 3 applies at
  offset +14. **No textual conflict, no edit appears twice.**
- No file is touched by all three. No edit region is shared by any two patches.

## 3. Verdicts

| Patch | Verdict |
|---|---|
| `F2-1-suspend-windows.patch` | **Apply with the named caveats.** No compile blocker was found and every construction/call site is covered. But it carries **two `major` behaviour findings the host operator must accept deliberately or fix first**: **F1** — during the `prepare→commit` handover window an idempotent spawn *retry* now errors instead of replaying its cached session id (the claim sits above the idempotency cache at `session_service.rs:1742` vs `:1859`); **F2** — this patch is the first production caller of `retain_spawn_owner`, so it *activates* the three previously inert `spawn_owners` gates, and any long-running spawn (e.g. a remote spawn waiting on a relay tunnel) now blocks handover for its whole duration. Also carry **F4** (runtime thread blocking on `status.write()` held across blocking I/O in `prepare_handover`), **F5** (Windows test-only verifier contamination via the process-global `OnceLock`), and **F6** (the new guarantee is not surfaced in the QA payload the Windows contract tests read). If F1 was not intended, move the claim below the idempotency/replay lookups before applying. |
| `F2-REMAINING.patch` | **Safe to apply as-is.** All findings are `note`: F9 (a `JoinError` is now absorbed as "no evidence yet"), F10 (16-hex-char digests mislabelled "sha256" — values verified correct), F11 (the deadline flag also reads `true` for a genuine `null` payload; the verdict logic and the 100 ms bound are untouched), F12 (existing tests verified not to break). The `forget_transferred_owner` call site is inside a `#[cfg(unix)]` region — the single most likely Windows compile break — and it is clean. |
| `SPLIT-FLOW-DIAGNOSTICS.patch` | **Safe to apply as-is.** All findings are `minor`/`note`: F13 (README §6's `expect 89a363a0` is stale — the patch is third in the stack at `b1f249f4`; harmless because the three files are identical at both revisions), F14 (the README should warn that `cargo build --features local-split-qa` has a documented `E0275` history in this tree and name `cargo check` as the discriminating first gate), F15 (the TS diagnostics are verified pure logging, inert in a normal and in a test build; only dead string literals ship). |

## 4. What a static pass cannot establish — stated as unassessable, not guessed

1. **Actual compiler behaviour.** No `cargo`, `rustc`, `bun` or `tsc` ran. Every Rust type-correctness
   claim here is argued from source reading, not from a compiler.
2. **Macro expansion.** The 8 new `eprintln!` sites and the `json!`/`format_args!` machinery around
   them are expanded by rustc. I verified each interpolated identifier is in scope and borrowed before
   any move, but only rustc performs the borrow check inside `format_args!`.
3. **Trait resolution in a higher-ranked position.** `install_ownership_verifier` takes
   `impl Fn(u32, &str) -> bool + Send + Sync + 'static` and is passed a closure literal whose
   parameters must be inferred from that bound. This is a standard pattern and should work, but I
   cannot prove it without a compiler.
4. **Auto-trait derivation.** `Send`/`Sync` for `TerminalService`, `HandoverManager`, `PtySession` and
   the two `Arc<QaBarrierChannel>`-capturing closures is argued from the fact that these values already
   cross `run_blocking`/`spawn_blocking` boundaries in the existing tree. That is strong evidence, not
   proof.
5. **The `E0275` recursion-limit trap.** Whether `cargo build --features local-split-qa` overflows the
   default recursion limit at any *new* `await` point cannot be determined statically; the tree
   documents that this failure mode exists and that `cargo check` can pass while `cargo build` fails.
6. **Platform-specific compilation.** Only one target is compiled at a time and none was compiled
   here. The `#[cfg(windows)]`, `#[cfg(unix)]`, `#[cfg(not(unix))]` and
   `#[cfg(not(any(unix, windows)))]` structure was read and is internally consistent (in particular
   the `forget_transferred_owner` call site's unix containment, §2.2), but Windows/macOS/Linux
   compilation is unverified.
7. **Runtime behaviour of every fix.** Not measured: whether `NtSuspendProcess` actually freezes the
   pane's workload; whether the verifier's `list_sessions()`/`get_session()` scan is correct under
   concurrent spawn/teardown; whether the handover-window spawn refusal (F1/F2) is user-observable;
   whether the added `run_blocking` hop preserves the intended 25 ms cadence; whether the
   switch-debug JSONL actually lands (that depends on a `VITE_SWITCH_DEBUG=1` UI rebuild the README
   itself flags).
8. **Whether any existing test now fails.** No test was run. I checked the specific assertions the
   READMEs claim are safe (F12) by reading them; everything else is unverified.
9. **Warnings.** No `deny(warnings)`/`-D warnings`/`[lints]` exists anywhere in the tree, so warnings
   cannot fail the host build — but the *set* of new warnings (e.g. dead code, unused imports) is a
   compiler output I did not produce.

## 5. Cleanup and state statement

- Scratch directories created for this audit (`/tmp/ferryx-audit-scratch`,
  `/tmp/ferryx-audit-scratch12`) were removed.
- `git status --porcelain` in the audited worktree is **empty**; HEAD is still
  `b1f249f4a710d82dedb314fd42dcc7f90316c8fd`.
- No source, test, script, plan, ledger or existing patch was modified; the only file written is this
  audit, under the gitignored `.omo/` tree.
- **Nothing was compiled, nothing was applied to the worktree, and nothing was committed.**

---

## Resolution (2026-10-05) - F1 and F2 in the regenerated `F2-1-suspend-windows.patch`

Regenerated artifact: `E/final/F2-1-suspend-windows.patch`,
sha256 `99c3d711ce0555fdfabbe7f3431fac89d1e6bef9c93b314dd0d436616c89351a`, 35 103 B, 793 lines,
**8 files** (was 25 221 B, 7 files). Nothing was compiled, nothing was applied to the worktree, and
nothing was committed while producing it; the method is this audit's own - `git` inspection and
`git apply`/`git apply --check` on `git archive b1f249f4` scratch copies.

| Finding | Status | What changed / the named residual |
|---|---|---|
| **F1** - an idempotent spawn retry now errors during the `prepare`->`commit` window | **FIXED** | The `retain_spawn_owner` claim moved from `daemon/session_service.rs:1742` (above the replays) to **below** the idempotency-cache replay, the machine `previous`-record replay and the live-session metadata replay, and **above** the `spawn_remote` early return - so a retry of an already-created spawn returns its cached session id again while both the local and remote create paths stay gated. The code comment records the ordering as load-bearing. **Residual:** no test pins it - the replay path has no existing test, and no compiler was available to validate a new one - so the invariant is stated in code and in `F2-1-SUSPEND-WINDOWS-REGRESSION.md §0.1` rather than enforced by a test. |
| **F2** - this patch first activates the three `spawn_owners` gates, so a long spawn blocks handover | **FIXED (bounded, typed, observable, tested); the delay itself is accepted by design** | Step 1 of the brief answered from the source: **no gate can hang** - each returns its refusal immediately, and there is no internal wait or deadline in `prepare_handover` / `commit_handover_v4` / `commit_handover_v5` to bound. What was missing was the other half of "bounded": the refusal was a bare `String` with no guard identity, so the one polling caller (`scripts/install-macos-app.mjs` driving `upgradeBinary`) could not tell retryable-busy from structurally broken. Step 3 applied: `daemon/handover.rs` now owns the token (`pub(crate) const HANDOVER_BUSY`, `pub(crate) fn is_spawn_gate_busy`) and builds every refusal from the live guard count (`HandoverManager::spawn_gate_refusal`, e.g. `HANDOVER_BUSY: 2 in-flight spawn(s) hold the handover gate`); `daemon/server.rs`'s new `handover_failure_response(context, message)` maps a spawn-gate refusal to the typed `DaemonResponse::HandoverRejected { reason }` at all three call sites (`PrepareHandover`, `CommitHandover`, `handle_upgrade_binary`) and leaves every other failure the generic `Error` it already was, message text unchanged; `daemon/handover.rs`'s test module gains the unix-gated `a_blocked_handover_reports_busy_and_succeeds_on_retry_after_the_spawn_completes`, which pins the retry contract (busy names the guard, manager stays `Active`, the same prepare succeeds once the spawn's guard drops). **Residual (accepted, deliberate):** a long spawn still delays handover for its whole duration - that is F2-2's fail-closed invariant - and the delay is bounded by that spawn, never unbounded; the attach window remains uncovered (**F3**). |
| F3, F4, F5, F6 | **unchanged, as written** | Out of this brief's scope. F3 is now restated explicitly in `F2-2-SPAWN-OWNER-GUARD.md §6.2` and `F2-1-SUSPEND-WINDOWS-REGRESSION.md §0.3`/`§5.3` instead of being left implicit; F4, F5 and F6 stand exactly as recorded above. |
| F7 | **corrected** | `F2-1-SUSPEND-WINDOWS-REGRESSION.md §6` now enumerates **both** `SuspensionReceiptWire` literal sites - `session.rs:562` (`set_suspension_receipt`, the production one the audit found missing) and `session.rs:1516` (the test) - so the list an operator would use to spot a missed site is complete. |
| F8-F15 | **unchanged, as written** | All `note`/`minor`; the regenerated patch does not touch them. F10's mislabelled 16-hex "sha256" values are unaffected (this patch does not alter the six F2-REMAINING files). |

### Apply re-verification (scratch copies only, on a pristine `git archive b1f249f4`)

Order unchanged: `F2-1` -> `F2-REMAINING` -> `SPLIT-FLOW-DIAGNOSTICS`, all `git apply -p1` at
repository root.

| # | Artifact | `git apply --check` | `git apply` | Offsets |
|---|---|---|---|---|
| 1 | regenerated `F2-1-suspend-windows.patch` (35 103 B, 8 files) | exit 0 | exit 0 | all 8 files "Applied ... cleanly", **no offset** |
| 2 | `F2-REMAINING.patch` (15 036 B, 6 files) | exit 0 | exit 0 | `daemon/server.rs` hunks 1-3 at **offset +16** - this patch's new 16-line `handover_failure_response` helper sits above them; `ipc/terminal.rs`, `terminal/pty.rs`, `terminal/qa_liveness.rs` and both `ui/**` files at offset 0 |
| 3 | `SPLIT-FLOW-DIAGNOSTICS.patch` (10 809 B, 3 files) | exit 0 | exit 0 | `ipc/terminal.rs` hunks 1-5 at **offset +14** - unchanged from §0 above (patch 2's 14 added lines); `ui/src/App.tsx` and `ui/src/state/workspaceStore.ts` at offset 0 |

No rejects, no `.orig` files, no conflict markers. The regenerated patch does not touch
`ipc/terminal.rs`, so patch 3's +14 is exactly what §0 already recorded; the only change to the
sequence's evidence is patch 2's `daemon/server.rs` offsets moving from 0 to +16, caused by this
patch's new helper (an offset, not a conflict - the three hunks still apply with their own context
intact).

**§0's patch-1 row is superseded** by the table above: the artifact is now 35 103 B / 8 files /
`99c3d711ce0555fdfabbe7f3431fac89d1e6bef9c93b314dd0d436616c89351a`, and its verdict is "apply" with F1
and F2 resolved as stated. Every other row of §0, every row of §1 and §2, and every item of §3 and §4
stands as written.
