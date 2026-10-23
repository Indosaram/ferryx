# F2 platform evidence gaps: Linux and Windows clippy, plus the Linux contract test

Commit: 66f388f3dd1c3f5f2afd9cf0e2f8423d8c7ab179 (main), run on 2026-10-22. These runs close items 2 and 3 of F2-review-r3.md ("Not verified by me").

| Check | Host | Toolchain | Exit | Errors | Log |
|---|---|---|---|---|---|
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` | omarchy (Linux) | cargo/clippy 1.98.0 | **0** | 0 | omarchy-clippy.log |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` | maho-win (Windows) | cargo/clippy 1.99.0, Zig 0.16.0, CARGO_INCREMENTAL=0 | **0** | 0 | windows-clippy.log |
| `cargo build --features qa-protocol-bump` (bumped daemon) | omarchy | 1.98.0 | **0** | 0 | omarchy-contract.log (BUMP BUILD) |
| `env -u FERRYX_DATA_DIR FERRYX_QA_BUMPED_DAEMON=<bumped ferryx> cargo test --test daemon_update_pending_contract -- --ignored --test-threads=1` | omarchy | 1.98.0 | **0** | 0 | omarchy-contract.log |

## Contract test (Linux branch)
- Result: `incompatible_old_owner_keeps_its_terminals_and_retires_when_idle ... ok`, with 1 passed and 0 failed in 9.19 s. CONTRACT_EXIT=0.
- The bumped daemon was copied from `.target-f2plat/debug/ferryx` to `.tmp-f2plat/ferryx-bumped` (sha256 prefix 47905e8eef02f561).
- `~/.local/share/ferryx/daemon-runtime` was empty before the run and empty after it. No real staging root was touched.
- `pgrep` for `ferryx --daemon` was empty both before and after. The test left no daemon behind. No PID was killed by this lane.
- I checked the test's isolation by reading the source before running it:
  - The old owner is spawned with `env_clear()` and points at temporary FERRYX_RUNTIME_DIR, DATA_DIR, SESSION_DIR, HOME, and TMPDIR.
  - The client uses `DaemonClient::new_with_socket(<temp runtime>/daemon.sock)` and sets `FERRYX_DATA_DIR` to a temp dir before any client exists.
  - It asserts that staging stays under that temp dir.
  - With the isolation env set, the supervisor policy never installs a real unit (supervisor.rs ISOLATED_RUNTIME_ENV).
- Build note: the bump build and the test ran with `CARGO_PROFILE_DEV_DEBUG=0`, because omarchy /home had only about 8 GB free (100% used). The 2026-10-19 w4c3 lane used debuginfo. This does not change the code under test.

## Clippy totals (warnings only; the plan command has no `-D warnings`)
| Host | lib | lib test | Targets reporting warnings | Unique warning locations | error lines |
|---|---|---|---|---|---|
| omarchy | 622 | 814 (577 duplicates) | 24 | 841 | 0 |
| maho-win | 641 | 809 (585 duplicates) | 24 | 851 | 0 |

## Warnings on daemon-lifetime lines
- **Method.** I ran `git blame 66f388f3` on every warning location.
  - Lines last changed on or after 2026-10-17: 55 on omarchy and 66 on maho-win.
  - Most come from non-daemon-lifetime commits: c7fbfab3 recovery, 8e1fd515 paired-host, and 153b3ebe billing.
  - The list below keeps only the daemon-lifetime commits (bbf29def, 9e429cbf, 2c61198b, 63845885, d2fccf63, 744d3063, d35f2215, 8e52369a, c21baecf, e4bc115f).
- **Caveat.** There is no pre-plan platform baseline, so "new" here means "on a line authored by this plan". None of these is an error, and none is `-D warnings`-gating.

### omarchy (Linux)
- `src-tauri/src/daemon/client.rs:6030` (e4bc115f) file opened with `create`, but `truncate` behavior not defined
- `src-tauri/src/daemon/supervisor_windows.rs:317` (d2fccf63) using `chunks_exact` with a constant chunk size

### maho-win (Windows)
- `src-tauri/src/daemon/control_server.rs:25` (9e429cbf) function `control_socket_path` is never used
- `src-tauri/src/daemon/control_wire_tests.rs:318` (c21baecf) constant `UPGRADE_CHILD_ROOT` is never used
- `src-tauri/src/daemon/control_wire_tests.rs:550` (2c61198b) function `spawn_request` is never used
- `src-tauri/src/daemon/control_wire_tests.rs:565` (2c61198b) function `assert_daemon_retiring` is never used
- `src-tauri/src/daemon/handover.rs:123` (d35f2215) enum `CommitResolution` is never used
- `src-tauri/src/daemon/handover.rs:130` (d35f2215) function `resolve_commit` is never used
- `src-tauri/src/daemon/handover.rs:611` (744d3063) variants `Control` and `AgentState` are never constructed
- `src-tauri/src/daemon/handover.rs:1058` (c21baecf) field `candidate_starter` is never read
- `src-tauri/src/daemon/handover.rs:1310` (8e52369a) method `note_transferred_local` is never used
- `src-tauri/src/daemon/handover.rs:1310` (8e52369a) methods `note_transferred_local`, `set_candidate_starter`, and `candidate_starter` are never used
- `src-tauri/src/daemon/owner_admission.rs:7` (bbf29def) unused import: `File`
- `src-tauri/src/daemon/server.rs:3395` (63845885) method `set_close_park_for_test` is never used
- `src-tauri/src/daemon/supervisor_windows.rs:317` (d2fccf63) using `chunks_exact` with a constant chunk size

