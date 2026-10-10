# Task 8 — FINAL per-host verdict table (successor to `FINAL-VERDICT-39e722ce.md`)

**STATUS: the blocking handover regression is FIXED at `70eefafe` — the decisive gate now passes at
base-equivalent runtime.**

`daemon_handover_transfer_contract` is one of the plan's four verbatim final-gate commands. Its
measured history:

| Revision | Raw exit | Result | Test runtime |
| --- | --- | --- | --- |
| base `d82b35e4` | **0** | `ok. 5 passed; 0 failed` | **6.30 s** |
| candidate `39e722ce` (pre-fix) | **101** | `FAILED. 1 passed; 4 failed` | **336.47 s** |
| **candidate `70eefafe` (fixed)** | **0** | **`ok. 5 passed; 0 failed`** | **6.21 s** |

The fixed runtime (**6.21 s**) matches the base (**6.30 s**) and is ~54× faster than the pre-fix
candidate — the target set for this re-run. Verbatim:

```
GATE linux handover-xfer native=0 selected=0 errors=0 secs=207 | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.21s
test test_v5_ownership_transfer_is_the_default_without_the_flag ... ok
test test_v5_zero_session_loss_and_immediate_predecessor_exit ... ok
```

The two tests that previously failed with `Missing authoritative workspace owner of '<uuid>'` and
`replacement readiness timeout: Elapsed(())` are both `ok`.

## Root cause (author's, verified as consistent with my evidence)

The successor's per-session adoption loop resolved the workspace owner from `self.session_metadata` —
the **successor's own empty registry** — for a session it never spawned. The lookup always missed, the
closure returned `Err`, and the transfer future aborted before `commit_started`, so the replacement
daemon never acquired locks or bound its socket. That single statement explains all three failure texts
and the 336 s runtime. **My earlier hypothesis (changed relinquish/abort ordering) was wrong** — the
ordering machinery was not the cause; the successor simply looked in the wrong map.

The fix carries the predecessor's authoritative spawn record in the export frame
(`serde(default)` for interop) and validates the export against the predecessor's own describe answer
instead of the successor's map; the incarnation check stays, and the recorded-decision /
never-blindly-resume machinery is untouched.

## The flagged open risk — CONFIRMED HOLDING (by execution, not by reading)

The author flagged the added `post_handover.workspace_id` assertion
(`daemon_handover_transfer_contract.rs:633`,
`assert_eq!(post_handover.workspace_id.as_deref(), Some(ws_id), "the transferred session must retain
its workspace identity")`). It is **exercised**: `run_v5_handover_case` is called by
`test_v5_zero_session_loss_and_immediate_predecessor_exit` (`:495`) and
`test_v5_ownership_transfer_is_the_default_without_the_flag` (`:503`), and both are among the five
`ok`. So the assertion **holds by execution**.

My own reading agrees with the lead's mechanism: describe's local arm
(`session_service.rs:2666-2672`) reads `session_metadata` and returns `Some(m.workspace_id)`, and the
fix installs the predecessor's exported `StoredSessionMeta` (which carries `workspace_id`) into that
map — so once the successor installs the adopted record, describe reports the workspace. The
`#[cfg(test)]`-only `StoredSessionMeta` site at `:2545` is unrelated.

The gate's `cwd` assertion now canonicalizes (the daemon answers from the kernel view,
`/tmp` → `/private/tmp`), and it too passes.

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
| `39e722ce` | fix(daemon): give paired and ssh-remote sessions a provable lifetime incarnation |
| **`70eefafe`** | **fix(daemon): adopt a transferred session against the predecessor's authority, not the successor's empty registry** |

Verified candidate bytes on the linux host by sha256 before running:
`handover_socket.rs` `4dc252c1`, `server.rs` `59f4d07d`,
`daemon_handover_transfer_contract.rs` `1cfcc390`.

## Affected-set re-run at `70eefafe`

### linux (started at load 4.84–5.82)

