# F2 Code quality review, round 4: daemon-independent-lifetime-20261017

- **Reviewer.** F2 r4, independent.
  - Source review of `66f388f3..cfe34b8e`.
  - The platform clippy at cfe34b8e was run once per host on omarchy and maho-win by this lane (the F2 evidence gap named by the orchestrator). Nothing was built on this Mac.
  - Findings are labelled SOURCE (file:line at cfe34b8e) or MEASURED (log path relative to this evidence dir).
- **Target.** FINAL_MAIN cfe34b8e = origin/main. The r3 base was 66f388f3.
- **Scope.**
  - The daemon-lifetime product and harness changes in range: the T12 Windows/Linux QA fixes merged at 3f6c1f78 (b2151db3, 60afe877, 7f00802d, a39422d3, 17625807, e7bad8a7, b08fab25, 0ac0f33e, 4dd0bd04), 134dcc40 (update_pending test determinism), and 22a8ecea (hub `cfg(unix)`).
  - Other sessions' work that touches daemon files: c3ea7010 (Linux hub env forwarding, hub start after handover), 0179db56/daad0abe (agent observer and agent control).
  - Other sessions' work is reviewed only where it touches the F2 criteria for the daemon files.

## recommendation
**APPROVE-SOURCE-ONLY-MACOS.**
- Clippy exits 0 at cfe34b8e on both platforms. The warnings it adds on lines this range changed are lints, not errors, and none sits on a daemon-lifetime line (see the Clippy section).
- The F2 criteria hold on the reviewed diff: typed errors, run_blocking for disk and process I/O, no fixed sleeps in new daemon-lifetime tests, cfg isolation with working fallbacks, no assertion weakening.
- macOS clippy evidence predates the range (F2-clippy-66f388f3.log). The only macOS-compiled change in range that touches daemon-lifetime code is portable Rust that both other platforms linted clean. That is why this is `-SOURCE-ONLY-MACOS`.

## blockers
None.

## originalIntent
Quitting, force-quitting, relaunching or updating Ferryx must never kill running terminals on any OS. F2 checks the code-quality bar on the shipped diff.

