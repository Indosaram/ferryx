# Task 9 — Windows half + QA producer/consumer compile verification: CONSOLIDATED REPORT

> ## ⚠ SUPERSESSION NOTE (added 2026-10-05 00:0x local, after this report was written)
>
> **This report describes `c34b90fc` and only `c34b90fc`.** While it was being finalised, both repair
> lanes the lead dispatched landed on the same branch, so the candidate HEAD has since moved:
>
> | Commit | Lane | Subject |
> |---|---|---|
> | `799a582d` | `st_01a1072c` (adapter) | `fix(qa): correct the barrier role map and lock the pre-arm contract` |
> | **`314251e0`** | `st_01a1072b` (rust) | `fix(qa): repair the local-split-qa compile errors in the QA producer set` |
>
> **Current candidate HEAD: `314251e0`** (tree clean, 0 dirty paths, 7 files / +90 / −24 over `c34b90fc`).
> `c34b90fc` remains in history at `HEAD~2`, so every verdict below stays valid **for the revision it
> names** — but **none of it is re-verification of `314251e0`**, and nothing in this report should be
> read as claiming the repairs work. A read-only check confirms both commits touch the exact sites this
> report routed (see §11); the re-verification is owed and was explicitly deferred to a re-dispatch.
>
> Two consequences worth stating plainly:
> 1. The adapter lane found **more** than this report did: my Windows run caught
>    `attach-handshake` killing `split-attach-stall`, but the same `BARRIER_ROLES` defect also killed
>    `diagnostic-classifier` (`backend-write`) and `split-concurrent` (`held-rpc`) — **three of the nine
>    scenarios, not one**. Those two are mac-side scenarios this dispatch never ran, so my report
>    under-counted the blast radius; the lane's correction is the better number.
> 2. The Windows full `--lib` that this report leaves in flight is a run of the **old** tree
>    (`source-21dea3c0` at `c34b90fc`); it says nothing about `314251e0`.

