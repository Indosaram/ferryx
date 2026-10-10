# Task 8 — FINAL per-host verdict table

Candidate **`abd9e890`** (tree clean), base **`d82b35e4`**.
Branch `work/local-pane-liveness-completion-foundation`.
Sole remote verifier; **no build or test ever ran on the control Mac** — every command below ran on
the named host. Task 9 (native matrix) and Task 10 (packaging) are out of scope for this dispatch.

Chain: `21dea3c0` → `764934a4` → `7c0fed30` → `17295029` → `b0ed4bef` → `d97233c1` → `6c69715f` → **`abd9e890`**

Verdict vocabulary: **RAN_PASSED** / **RAN_FAILED** / **NOT_RUN_BLOCKED** (a gate whose prerequisite
failed and which therefore never executed — never a pass) / **NOT_RUN**.

---

## 1. Criterion 1 — verbatim linux retrievals  → **PASS**

| Artifact | Content |
| --- | --- |
| `linux/RETRIEVAL-daemon_persistence_contract-failure.md` | failing test `test_daemon_output_sequence_contiguity_and_replay_gap`; panic at `tests/daemon_persistence_contract.rs:1098:9`; `assertion \`left == right\` failed / left: 0 / right: 1`; 15 selected, 14 passed / 1 failed, exit 101 |
| `linux/RETRIEVAL-daemon_handover_transfer_contract-compile-error.md` | `error[E0063]: missing field \`local_split\` in initializer of \`DaemonRequest\` → tests/daemon_handover_transfer_contract.rs:109:28`; `could not compile \`ferryx\` (test "daemon_handover_transfer_contract")`; exit 101 |

---

## 2. Criterion 2 — `cargo check --all-targets` at abd9e890  → **RAN_FAILED on mac+linux, RAN_PASSED on windows**

| Exact command | Host | Raw native exit | Asserted line | Verdict | Evidence |
| --- | --- | --- | --- | --- | --- |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | mac | **101** | `error[E0063] … tests/daemon_handover_transfer_contract.rs:109:28` + `tests/ipc_hardening_contract.rs:110:9` | **RAN_FAILED** | `mac/logs/all-targets.log`, `mac/rustA.log` |
| same | linux | **101** | identical two sites | **RAN_FAILED** | `linux/logs/all-targets.log`, `linux/rustA.log` |
| same | windows | **0** | `Finished` | RAN_PASSED | `windows/logs/all-targets.log`, `windows/rustA.log` |

Remaining sites (identical on mac and linux):

```
error[E0063]: missing field `local_split` in initializer of `DaemonRequest`
   --> tests/daemon_handover_transfer_contract.rs:109:28

error[E0063]: missing fields `create_only`, `prepared_local_split` and `remaining_ms` in initializer of `SpawnTerminalRequest`
   --> tests/ipc_hardening_contract.rs:110:9
```

**Definitive site list via per-target sweep** (`linux/rustC-targets.log`): **81 test targets → 79 OK /
2 FAIL**, **12 examples → 12 OK** (including `ssh_password_fixture`, which the worker repaired). The
repo holds 81 `src-tauri/tests/*.rs` and 12 `src-tauri/examples/*.rs` files, so the sweep covered every
target. A single
`--all-targets` run does **not** list every broken target — cargo stops scheduling new targets after a
failure — so the sweep is the authoritative evidence. Full analysis: `ALL-TARGETS-VERDICT.md`.

---

## 3. Criterion 3 — `local_split_reliability_` at abd9e890  → **RAN_PASSED x3**

| Exact command | Host | Raw native exit | Asserted line | Selected | Verdict | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| `--lib local_split_reliability_ -- --list` | mac | 0 | selectors listed | 15 | RAN_PASSED | `mac/logs/split-list.log` |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | mac | **0** | `test result: ok. 15 passed; 0 failed` | 15 | **RAN_PASSED** | `mac/logs/split.log` |
| `--lib local_split_reliability_ -- --list` | linux | 0 | selectors listed | 15 | RAN_PASSED | `linux/logs/split-list.log` |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | linux | **0** | `test result: ok. 15 passed; 0 failed` | 15 | **RAN_PASSED** | `linux/logs/split.log` |
| `--lib local_split_reliability_ -- --list` | windows | 0 | selectors listed | 14 | RAN_PASSED | `windows/logs/split-list.log` |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | windows | **0** | `test result: ok. 14 passed; 0 failed` | 14 | **RAN_PASSED** | `windows/logs/split.log` |
| `--lib split_journal -- --list` / run | linux | 0 / 0 | `ok. 7 passed; 0 failed` | 7 | RAN_PASSED | `linux/logs/journal-list.log`, `journal.log` |
| `--lib split_journal -- --list` / run | windows | 0 / 0 | `ok. 7 passed; 0 failed` | 7 | RAN_PASSED | `windows/logs/journal-list.log`, `journal.log` |