## desiredOutcome
- The T12 product and harness fixes and 22a8ecea are correct, minimal and portable.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` passes on Linux and Windows at the final commit.

## userOutcomeReview
- The changes the user depends on are small and fail-safe.
- The Windows build is fixed (22a8ecea). Before it, main did not compile on Windows at all (F3/windows/run2/build-A.log:187-200), which the F3 Windows lane proved. The F3 runs at cfe34b8e build and pass on Windows, so the fix delivers.
- The rest of the range adds QA observability (a pending log line, QA-only env forwarding) and test determinism. It does not change daemon lifecycle decisions.

## Criterion check on 66f388f3..cfe34b8e (daemon-lifetime files)
| Criterion | Result | Evidence |
| --- | --- | --- |
| Typed errors, no regex matching of IPC errors in product | PASS | SOURCE update_pending.rs:37-48 `log_line()` writes `code=DAEMON_UPDATE_PENDING liveSessions=<n\|null> retireRequested=<bool>`. That is a log line for operators and QA, not an IPC contract. The error to the frontend is still `IpcErrorCode::DaemonUpdatePending` via `into_error` (:50+). The QA harness reads the log line (allowed: machine-consumed). The product UI still reads the structured `details`. |
| run_blocking for disk, git and process I/O | PASS | SOURCE: the range adds no new sync disk or process I/O on async daemon paths. update_pending.rs only restructures `enter`. supervisor_linux.rs `forwarded_key`/`secret_env` are pure filters. supervisor_windows.rs adds one const entry. 22a8ecea adds a non-unix stub returning `None`. The hub `start` is spawned after readiness (cli.rs:3053-3062) and is other-session work. |
| No fixed sleeps in tests | PASS for daemon-lifetime; NOTE for other sessions | MEASURED: an added-line grep over src-tauri/src/daemon and scripts/qa/daemon-lifetime in 66f388f3..cfe34b8e finds no new `sleep`, `setTimeout` or `Start-Sleep` in daemon-lifetime tests. 134dcc40 removes a racy captured-subscriber assertion (update_pending_tests.rs:169-195) in favour of the returned decision, which removes nondeterminism. NOTE N-S1 (other session, not daemon-lifetime): daemon/agent_observer.rs:398 polls with `sleep(10ms)` under a 2 s deadline, and :421 has a fixed `sleep(200ms)` before a negative assertion (0179db56, Cursor Agent). |
| Cross-platform cfg isolation with working fallbacks | PASS | SOURCE readiness.rs (22a8ecea): `#[cfg(unix)] disk_sample` (statvfs) and `#[cfg(not(unix))] disk_sample -> None`. `classify_disk_pressure` maps None to "unknown", the same degradation as the /proc probes. supervisor_windows.rs:59-67 `QA_FORWARDED_ENV` gains `FERRYX_QA_DISABLE_CONTROL` only under `feature = "local-split-qa"`; the non-QA arm stays `[&str; 0]`. MEASURED: Windows clippy at cfe34b8e compiles `ferryx-hub` and `ferryx` for windows-msvc with EXIT=0 (w5-qa/F2-platform/windows-clippy-cfe34b8e.log). |
| No assertion weakening | PASS | SOURCE update_pending_tests.rs:169-195 (134dcc40). The old assertion counted `RetireWhenIdle` in captured logs. The new one asserts `logged == [RETIRE_SENT_LOG]` across two entries, the exact op sequence `[Inventory, RetireWhenIdle, Inventory]`, and "no Shutdown". This is stronger, not weaker, and the root cause (global tracing callsite interest cache) is documented in the commit. supervisor_windows.rs:1343-1347 adds positive and negative cases ("never into a user task"). supervisor_linux.rs `unit_forwards_hub_env_and_keeps_hub_tokens_out_of_the_unit` asserts both inclusion and exclusion. Harness tests (replace.test.mjs, guards.test.mjs) add refusal cases ("a reused pid is never signalled", `KILL_REFUSED`). |
| No dead code or placeholders in the range's daemon-lifetime lines | PASS | MEASURED: an added-line grep for TODO, FIXME, `todo!`, `unimplemented!`, `allow(dead_code` and `#[ignore` in src-tauri/src/daemon finds none. |

### 22a8ecea (hub `cfg(unix)`)
- Minimal (+9, one file).
- The signature is unchanged for both callers (http.rs:68, lanes/guard.rs:133).
- The F3 Windows lane's static scan (F3/windows/PROGRESS.md Step 8) showed readiness.rs to be the only ungated unix API in the 43 commits.
- MEASURED now: both platforms' clippy compile ferryx-hub (`Checking ferryx-hub v0.1.0`) and finish with EXIT=0.
- It is also gated in w5-qa/F3/hubfix-gate-review.md.

### T12 harness fixes (scripts/qa/daemon-lifetime, 18 files, +1035/-82)
- **Teardown order on Windows** (run.mjs:364-385): QA tasks are removed before the recorded kill, and again after it.
  - The first pass is there because a watchdog respawns a killed daemon.
  - The second pass catches a still-alive GUI that re-registered its task.
  - Both are pinned in windows-all-fix.test.mjs:1156-1196.
- **Q13 legacy task:** it is registered by its UTF-16 definition record (jobs.mjs:126-182), from a non-elevated helper task that `finally` deletes.
- Kills stay pid-recorded and re-validated (`KILL_REFUSED`, reused pid).
- **NOTE N-H1 (open item O1, see F1-compliance-r4.md section 5).** On Windows, `remove()` deletes the supervisor-dir record only for the legacy task. Each generation task's `Ferryx-ferryx-qa-<runId>-<key>.xml` is left behind (40 files on maho-win since 2026-10-21). This is a harness cleanup gap against the todo 12 "no ... dir remains" line. It is not an F2 code-quality criterion.

## Clippy

### Command and environment
Each host ran the same command in a fresh worktree at cfe34b8e, transferred by git bundle:

`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`

