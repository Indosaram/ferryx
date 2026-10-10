# Task 9 — Phase 1 PASS 2: the `local-split-qa` compile gate at `314251e0`

> **Supersession:** every measurement here is bound to **`314251e0`**. The one gated-test failure reported
> in §3 is **fixed and committed as `5245e6ba`** (see `../REPORT-PASS2.md` banner). `qa_split_producers` is
> expected **15/15** at `5245e6ba` and must be re-measured, not cited from this file.

Verifier: sole remote verifier (pass 2, successor of `st_01a10728`). Date: 2026-10-04/05 (+0900).
Candidate: `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, HEAD **`314251e0`**
(`314251e08e1457ebf611953edf259e2178bd9a07`), tree **clean** (0 dirty paths).
Chain: `314251e0` (compile fixes) → `799a582d` (barrier role map) → `c34b90fc` → `dd9e6813` → `70eefafe` → `39e722ce` → … → base `d82b35e4`.

**VERDICT: PASS on the compile gate. FAIL on one gated test. Phase 2 scenarios: NOT_RUN_BLOCKED for a
NEW, reproduced reason (see `../win-pass2/`), not the old compile blocker.**

Host: linux **omaki** (`indo@100.91.254.71`), 12 cores, rustc/cargo **1.98.0**, btrfs `/home` 382 GiB free.
Staged tree `/home/indo/ferryx-pane-completion/source-21dea3c0`, carried from the predecessor's
`c34b90fc` staging.

**The predecessor's verdict files in `../compile/` and `../win/` are bound to `c34b90fc` and are cited
here only for `c34b90fc`. They are NOT evidence about `314251e0`.** Their `REPORT.md` carries a
supersession banner; this file is the pass-2 successor.

---

## 0. Provenance of the staged tree (mtime trap defeated)

| Step | Evidence |
|---|---|
| Pre-state identity | all 7 delta files hashed on the host **before** extraction and compared against `git cat-file blob c34b90fc:<path>` — **7/7 OK** (including `server.rs df27c535fefdb55b`, the predecessor's own A/B anchor) |
| Delta | `git archive` of exactly the 7 paths `c34b90fc..314251e0` (`delta-314251e0.tar.gz`, 171 201 B) — the commit diff is 7 files, +90/−24 |
| Post-extract identity | all 7 files re-hashed against `git cat-file blob 314251e0:<path>` — **7/7 `STAGED_OK`**, 0 mismatches |
| mtime trap | `find src-tauri -name '*.rs' -o -name '*.toml' \| xargs touch` after extraction (`TOUCHED_AT 2026-10-04T23:48:04+09:00`) |
| Observed recompilation | gate 1's log shows a real `Compiling ferryx v2026.928.7 (…/source-21dea3c0/src-tauri)`, never `Fresh` |
| Ghostty pin | `src-tauri/vendor/ghostty -> /home/indo/ferryx-pane-completion/ghostty-21dea3c0` @ `6a508fd5e34c7e222c052a6d00bb3891ff3feace` (unchanged) |

File hashes at `314251e0` (for reuse):

```
scripts/lib/qa-scenarios/common-harness.mjs ec15855d53f41d9a3685aa81ab96ef23e6f48baf28a478ffd25816954a121505
scripts/qa/pane-liveness.mjs                b5cb88c26bf368eff178969b0e7ec3a1b087c76bd7fd1fe24310df2f70946a83
scripts/qa/pane-liveness.test.mjs           0dedd762d69c3491070bc92457f5c4d335bfe39c529c88a8dad61ae863e7f23a
src-tauri/src/daemon/qa_producers.rs        9d5f4b6bf0b4de90f072a312f2da7ba9d8c6f40406742ac34c5af3420e823a37
src-tauri/src/daemon/server.rs              d3e905f170791db762efce730cbb794e643c60ae72444bd8cf250d78121007ad
src-tauri/src/ipc/qa_barrier.rs             e0c23509632ee194bfaeee2db13eb46200be49cddc33ab3c243aae87d4b4e56e
src-tauri/src/ipc/terminal.rs               776f423d37170cf8714465c91c0e3e9a898e0e0ab51897cc069d12e08dc98733
```

---

## 1. Gate table — linux (omaki), cwd `source-21dea3c0` @ `314251e0`

| # | Exact command | Raw exit | Selected | Asserted line | Verdict |
|---|---|---|---|---|---|
| 1 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **0** | — | `Finished dev profile [unoptimized + debuginfo] target(s) in 1m 41s` | **PASS** (was **101** at `c34b90fc`) |
| 2 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | `Finished dev profile … in 19.10s` | **PASS** |
| 3 | `cargo build --manifest-path src-tauri/Cargo.toml` (default) | **0** | — | `Finished dev profile …` | **PASS** |
| 4 | `cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa` | **0** | — | `Finished dev profile …` | **PASS** — QA binary produced |
| 5 | `--lib --features local-split-qa qa_barrier -- --list` | **0** | **19** | 19 `: test` lines | **PASS** (was 11/0 at `70eefafe`) |
| 6 | `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | **0** | **19** | `test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 2730 filtered out; finished in 0.06s` | **PASS** |
| 7 | `--lib --features local-split-qa qa_producers -- --list` | **0** | **10** | 10 `: test` lines | **PASS** |
| 8 | `--lib --features local-split-qa qa_producers -- --nocapture --test-threads=1` | **0** | **10** | `ok. 10 passed; 0 failed; … finished in 0.00s` | **PASS** |
| 9 | `--lib --features local-split-qa qa_liveness -- --list` | **0** | **13** | 13 `: test` lines | **PASS** |
| 10 | `--lib --features local-split-qa qa_liveness -- --nocapture --test-threads=1` | **0** | **13** | `ok. 13 passed; 0 failed; … finished in 0.67s` | **PASS** |
| 11 | `--lib --features local-split-qa qa_split_producers -- --list` | **0** | **15** | 15 `: test` lines | **PASS** (the predecessor's source-derived "would select 15" is confirmed by execution) |
| 12 | `--lib --features local-split-qa qa_split_producers -- --nocapture --test-threads=1` | **101** | **15** | `FAILED. 14 passed; 1 failed; … 2734 filtered out; finished in 0.00s` | **FAIL — 1 test** (see §3) |
| 13 | negative control: `node scripts/qa/pane-liveness.mjs --scenario split-happy --binary <QA binary> --evidence-dir /tmp/t9p2-linux-ev --isolation-root /tmp/t9p2-linux-iso` | **4** | — | `NATIVE_AUTOMATION_UNSUPPORTED: native desktop automation is not implemented for platform linux` → `verdict: BLOCKED`, `cleanupGate.ok` present | **structurally-not-run on linux** (the runner is macOS/Windows only; it refuses before any launch) |

Logs: `linux/logs/01-qa-check.log`, `02-default-check.log`, `02b-default-build.log`,
`03-list-*.log` ×4, `04-run-*.log` ×4, `05-qa-build.log`, `linux/linux-scenario-split-happy.log`.

### 1.1 The dispatch's expected count corrected

The dispatch predicted "roughly 48 gated tests across the four selectors". The measured total is
**57 selected / 56 passed / 1 failed**: `qa_barrier` 19 + `qa_producers` 10 + `qa_liveness` 13 +
`qa_split_producers` 15 = **57**. (The dispatch's 48 came from the lead's earlier source estimate
of 15 + 10 + 13 + 10; `qa_barrier` actually holds 19 `#[test]` fns, not 10 — measured by `--list`,
not by grep.)

### 1.2 No QA surface leaks into a default build

The default `--all-targets` check compiles with the QA modules absent, and the default **binary**
carries no QA marker:

`linux/marker-scan-default.txt` — binary `sha256 a89c67c72b474d2aa6d878a3edef62ce281026841045c4e22d2dc34f2804bc38`:

| Marker | Default binary |
|---|---|
| `FERRYX_QA_BARRIER_DIR`, `FERRYX_QA_OPERATION_ID`, `FERRYX_QA_FIXTURE_SETUP_UNSETTLED`, `FERRYX_QA_RETRY_REFUSED`, `FERRYX_QA_STALE_BINDING_UNSERVICED` | **ABSENT** (5/5) |
| `collect_gui_fixture_sessions`, `start_gui_boot_channel`, `qa_producers`, `qa_liveness` | **ABSENT** (4/4) |
| `marker-output`, `split-create`, `attach-handshake`, `cancel-ack`, `held-rpc` | **ABSENT** (5/5) |
| `Attach requires the persisted seven-field pane binding`, `Attach binding incarnation cannot be proven` | **PRESENT** (2/2, candidate-lineage positive control from `5464da0d`) |

### 1.3 Positive control — the QA binary really carries the QA surface

`linux/marker-scan-qa.txt`, binary `sha256 c653b0343dadce9215dea26396bc71ebefcdc744f914b2aceccd61ad1242cdc4` (964 747 448 B):
**15/15 QA markers PRESENT**, including `collect_gui_fixture_sessions`, `start_gui_boot_channel`,
`qa_producers`, `qa_liveness`, `split-create`, `attach-handshake`, `cancel-ack`, `held-rpc`,
`marker-output`, `fixture-setup`.

**This corrects a predecessor honesty note.** The dispatch records that "the suggested
`strings`-for-a-new-symbol proof does NOT work on this host (MSVC keeps names in a `.pdb`)". That is
true of the **default** build only. On the **QA-feature** build the `local-split-qa`-gated string
literals ARE in the image (verified on both hosts: linux 15/15, Windows 16/18 with the same method),
so a positive revision proof *is* available for the QA binary by literal scan. On the default build
the literals are absent **by construction**, so the same scan is a valid *negative* control (no QA
surface ships) and an invalid revision proof — the predecessor's caveat is exactly right for that
binary and wrong for the QA one.

---

## 2. Was the predecessor's 16-site compile failure really fixed?

| Site (owning lane) | Fix in `314251e0` | Verified |
|---|---|---|
| `daemon/qa_producers.rs:372`, `:458` — `server.session_service()` called as a method | field access `server.session_service` / `server.session_service.session_metadata` | compiles (gate 1) |
| `daemon/server.rs:2923` — `Arc<DaemonServer>` where `&DaemonServer` required | `self.as_ref()` | compiles |
| `ipc/terminal.rs:2487`, `:3205`, `:3206` — `operation_state` typed for `<String>`, handed `<u64>` | generic `fn operation_state<Epoch>(state: &SplitOperationResult<Epoch>)` | compiles |
| `daemon/qa_producers.rs:682/708/738` — `PathBuf::path()` | `dir.join(...)` (the test-local `dir` binding) | compiles |
| `daemon/qa_producers.rs:771` — test-local `fn channel` shadowing `pub fn channel()` | renamed to `fn test_channel` (7 call sites) | compiles |
| `ipc/qa_barrier.rs:1222` — `Arc::clone(channel)` on `&QaBarrierChannel` | signature is now `channel: &Arc<QaBarrierChannel>`; the test builds `Arc::new(QaBarrierChannel::new(…))` | compiles |

**All 16 sites are closed.** The A/B direction is unambiguous because the *same tree* is exit 0 after
applying only this 7-file delta, while the predecessor measured the same command at **101** on
`c34b90fc` with the pre-delta hashes (`server.rs df27c535fefdb55b`) and at **0** on `70eefafe`.

### 2.1 The adapter role-map repair (`799a582d`) — blast radius 3 of 9

`common-harness.mjs` `BARRIER_ROLES` now maps the four non-handover barriers to `null`
(`backend-write`, `presentation`, `attach-handshake`, `held-rpc`) and keeps the four handover roles
legal. The regression lock (`pane-liveness.test.mjs:329-372`) replays the runner's exact pre-arm
expression for all nine scenarios; it passes inside the runner's own suite (29/29 recorded by the
predecessor at `c34b90fc`, and `SCENARIO_PLANS` is now exported so the replay does not need a launch).