**Verifier:** sole remote verifier `st_01a10728`. **Date:** 2026-10-04 (+0900).
**Candidate (as verified):** `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`c34b90fc`**
(`c34b90fcdb5ec6ff6ca34c80cada038b921d626a`), tree **clean**, 14 files / +6753 / −93, on top of
`dd9e6813` … base `d82b35e4`.

## HEADLINE

**The QA producer/consumer set does not compile.** `cargo check/build --features local-split-qa` is
**exit 101 with 16 error sites on linux and 6 on Windows** (lib build), and an A/B proves it is
**candidate-caused**: the identical command is **exit 0, 0 errors** at `70eefafe`, one commit earlier.

Consequently the three Windows scenarios are **NOT_RUN_BLOCKED** — no QA-feature binary exists to run
them against, and **no screenshot was produced, so the independent image-reader lane has nothing to
read**. That is reported as unproven, never as a pass.

A **second, independent blocker** was found by running the runner against the *default* binary:
`split-attach-stall` aborts in its own adapter before launching anything
(`BARRIER_ROLES['attach-handshake'] = 'producer'` vs `prearm()`'s predecessor/successor-only rule).

Everything that *could* be verified was verified and is green (default build both hosts; the four
default-feature regression gates; the runner unit suite ×2; and a fail-closed negative control).

## 1. Per-platform verdict tables

### 1.1 linux (omaki, `indo@100.91.254.71`), rustc/cargo 1.98.0, 12 cores

| # | Exact command (cwd `source-21dea3c0` @ `c34b90fc`) | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | `Finished dev profile … in 2m 58s` (errors=0) | **PASS** |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **101** | — | `error: could not compile ferryx (lib) due to 7 previous errors` / `(lib test) due to 9 previous errors` | **FAIL** (candidate-caused; A/B) |
| 3 | `--lib --features local-split-qa -- --list` (`qa_barrier`/`qa_producers`/`qa_liveness`/`qa_split_producers`) | — | — | — | **NOT_RUN_BLOCKED** (gate 2) |
| 4 | `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | **15** | `ok. 15 passed; 0 failed; … in 0.01s` | **PASS** |
| 5 | `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | **54** | `ok. 54 passed; 0 failed; … in 1.39s` | **PASS** |
| 6 | `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | **0** | **5** | `ok. 5 passed; 0 failed; … in 5.81s` | **PASS** |
| 7 | `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | — | — | — | **NOT_RUN_BLOCKED — REGRESSION** (green 11/0 at `70eefafe`) |
| 8 | `--lib -- --test-threads=1` | **101** | 2683 | `FAILED. 2646 passed; 31 failed; 6 ignored; … in 457.89s` | **FAIL — 1 flaky joiner, 0 new candidate-caused** |
| 9 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **0** | **29** | `Test Files 1 passed (1)` / `Tests 29 passed (29)` | **PASS** |
| 10 | A/B side A: same as gate 2 with the 17 files reverted to `70eefafe` + 2 new modules deleted | **0** | — | `Finished dev profile … in 6m 13s` (errors=0) | **PASS** — the failure is NOT pre-existing |
| 11 | A/B side B: restore `c34b90fc` | **101** | — | 18 error blocks | **FAIL** — reproduces |
| 12 | `--lib remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked -- --nocapture --test-threads=1` ×3 | **101 / 0 / 0** | 1 each | rep 1 `FAILED. 0 passed; 1 failed` (4.22 s); reps 2–3 `ok. 1 passed` (0.54 s, 0.55 s) | **FLAKY — 1 of 3 failed**; panic in `tests/support/machine_input_cancellation.rs:31` (`A10 kernel WouldBlock`) |

Logs: `compile/linux/logs/01..09`, `11-ab-*`, `12-ab-*`, `13-flaky-rep-*`.

### 1.2 Windows (maho-win, `sook@100.126.171.58`, `DESKTOP-1LAPJMP`), rustc/cargo 1.97.0

| # | Exact command (cwd `source-21dea3c0` @ `c34b90fc`) | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| W1 | `bun install --cwd ui --frozen-lockfile` | **0** | — | — | **PASS** |
| W2 | `bun run --cwd ui build` | **0** | — | — | **PASS** |
| W3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default) | **0** | — | `Compiling ferryx v2026.928.7` → `Finished dev profile … in 3m 05s` | **PASS** |
| W4 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **101** | — | `error: could not compile ferryx (lib) due to 6 previous errors`; **no binary produced** | **FAIL** (candidate-caused) |
| W5 | `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | **50** | `ok. 50 passed; 0 failed; 0 ignored` | **PASS** |
| W6 | `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | **14** | `ok. 14 passed; 0 failed; 0 ignored` | **PASS** |
| W7 | `--lib split_journal -- --nocapture --test-threads=1` | **0** | **7** | `ok. 7 passed; 0 failed; 0 ignored` | **PASS** |
| W8 | `--lib -- --test-threads=1` | **IN FLIGHT** at report time | — | log stopped at `test ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection ...` — the **same stall point as Task 8's `pass3/windows/logs/full-lib-HUNG.log`** | **NOT_RUN (in flight)** — handle in §1.3 |
| W9 | `--features local-split-qa` (any test/`--list`) | — | — | — | **NOT_RUN_BLOCKED** (W4) |
| W10 | `--test daemon_handover_transfer_contract` / `ipc_hardening_contract` | **0** | **0** | targets carry `#![cfg(unix)]` → compile to nothing | **structurally-not-applicable — NOT a pass** |
| W11 | `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | **0** | **29** | `Tests 29 passed (29)` in 1.06 s | **PASS** |

### 1.3 Full library suite, both hosts

| Host | Raw exit | Result line | Task 8 reference | Delta |
|---|---|---|---|---|
| linux | **101** | `2646 passed; 31 failed; 6 ignored` | `70eefafe`: 2647 / 30 / 6 | **left 0, joined 1** |
| Windows | **IN FLIGHT at report time** | — | `abd9e890`: 2201 / 70 / 8 in 1095.61 s | — |

**Windows full `--lib` — the exact in-flight handle, so it is verifiable rather than merely "waiting":**

- command: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- host: `sook@100.126.171.58` (`DESKTOP-1LAPJMP`), cwd `C:\Users\sook\ferryx-pane-completion\source-21dea3c0`
- log: `…\task9-c34b90fc\win-06-full-lib.log` — **397 866 bytes / 1796 `test ` lines, byte-frozen since
  23:28:59 local** (re-sampled at 23:33 and again after dispatch closure; no growth)
- driver: `win-gates.ps1` under PowerShell, session `bash_21`; log-watch monitors
  `mon_46DTZ28TTYN0GS58` (gate run) and `mon_J4NF41Z1KC386JW8` (completion sentinel on
  `FULL_LIB_FAILURE_COUNT` / `WIN_GATES_DONE`)
- **DISPOSITION: LEFT IN FLIGHT BY DISPATCH DECISION — not a product signal, do not wait on it.**
  First sample 23:33 local, and again at **00:00:00 local: the log is still byte-frozen at 397 866 B with
  mtime 23:28:59 — 31 minutes of zero growth.** Its last *completed* line is
  `test ssh::bridge::tests::ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping ... FAILED`,
  and **Task 8's `pass3/windows/logs/full-lib-HUNG.log` hangs on exactly the next test in that same
  sequence**, printing `test ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection ...` with no
  result at its line 2580 (that file's own last line). Task 8's *completed* run needed **1095.61 s** to
  reach 2282 test lines / 2201 passed / 70 failed, so this test blocks for many minutes on this host — but
  a 31-minute freeze is beyond that window and is now better described as **a hang reproducing Task 8's**,
  not merely a slow test. The Windows full `--lib` row stays **NOT_RUN (in flight / hung)** in this report;
  a successor run should re-measure it on a drained host rather than treat the freeze as a finding.
  **One task-owned child is still alive** because the run was left in place by instruction: `ferryx.exe`
  PID **23672** (started 23:55:58, under `…\source-21dea3c0\src-tauri\target\debug\ferryx.exe`) — see §7.

The single joiner on linux is `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked` —
**flaky, previously recorded as flaky by Task 8**, and outside the diff (`c34b90fc` and `dd9e6813` touch
**no file under `src/remote/`**). **Zero new candidate-caused failures.**

## 2. Every failure verbatim, with its owning lane

Source: `compile/linux/logs/02-qa-check.log` (linux, 16 unique sites) and
`win/logs/cargo-build-qa.log` (Windows, same 6 lib sites).

### 2.1 daemon lane — `daemon/qa_producers.rs`, `daemon/server.rs`, `ipc/terminal.rs`

```
error[E0599]: no method named `session_service` found for reference `&DaemonServer` in the current scope
  --> src/daemon/qa_producers.rs:372:18
   |  match server.session_service().handle_describe_session(session_id) {
   |               ^^^^^^^^^^^^^^^-- help: remove the arguments
   |   field, not a method
```
```
error[E0599]: no method named `session_service` found for reference `&std::sync::Arc<DaemonServer>` in the current scope
  --> src/daemon/qa_producers.rs:458:31
   |  let metadata = server.session_service().session_metadata.read();
```
```
error[E0308]: mismatched types
  --> src/daemon/server.rs:2923:83
   |  crate::daemon::qa_producers::describe_incarnation(self, &session_id);
   |  expected `&DaemonServer`, found `Arc<DaemonServer>`
```
```
error[E0308]: mismatched types
  --> src/ipc/terminal.rs:2487:45
   |  let state = operation_state(&operation).to_string();
   |  expected `&SplitOperationResult<String>`, found `&SplitOperationResult<u64>`
```
```
error[E0308]: mismatched types
  --> src/ipc/terminal.rs:3205:33
   |  operation_state(&operation),
```
```
error[E0631]: type mismatch in function arguments
  --> src/ipc/terminal.rs:3206:37
   |  status.as_ref().map(operation_state),
   |  expected `fn(&SplitOperationResult<u64>) -> _`, found `fn(&SplitOperationResult<String>) -> _`
```
```
error[E0599]: no method named `path` found for struct `std::path::PathBuf` in the current scope
  --> src/daemon/qa_producers.rs:682:26   (also :708:26, :738:26)
   |  let legacy = dir.path().join("legacy-7-1.sock");
   |  help: there is a method `as_path` with a similar name
```
```
error[E0618]: expected function, found `qa_barrier::QaBarrierChannel`
  --> src/daemon/qa_producers.rs:771:23
   |  67 | pub fn channel() -> Option<Arc<QaBarrierChannel>> { … }
   | 619 | fn channel(dir: &Path) -> QaBarrierChannel { … }   // shadows the above
   | 771 | let channel = channel(&dir);
```

The three `PathBuf::path()` sites and the `channel` shadowing site are `#[cfg(test)]`-only (they appear
in the `lib test` pass, not the Windows `cargo build` pass).