- **Bundle:** `refs/clippy-final/head ^824b84e9`. sha256 9361f46ed04562d1a3f5742d51ceaeeb5cd06d5ec03555792c952ad3eecda8fa, identical on the Mac, omarchy (`sha256sum`) and maho-win (`certutil`).
- **ghostty submodule:** 6a508fd5, initialised with `-c protocol.file.allow=always`.
- **ui/dist:** a placeholder `ui/dist/index.html` was created, because tauri-build needs frontendDist to exist.

### Results
| Host | Toolchain and env | EXIT | error lines | Finished | Log |
| --- | --- | --- | --- | --- | --- |
| omarchy (Linux) | cargo/clippy 1.98.0; TMPDIR=~/code/ferryx-wt/.tcf; private CARGO_TARGET_DIR ~/code/ferryx-wt/.target-clippy-final-20261023 | **0** | 0 | `Finished dev profile … in 1m 37s` | w5-qa/F2-platform/omarchy-clippy-cfe34b8e.log |
| maho-win (Windows) | cargo 1.99.0 / clippy 0.1.99; zig 0.16.0 (C:\Users\sook\ferryx-wt\.zig-0.16.0 first on PATH); cmake 4.4.4 (WinGet, first on PATH); CMAKE_GENERATOR unset; CMAKE_POLICY_VERSION_MINIMUM=3.5; CARGO_INCREMENTAL=0; private CARGO_TARGET_DIR C:\Users\sook\ferryx-wt\.target-clippy-final-20261023 | **0** | 0 | `Finished dev profile … in 2m 48s` | w5-qa/F2-platform/windows-clippy-cfe34b8e.log |

### Warning totals (the plan command has no `-D warnings`, so warnings do not gate)
| Host | lib | lib test | unique `src/` warning locations | on lines added in 66f388f3..cfe34b8e |
| --- | --- | --- | --- | --- |
| omarchy | 627 | 820 (583 duplicates) | 838 (834 at 66f388f3, same method) | 4 |
| maho-win | 646 | 814 (590 duplicates) | 849 (844 at 66f388f3, same method) | 5 |

The "added lines" are the `git diff -U0 66f388f3..cfe34b8e -- src-tauri crates` new-side line numbers, intersected with the warning locations.

Unique locations are counted as distinct `path:line` from `warning:` headers plus their `-->` line, using the same parser on both revisions' logs. That is why they differ slightly from F2-platform/summary.md's 841/851.

### omarchy warnings on lines added in range (MEASURED, blamed)
- `src/daemon/client.rs:1992`: collapsible match. 0179db56, other session.
- `src/hub_runtime.rs:241`: "unneeded `return` statement". da1cdeb8, hub, other session.
- `src/terminal/paired_daemon.rs:312`/`:323`: large `Err` variant. cb1ee6c4, other session.

### Windows warnings on lines added in range (MEASURED, blamed)
- `src/daemon/client.rs:1992` "this `match` can be collapsed into the outer `match`". 0179db56 (agent-control Wait timeout, Cursor Agent). This is not daemon-lifetime.
- `src/hub_runtime.rs:335` "field `tasks` is never read" and `:588` "function `home_dir` is never used". da1cdeb8 (hub, other session). The Windows-only dead code fits the unix-only hub control socket.
- `src/terminal/paired_daemon.rs:312`/`:323` "the `Err`-variant … is very large". cb1ee6c4 (paired direct route, other session).

On either platform, none is on a daemon-lifetime line (T12 fixes, 134dcc40, 22a8ecea). The Windows-only daemon dead-code lints listed in F2-platform/summary.md (control_server.rs:25, handover.rs:123/130/611/1058/1310, owner_admission.rs:7, etc.) are unchanged, carried from 66f388f3.

`ferryx-hub` is a path dependency, not a workspace member, so clippy only compiles it (`Checking ferryx-hub`). It reports no lints for it. Compiling it is exactly what proves 22a8ecea on windows-msvc.

### Run notes
- **omarchy attempt 1** was started with `setsid nohup`. It stopped at "Checking x11rb" when its tailscale ssh session ended, with no exit line. It is kept as w5-qa/F2-platform/omarchy-clippy-cfe34b8e-attempt1-killed.log.
- **omarchy attempt 2** ran in the same worktree and target, under a held ssh session, with no product or env difference. That is why it finished in 1m 37s: the dependencies were already checked.
- maho-win C: free was 30.2 GB before and 28.0 GB after. No vctip.exe or cargo process was left behind (`Get-Process vctip,cargo,rustc,clippy-driver`: none).