**Blast-radius correction adopted from the lead, and confirmed by source:** the defect killed
**three of the nine** scenarios, not one — `diagnostic-classifier` (arms `backend-write` first),
`split-attach-stall` (arms `attach-handshake`) and `split-concurrent` (arms `held-rpc`), because
`pane-liveness.mjs:265` pre-arms before every launch. Two of those three are mac-side and stay
parked; the Windows-eligible one is `split-attach-stall`. **`split-attach-stall` now reaches a
launch** — verified in §3/`../win-pass2/`: its failure is no longer the pre-launch
`ASSERTION_FAILURE: targetRole … got "producer"`.

---

## 3. The one gated test that FAILS — verbatim, with its owning lane

Command (verbatim, in gate 12):

```
cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_split_producers -- --nocapture --test-threads=1
```

```
running 15 tests
test ipc::terminal::qa_split_producers::tests::retry_and_batch_controls_are_read_only_for_this_run ...
thread 'ipc::terminal::qa_split_producers::tests::retry_and_batch_controls_are_read_only_for_this_run' (3312498) panicked at src/ipc/terminal.rs:3876:17:
assertion failed: read_control(&dir, name, &channel).is_some()
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED
test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 2734 filtered out; finished in 0.00s
error: test failed, to rerun pass `--lib`
```

