# Task 8 — FINAL per-host verdict table

**STATUS: Task 8 is NOT acceptable at `39e722ce` — one confirmed candidate regression blocks it.**
`daemon_handover_transfer_contract` (one of the plan's four verbatim final-gate commands) passes at
base `ok. 5 passed; 0 failed` in **6.30 s** and fails at the candidate `1 passed; 4 failed` in
**336.47 s** (§6a). A backend repair is required; the verifier did not attempt it. Everything else in
this dispatch is green or classified with evidence.

**Self-contained handoff for Task 9.** Candidate worktree
`/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`,
branch `work/local-pane-liveness-completion-foundation`, **HEAD `39e722ce`**, base **`d82b35e4`**.
All commands ran on the named remote host; **nothing was built or tested on the control Mac**.

## Candidate chain (every commit since `21dea3c0`)

| Commit | Subject |
| --- | --- |
| `21dea3c0` | fix(pane-liveness): land round-3 backend, native, frontend and adapter repairs |
| `764934a4` | fix(native): clone the completion window before moving it into the render closure |
| `7c0fed30` | test(ui): bind the transport listSessions fixture to the incarnation contract |
| `17295029` | test(native): script one acquisition for the presentation accessor test |
| `b0ed4bef` | test(daemon): reconcile the create stage before asserting the spawn write |
| `d97233c1` | test(daemon): complete the integration fixtures for the new wire fields |
| `6c69715f` | test(daemon): complete the remaining all-targets fixtures for the new wire fields |
| `abd9e890` | fix(daemon): make the split journal temp name portable on Windows |
| `ad0ffb5a` | test(daemon): update the last two all-targets constructors for the widened wire types |
| `11c3a46a` | test(ipc): supply the persisted seven-field binding to the two attach fixtures |
| `e57685ec` | test(native): drive the real deferred presentation path in the four bounds-IPC tests |
| `426d1b27` | fix(ipc): wrap the attach fixture's incarnation in Some for the tuple field |
| **`39e722ce`** | **fix(daemon): give paired and ssh-remote sessions a provable lifetime incarnation** |

Verified candidate bytes on each host by sha256 before running: `session_service.rs` `7d1a1aaa`,
`ipc/terminal.rs` `3946a80b`, `paired_runtime.rs` `46344a52`, `ipc/tests.rs` `f7393930`,
`surface_host.rs` `98d35560`, `daemon_handover_transfer_contract.rs` `875151b3`,
`ipc_hardening_contract.rs` `e89551f1`.

## Verdict vocabulary

**RAN_PASSED** / **RAN_FAILED** / **RAN_FAILED (contaminated — result discarded)** / **NOT_RUN** /
**NOT_RUN_BLOCKED**. A **`0 selected`** result is **never** a pass: it is either a target gated
`#![cfg(unix)]` that compiles to nothing on Windows, or a mis-invoked command.

## 1. Compile gates

| Exact command | Host | Raw exit | Asserted line | Selected | Verdict | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | mac | **0** | `Finished`, errors=0 | — | **RAN_PASSED** | `mac/logs/H-all-targets.log` |
| same | linux | **0** | `Finished`, errors=0 | — | **RAN_PASSED** | `linux/logs/H-all-targets.log` |
| same | windows | **0** | `Finished`, errors=0 | — | **RAN_PASSED** | `windows/logs/H-all-targets.log` |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` | mac | **0** | selectors listed | **2693** | RAN_PASSED | `mac/logs/H-lib-compile.log` |
| same | linux | **0** | selectors listed | **2675** | RAN_PASSED | `linux/logs/H-lib-compile.log` |

The E0308 blocker (`src/ipc/tests.rs:309`) is fixed; the lib test target compiles everywhere.

## 2. The two new paired-incarnation tests

| Exact test | Host | Raw exit | Asserted line | Verdict |
| --- | --- | --- | --- | --- |
| `terminal::paired_runtime::tests::paired_session_incarnation_is_stable_per_actor_and_distinct_across_reincarnation` | mac / linux | **0 / 0** | `ok. 1 passed; 0 failed` | **RAN_PASSED** |
| `daemon::session_service::tests::paired_describe_reports_the_incarnation_the_attach_fence_proves` | mac / linux | **0 / 0** | `ok. 1 passed; 0 failed` | **RAN_PASSED** |

## 3. Attach pair — STATE CHANGED: FAILED → PASS

| Exact test | `abd9e890` | mac `39e722ce` | linux `39e722ce` |
| --- | --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | **FAILED** (`ipc/tests.rs:313:6`, UnsupportedCapability) | **0 — 1 passed** | **0 — 1 passed** |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | **FAILED** (`ipc/tests.rs:3426:6`) | **0 — 1 passed** | **0 — 1 passed** |

The p13 pass is the **settling experiment for the paired-fence question**: the fence *accepts* a proxy
session when the incarnation is provable, isolating the real-world failure to the daemon answering
`None` rather than to the fence's own logic.

## 4. The four `surface_host` presentation tests — STATE CHANGED: FAILED → PASS

Exact: `native_terminal::surface_host::tests::<name>`, `-- --exact`.

| Test | `abd9e890` (all 3) | mac | linux | windows |
| --- | --- | --- | --- | --- |
| `bounds_ipc_presents_when_browser_child_is_open` | **FAILED** | **0 — 1 passed** | **0 — 1 passed** | **0 — 1 passed** |
| `deferred_bounds_retry_does_not_restore_obsolete_width` | **FAILED** | **0 — 1 passed** | **0 — 1 passed** | **0 — 1 passed** |
| `synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | **FAILED** | **0 — 1 passed** | **0 — 1 passed** | **0 — 1 passed** |
| `synchronized_output_bounds_ipc_waits_for_actual_presentation` | **FAILED** | **0 — 1 passed** | **0 — 1 passed** | **0 — 1 passed** |

Author's "tests were wrong, production untouched" verdict is **VERIFIED by execution on all three
hosts**; every hunk sits inside `mod tests` (`git show` hunk headers).

## 5. Filter gates

| Exact command | Host | Raw exit | Asserted line | Selected | Verdict | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| `--lib pane_liveness_ -- --list` | linux | 0 | selectors listed | **47** | RAN_PASSED | `linux/logs/H-pane-list.log` |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | linux | **0** | `ok. 47 passed; 0 failed` | 47 | **RAN_PASSED** | `linux/logs/H-pane.log` |
| `--lib pane_liveness_ -- --list` | windows | 0 | selectors listed | **43** | RAN_PASSED | `windows/logs/H-pane-list.log` |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | windows | **0** | `ok. 43 passed; 0 failed` | 43 | **RAN_PASSED** | `windows/logs/H-pane.log` |
| `--lib local_split_reliability_ -- --list` | linux | 0 | selectors listed | **15** | RAN_PASSED | `linux/logs/H-split-list.log` |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | linux | **0** | `ok. 15 passed; 0 failed` | 15 | **RAN_PASSED** | `linux/logs/H-split.log` |
| `--lib local_split_reliability_ -- --list` | windows | 0 | selectors listed | **14** | RAN_PASSED | `windows/logs/H-split-list.log` |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | windows | **0** | `ok. 14 passed; 0 failed` | 14 | **RAN_PASSED** | `windows/logs/H-split.log` |
| `--lib split_journal -- --nocapture --test-threads=1` | windows | **0** | `ok. 7 passed; 0 failed` | 7 | **RAN_PASSED** | `windows/logs/H-journal.log` |
| `--test daemon_handover_transfer_contract -- --list` | linux | **0** | selectors listed | **5** | RAN_PASSED | `linux/logs/H-xfer-list.log` |
| `--test daemon_handover_transfer_contract -- --list` | windows | 0 | list empty | **0** | RAN_PASSED-as-empty — target is `#![cfg(unix)]`, compiles to nothing on Windows; **not a pass** | `windows/logs/H-xfer-list.log` |
| `--test ipc_hardening_contract -- --nocapture --test-threads=1` | windows | 0 | `ok. 0 passed; 0 failed` | **0** | same `#![cfg(unix)]` case; **not a pass** | `windows/logs/H-hardening.log` |

## 6. Two targets that could never run before now run — one CANDIDATE-CAUSED, one PRE-EXISTING

Both are integration binaries that **did not compile** at earlier commits, so `39e722ce` is their
first-ever execution. Both are now classified by base A/B on the same host:

- `daemon_handover_transfer_contract` → **CANDIDATE-CAUSED** (base green in 6.30s, candidate 4/5 failing in 336.47s)
- `ipc_hardening_contract` → **PRE-EXISTING** (both sides `1 passed; 6 failed`)

### 6a. `daemon_handover_transfer_contract` — quiet number `FAILED. 1 passed; 4 failed`

One of the plan's four verbatim final-gate commands, so it must end green or
classified-pre-existing-with-evidence. `--list` selects **5** (`native=0`).

**Loaded** (3 concurrent suites, load 12–26 on 12 cores) and **quiet** (load 2.8–7.5) give the **same
`1 passed; 4 failed`**, so load is *not* the explanation:

| Side | Raw exit | Asserted line | Result |
| --- | --- | --- | --- |
| candidate `39e722ce` (loaded) | **101** | — | `FAILED. 1 passed; 4 failed` (338.25s) |
| candidate `39e722ce` (quiet) | **101** | — | `FAILED. 1 passed; 4 failed` (336.47s) |

Verbatim signatures:

```
a_multi_session_v5_handover_preserves_every_session_id
  tests/daemon_handover_transfer_contract.rs:467:14  replacement readiness timeout: Elapsed(())
an_upgrade_binary_request_hands_every_session_to_the_new_daemon
  tests/daemon_handover_transfer_contract.rs:1103:9  no successor answered on the canonical socket within 60s
test_v5_ownership_transfer_is_the_default_without_the_flag
  tests/daemon_handover_transfer_contract.rs:467:14  replacement readiness timeout: Elapsed(())
  (preceded by: Ferryx daemon error: Missing authoritative workspace owner of '<uuid>')
test_v5_zero_session_loss_and_immediate_predecessor_exit
  tests/daemon_handover_transfer_contract.rs:467:14  replacement readiness timeout: Elapsed(())
  (preceded by: Ferryx daemon error: Missing authoritative workspace owner of '<uuid>')
a_session_exported_to_a_slow_successor_survives_and_returns_on_abort   PASSES
```

**Why this area:** the candidate changed exactly it — `b0ed4bef` added the production rollback helper
requiring successor relinquishment before predecessor abort, the HandoverTransaction decision
recording/reader, the ambiguous abort/commit paths, and un-gated `handover_transaction` from unix. The
four signatures are consistent with changed relinquish/abort ordering.

**A/B — CANDIDATE-CAUSED.** Base passes everything the candidate fails, and the runtime gap is 53×:

| Side | Raw exit | Result line | Duration |
| --- | --- | --- | --- |
| base `d82b35e4` | **0** | `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` | **6.30 s** |
| candidate `39e722ce` | **101** | `FAILED. 1 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out` | **336.47 s** |

Base verbatim: `test test_v5_ownership_transfer_is_the_default_without_the_flag ... ok`,
`test test_v5_zero_session_loss_and_immediate_predecessor_exit ... ok` (all five `ok`).

**This is a confirmed candidate regression in the handover/relinquish path** and it blocks Task 8
acceptance, since this target is one of the plan's four verbatim final-gate commands. Evidence:
`linux/xfer-base-only.log`, `linux/logs/xfer-base-only.log`,
`linux/logs/ab-xfer-cand.log`, `linux/newtargets-ab.log`. **Routing a backend repair — the verifier
does not repair product code.**

### 6b. `ipc_hardening_contract` — `FAILED. 2 passed; 5 failed`

`--list` selects **7**. All five failures share one signature — a **fixture-setup fault, not a logic
assertion**:

```
state() called before manage() for alloc::sync::Arc<ferryx_lib::daemon::client::DaemonClient>
  at tauri-2.11.5/src/lib.rs:734:7
```

Failing: `dirty_delete_returns_structured_error_code`,
`swapped_checked_out_branches_cannot_delete_a_stale_identity_slot`,
`worktree_status_does_not_emit_when_dirty_state_is_unchanged`,
`worktree_status_emits_dirty_changed_on_clean_to_dirty_transition`,
`worktree_status_emits_dirty_changed_on_dirty_to_clean_transition`.

**A/B — PRE-EXISTING (both sides fail identically):**

| Side | Raw exit | Result |
| --- | --- | --- |
| candidate `39e722ce` | **101** | `FAILED. 1 passed; 6 failed` |
| base `d82b35e4` | **101** | `FAILED. 1 passed; 6 failed` |

The identical signature on both sides means the fixture was **always** broken — the candidate's
widened `SpawnTerminalRequest`/command surface did **not** cause it. **Recorded as
pre-existing-with-evidence; the gate stays FAILED-but-classified.** (Note the count varied between
observations — `2 passed / 5 failed` under host load 26, `1 passed / 6 failed` at load 5.5 — so this
target is also load-sensitive, but it fails on **both** sides at every load observed.)

## 7. Full `--lib` delta — linux (COMPLETE)

| | Result line |
| --- | --- |
| `abd9e890` | `FAILED. 2629 passed; 37 failed; 6 ignored; 0 measured; 0 filtered out; finished in 451.09s` |
| `39e722ce` | `FAILED. 2638 passed; 31 failed; 6 ignored; 0 measured; 0 filtered out; finished in 489.97s` |

| | Count |
| --- | --- |
| failing at `abd9e890` | 37 |
| failing at `39e722ce` | 31 |
| **left the failing set** | **7** |
| **joined the failing set** | **1** |
| still failing | 30 |

**Left (7):** the six candidate-caused tests (both `ipc::tests` attach, all four
`native_terminal::surface_host::tests::*` presentation) plus
`remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked` (the
previously-flaky one, now passing).

**Joined (1): `terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread`** — `ok` at
`abd9e890`, now `FAILED` at `src/terminal/tests.rs:659:5` with
`write future must remain pending while PTY queue is saturated`.

**A/B (2 reps per side, same host, `--exact --test-threads=1`, `linux/wedged-ab.log`):**

| Side | rep 1 | rep 2 | Verdict |
| --- | --- | --- | --- |
| base `d82b35e4` | **PASS** (0.74s) | **PASS** (0.41s) | stable |
| candidate `39e722ce` | **FAIL** (30.15s) | **PASS** (0.49s) | **flaky** |

**Classification: candidate-side flake — NOT a deterministic regression, and NOT pre-existing.**
Base is 2/2 green while the candidate is 1/2, so the candidate *can* pass it; but base never failed,
so this is not pre-existing either. The failing rep took **30.15s** against **0.49s** for the passing
candidate rep, i.e. a slow/saturated path, not a subtle assertion drift. Materially:
`src-tauri/src/terminal/tests.rs` is **unchanged** by the candidate, but the candidate rewrote the PTY
write path this test exercises (`pty.rs` +285, `service.rs` +303, `session.rs` +156). Routing
guidance: **treat as a candidate-side timing flake in the rewritten PTY write path, to be resolved by
more repetitions or a targeted investigation — not as a confirmed regression, and not as pre-existing.**
Only one of the two full-suite observations (the `39e722ce` run) caught it, so it is genuinely
intermittent.

**Nothing else joined:** set-comparing the 30 still-failing against the `abd9e890` classification
gives **zero** entries not already classified (all 30 are pre-existing), and the 8 classification
entries absent from the still-failing set are exactly the seven above plus the extra flaky
`sibling_pty_is_responsive_when_first_input_is_saturated`.

## 8. mac full `--lib` — NOT_RUN (disk + hang), stated with readings

**NOT_RUN.** Two independent reasons, both recorded rather than papered over:

1. **Hung at 1079 of 2693 tests** on `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution`
   — the *same* test that hung at base `d82b35e4` on mac and linux. Confirmed a true hang, not
   slowness: test-binary CPU went **0:20.55 → 0:20.64 over 45 s** (~0.09 s of work) with the test count
   frozen. Partial log preserved: `mac/logs/I-full-lib-HUNG-at-p11-reaper.log` (2285 lines).
   Killed only this dispatch's pids.
2. **Disk**: mac free space was **3.3 GiB** at the end of the run, **below the ~5 GiB floor**, so per
   the agreed policy no further mac gate was started. The fullness is **foreign**
   (`maho-workspace` 114G, `ferryx-monitor-2026.1003.2` 27G, `ferryx-input-diag-9297` 23G — other
   sessions' trees, **not touched**); the only thing I reclaimed was my own regenerable
   `source-21dea3c0/target`.

**mac results that DID land** (`mac/rustI-gates.log`, all `native=0` unless noted): `all-targets` 0;
`lib-compile` 0 (2693 selected); the **four `surface_host` presentation tests 1/0 each**; both new
paired tests 1/0; `pane-list` 47 selected and `pane` **47 passed / 0 failed**; `split-list` 15 and
`split` **15 passed / 0 failed**.

**mac `ipc-hist` = RAN_FAILED, environment-shaped (not classified as candidate-caused):**
`tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` fails on mac
**consistently at ~61 s** with
`spawn: IpcError { code: IoError, message: "Timed out waiting for daemon response (15s)" }` at
`src/ipc/tests.rs:283:6` — the **spawn** step, *before* the attach binding is used. Linux passes the
identical test in **1.63 s**. The mac host measured **load average 22.31 on 12 cores**, and the same
`pane_liveness_` suite takes **62.96 s on mac vs 1.35 s on linux** (~47× slower). Mac `ipc-p13`
**passes** (`native=0`). This is a host-throughput effect, reported as RAN_FAILED with that evidence,
**not** routed as a candidate regression.

## 9. Contamination and invalidated results (verifier-side, not the product's)

| What | Cause | Disposition |
| --- | --- | --- |
| mac `sh-*` ×4 + `ipc-hist` | mac disk hit **167 MiB free**; gates died with `No space left on device (os error 28)`, one test with `Timed out waiting for daemon response (15s)` | **DISCARDED**; my regenerable `target` (15G) purged → 13–14 GiB free; re-run |
| `rgateG` (all hosts) | launched `rgateH` without stopping G, then overwrote the tree → G's later gates ran on mixed bytes | killed; log kept as `rustG-gates.STALE.log`; **only H cited** |
| windows `all-targets native=1` | my script dropped the `check` subcommand | re-run → **0** |

Foreign directories were **not** touched: `maho-workspace` 114G, `ferryx-monitor-2026.1003.2` 27G,
`ferryx-input-diag-9297` 23G belong to other sessions.

## 10. Run handles (for re-measurement)

| Run | Host | Command | Log | Monitor | Status |
| --- | --- | --- | --- | --- | --- |
| linux H | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/rgateH-gates.sh linux /home/indo/ferryx-pane-completion/source-21dea3c0 /home/indo/ferryx-pane-completion/task8-21dea3c0` | `task8-21dea3c0/rustH-gates.log` | `mon_HCTVC0MKCST6CXXG` | **COMPLETE** (`RUSTH_DONE linux`) |
| mac lib-only | `I552267@100.65.239.35` | `sh /Users/I552267/ferryx-pane-completion/task8-21dea3c0/rgateI-mac-lib.sh mac /Users/I552267/ferryx-pane-completion/source-21dea3c0 /Users/I552267/ferryx-pane-completion/task8-21dea3c0` | `task8-21dea3c0/rustI-gates.log` | `mon_MK718Y39JJ5DKN1V` | **COMPLETE** (`RUSTI_DONE mac`); full-lib hung |
| linux new-targets A/B | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/ulw.newtargets-ab.sh` | `task8-21dea3c0/newtargets-ab.log` | `mon_39CBCFRCFV8W0Y72` | hardening done; **xfer in flight** |
| linux wedged A/B | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/ulw.wedged-ab.sh` | `task8-21dea3c0/wedged-ab.log` | `mon_3FCC7027SN97QW7Y` | **COMPLETE** |

## 11. mac `--lib` delta — NOT_RUN

mac `39e722ce` full `--lib`: **NOT_RUN** — see §8. Baseline at `abd9e890` was
`2635 passed; 49 failed; 6 ignored`. The delta cannot be computed because the run hung and the host was
below the disk floor.

## 12. Cleanup receipts

| Resource | Teardown | Receipt |
| --- | --- | --- |
| mac base-A/B tree (`source-base`, `task8-base`) | `sh cleanup-pass4-unix.sh mac` | `CLEANUP_OK mac sourceBaseAbsent=true task8BaseAbsent=true freeBefore=12Gi freeAfter=18Gi` |
| linux base-A/B tree | `sh cleanup-pass4-unix.sh linux` | `CLEANUP_OK linux sourceBaseAbsent=true task8BaseAbsent=true freeBefore=450G freeAfter=457G` |
| windows base tree | `win-cleanup-pass4.ps1` + `rd /s /q` ×5 | `task8-base` absent; `source-base` reduced to an **empty directory skeleton** (0 bytes of files) whose `src-tauri` the OS keeps locked despite no process referencing it; `C:` 40.4 → 47.3 GB |
| mac `source-21dea3c0/target` | `rm -rf` (twice) | regenerable; freed 15G each time (needed to clear os-error-28) |
| local `/tmp` helper files | `rm -f` | none remain |
| foreign `cargo.exe` on maho-win (8 procs) | **deliberately untouched** | not descendants of this dispatch's runner (parents = `rustup.exe`); another session's |

## 13. Residual classified set (carried to Task 9)

### NEW since the `abd9e890` classification — three additions

| Item | Classification | Evidence |
| --- | --- | --- |
| `daemon_handover_transfer_contract` (5 tests, 4 failing) | **CANDIDATE-CAUSED** — base `ok. 5 passed` in 6.30s vs candidate `1 passed; 4 failed` in 336.47s | `linux/xfer-base-only.log`, `linux/logs/ab-xfer-cand.log` |
| `ipc_hardening_contract` (7 tests, 6 failing) | **PRE-EXISTING** — base `1 passed; 6 failed` == candidate `1 passed; 6 failed` | `linux/logs/ab-hardening-{base,cand}.log` |
| `terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread` | **candidate-side FLAKE** — base [PASS, PASS] vs candidate [FAIL, PASS] | `linux/wedged-ab.log` |

### Carried forward from `abd9e890`


| Class | linux | mac | windows |
| --- | --- | --- | --- |
| candidate-caused | **6** | **6** | **5** |
| pre-existing | 30 | 43 | 65 |
| flaky | 1 | 0 | 0 |
| unclassified | **0** | **0** | **0** |

The mac and linux candidate-caused sets were the **identical six tests**. Windows shows five: the four
`surface_host` presentation tests plus
`daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent` — the
latter reported **UNRESOLVED BY REPETITION** (base ×2 stopped at 27890 bytes with no verdict and no
timeout panic; candidate ×2 emitted `Other("Timed out waiting for PTY reader shutdown")` at
`session_service_machine_tests.rs:344:63`), **not** a confirmed regression. The two `ipc::tests`
attach tests **cannot exist on Windows**: `src-tauri/src/ipc/mod.rs:57-58` gates that module behind
`#[cfg(all(test, unix))]`.
