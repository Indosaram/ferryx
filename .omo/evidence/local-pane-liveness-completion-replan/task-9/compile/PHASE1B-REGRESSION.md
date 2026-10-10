# Task 9 — Phase 1b: default-feature regression gates and the QA-compile A/B

Companion to `PHASE1-COMPILE-VERDICT.md`. Host: linux **omaki** (`indo@100.91.254.71`), staged tree
`/home/indo/ferryx-pane-completion/source-21dea3c0` at `c34b90fc` (5086/5086 manifest rows matching).

## 1. Decisive A/B — is the QA compile failure introduced by this work?

Question: does `cargo check --all-targets --features local-split-qa` fail at `70eefafe` (the last
revision before the QA producer surface landed) as well, or only from `dd9e6813`/`c34b90fc` onward?

Method: in the same tree, restore the **17 modified files** to `70eefafe`, **delete** the two new
modules, `touch` all Rust sources, check; then restore `c34b90fc` (17 files + 2 new) and check again.
Both sides verified by source hash before running:

| Side | `src-tauri/src/daemon/server.rs` sha256 prefix | Expected |
|---|---|---|
| A = `70eefafe` | `59f4d07d0380d608` | `59f4d07d0380d608` ✓ |
| B = `c34b90fc` | `df27c535fefdb55b` | `df27c535fefdb55b` ✓ |

| Side | Exact command | Raw exit | Errors | Result |
|---|---|---|---|---|
| **A `70eefafe`** | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features local-split-qa` | **0** | **0** | `Finished dev profile [unoptimized + debuginfo] target(s) in 6m 13s` |
| **B `c34b90fc`** | same | **101** | **16 sites** | see `PHASE1-COMPILE-VERDICT.md` §2.2 |

**Conclusion: the QA feature combination compiles cleanly at `70eefafe`.** The failure is introduced by
the QA-producer work that landed in `dd9e6813` + `c34b90fc` — **candidate-caused, with A/B evidence**,
not a pre-existing property of the tree or the toolchain.

Logs: `compile/linux/logs/11-ab-70eefafe-qa-check.log`, `12-ab-c34b90fc-qa-check.log`.

## 2. Previously-green gates that do not need the QA feature (linux, `c34b90fc`)

| Exact command | Raw exit | Selected | Result line | Task 8 @ `70eefafe` | Verdict |
|---|---|---|---|---|---|
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | 15 | `ok. 15 passed; 0 failed` (0.01 s) | 15/0 | **PASS** |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | 54 | `ok. 54 passed; 0 failed` (1.39 s) | 54/0 | **PASS** |
| `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | **0** | 5 | `ok. 5 passed; 0 failed` (5.81 s) | 5/0 (6.21 s) | **PASS** |
| `--lib -- --test-threads=1` | **101** | 2683 | `FAILED. 2646 passed; 31 failed; 6 ignored` (457.89 s) | 2647 / 30 / 6 | **FAIL — +1 flaky** |
| `--lib --features local-split-qa qa_barrier` | — | — | **cannot compile** | `ok. 11 passed` | **NOT_RUN_BLOCKED — regression** |

### 2.1 Full-`--lib` set diff by test name

Set-comparing the failing test names against the recorded `70eefafe` failure list:

| | Count |
|---|---|
| failing at `70eefafe` | 30 |
| failing at `c34b90fc` | 31 |
| **left** | **0** |
| **joined** | **1** |