### 2.2 stale-trigger lane — `ipc/qa_barrier.rs`

```
error[E0308]: mismatched types
   --> src/ipc/qa_barrier.rs:1222:34
    |  let channel = Arc::clone(channel);
    |  expected `&Arc<_, _>`, found `&QaBarrierChannel`
```

### 2.3 QA adapter / scripts lane — a self-contradiction that blocks `split-attach-stall`

Not a compile error; a runtime assertion that fires **before the product is launched**:

```
HarnessError: ASSERTION_FAILURE: targetRole must be 'predecessor' or 'successor', never a session ID or wildcard: got "producer"
    at BarrierHub.prearm (…/scripts/lib/qa-scenarios/common-harness.mjs:514:15)
    at main (…/scripts/qa/pane-liveness.mjs:265:16)
```

`common-harness.mjs` maps `'attach-handshake': 'producer'` in `BARRIER_ROLES` while `prearm()` in the
same file accepts only `'predecessor'`/`'successor'`; `pane-liveness.mjs` pre-arms every barrier with
`BARRIER_ROLES[barrier]` **before** `spawnOwned`. Present since `5464da0d`, unchanged by `c34b90fc`;
`scripts/` does not exist at base, so it is candidate-authored.

## 3. Windows scenario table (the three required scenarios)