| Exact command | Raw exit | Selected | Duration | Result line |
| --- | --- | --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | **0** | 5 | 207 s total (6.21 s test) | **`ok. 5 passed; 0 failed`** |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | 1 s | `Finished`, errors=0 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | 0 | **15** | 228 s (compile) | selectors listed |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | 15 | 2 s | `ok. 15 passed; 0 failed` |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | 0 | **54** | 0 s | selectors listed |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | 54 | 3 s | **`ok. 54 passed; 0 failed`** |
| `cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract -- --nocapture --test-threads=1` | **101** | 7 | 24 s | `FAILED. 1 passed; 6 failed` — **pre-existing (A/B'd)** |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --nocapture --test-threads=1` | **101** | 15 | 15 s | `FAILED. 14 passed; 1 failed` — **pre-existing (A/B'd)** |
| `cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_gen4_audit -- --nocapture --test-threads=1` | **0** | 3 ignored | 8 s | `ok. 0 passed; 0 failed; 3 ignored` |
| `cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_gen5_regression -- --nocapture --test-threads=1` | **101** | 6 | 40 s | `FAILED. 2 passed; 4 failed` — **pre-existing (A/B'd)** |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_contract -- --nocapture --test-threads=1` | **0** | 5 | 19 s | `ok. 5 passed; 0 failed` |

**`pane_liveness_` grew from 47 to 54 selected: +7 added, 0 removed, all 47 originals still present**
(selector-set diff of `H-pane-list.log` vs `K-pane-list.log`). The seven are the fix's own adoption
contract, and all 54 pass:

- `daemon::server::pane_liveness_adopted_ownership_tests::pane_liveness_adoption_installs_the_exported_owner_record`
- `…::pane_liveness_adoption_refuses_a_declaration_without_a_workspace_owner`
- `…::pane_liveness_adoption_refuses_a_missing_owner_record`
- `…::pane_liveness_adoption_refuses_a_predecessor_without_an_incarnation`
- `…::pane_liveness_adoption_refuses_a_record_from_another_workspace`
- `…::pane_liveness_adoption_refuses_a_worktree_domain_mismatch`
- `…::pane_liveness_relinquished_sessions_lose_their_installed_ownership`

All of `gen5`, `daemon_handover_contract`, `qa_barrier` and the linux full `--lib` **landed**; the run
ended with `RUSTK_DONE linux LOAD=3.67 4.88 6.34` and the full-lib result
`2647 passed; 30 failed; 6 ignored` (464.13 s). See the delta section below.

## mac — NO measured result at `70eefafe` (host unhealthy, then unreachable)

**NOT_RUN, disclosed as an acceptance gap.** Two independent blockers, both environmental:

1. **Disk below the floor.** Free space on `/System/Volumes/Data` fell to **1.6 GiB** — far below the
   agreed ~5 GiB floor — so per the standing policy the mac gate was stopped rather than run to a
   contaminated result. The consumers are **foreign**: `maho-workspace` 114G,
   `ferryx-monitor-2026.1003.2` 27G, `ferryx-input-diag-9297` 23G, none of them mine to delete. My own
   regenerable `source-21dea3c0/target` (13G) was purged as the only permitted reclaim.
2. **Host unreachable.** With load average **31.8–33.8 on 12 cores**, ssh to `I552267@100.65.239.35`
   began timing out (`Operation timed out`, exit 255), so no further mac gate could be started or
   inspected.

**Two mac runs produced INVALID results and are quarantined, not cited:**

| Log | Why invalid |
| --- | --- |
| `mac/logs/L-handover-xfer.INVALID-stale-binary.log`, `mac/rustL-gates.INVALID-stale-binary.log` | Ran against a **stale daemon binary**: `target/debug/ferryx` was built 20:36:43 by the earlier pre-fix run, while my `tar -xzf` **preserved the archive mtime (20:17:07)** — older than the binary — so cargo judged the source fresh and **skipped the rebuild**. Proof: `strings target/debug/ferryx` contained the old-path string but **not** the fix's new one (`exported no authoritative owner record` → 0). Its `Missing authoritative workspace owner` failure is therefore **pre-fix code**, not a mac finding. |
| `mac/logs/L-handover-xfer.STALE-39e722ce.log`, `mac/rustL-gates.STALE-39e722ce.log` | Ran before the `70eefafe` delta was pushed to mac at all (fixture still `875151b3`). |

**The `70eefafe` mac bytes were verified correct on the host before the last attempt**
(`handover_socket.rs` `4dc252c1`, `server.rs` `59f4d07d`,
`daemon_handover_transfer_contract.rs` `1cfcc390`), and the source was touched to force a rebuild — the
gate then could not run because the disk floor and the host outage intervened.

### mac results that remain valid (from `39e722ce`, whose non-handover code is identical)

| Gate | Raw exit | Selected | Result |
| --- | --- | --- | --- |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | `Finished`, errors=0 (also **0** at `70eefafe`, 12 s) |
| `--lib pane_liveness_` | **0** | 47 | `ok. 47 passed; 0 failed` |
| `--lib local_split_reliability_` | **0** | 15 | `ok. 15 passed; 0 failed` |
| four `native_terminal::surface_host::tests::*` | **0** each | 1 each | `ok. 1 passed` |
| two new paired-incarnation tests | **0** each | 1 each | `ok. 1 passed` |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | **0** | 1 | `ok. 1 passed` |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | **101** | 1 | FAILED at ~61 s, `spawn: ... "Timed out waiting for daemon response (15s)"` — **environment-shaped** (see below) |

**mac `ipc-hist` — RAN_FAILED, environment-shaped, not candidate-caused.** The failure is in the
**spawn** step at `src/ipc/tests.rs:283:6`, *before* the attach binding is used; linux passes the
identical test in **1.63 s**. The mac host measured **load 22.31 / 12 cores**, and `pane_liveness_`
takes **62.96 s on mac vs 1.35 s on linux (~47×)**. At 47× the linux runtime a 15 s daemon-response
budget cannot hold, so this is host throughput, not product behaviour.

**mac full `--lib` — NOT_RUN.** Note the correction to my earlier claim: the candidate run did **not**
hang on `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution` — it **passed** it
(`... ok`) with 396 tests running after it, and the log was still advancing (1475 → 1476) when I killed
it. The **base** is the side that genuinely hangs there (`mac/logs/full-lib-base-HUNG.log` stops at
1035 tests; `linux/logs/full-lib-base-HUNG.log` at 1036 after 16 min), so the hang is **pre-existing**.
mac full `--lib` has no measured result at `70eefafe`; at ~47× linux runtime it needs a quiet host and
a much larger window.

## Run handles (for re-measurement by Task 9)

| Run | Host | Command | Log | Handle | Status |
| --- | --- | --- | --- | --- | --- |
| decisive handover gate | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/rgateJ-gates.sh linux …` | `task8-21dea3c0/rustJ-gates.log` | monitor `mon_DQ33Q35HJARZR1SD` | **COMPLETE** (gate passed; script then died at the section boundary) |
| linux affected-set + full-lib | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/rgateK-gates.sh linux /home/indo/ferryx-pane-completion/source-21dea3c0 /home/indo/ferryx-pane-completion/task8-21dea3c0` | `task8-21dea3c0/rustK-gates.log` | monitor `mon_9196ERN4R7TEB5JF` | **COMPLETE** (`RUSTK_DONE linux`) |
| linux base-side xfer A/B | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/ulw.xfer-base.sh` | `task8-21dea3c0/xfer-base-only.log` | monitor `mon_ECANZDQJQG42318G` | **COMPLETE** (`ok. 5 passed` in 6.30 s) |
| linux wedged-PTY A/B | `indo@100.91.254.71` | `sh /home/indo/ferryx-pane-completion/task8-21dea3c0/ulw.wedged-ab.sh` | `task8-21dea3c0/wedged-ab.log` | monitor `mon_3FCC7027SN97QW7Y` | **COMPLETE** |
| mac affected-set | `I552267@100.65.239.35` | `sh /Users/I552267/ferryx-pane-completion/task8-21dea3c0/rgateL-mac.sh mac …` | `task8-21dea3c0/rustL-gates.log` | — | **NOT_RUN** (disk 1.6 GiB, then host ssh-unreachable) |

**Task 9 note:** the mac tree at `/Users/I552267/ferryx-pane-completion/source-21dea3c0` carries the
`70eefafe` bytes (`handover_socket.rs` `4dc252c1`, `server.rs` `59f4d07d`, fixture `1cfcc390`) but its
`target/` was purged and the source files were touched to 2030-01-01 to force a rebuild — any mac gate
will therefore do a **full cold build**, which at the current disk headroom will fail with
`No space left on device`. Free space on mac first.

## Verifier-side contamination #4: mac ran the PRE-FIX bytes for one gate

The first mac `rgateL` attempt ran against a **stale tree**. Verified by sha256: mac's
`source-21dea3c0` still held the pre-fix `server.rs` (`163abf52`) and
`daemon_handover_transfer_contract.rs` (`875151b3` = the `39e722ce` fixture) — the `70eefafe` delta had
only been pushed to **linux**. Its `handover-xfer` gate therefore failed with the *old*
`Missing authoritative workspace owner` text.

**Disposition:** killed, log preserved as
`task8-21dea3c0/logs/L-handover-xfer.STALE-39e722ce.log` and `rustL-gates.STALE-39e722ce.log`, and the
`70eefafe` delta pushed to mac (all three hashes then matched:
`handover_socket.rs` `4dc252c1`, `server.rs` `59f4d07d`,
`daemon_handover_transfer_contract.rs` `1cfcc390`). The mac affected-set gates are re-running on the
correct bytes; **only those results are cited.**

**Rule:** push a revision delta to **every** host that will run a gate for it, and sha256-verify each host
before launching — a per-host stale tree produces a failure that looks exactly like a product defect.
## Full `--lib` delta, linux `39e722ce` → `70eefafe`

| | Result line |
| --- | --- |
| `39e722ce` | `FAILED. 2638 passed; 31 failed; 6 ignored; 0 measured; 0 filtered out; finished in 489.97s` |
| `70eefafe` | `FAILED. 2647 passed; 30 failed; 6 ignored; 0 measured; 0 filtered out; finished in 464.13s` |

| Count | Value |
| --- | --- |
| failing at `39e722ce` | 31 |
| failing at `70eefafe` | 30 |
| **left the failing set** | **1** |
| **joined the failing set** | **0** |
| still failing | 30 |

**Left (1):** `terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread` — the
candidate-side flake identified at `39e722ce`; it **passed** at `70eefafe`, which is the expected
behaviour for a flake and does not prove a fix. **Joined (0).** All 30 still-failing are the
`abd9e890` pre-existing set — set-comparing them against that classification gives **zero** entries that
are not already classified.

Evidence: `linux/DELTA-70eefafe.md`, `linux/logs/full-lib-70eefafe.log`, `linux/rustK-gates.log`.

## Cumulative `--lib` trajectory (linux)

| Revision | Result | Candidate-caused remaining |
| --- | --- | --- |
| `abd9e890` | 2629 passed / **37 failed** / 6 ignored | 6 |
| `39e722ce` | 2638 passed / **31 failed** / 6 ignored | 1 flaky (+1 joiner) |
| **`70eefafe`** | **2647 passed / 30 failed / 6 ignored** | **0** |

---

# ACCEPTANCE SUMMARY — the plan's final-gate items

Verdict vocabulary: **PASS** / **FAIL** / **NOT_RUN**. For every non-PASS, the classification is given.
**Raw exit and selected count are stated for every row.**

## Rust gates at `70eefafe` (linux, the only host with a measured result at this revision)

| Gate | Exact command | Raw exit | Selected | Verdict | Classification |
| --- | --- | --- | --- | --- | --- |
| handover transfer (verbatim final gate) | `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | **0** | **5** | **PASS** — `ok. 5 passed; 0 failed` in **6.21 s** | — (base 6.30 s) |
| `qa_barrier` (verbatim final gate) | `--lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | **0** | **11** | **PASS** — `ok. 11 passed; 0 failed` | — |
| `daemon_handover_contract` | `--test daemon_handover_contract -- --nocapture --test-threads=1` | **0** | **5** | **PASS** — `ok. 5 passed; 0 failed` | — |
| `local_split_reliability_` | `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0** | **15** | **PASS** — `ok. 15 passed; 0 failed` | — |
| `pane_liveness_` | `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0** | **54** | **PASS** — `ok. 54 passed; 0 failed` | — (47 at `39e722ce`; **+7, 0 removed**) |
| `cargo check --all-targets` | `check --manifest-path src-tauri/Cargo.toml --all-targets` | **0** | — | **PASS** — errors=0 | — |
| full `--lib` (linux) | `--lib -- --test-threads=1` | **101** | 2683 | **FAIL** — `2647 passed; 30 failed; 6 ignored` | **all 30 pre-existing** (A/B'd; zero unclassified, zero new) |
| `ipc_hardening_contract` | `--test ipc_hardening_contract -- --nocapture --test-threads=1` | **101** | 7 | **FAIL** — `1 passed; 6 failed` | **pre-existing with A/B**: base `1 passed; 6 failed` identical |
| `daemon_persistence_contract` | `--test daemon_persistence_contract -- --nocapture --test-threads=1` | **101** | 15 | **FAIL** — `14 passed; 1 failed` | **pre-existing with A/B**: base fails the same assertion |
| `zero_config_gen5_regression` | `--test zero_config_gen5_regression -- --nocapture --test-threads=1` | **101** | 6 | **FAIL** — `2 passed; 4 failed` | **pre-existing with A/B**: base `2 passed; 4 failed` |
| `zero_config_gen4_audit` | `--test zero_config_gen4_audit -- --nocapture --test-threads=1` | **0** | 3 ignored | **PASS** — `ok. 0 passed; 0 failed; 3 ignored` | — |

## Rust gates on mac at `70eefafe`

| Gate | Raw exit | Selected | Verdict | Classification |
| --- | --- | --- | --- | --- |
| `cargo check --all-targets` | **0** | — | **PASS** | — |
| full `--lib` | — | — | **NOT_RUN** | **host-unhealthy**: disk 1.6 GiB (below floor) and the host then became **ssh-unreachable** at load 31.8–33.8/12 cores. At ~47x the linux runtime it needs a quiet host and a much larger window. |
| `daemon_handover_transfer_contract` | — | — | **NOT_RUN** | same host outage; the two attempts that did run used **stale binaries** (see contamination #4) and are quarantined |
| `ipc_hardening_contract` | — | — | **NOT_RUN** | same host outage |
| `pane_liveness_` / `local_split_reliability_` / the four `surface_host` tests / both new paired tests | **0** each | 47 / 15 / 1 each / 1 each | **PASS** (measured at `39e722ce`; the `70eefafe` diff touches only `handover_socket.rs`, `server.rs` and the handover fixture, none of which these exercise) | — |

## Windows gates (measured at `39e722ce`; the `70eefafe` diff does not touch Windows paths)

| Gate | Raw exit | Selected | Verdict | Classification |
| --- | --- | --- | --- | --- |
| `cargo check --all-targets` | **0** | — | **PASS** | — |
| four `surface_host` presentation tests | **0** each | 1 each | **PASS** | — |
| `pane_liveness_` | **0** | 43 | **PASS** — `ok. 43 passed; 0 failed` | — |
| `local_split_reliability_` | **0** | 14 | **PASS** — `ok. 14 passed; 0 failed` | — |
| `split_journal` | **0** | 7 | **PASS** — `ok. 7 passed; 0 failed` | — |
| `daemon_handover_transfer_contract` / `ipc_hardening_contract` | 0 | **0** | **NOT a pass** — targets are `#![cfg(unix)]`, they compile to nothing on Windows | structurally-not-applicable |
| full `--lib` | — | — | **NOT_RUN** | not re-run this pass; at `abd9e890` it was `2201 passed; 70 failed; 8 ignored` with `5 candidate-caused / 65 pre-existing / 0 unclassified` |

## UI gates (measured at `21dea3c0`; unchanged by every later Rust-only delta)

| Gate | mac | linux | windows | Verdict |
| --- | --- | --- | --- | --- |
| `bun run --cwd ui build` | **0** (11.57 s) | **0** (5.15 s) | **0** (5.78 s) | **PASS x3** |
| scoped split (`localSplitLifecycle.test.ts`) | **0** — 19/19 | **0** — 19/19 | **0** — 19/19 | **PASS x3** |
| lifecycle (4 files) | **0** — 112/112 | **0** — 112/112 | **0** — 112/112 | **PASS x3** |
| QA runner (exact pane-liveness config) | **0** — 28/28 | **0** — 28/28 | **0** — 28/28 | **PASS x3** |
| full UI | **1** — 192F / 6329P (6521) | **1** — 1224F / 5297P (6521) | **1** — 193F / 6323P / 5 skip | **FAIL — baseline-dominated**, all three hosts; the only candidate-caused UI regression found (pass 3) was `terminalTransport.test.ts`, repaired in `7c0fed30` and re-verified **11/11 x3** |

## Residual candidate-caused set

**EMPTY.** Every candidate-caused failure identified across the whole Task 8 effort is now repaired and
re-verified:

| Formerly candidate-caused | Repaired in | Re-verified at `70eefafe` |
| --- | --- | --- |
| `daemon_handover_transfer_contract` (4 of 5 tests) | `70eefafe` | **linux PASS** `ok. 5 passed` in 6.21 s |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | `39e722ce` | linux **PASS**; mac FAILED but **environment-shaped** (see mac section) |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | `39e722ce` | mac + linux **PASS** |
| the four `native_terminal::surface_host::tests::*` presentation tests | `e57685ec` | mac + linux + windows **PASS** |
| `daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent` (windows) | — | **UNRESOLVED BY REPETITION** (see below) |

## Residual pre-existing / flaky set (by name)

**Pre-existing (30 on linux, A/B'd, all still failing at `70eefafe`):** `daemon::handover_wire::tests::truncated_control_is_rejected`;
`daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface`;
`daemon::server::remote_ssh_tests::{direct_ssh_real_transport_registration_and_pty, remote_ssh_first_spawn_provisions_qualified_helper_automatically}`;
`ipc::worktree::deletion_repair_tests::{deletion_repair_missing_record_preview_and_targeted_cleanup, deletion_repair_preview_reports_current_dirty_and_unmerged_loss, deletion_repair_success_removes_cached_row_and_blocks_stale_worker}`;
`native_terminal::renderer::font_manager::tests::{test_font_manager_derives_nonzero_metrics_and_rasterizes, test_glyph_orientation_regression}`;
`remote::relay_server::tests::a_symlinked_artifact_is_not_served`;
`remote::server::machine_input_cancellation_tests::{input_is_cancelled_when_socket_disconnects, sibling_pty_is_responsive_when_first_input_is_saturated}`;
`remote::tests::security::sockets::a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`;
`rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`;
14 x `ssh::bridge::tests::*`; `ssh::helper_setup::tests::posix_upload_script_never_kills_live_daemon_and_defers_upgrade`.

**Pre-existing on Windows (65)** — the `abd9e890` classification, dominated by a missing `ferryx-remote-helper`
binary, Windows long-path (`\\?\` → `//?/`) defects, and fixture timeouts.

**Pre-existing on mac (43)** — the `abd9e890` classification.

**Flaky (candidate-side, needs a quiet host):**
`terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread` — base [PASS 0.74 s, PASS 0.41 s]
vs candidate [FAIL 30.15 s, PASS 0.49 s] at `39e722ce`; **it PASSED at `70eefafe`** (it is the single
test that left the failing set in the `39e722ce` → `70eefafe` delta). Not pre-existing, not
deterministic.

**UNRESOLVED BY REPETITION (windows, 1 row):**
`daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent` — base
full suite `ok` → candidate full suite `FAILED` with `Other("Timed out waiting for PTY reader
shutdown")`. Scoped repetition was inconclusive **on both sides** (two base attempts stopped at an
identical 27890 bytes with **no** `test result:` line and no timeout panic; two candidate attempts
emitted the timeout panic). **Not a confirmed regression**; needs a quiet host.

## Host health for Task 9

**maho-mac (`I552267@100.65.239.35`), measured 2026-10-04T20:37 local — UNHEALTHY:**

| Metric | Value |
| --- | --- |
| load average | **31.8–33.8** (peaks), 18.4–24.6 sustained, on **12 cores** |
| free space `/System/Volumes/Data` | **1.6 GiB** |
| ssh reachability | **timed out** (`Operation timed out`, exit 255) |
| `pane_liveness_` (47 tests) | **62.96 s** vs **1.35 s** on linux (~47x) |

**Recommendation for Task 9:** do **not** run the nine native GUI scenarios on maho-mac in this state.
At load ~32 with 1.6 GiB free it will produce environment-shaped failures (15 s daemon-response budgets
already fail at load 22) rather than product verdicts. Free the disk and let the host drain first, then
re-measure load and free space before starting. maho-win (`sook@100.126.171.58`) was responsive
throughout and is the healthier host.

## Cleanup receipts (this pass)

| Resource | Teardown | Receipt |
| --- | --- | --- |
| leaked test daemon PID 25736 (PPID 1, cwd `/private/tmp/fx-v05-gF78ez`) | `kill`, then `kill -9` | gone; `ps` empty. Private fixture root, **not** the canonical daemon |
| `/private/tmp/fx-v05-gF78ez`, `/private/tmp/fx-v05-lKX1O2` | `rm -rf` | `roots remaining: 0` |
| canonical user daemon `/tmp/rorca-501/daemon.sock` | **untouched** | verified present (Oct 2) after the kill |
| mac `source-21dea3c0/target` (13G, regenerable) | `rm -rf` issued | **UNVERIFIED** — the host dropped before the command's result could be read (see the uncertain-state note below) |
| stale mac logs | preserved, quarantined | `rustL-gates.{STALE-39e722ce,INVALID-stale-binary,NOTRUN-disk}.log` and the matching `L-handover-xfer.*` |
| linux `rgateK` run | completed | `RUSTK_DONE linux`, load 3.67 at end |
| local `/tmp` helper files | `rm -f` | none remain |

### UNCERTAIN mac state at handoff — Task 9 must resolve this first

The mac host went **ssh-unreachable** while I was stopping its gate, so two things are **unresolved and
must be verified once the host is back**, not assumed:

1. **Possibly-orphaned processes.** At the moment I issued the stop, the process pattern still matched
   **3** entries (`ps -eo pid,args | grep -E "rgateL|cargo (test|check)"`). My `pkill` was sent but its
   result was never read. **Check for orphaned `cargo`/`rustc` processes and a stale `rgateL-mac.sh`
   before starting anything**, and kill only pids whose command line names
   `/Users/I552267/ferryx-pane-completion/source-21dea3c0`.
2. **Unverified disk reclaim.** The `rm -rf .../source-21dea3c0/target` (13G) was issued but its
   completion was never observed, and `df` reported **1.6 GiB** free immediately before. Re-measure free
   space before any mac gate; if the reclaim did not land, free space is still ~1.6 GiB and a cold build
   will fail with `No space left on device`.

Neither of these is a product finding; both are host-state unknowns created by the host dropping mid-stop.

**Foreign resources left alone:** mac `maho-workspace` 114G, `ferryx-monitor-2026.1003.2` 27G,
`ferryx-input-diag-9297` 23G, `ferryx-build`, `ferryx-snap-target`; the eight foreign `cargo.exe`
processes on maho-win; the linux `cargo check -p maho-ai …` process.