The Windows-only entries are dead-code and unused-import lints that do not appear on Linux. That pattern suggests the items' callers sit behind `cfg(unix)` (for example the UDS control socket, the handover candidate starter, and the unix-only control-wire tests). This is an inference from the Linux/Windows diff; I did not trace each item's cfg gates in the source. Either way they are lints, not errors, and not behavioral findings.

## Run notes
- **Windows first attempt (kept as windows-clippy-attempt1-zig017.log).** It exited 101 because PATH had Zig 0.17.0:
  - `[ghostty build error] Incompatible Zig version '0.17.0': libghostty-vt requires Zig 0.16.0`
  - This is an environment issue, not a code issue. The rerun prepended `C:\Users\sook\ferryx-wt\.zig-0.16.0` to PATH, the same as the w4c4 lane's run-win.ps1, and exited 0.
- **Isolation.** I used separate target dirs: `~/code/ferryx-wt/.target-f2plat` and `C:\Users\sook\ferryx-wt\.target-f2plat`. The shared .target-w5qa-20261019 dirs were not used.
- **omarchy ghostty submodule.** The canonical clone has no `.git/modules`, so the submodule was initialised with `-c protocol.file.allow=always` from the local clone at `.git/worktrees/w5qa-v1-20261019/modules/src-tauri/vendor/ghostty`, which already contained 6a508fd5.
- **Cleanup.** Both f2-plat-20261022 worktrees were removed (`worktree remove --force` followed by `prune`), and both .target-f2plat dirs plus omarchy .tmp-f2plat were deleted.
  - Remaining worktrees are all foreign and were left alone. omarchy: daemon-lifetime-w5-t12-20261018, qr-mainbase-20261017, qr-removal-20261017, w5qa-baseline-20261019, w5qa-v1-20261019.
  - maho-win: daemon-lifetime-w5-t12-20261018, qa-int2-win-20261022, qa-int2-win-base-20261022, qa-int2-win-v1-20261022, w5qa-baseline-20261019, w5qa-r3-20261020, w5qa-v1-20261019.


## cfe34b8e rerun (2026-10-23): closes the F2 r4 gap left by T12 and 22a8ecea, which changed src-tauri and crates after 66f388f3
| Check | Host | Toolchain | Exit | Errors | Log |
|---|---|---|---|---|---|
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` @ cfe34b8e | omarchy (Linux) | cargo/clippy 1.98.0; TMPDIR=~/code/ferryx-wt/.tcf; private target ~/code/ferryx-wt/.target-clippy-final-20261023 | **0** | 0 | omarchy-clippy-cfe34b8e.log (attempt 1 died with its ssh session, no exit line: omarchy-clippy-cfe34b8e-attempt1-killed.log) |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` @ cfe34b8e | maho-win (Windows) | cargo 1.99.0 / clippy 0.1.99; zig 0.16.0 first on PATH; cmake 4.4.4 first on PATH; CMAKE_GENERATOR unset; CMAKE_POLICY_VERSION_MINIMUM=3.5; CARGO_INCREMENTAL=0; private target C:\Users\sook\ferryx-wt\.target-clippy-final-20261023 | **0** | 0 | windows-clippy-cfe34b8e.log |

- **Transfer.** Bundle `refs/clippy-final/head ^824b84e9`, sha256 9361f46e…a8fa, the same on both hosts. Fresh worktrees `clippy-final-20261023` at cfe34b8e, ghostty 6a508fd5 (`protocol.file.allow=always`), placeholder ui/dist.
- **Totals.** lib / lib test warnings: omarchy 627 / 820 (583 dup), maho-win 646 / 814 (590 dup).
- **Warnings on lines added in 66f388f3..cfe34b8e.** omarchy 4, maho-win 5. All are on other sessions' lines: client.rs:1992 0179db56, hub_runtime.rs da1cdeb8, paired_daemon.rs cb1ee6c4. None is on a daemon-lifetime line. Detail: ../../F2-review-r4.md.
- **Cleanup.** Removed both clippy-final-20261023 worktrees (`worktree remove --force` + `prune`), refs/clippy-final/head, both private target dirs, both bundles and scratch scripts. No vctip.exe or cargo process was left on maho-win.