| Scenario | Raw native exit | Asserted line / error | Verdict | Image-reader verdict |
|---|---|---|---|---|
| `split-happy` | **7** | `BARRIER_ACK_TIMEOUT: product did not settle barrier fixture-setup receipt[0] within 9000ms` → `verdict: BLOCKED`, `cleanupGate.ok: true` | **NOT_RUN_BLOCKED** | **none — no capture produced** |
| `split-attach-stall` | **1** | `ASSERTION_FAILURE: targetRole … got "producer"` at `common-harness.mjs:514` (pre-launch); no `latest.json` | **NOT_RUN_BLOCKED** | **none — no capture produced** |
| `split-cancel` | **1** (kill artifact; the runner's own mapping would be 7) | same `BARRIER_ACK_TIMEOUT` / `BLOCKED` / `cleanupGate.ok: true` | **NOT_RUN_BLOCKED** | **none — no capture produced** |

**Blocker (a):** no `--features local-split-qa` binary can be built at this revision (§1, W4).
**Blocker (b):** `split-attach-stall` cannot run even with a green binary (§2.3).

Full detail, the negative control, provenance and cleanup receipts:
`win/WINDOWS-SCENARIO-VERDICT.md`.

### 3.1 Negative control — the harness fails closed (so NOT_RUN_BLOCKED is safe, not merely unverified)

Default-feature binary from the same staged tree (`fcab1abeea12ad18…`, 101 938 688 bytes, built by
`cargo build`, exit 0, real `Compiling ferryx v2026.928.7`) launched by the exact scenario argv:

- `FERRYX_QA_BARRIER_DIR`, `FERRYX_QA_RETRY_REFUSED`, `FERRYX_QA_STALE_BINDING_UNSERVICED`,
  `FERRYX_QA_FIXTURE_SETUP_UNSETTLED` → **absent** from the binary.
- `Attach requires the persisted seven-field pane binding`, `Attach binding incarnation cannot be proven`
  → **present** (candidate-lineage proof; both literals were introduced by `5464da0d`).
- Outcome: `BLOCKED` + exit 7, `cleanupGate.ok: true` — **no false PASS is reachable.**

## 4. Residual classification

| Item | Class | Evidence |
|---|---|---|
| QA-feature compile failure, 16 sites, both hosts | **candidate-caused** | A/B: exit 0 at `70eefafe`, exit 101 at `c34b90fc`, same tree, hashes verified |
| `qa_barrier` gate (previously green 11/0) now uncompilable | **candidate-caused regression** | Task 8 `qa_barrier` PASS 11/0 at `70eefafe` |
| `attach-handshake → 'producer'` vs `prearm` rule | **candidate-caused** (adapter lane) | present since `5464da0d`; `scripts/` absent at base |
| `input_is_cancelled_when_grant_is_revoked` joining the failure set | **flaky / not candidate-caused** | Task 8 already recorded it flaky; no `src/remote/` file in either QA commit's diff |
| Default build red | **n/a** | green on linux (check) and Windows (build) |
| Windows binary hashes differ between two identical builds | **environment** | `1b7d5a0c…` vs `fcab1abe…`; binary sha256 is not a revision proof on this host |
| `#![cfg(unix)]` integration targets selecting 0 on Windows | **structurally-not-run** | 0 selected is **not** a pass |
| Windows suspension | **structurally-not-run** | `terminal/suspension/windows.rs` returns typed `UnsupportedPlatform`; never reached |
| Latent `#[cfg(feature = "local-split-qa")]`-alone blocks in `surface_host.rs` while `qa_barrier` needs both features | **pre-existing** | recorded by the lead pre-freeze; not exercised by any gate command |
| `split-cancel` runner hang (descendant holds the parent's stdio pipe) | **environment/robustness observation** | runner printed its full result, then never exited; `taskkill /T` required |

## 5. Staging and provenance (no stale-binary trap)

| Check | Result |
|---|---|
| linux pre-state | `70eefafe` (`handover_socket.rs` `4dc252c1bafe10e1…`) |
| Windows pre-state | `39e722ce` (`handover_socket.rs a5d7e0b4…`, `server.rs 163abf52…`, `ipc/terminal.rs 3946a80b…`, `qa_barrier.rs 5391a9f4…`) |
| Windows full-manifest check vs the `c34b90fc` manifest | **checked=5086 missing=0 diff=0** |
| linux post-stage hashes vs the `c34b90fc` manifest | `qa_producers.rs aa4300a6…`, `qa_liveness.rs a96ae6cb…`, `qa_barrier.rs f0c63df2…`, `ipc/terminal.rs 51ede9ce…`, `surface_host.rs 24911238…`, `pane-liveness.mjs e2dbceed…` — **all six equal** |
| Deltas | `git archive`: 19 paths `70eefafe→c34b90fc`, 21 paths `39e722ce→c34b90fc`, **0 deletions** |
| mtime trap | every extracted file **and** every `.rs`/`.toml` in the crate touched (`TOUCHED_AT 2026-10-04T22:53:21+09:00`); stale `ferryx.exe`/`.pdb` deleted before both Windows builds; both logs show a real `Compiling ferryx` (never `Fresh`) |
| Ghostty pin | junction `C:\Users\sook\task2-ghostty-6a508fd5` @ `6a508fd5e34c7e222c052a6d00bb3891ff3feace`; linux symlink `ghostty-21dea3c0` @ same SHA |
| **`strings`-for-a-new-symbol proof** | **NOT AVAILABLE on this host, reported rather than faked:** MSVC keeps function names in a `.pdb` and no `.pdb` exists (`PROV_BIN_EXISTS True PDB_EXISTS False`); every `c34b90fc`-unique string literal sits inside a `local-split-qa` block and is therefore absent from the default binary by construction (`activeAttachTuple` occurs only at `surface_host.rs:982`, inside the `#[cfg(feature = "local-split-qa")]`-gated `emit_stale_receipt_rejected_qa`). Revision proof rests on source-manifest identity + observed recompilation + deleted stale artifacts + candidate-lineage literals (`Attach requires the persisted seven-field pane binding`, `Attach binding incarnation cannot be proven`, both from `5464da0d`) — the literals alone do not discriminate `c34b90fc` from `39e722ce`. |

## 6. Host load recorded with every result

| Host / moment | Reading |
|---|---|
| linux start / default check / QA check | load 1.84 → 4.89 → (QA check 2 min) |
| linux regression + full-lib | load 1.92 → 7.82 → 2.93 |
| linux A/B | load 9.32 at start |
| Windows staging / default build / QA build | 31 % → 43 % → ~43 %, `FREE_GB 29.04 → 33.3` |
| Windows negative control | load 53 %, `FREE_GB 32.4` |
| Windows gates | load 62 %, `FREE_GB 32.41` |

The QA compile failure is a hard type error returned in 73 s (Windows) / 2 min (linux); **no timeout is
involved and load is not an explanatory factor.**

## 7. Cleanup receipts

| Resource | Teardown | Receipt |
|---|---|---|
| Windows `ferryx.exe` PID 3136 (left by the first negative control) | `Stop-Process -Force` | `TEARDOWN_3136_ALIVE_AFTER False` |
| Windows `ferryx.exe` PID 8820 (left by the hung `split-cancel`) | `taskkill /T /F` | gone; final sweep: **zero** task-owned `ferryx` |
| Windows `ferryx.exe` PID **23672** (child of the full-`--lib` test) | **NOT reaped — left in place by instruction** | still alive at 00:00 local; owned (path under `…\source-21dea3c0\…`); it is the hung run's own child, so killing it would disturb the run the lead asked to leave. **Cost disclosed:** it holds a `ferryx.exe` handle and its run also keeps ~1 cargo session busy on maho-win; it dies with that run or when the host drains |
| Windows node PID 16964 (hung runner) | `taskkill /T /F` | gone |
| `…\task9-c34b90fc\win-runtime-default\split-attach-stall` | **leftover, reported** | empty dir skeleton (0 files) — the runner died before cleanup; owned, harmless |
| `…\win-runtime-default\{split-happy,split-cancel}` | removed by the runner | `NEG2_ISO_LEFTOVER … False` |
| foreign `cargo.exe` ×8 on maho-win, other sessions' trees on linux | **untouched** | not this session's to kill |
| linux `task9-c34b90fc/`, Windows `task9-c34b90fc/` (owned) | retained as the evidence source | logs pulled into `task-9/compile/linux/logs/` and `task-9/win/logs/` |
| user desktop / production app / production daemon | **never addressed** | — |

## 8. What the parked mac half must still cover

Blocked on the same compile failure, plus the dispatch's own notes:

1. **Re-run the compile gate on mac** once the 16 sites are repaired — the mac-specific surface
   (`native_terminal/surface_host.rs`, `ipc/native_terminal.rs`) is compiled on linux/Windows but the
   mac-only `cfg` arms are not.
2. `diagnostic-classifier` (native + the deferred headless smoke), `retained-handover`, `handover-abort`,
   `stale-binding`, and `suspension-ownership` — the last still blocked by the `externally-stopped`
   fixture-kind gap (`qa_barrier.rs` `collect_gui_fixture_sessions` claims only `idle`/`source`/observed
   `externally-stopped`, never `created`/`adopted`).
3. `split-happy` / `split-attach-stall` / `split-cancel` / `split-concurrent` on a **QA-feature** binary.
4. The **independent image-reader lane** for every owned-window capture — it has never executed; the
   three Windows captures do not exist, so the `marker-recognition.json` handshake is untested.
5. The Task 8 host-state unknowns (whether its `pkill` landed; whether the 13 GB `target` reclaim
   finished) — mac answered ssh during this pass at load 21.01 and was deliberately not used.
6. Windows suspension remains **honest-unsupported** (`terminal/suspension/windows.rs` → typed
   `UnsupportedPlatform`): report unsupported-with-reason, never a pass.

## 9. Not done, and why

- **Phase 2 scenarios: NOT_RUN_BLOCKED** for the three Windows scenarios (two independent blockers, §3).
- **QA selectors: NOT_RUN** — 48 `#[test]` functions wait behind the gate (10 `daemon/qa_producers.rs`,
  13 `terminal/qa_liveness.rs`, 10 `ipc/qa_barrier.rs`, 15 `ipc::terminal::qa_split_producers::tests`).
  They were enumerated from source, never executed.
- **No image-reader child was spawned** — there was no capture to hand over, and inventing a verdict
  would be the exact fabrication the brief forbids.

## 10. Evidence index — every cited artifact with its producer

Root: `C/.omo/evidence/local-pane-liveness-completion-replan/task-9/`

| Artifact | Producer | What it establishes |
|---|---|---|
| `REPORT.md` | verifier `st_01a10728` | this consolidated report |
| `compile/PHASE1-COMPILE-VERDICT.md` | verifier | the 16 compile error sites, per-lane routing, staging/provenance |
| `compile/PHASE1B-REGRESSION.md` | verifier | the A/B proof and the default-feature regression table |
| `win/WINDOWS-SCENARIO-VERDICT.md` | verifier | the three scenario rows, the negative control, cleanup receipts |
| `compile/linux/logs/01-default-check.log` | cargo (linux) | default `--all-targets` exit 0 |
| `compile/linux/logs/02-qa-check.log` | cargo (linux) | the QA-feature compile failure (16 sites) |
| `compile/linux/logs/06..08-*.log` | cargo (linux) | `local_split_reliability_` 15/0, `pane_liveness_` 54/0, handover-transfer 5/0 |
| `compile/linux/logs/09-full-lib{,-failures.txt}.log` | cargo (linux) | full `--lib` 2646/31/6 and the failing test names |
| `compile/linux/logs/11-ab-70eefafe-qa-check.log` | cargo (linux, reverted tree) | **A/B side A: exit 0** |
| `compile/linux/logs/12-ab-c34b90fc-qa-check.log` | cargo (linux, restored tree) | **A/B side B: exit 101** |
| `compile/linux/logs/13-flaky-rep-{1,2,3}.log` | cargo (linux) | the flaky test's 1-of-3 failure |
| `win/logs/cargo-build-default{,-restore}.log` | cargo (Windows) | default build exit 0, real `Compiling ferryx` |
| `win/logs/cargo-build-qa.log` | cargo (Windows) | QA build exit 101, 6 lib errors, no artifact |
| `win/logs/win-0{1..5}-*.log` | cargo (Windows) | pane_liveness 50/0, local_split 14/0, split_journal 7/0 |
| `win/logs/win-06-full-lib.log` (host-side) | cargo (Windows) | full `--lib`, **in flight** — see §1.3 for the handle |
| `win/logs/win-gates.log` | verifier's PowerShell driver | the per-gate exit codes and result lines |
| `win/logs/negctl{,2}-*.log` | the QA runner (`node scripts/qa/pane-liveness.mjs`) | the fail-closed negative control |
| `win/logs/runner-vitest.log` | vitest via bun | runner unit suite 29/29 |
| `win/logs/ui-build.log`, `bun-install.log`, `win-stage.log` | bun / the staging script | staging mechanics and provenance |
| `win/NOTEPAD.md` | the earlier recon verifier | pre-existing recon (F1–F7), cited not re-derived |

**No artifact in this bundle has an unnamed producer.** Every screenshot, receipt, and readiness file the
brief warns about is **absent by necessity** — no scenario reached a native action.

## 11. Post-report check: what the two repair commits touch (READ-ONLY, not re-verification)

Added after the report was finalised, when the branch advanced under it. **No build and no test was run
for this section** — it is a diff read, recorded only so the re-dispatch knows whether the routed sites
were addressed. **It is not evidence that 314251e0 compiles.**

| Commit | Lane | Files | Delta |
|---|---|---|---|
| `799a582d` | `st_01a1072c` (adapter) | `scripts/lib/qa-scenarios/common-harness.mjs`, `scripts/qa/pane-liveness.mjs`, `scripts/qa/pane-liveness.test.mjs` | +78 / −6 |
| `314251e0` | `st_01a1072b` (rust) | `src-tauri/src/daemon/qa_producers.rs`, `daemon/server.rs`, `ipc/qa_barrier.rs`, `ipc/terminal.rs` | +18 / −18 |

Every site this report routed appears addressed, at its stated cause:

| My reported site | Repair in 314251e0 | Matches my diagnosis? |
|---|---|---|
| `qa_producers.rs:372`, `:458` — `session_service()` as a method | field access (`server.session_service`), auto-derefs the `Arc` | **yes** |
| `server.rs:2923` — `Arc<DaemonServer>` vs `&DaemonServer` | `self.as_ref()` | **yes** |
| `ipc/terminal.rs:2487/3205/3206` — `operation_state` pinned to `<String>` | helper made **generic over the epoch type**; body still matches only the variant discriminant | **yes** |
| `qa_producers.rs:682/708/738` — `PathBuf::path()` | `dir.join(...)` | **yes** |
| `qa_producers.rs:771` — `channel` shadowing | renamed `test_channel` at all eight call sites | **yes** |
| `qa_barrier.rs:1222` — `Arc::clone` on `&QaBarrierChannel` | signature now `&Arc<QaBarrierChannel>` | **yes** |
| `BARRIER_ROLES['attach-handshake'] = 'producer'` vs `prearm()` | the four non-handover entries are now **`null`** (role-less, which `prearm` supports); handover entries keep predecessor/successor | **yes, and broader** — it also unblocks `diagnostic-classifier` and `split-concurrent` |

Commit messages claim "no `#[allow]`, no `unwrap`, no `todo!()`, no deleted call and no weakened test",
and that every feature gate is unchanged. **Those are the lanes' claims, not my measurement.** The
re-verification set is unchanged: the four QA selectors with `--list` (48 gated tests), the QA-feature
build on both hosts, then the three Windows scenarios.

## 12. Successor corrections to this report (pass 2 at 314251e0, added 2026-10-05)

The pass-2 verifier re-ran the repaired tree and measured numbers that **correct two claims in this
report**. Recorded here so the bundle does not keep a wrong statement standing. Evidence:
`task-9/{compile-pass2,win-pass2}/` (theirs), not this dispatch's.

| This report said | Pass-2 measured | Verdict on my claim |
|---|---|---|
| "the QA selectors would select **48** tests" (source-derived: 10 + 13 + 10 + 15) | **57** selected — `qa_barrier` **19**, `qa_producers` 10, `qa_liveness` 13, `qa_split_producers` **15** | **my count was LOW.** Source enumeration undercounted `qa_barrier` (10 vs 19 — the selector matches test paths *containing* the name, not only tests declared in that file). Execution supersedes it; **57 is the correct number** |
| "the `strings`-for-a-symbol revision proof is **impossible on maho-win**" | true for the **default** binary (0/14 QA literals, absent by construction) but **false for the QA binary** — **14/14** QA literals present | **my claim was overbroad.** It should have been scoped to the default build. The default-build absence is a valid *negative* control and the QA-build presence is a real *positive* revision proof |
| binary sha256 is not a revision proof on that host | confirmed (two identical builds differed) | **stands** |
| the `split-cancel` runner hang — "robustness observation" | **now explained**: the runner's `taskkill /T` (no `/F`) does not kill the app's daemon grandchild, which holds the inherited stdio pipe, so node never exits after printing its complete result; reaping only the staged-tree `ferryx.exe` lets node exit with its real code | **stands, and is better characterized by the successor** |

Also confirmed by pass 2, so these parts of my report hold: `314251e0` makes
`cargo check --all-targets --features local-split-qa` **exit 0** (was 101 with 16 sites), the default
`--all-targets` stays 0, and `799a582d` lets `split-attach-stall` really pre-arm and launch where it
previously died pre-launch. Blast radius of the role-map defect was **3 of 9** scenarios, as the adapter
lane said and my banner already records.

**New blocker pass 2 found, which this report did not and could not:** every native scenario now dies at
`fixture-setup` with `requires at least one source session, got 0` — nothing anywhere constructs a
session in the isolated profile (`grep` for `spawnTerminal|spawn_terminals|createSession` over the QA
adapters = 0 hits). That is a **QA adapter/scripts lane** gap that blocks all three Windows scenarios and
every mac scenario, and it is independent of the compile repair this dispatch verified.

**One gated test fails at `314251e0`** (test-only, candidate-caused):
`ipc::terminal::qa_split_producers::tests::retry_and_batch_controls_are_read_only_for_this_run` panics at
`ipc/terminal.rs:3876` because its fixture builds `QaBarrierChannel::new(..)` without the operation
nonce, so `read_control` refuses every control. Its sibling `control_files_are_read_only_for_this_run`
(`:3446`) calls `scan_and_ack_arms()` and passes, which pins the missing step.