**Reproduced deterministically** in isolation (`0 passed; 1 failed` on a single-filter rerun on the
same host), so it is not a load or scheduling artifact.

### 3.1 Root cause (source-derived, reproduced)

Owning lane: **the QA producer/consumer lane that authored `ipc/terminal.rs`'s `qa_split_producers`
module** (test-only; no product behaviour is affected).

The test builds a channel and immediately writes a control that echoes the run id and a nonce, then
requires `read_control(..).is_some()`:

```rust
let channel = QaBarrierChannel::new(dir.clone(), "qa-run-gui".into());
for name in [RETRY, SPLIT_CONCURRENT_BATCH] { … assert!(read_control(&dir, name, &channel).is_some()); }
```

but it never supplies the operation nonce, and `QaBarrierChannel::new` leaves `operation_nonce: None`
(`qa_barrier.rs:311`). `read_control` refuses anything it cannot correlate:

```rust
let expected = channel.operation_id()?;                     // → None
if value.get("operationId")… != Some(expected.as_str()) { return None; }
```

and `operation_id()` falls back to the first *armed* spec only:

```rust
pub fn operation_id(&self) -> Option<String> {
    if let Some(nonce) = &self.operation_nonce { return Some(nonce.clone()); }
    self.arms.lock().ok().and_then(|arms| arms.values().next().map(|s| s.operation_id.clone()))
}
```