**The five Windows `Io(Os { code: 123, kind: InvalidFilename })` failures are gone** (Windows went from
9 passed / 5 failed to 14 passed / 0 failed, native exit 0).

**Selected count is 14/15/15, not the predicted 16/16/15.** The worker's new test
`local_split_temp_names_are_portable_bounded_and_unique` lives in `daemon::split_journal::tests::`, so it
does not match the `local_split_reliability_` filter. It is proven directly with the `split_journal`
filter instead: 7 selected and 7 passed on **both** linux and windows, the new test name present and
`ok` on both.

**Mutation proof of the new test** (linux): reverting the naming to the pre-fix
`thread_name.unwrap_or("writer").to_string()` → `mutated_exit=101`, panic at `split_journal.rs:421`
(RED); restoring the file (sha256 back to `412870e8…`) → exit 0 (GREEN).

---

## 4. Criterion 4 — full `--lib` classification vs base d82b35e4  → **PASS (all three hosts)**

| Host | Candidate abd9e890 | Base d82b35e4 | candidate-caused | pre-existing | flaky | unclassified | File |
| --- | --- | --- | --- | --- | --- | --- | --- |
| linux | 2629 passed / **37 failed** / 6 ignored | 2554 passed / **31 failed** / 6 ignored (1 skipped) | **6** | 30 | 1 | **0** | `linux/CLASSIFICATION-linux.md` |
| mac | 2635 passed / **49 failed** / 6 ignored | 2557 passed / **44 failed** / 6 ignored (2 skipped) | **6** | 43 | 0 | **0** | `mac/CLASSIFICATION-mac.md` |
| windows | 2201 passed / **70 failed** / 8 ignored | **bounded at 2174/2280 tests** (`full-lib-base-PARTIAL.log`) | **5** | 65 | 0 | **0** | `windows/CLASSIFICATION-windows.md` |

Both sides were run with the same command (`--lib -- --test-threads=1`) on the same host.

**Windows method note (declared, not blurred).** The Windows base run was **bounded** at 2174 of 2280
tests: it was spinning on `worktree::disk_tests::disk_scan_plain_folder_has_no_worktrees` (a Windows
temp-dir scan hazard — the candidate run also spun on that test for ~6 minutes). Its explicit verdict
lines are still a valid A/B oracle for every test it reached, so classification used a **strict rule**:
`pre-existing` requires an explicit `test NAME ... FAILED` line at base, `candidate-caused` requires an
explicit `test NAME ... ok` line, and anything else stays `unclassified`. 9 tests started at base
without a verdict line, so that rule is load-bearing. The one test the base run never reached,
`worktree::tests::plain_folder_without_git_registers_and_guards_worktrees`, was resolved by a scoped
run: it is `FAILED. 0 passed; 1 failed` **at base too** (`assertion left == right`, a `\\?\` long-path
truncation at `src/worktree/mod.rs:49`), so it is pre-existing and the Windows set closes at
**5 candidate-caused / 65 pre-existing / 0 unclassified**.

**The four `surface_host` presentation failures are candidate-caused on all three hosts.** The two
`ipc::tests` attach failures are candidate-caused on mac and linux and **cannot exist on Windows**:
`src-tauri/src/ipc/mod.rs:57-58` gates the whole module behind `#[cfg(all(test, unix))]` (verified —
0 grep hits for those names in the Windows log, 4 hits each in the mac log). Windows shows one further
candidate-caused test, `daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent`
(base full suite **ok** → candidate full suite **FAILED** with
`Other("Timed out waiting for PTY reader shutdown")` at `session_service_machine_tests.rs:344:63`).