## NOTEs
- **N-S1** (other session): agent_observer.rs:398 is a sleep-poll and :421 is a fixed sleep before a negative assertion. It is outside the daemon-lifetime scope; report it to its owner.
- **N-H1 / O1:** the harness leaves Windows supervisor-dir records behind. See F1-compliance-r4.md section 5 for the exact fix.
- **N-TE3** (carried): the Q04 harness parses the Debug-formatted `ownerState`.
- **N-RB2, N-PTY1, N-T1** (carried).
- **Windows QA environment** (F1 r4 N11): the build env on maho-win changed (CMake 4.4.4, `CMAKE_POLICY_VERSION_MINIMUM`, HKCU `CMAKE_GENERATOR` removed). This clippy used the same env. The cmake crate's `-MD` override now applies (final2/PROGRESS.md Step 4).

## Slop / overfit pass (remove-ai-slops and programming criteria, applied directly; skill files not loaded)
- **update_pending `enter_and_decide_log`.** It is a small extraction that makes the "log once" decision observable without a global subscriber. It is justified by a measured flake (2 of 5 runs on maho-win, per the 134dcc40 message). It is not speculative.
- **`log_line()`.** It formats one line. Its text is machine-consumed by QA Q09, so pinning it with a test is allowed. It does not mirror the implementation beyond that contract.
- **Tests in range.** No deletion-only, tautological or implementation-mirroring tests. The harness test files are large (windows-all-fix.test.mjs +286), but each case names a measured failure (only-q11 27232, only4 leftover, Q13 WORKGROUP) and asserts call order or arguments. That is acceptable for a harness whose failures were environmental.
- **Linux hub key filter** (`hub_config_key`/`hub_secret_key`). Two tiny predicates with one test. They are not over-abstracted.
- **Code review report check.** No separate code-review report covers this range. The skill-perspective pass is this section. w5-qa/t12-final-gate-review.md:100 shows the same overfit criterion for the T12 commits ("I found no tautological or deletion-only tests").

## Checked artifact paths
- Logs:
  - w5-qa/F2-platform/{summary.md, windows-clippy-cfe34b8e.log, omarchy-clippy-cfe34b8e.log, windows-clippy.log (66f388f3 baseline for the location diff)};
  - w5-qa/F2-clippy-66f388f3.log (macOS, existence).
- Reviews and progress:
  - w5-qa/t12-final-gate-review.md, w5-qa/t12-regate-review.md;
  - w5-qa/F3/hubfix-gate-review.md (existence);
  - w5-qa/F3/windows/PROGRESS.md, w5-qa/F3/windows/final2/PROGRESS.md.
- Diff 66f388f3..cfe34b8e:
  - src-tauri/src/daemon/{client,protocol,server,session_service,supervisor_linux,supervisor_windows,update_pending,update_pending_tests,a03_owner_cli_fixture,mod}.rs;
  - lib.rs, main.rs;
  - crates/ferryx-hub/src/readiness.rs (22a8ecea);
  - scripts/qa/daemon-lifetime/lib/jobs.mjs, run.mjs (teardown).
- Source at cfe34b8e: cli.rs:3040-3070, hub_runtime.rs:227-300, agent_observer.rs:385-430.

## Not verified, with the command that settles it
1. **macOS clippy at cfe34b8e** (`cfg(target_os = "macos")` code in range: none daemon-lifetime). On this Mac, by the orchestrator only: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` in a fresh worktree at cfe34b8e.
2. **Unit tests at cfe34b8e on each host** (clippy proves compilation, not test results). The last measured runs are the T12 gates (t12-regate-review.md: `daemon::` 394 passed, at 3f6c1f78's product tree). Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- daemon:: terminal::pty session::` on omarchy and maho-win.
3. **The harness's own node tests at cfe34b8e:** `bun test scripts/qa/daemon-lifetime` (or `node --test` as the lanes ran them).

VERDICT: APPROVE-SOURCE-ONLY-MACOS