With no arm file written and no env nonce, `operation_id()` is `None`, so **every** control is refused
— including the one the test expects to be honored. Its sibling
`control_files_are_read_only_for_this_run` (`:3446`) does call `channel.scan_and_ack_arms()` after
writing an arm and **passes**, which pins the missing step.

**Classification: test-only defect, candidate-caused.** The production path is not implicated — the
runner always exports `FERRYX_QA_OPERATION_ID` (`common-harness.mjs` `BarrierHub.env()`), and the
`read_control` refusal is the intended fail-closed behaviour for an uncorrelatable control. The
assertion's intent (a control echoing this run's nonces is honored, one that does not is refused) is
correct; the fixture just never establishes the run's operation identity.

**Not repaired by me** — the verifier does not edit product or test code. Routed to the owning lane, which
repaired it and committed it as **`5245e6ba`** (the fixture writes the `RETRY` arm and calls
`scan_and_ack_arms()`; assertions untouched). **Re-measure at `5245e6ba`; do not cite this 14/15 as
current.**

---

## 4. Residual classification (linux)

| Item | Class | Evidence |
|---|---|---|
| `--features local-split-qa` compile failure at `c34b90fc` (16 sites) | **candidate-caused, now FIXED** | gate 1 exit 0 at `314251e0`; predecessor A/B exit 101 at `c34b90fc` with pre-delta hashes |
| `qa_barrier` gate previously uncompilable (11/0 at `70eefafe`) | **regression closed** | gate 5/6: 19 selected, `ok. 19 passed; 0 failed` |
| adapter `BARRIER_ROLES` self-contradiction | **candidate-caused, now FIXED** | `799a582d`; pre-arm replay test green; `split-attach-stall` reaches a launch |
| `qa_split_producers::retry_and_batch_controls_are_read_only_for_this_run` | **candidate-caused, test-only → FIXED at `5245e6ba`** | §3, deterministic reproduction |
| Default build / default binary gaining QA symbols | **not observed** | gate 2/3 exit 0; marker scan 0/14 QA markers present |
| The dispatch's "~48 gated tests" estimate | **corrected** | measured 57 via `--list` on all four selectors |
| `strings`-for-a-symbol revision proof "impossible on maho-win" | **corrected for the QA binary** | QA binary carries 15/15 (linux) and 16/18 (Windows) QA literals; the caveat holds only for the default build |