**This row is UNRESOLVED BY REPETITION, not a confirmed regression.** Scoped repetition was
inconclusive on **both** sides: two base attempts each stopped at an identical 27890 bytes with no
`test result:` line and no timeout panic, and two candidate attempts stopped after emitting the timeout
panic — **no scoped run on either side produced a verdict**, so `native=-1` in
`windows/ab2-windows.log` / `windows/ab3-windows.log` is a kill artifact from the verifier terminating
the run, not an outcome. The row therefore rests on the single full-suite A/B, which cannot separate a
real candidate regression from a load-sensitive flake or from the candidate newly **detecting** a
pre-existing condition (base has no timeout detection of its own, and its scoped run also never
completed). Full evidence: `windows/CLASSIFICATION-windows.md`, section "Exact scoped-A/B evidence".

**The mac and linux candidate-caused sets are exactly the same six tests** (set comparison: nothing
mac-only, nothing linux-only), and they were proven deterministic with a **two-repetition isolated A/B
per side** on linux (`linux/ab-classify2.log` + `linux/logs/ab/`, 32 runs):

| Test | Panic site | Message |
| --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | `src/ipc/tests.rs:313:6` | `UnsupportedCapability: "Attach requires the persisted seven-field pane binding; pass attachTuple"` |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | `src/ipc/tests.rs:3426:6` | same |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | `surface_host.rs:5961:9` | `assertion failed: receipt.presented` |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | `surface_host.rs:6183:13` | `assertion failed: latest.presented` |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | `surface_host.rs:6269:17` | `assertion failed: result.unwrap().presented` |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | `surface_host.rs:6072:9` | `assertion failed: receipt.presented` |

All four `surface_host` names exist at base (`git grep` = 1 each) and `surface_host.rs` grew
+1373/−49 in the candidate, so the break is the candidate's, not a renamed or absent test.

**Flaky, NOT candidate-caused** —
`remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`: base
[PASS, FAIL] vs candidate [FAIL, PASS]. A single-run A/B mislabels it; repetition is what caught it.

### `zero_config_gen5_regression` — 4 failures, A/B-proven PRE-EXISTING

Candidate linux `2 passed; 4 failed` (31.10 s) vs base linux `2 passed; 4 failed` (29.50 s) — the same
four names with the same messages (line numbers differ by 3 because the candidate inserted 3 fixture
lines). Evidence: `linux/logs/gen5-base.log`, `linux/logs/zero_config_gen5_regression.log`.

### Hangs found (reported, never suppressed; skipped tests are NOT_RUN, never passes)

| Host / side | Hung at | Treatment |
| --- | --- | --- |
| linux base | `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution` | SIGTERM after 1036 tests; `linux/logs/full-lib-base-HUNG.log`; rerun with `--skip` |
| mac base | the same test | SIGTERM after 1035 tests; `mac/logs/full-lib-base-HUNG.log`; rerun with `--skip` |
| mac base (2nd) | `remote::machine_operation_journal::contention_tests::journal_mutation_deadline_retains_admission_until_worker_drains` | SIGTERM after 1707 tests; `mac/logs/full-lib-base-HUNG2.log`; both now skipped |
| windows candidate | `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection` | killed only this session's pids; `windows/logs/full-lib-HUNG.log`; rerun with `--skip` |

The base-hung test **passes at the candidate** on both mac and linux; the Windows-hung test **fails**
(does not hang) on linux and mac at the candidate with
`bridge_tests.rs:759 "Loopback SSH must be available for live OpenSSH bridge test"`. Both hangs are
environment stalls, not candidate logic.

---

## 5. Criterion 5 — established gates re-cited

### Rust gates at abd9e890