Joined: `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`.
Classification: **flaky, already recorded as such by Task 8** ("linux `input_is_cancelled_when_grant_is_revoked`
flaky on both sides"); it is also absent from the `39e722ce` set. `c34b90fc`'s 14-file diff and
`dd9e6813`'s 7-file diff contain **no file under `src/remote/`**, so this code path is untouched by the
QA work.

### 2.2 Flaky characterisation — 3 isolated repetitions (`--test-threads=1`)

Exact command: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked -- --nocapture --test-threads=1`

| Rep | Raw exit | Result line |
|---|---|---|
| 1 | **101** | `FAILED. 0 passed; 1 failed; … finished in 4.22s` |
| 2 | **0** | `ok. 1 passed; 0 failed; … finished in 0.54s` |
| 3 | **0** | `ok. 1 passed; 0 failed; … finished in 0.55s` |

**1 of 3 failed — non-deterministic, confirming the flaky classification.** The failure is inside the
test's own support harness, not product logic:

```
test remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked ... A10 kernel WouldBlock original_pid=3122408 accepted=11776
thread '…' panicked at src/remote/../../tests/support/machine_input_cancellation.rs:31:14:
FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2682 filtered out; finished in 4.22s
```

The panic site is in **test support code neither QA commit touches**
(`git diff --name-only 70eefafe c34b90fc -- src-tauri/src/remote/` = 0 files; `dd9e6813` touches 0 files
under `src/remote/`). Logs: `13-flaky-rep-1..3.log`.

**Zero new candidate-caused failures** in the full library suite.

## 3. Windows default-feature gates (maho-win, `c34b90fc`)

Host: load 62 %, 32.41 GiB free, uptime 1d12h44m.

| Exact command | Raw exit | Result line | Task 8 @ `39e722ce` | Verdict |
|---|---|---|---|---|
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | `ok. 50 passed; 0 failed; 0 ignored` (0.12 s) | 43/0 | **PASS** — count follows the `70eefafe` adoption fix, not the QA surface (see note) |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | `ok. 14 passed; 0 failed; 0 ignored` (0.15 s) | 14/0 | **PASS** |
| `--lib split_journal -- --nocapture --test-threads=1` | **0** | `ok. 7 passed; 0 failed; 0 ignored` (0.07 s) | 7/0 | **PASS** |
| `--lib -- --test-threads=1` | (see `win-06-full-lib.log`) | — | 2201 passed / 70 failed / 8 ignored at `abd9e890` | recorded below |
| `--features local-split-qa` (any test) | — | **cannot compile** | — | **NOT_RUN_BLOCKED** |
| `--test daemon_handover_transfer_contract` / `ipc_hardening_contract` | — | targets are `#![cfg(unix)]` → compile to nothing | 0 selected | structurally-not-applicable (**not a pass**) |

The `pane_liveness_` selector count on Windows is 43 at `39e722ce` → **50** here, and linux is 47 → **54**
over the same span. That `+7` is attributable to **`70eefafe`'s seven adoption-contract tests**, not to
the QA surface: linux measures 54 at `70eefafe` *and* at `c34b90fc`, so the QA commits add **zero**
`pane_liveness_` selectors. Windows is exactly 4 below linux on both revisions (the four unix-only
presentation tests). **None of the 48 QA-gated tests can be counted in any of these numbers**, because
their modules do not compile under this feature set.

## 4. Runner unit suite (`scripts/qa/pane-liveness.test.mjs`), both hosts

Exact command: `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs`

| Host | Raw exit | Result |
|---|---|---|
| linux | **0** | `Test Files 1 passed (1)` / `Tests 29 passed (29)` in 1.06 s |
| Windows | **0** | `Test Files 1 passed (1)` / `Tests 29 passed (29)` in 1.06 s |

Task 8 recorded 28/28 at `21dea3c0`; `c34b90fc` adds one case (29). The runner's own unit suite is
green on both hosts — but per the brief, **runner unit tests are not native execution** and are
recorded here only as a regression signal.

## 5. UI half

`ui/` is **unchanged** between `39e722ce` and `c34b90fc` (0 files), so Task 8's UI numbers
(build exit 0 ×3; split 19/19 ×3; lifecycle 112/112 ×3; full UI per host) remain the applicable
measurement for this revision and are cited, not re-run. The only `ui/` file touched after
`21dea3c0` is `terminalTransport.test.ts` (repaired in `7c0fed30`, re-verified 11/11 ×3).