## 5. What this pass did NOT measure (honest scope)

- The mac-only `cfg` arms of the QA compile gate (maho-mac parked by user instruction).
- The Windows side of gates 5–12 (the QA **binary** build was measured on Windows — see
  `../win-pass2/`; the QA **lib test** selectors were not run on Windows in this pass).
- The four remaining default-feature gates that the predecessor already measured green at `c34b90fc`
  (`local_split_reliability_`, `pane_liveness_`, `daemon_handover_transfer_contract`, runner vitest):
  not re-run, because the pass-2 delta touches no file those gates select (`local_split_reliability_`
  and `pane_liveness_` live in `daemon/split_journal.rs`, `ipc/pane_liveness*`, `terminal/**`; the
  delta touches `daemon/qa_producers.rs`, `daemon/server.rs`, `ipc/qa_barrier.rs`, `ipc/terminal.rs`
  only inside `#[cfg(all(feature = "local-split-qa", …))]` blocks or in test-only lines).
- The full `--lib` suite (the predecessor's 2646/31/6 at `c34b90fc`, all 31 classified pre-existing
  or flaky) — not re-run for the same reason.

## 6. Cleanup receipts (linux)

| Resource | Teardown | Receipt |
|---|---|---|
| background gate sessions (`bash_25`, `bash_36`, `bash_106`) | completed on their own (exit 0) | final lines `GATE1_QA_CHECK_EXIT=0`, `LINUX_GATES_PHASE_A_DONE`, `LINUX_QABUILD_DONE` |
| monitors `mon_50JEX709QCM9FDQ5`, `mon_8NZ0S65MF7KNNG0Q`, `mon_2TAA794WFK7R8DJY` | completed on their sentinels | `watcher completed (exit code 0)` |
| `/tmp/t9p2-linux-iso`, `/tmp/t9p2-linux-ev` (the linux negative control's roots) | removed by the runner's own `cleanupGate` before the run returned | `cleanupGate.ok` in `linux/linux-scenario-split-happy.log`; re-checked absent |
| staged tree `source-21dea3c0` | **retained** as the evidence source (owned) | — |
| foreign trees on the host (`task8-*`, `task9-c34b90fc`, other sessions' `target/`) | **untouched** | not this session's to remove |