| Exact command | Host | Raw native exit | Asserted line | Selected | Verdict | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| `--lib pane_liveness_ -- --list` / run | mac | 0 / **0** | `ok. 47 passed; 0 failed` | 47 | **RAN_PASSED** | `mac/logs/pane-list.log`, `pane.log` |
| `--lib pane_liveness_ -- --list` / run | linux | 0 / **0** | `ok. 47 passed; 0 failed` | 47 | **RAN_PASSED** | `linux/logs/pane-list.log`, `pane.log` |
| `--lib pane_liveness_ -- --list` / run | windows | 0 / **0** | `ok. 43 passed; 0 failed` | 43 | **RAN_PASSED** | `windows/logs/pane-list.log`, `pane.log` |
| `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | linux | **0** | `ok. 11 passed; 0 failed` | 11 | **RAN_PASSED** | `linux/logs/qa_barrier.log` |
| `--test daemon_handover_contract -- --nocapture --test-threads=1` | linux | **0** | `ok. 5 passed; 0 failed` | 5 | **RAN_PASSED** | `linux/logs/daemon_handover_contract.log` |
| `--test daemon_persistence_contract -- --nocapture --test-threads=1` | linux | **101** | `FAILED. 14 passed; 1 failed` | 15 | **RAN_FAILED — PRE-EXISTING (A/B proven)** | `linux/logs/daemon_persistence_contract.log`, `linux/SINGLE-TEST-AB-daemon_persistence_contiguity.md` |
| `--lib suspension -- --nocapture --test-threads=1` | linux | **0** | `ok. 12 passed; 0 failed` | 12 | **RAN_PASSED** | `linux/logs/unix-suspension.log` |
| `--test zero_config_gen4_audit -- --nocapture --test-threads=1` | linux | 0 | `ok. 0 passed; 0 failed; 3 ignored` | 3 (all `#[ignore]`) | RAN_PASSED | `linux/logs/zero_config_gen4_audit.log` |
| `--test daemon_handover_transfer_contract -- --list` | mac / linux | **101** / **101** | `error[E0063] … :109:28` | — | **RAN_FAILED** | `{mac,linux}/logs/transfer-list.log` |
| `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | mac / linux | — | prerequisite list gate failed | — | **NOT_RUN_BLOCKED** | `{mac,linux}/logs/transfer.log` |
| `--test daemon_handover_transfer_contract -- --list` / run | windows | 0 / 0 | `ok. 0 passed; 0 failed` | **0** | RAN_PASSED (file is `#![cfg(unix)]`; 0 selected, **not** a skip) | `windows/logs/transfer-list.log`, `transfer.log` |
| `--test ipc_hardening_contract -- --nocapture --test-threads=1` | linux | **101** | E0063 `SpawnTerminalRequest` missing 3 fields | — | **RAN_FAILED (compile)** | `linux/logs/ipc_hardening_contract.log` |

### UI gates (established in pass 3; `ui/dist` bound to `21dea3c0`, unchanged by the Rust-only deltas)

| Exact command | mac | linux | windows | Evidence |
| --- | --- | --- | --- | --- |
| `bun run --cwd ui build` | 0 — `✓ built in 11.57s` | 0 — `✓ built in 5.15s` | 0 — `✓ built in 5.78s` | `*/logs/ui-build.log` |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | 0 — 19/19 | 0 — 19/19 | 0 — 19/19 | `*/logs/ui-split.log` |
| lifecycle 4-file command | 0 — 112/112 | 0 — 112/112 | 0 — 112/112 | `*/logs/ui-lifecycle.log` |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | 0 — 28/28 | 0 — 28/28 | 0 — 28/28 | `*/logs/runner.log` |
| `bun run --cwd ui test` (full) | 1 — 192 failed / 6329 passed (6521) | 1 — 1224 failed / 5297 passed (6521) | 1 — 193 failed / 6323 passed / 5 skipped (6521) | `*/logs/full-ui.log` |

The full-UI runs are baseline-dominated; the only candidate-caused UI regression found in pass 3 was
`terminalTransport.test.ts`, repaired in `7c0fed30` and re-verified 11/11 on all three hosts.

---

## 6. Criterion 6 — cleanup receipts

Full inventory: `CLEANUP-INVENTORY.md`. Consolidated receipts: `CLEANUP-RECEIPTS.md`.

| Resource | Teardown | Receipt |
| --- | --- | --- |
| mac superseded staging (`source`, `task8-172baa87`, `task8-5464da0d`) | `rm -rf` | `mac/DISK-RECLAIM-RECEIPT.md` — freed 5.1G (df 127 MiB → 5.2 GiB) |
| mac `source-21dea3c0/target` (regenerable build cache) | `rm -rf` | recorded in `mac/DISK-RECLAIM-RECEIPT.md` — freed 17G (→ 18 GiB) |
| windows superseded `ferryx-pane-completion\source` | `Remove-Item -Recurse -Force` | `windows/DISK-RECLAIM-RECEIPT.md` — freed 24.21 GB (17.6 → 40.4 GB free) |
| mac base-A/B trees (`source-base`, `task8-base`) | `sh cleanup-pass4-unix.sh mac` | `CLEANUP_OK mac sourceBaseAbsent=true task8BaseAbsent=true freeBefore=12Gi freeAfter=18Gi` |
| linux base-A/B trees (`source-base`, `task8-base`, `task8-21dea3c0-base`) | `sh cleanup-pass4-unix.sh linux` | `CLEANUP_OK linux sourceBaseAbsent=true task8BaseAbsent=true freeBefore=450G freeAfter=457G` |
| 42 local temp files (tarballs, scripts, name lists) | `rm -f` | only 4 remain in use (`classify-lib-failures.mjs`, `ulw.win-lib-fails.txt`, `win-base-check.ps1`, `win-base-prog.ps1`) |
| mac hung run logs | preserved, not deleted | `mac/logs/full-lib-base-HUNG.log`, `full-lib-base-HUNG2.log` |
| linux hung run log | preserved, not deleted | `linux/logs/full-lib-base-HUNG.log` |
| windows hung run log | preserved, not deleted | `windows/logs/full-lib-HUNG.log` |
| foreign `cargo.exe` processes on maho-win | **deliberately untouched** | not descendants of this dispatch's runner (parents = `rustup.exe`); another session's work |

Remote base-A/B trees and helper scripts are listed in `CLEANUP-INVENTORY.md` with their teardown
commands; their receipts are appended as they are removed.

---

# Addendum — follow-up dispatch results (HEAD `e57685ec`)

## A. BLOCKER: the current HEAD does not compile the lib test target

`error[E0308]` at **`src-tauri/src/ipc/tests.rs:309`** — `incarnation` is given a `String` where
`Option<String>` is required. Introduced by **`11c3a46a`** (the attach-fixture repair), not by
`e57685ec`. Because it is in the **lib test target**, every `cargo test --lib …` and
`cargo check --all-targets` fails on **all three hosts** (`could not compile ferryx (lib test)`).
Detail: `BLOCKER-11c3a46a-lib-test-does-not-compile.md`; raw log:
`linux/logs/E0308-lib-test-compile-failure.log`.

**Nothing at `e57685ec` can be verified until this one line is fixed.** The "four presentation tests
now pass" claim is therefore **UNVERIFIED**.

## B. Criterion 2 — `cargo check --all-targets` + sweep

| HEAD | mac | linux | Evidence |
| --- | --- | --- | --- |
| `ad0ffb5a` | **0**; sweep **81/81 targets, 12/12 examples** | **0**; sweep **81/81 targets, 12/12 examples** | `mac/rustE-targets.log`, `linux/rustE-targets.log`, `*/logs/all-targets.log` |
| `e57685ec` | **101** (blocker A) | **101** (blocker A) | `mac/rustF-gates.log`, `linux/rustF-gates.log` |

The two fixture sites that failed at `abd9e890` are **fixed** at `ad0ffb5a` — proven exhaustively by
per-target sweeps on both hosts, which is the definitive form given that a single `--all-targets` run
under-reports sites. The `e57685ec` result is the blocker, not a fixture problem.

## C. Owed A/B — `test_daemon_output_sequence_contiguity_and_replay_gap` (linux) → **PRE-EXISTING**

| Side | Raw native exit | Asserted line | Assertion |
| --- | --- | --- | --- |
| base `d82b35e4` | **101** | `tests/daemon_persistence_contract.rs:1093:9` | `assertion left == right failed` — `left: 0` / `right: 1` |
| candidate `abd9e890` | **101** | `tests/daemon_persistence_contract.rs:1098:9` | `assertion left == right failed` — `left: 0` / `right: 1` |

Both sides fail identically (the 5-line offset is the fixture lines the candidate inserted), so the
candidate did **not** introduce it. **Do not route a repair on the strength of a candidate
regression.** Detail: `linux/SINGLE-TEST-AB-daemon_persistence_contiguity.md`.

## D. Windows `interrupted_spawn` caveat — evidence complete

**UNRESOLVED BY REPETITION**, not a confirmed regression. Full command/host/log/byte-count evidence:
`windows/CLASSIFICATION-windows.md`, section "Exact scoped-A/B evidence".

## E. Paired/remote attach fence — **runtime behavior UNVERIFIED**

A real paired session could not be established in scope (no app daemon socket on any host, no relay
host, and no existing test drives `handle_describe_session` with a paired id — it is `pub(super)`).
Source-level A/B **is** decided: the fence
(`"Attach requires the persisted seven-field pane binding"`, `"Attach binding incarnation cannot be
proven"`) is **absent at base, present at candidate**, introduced by `5464da0d`; the paired branch's
`incarnation: None` is **pre-existing**. Detail and the exact settling experiments:
`PAIRED-ATTACH-FENCE-PROBE-STATUS.md`.
