# Gate review: hub statvfs Windows fix (22a8ecea)

recommendation: REJECT (needs-fix)

Commit: 22a8ecea51809b69c4a1ad6bd01d45ed41ee7a34 (branch fix/hub-statvfs-windows), parent origin/main 3f6c1f78. One file, +9 lines: crates/ferryx-hub/src/readiness.rs.
Bundle: hubfix-gate-20261023.bundle sha256 92ee1244bb536a43a36e732ab70d3117b3509c009cc7ce0014bb94ba50684eda. MEASURED hubfix-gate/prep.log: certutil on maho-win gives the same value. omarchy sha256sum also matches.
Evidence dir (all MEASURED paths are relative to it): /Volumes/T9-Mac/project/ferryx-wt/.evidence/daemon-independent-lifetime-20261017/w5-qa/F3/hubfix-gate/

## originalIntent / desiredOutcome
- originalIntent: unblock plan F3 (daemon-independent-lifetime). origin/main 3f6c1f78 does not compile on Windows because readiness.rs calls libc::statvfs (MEASURED w5-qa/F3/windows/run2/build-A.log:187-200).
- desiredOutcome: one commit after which the Windows app and the hub crate build and test green, Unix behaviour is unchanged, and F3 Windows QA can run.

## userOutcomeReview
The fix is correct and complete for the compile break, and it is the minimal change: `cargo check --all-targets` of the whole app is now green on windows-msvc, and Unix is unchanged. It still fails the brief's stated bar, because `cargo test` of ferryx-hub on Windows exits 101 (15 failures, 3 of them even with `sh` on PATH). That is test-side Windows breakage the compile error used to hide, not damage done by this commit. Under criterion 3 as written, the gate cannot confirm.

## blockers
1. violatedCriterion: 3 ("cargo test -p ferryx-hub ... Both must exit 0" on maho-win).
   observation: on Windows, `cargo test --manifest-path crates/ferryx-hub/Cargo.toml` at 22a8ecea exits 101: 155 passed, 15 failed. With `C:\Program Files\Git\usr\bin` (sh) prepended to PATH it is still 101: 167 passed, 3 failed.
   evidencePointer: MEASURED hubfix-gate/win-test.log:550 and win-test-progress.txt (EXIT test=101); MEASURED hubfix-gate/win-test2.log:207 and win-test2-progress.txt (EXIT test=101).
   The 3 failures that remain with sh available:
   - SOURCE crates/ferryx-hub/src/chat/buttons.rs:243/257 `a_content_intel_button_runs_the_configured_command`: on Windows the `printf` resolves to Git's printf and the `\n` in the JSON arg is not a newline. Output `["ok%sokpublishok86"]` vs `["okpublish\n86"]` (MEASURED win-test2.log:185-188).
   - SOURCE crates/ferryx-hub/src/autonomy.rs:681-701 `spend_stays_pending_and_verified_needs_the_calendar_readback`: the test execs a `#!/bin/sh` script directly, giving os error 193 "not a valid Win32 application" (MEASURED win-test2.log, panic at autonomy.rs:701).
   - SOURCE crates/ferryx-hub/src/inbox_watch.rs:1104-1118 `collection_runs_the_imsg_and_mail_commands_and_a_gateway_row`: same shebang exec, os error 193 (MEASURED win-test2.log, panic at inbox_watch.rs:1118).
   The 12 more that fail without sh on PATH, all "program not found" from `Command::new("sh")` (SOURCE cron_exec.rs:399, tools.rs:65): cron_exec x5, cron.rs:235, schedule.rs x5 (1838/1901/1922/1957/1987), tools.rs:265 (MEASURED win-test.log:446-550).
   Attribution: none of these tests touches readiness.rs. They come from b2e3c53b, 4fa29009, d2745f8e, da1cdeb8, 5a9962a7 and c916056f (git log -S), so 22a8ecea did not cause them. Before this fix they could not run on Windows at all, because the crate did not compile. The fix is one of two things that need to happen; the other is a Windows-portable hub test suite (gate the shell-dependent tests with cfg(unix), or make them portable).

## Criterion results
1. POSIX-only surface (SOURCE grep `libc::|std::os::unix|nix::|getuid|geteuid|statvfs` over crates/ferryx-hub plus src-tauri callers): PASS.
   - readiness.rs:41-44 (new cfg(not(unix)) -> None) and :46-61 (statvfs arm, now cfg(unix)).
   - autonomy.rs:683, inbox_watch.rs:1110, meta/supervisor.rs:46/159: `std::os::unix::fs` inside cfg(unix).
   - meta/supervisor.rs:118-136: libc::kill under cfg(unix) through a local cfg(unix) `mod libc`, with a not(unix) arm.
   - control.rs:68, schedule.rs:605/641: Unix sockets under cfg(unix). src-tauri/src/hub_runtime.rs:232/255, 409, 593/661 call them only under cfg(unix) and have not(unix) arms.
   - No nix, no getuid. The compiler agrees: there are no other compile breaks (MEASURED hubfix-gate/win-check.log, `Finished`, EXIT check=0 in win-probe-progress.txt; the hub test build also compiles all of its test targets on Windows, see win-test2.log).
2. None handling: PASS (source).
   - SOURCE http.rs:65-74 `ready()` does `.unzip()`, giving (None, None), and classify_disk_pressure returns "unknown" (readiness.rs:13-15). No unwrap.
   - SOURCE lanes/guard.rs:131-134 `?` -> HostLoad::sample unwrap_or(100) (guard.rs:82): disk admits on Windows, the same fail-open as the /proc load and memory probes.
   - Runtime: http::tests hits /api/ready on Windows and passes (it is not in the failure list in win-test.log/win-test2.log). That test asserts only status 200, not the "unknown" body (SOURCE http.rs:534-544), so "unknown" on Windows is source-verified, not measured.
3. maho-win build proof: check PASS, test FAIL (blocker 1).
   - Fresh worktree C:\Users\sook\ferryx-wt\hubfix-gate-20261023 from the bundle, sha checked by certutil. Submodule init with protocol.file.allow=always succeeded. zig 0.16.0 first on PATH, CMAKE_GENERATOR=Visual Studio 17 2022 (MEASURED win-probe-env.txt). CARGO_TARGET_DIR=.target-hubfix-gate, CARGO_INCREMENTAL=0.
   - `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`: exit 0 (MEASURED win-probe-progress.txt, win-check.log). Env note: tauri-build requires `ui/dist`, and a fresh worktree has none, so the first attempt panicked at build.rs:122 "resource path ..\ui\dist doesn't exist". I created a placeholder ui/dist/index.html (build.ps1). The run1 F3 harness builds the UI first, so this is not a product issue.
   - Brief deviation: the command `cargo test -p ferryx-hub` as written cannot run anywhere ("package ferryx-hub cannot be tested because it requires dev-dependencies and is not a member of the workspace"): crates/ferryx-hub/Cargo.toml declares its own `[workspace]`. I used `cargo test --manifest-path crates/ferryx-hub/Cargo.toml` on both hosts.
   - C: free fell from 7.73 GB to a low of ~3.85 GB; after cleanup it is 7.68 GB.
4. omarchy Unix: PASS. `cargo test --manifest-path crates/ferryx-hub/Cargo.toml` at 22a8ecea: 172 passed, EXIT=0 (MEASURED hubfix-gate/linux-test2.log:183-191).
   - Env note: with the mandated TMPDIR=/home/indo/code/ferryx-wt/.tmp-hubfix-gate, control::tests::stop_reaches_the_socket_and_the_reply_names_the_op failed with "path must be shorter than SUN_LEN" (MEASURED linux-test.log:262,408-416). That is the socket path length, not the fix. The rerun used TMPDIR=/home/indo/code/ferryx-wt/.thg (still not /tmp) and passed. I used a private CARGO_TARGET_DIR (.target-hubfix-gate) because the shared .target was locked by a foreign build.
5. Mutation:
   - Windows, dropping the cfg(not(unix)) arm: `cargo check --all-targets` exits 101 with E0425 "cannot find function `disk_sample` in module `crate::readiness`" at http.rs:68 and lanes/guard.rs:133 (MEASURED hubfix-gate/win-mut-check.log:2-31, win-mut-progress.txt arm-present=True, EXIT mutcheck=101). The baseline main failure (E0425 statvfs) is in w5-qa/F3/windows/run2/build-A.log:187-200. I restored the file afterwards.
   - Unix, altering the unix arm's result (`Some((total, free))` -> `None`): all 172 tests still pass (MEASURED hubfix-gate/linux-mut-unix.log). No test covers the unix arm's result, and that gap predates this commit. NOTE, not a blocker.

## remove-ai-slops / programming pass (direct)
- Diff: 9 added lines. One cfg(not(unix)) stub, one cfg(unix) attribute, and a 3-line doc comment that states the why. No new helpers, no parsing or normalization, no tests added. Nothing tautological, mirroring, or deletion-only. No scope drift. No slop findings.
- Doc comment claim "matching the /proc-based load and memory probes in lanes::guard": accurate (SOURCE guard.rs:76-83).
- No code review report, manual QA matrix or notepad was supplied to this gate. My checks therefore stand alone, and I could not confirm reviewer skill-perspective coverage.

## Notes (not blockers)
- The commit message says "both the libc::statvfs type and function"; that matches build-A.log:187-200.
- Pre-existing, outside this diff: on Windows `/api/ready` always reports disk "unknown", and the lane guard never refuses on disk. Follow-up: a GetDiskFreeSpaceExW arm.

## Exact evidence gaps
- The "unknown" body on Windows is not asserted by any test; I verified it in source only.
- No test covers the Unix disk_sample result (mutation survived).
- Code review report, QA matrix and notepad were not provided.

## Cleanup
- maho-win: worktree hubfix-gate-20261023 removed, worktree list pruned, refs/hubfix-gate/head deleted, .target-hubfix-gate deleted (target-exists=False), bundle/scripts/ev dir removed, C: free 7.68 GB (MEASURED hubfix-gate/win-cleanup.log). The cleanup's process-kill step only matched its own wrapper cmd.exe (PIDs 22280, 12028); no cargo or rustc of mine was left running. Foreign worktrees, flaky-fix-20261023 and qaruns were untouched.
- omarchy: worktree removed and pruned, ref deleted, .target-hubfix-gate/.tmp-hubfix-gate/.thg/bundle removed (MEASURED hubfix-gate/lin-cleanup.log).
- Mac: no builds. The bundle in /tmp was removed, and fix worktree fix-hub-statvfs-20261023 is still clean at 22a8ecea.

VERDICT: needs-fix

---

## Re-verdict under corrected criterion 3 (2026-10-23, supersedes the recommendation and VERDICT above)

The user corrected criterion 3 to: "on maho-win, `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` exits 0, and no Windows hub test failure is caused by the diff; pre-existing failures are follow-ups with file:line, not blockers." The earlier `VERDICT: needs-fix` line above is kept as history.

The correction is sound, and I agree with it. 22a8ecea's purpose (commit message, brief Facts) is to make ferryx-hub, and the src-tauri that depends on it, compile on windows-msvc. F3 Windows QA builds the app and never runs the hub's own test suite. The original criterion 3 also required test fixes in files this commit does not own.

Re-evaluated from existing evidence only (no reruns):
- Part (a), check exits 0: MET. MEASURED hubfix-gate/win-probe-progress.txt `EXIT check=0`; hubfix-gate/win-check.log ends `Finished dev profile`. Same worktree, zig 0.16.0, VS 17 2022 generator, CARGO_INCREMENTAL=0 (hubfix-gate/win-probe-env.txt).
- Part (b), no Windows hub test failure caused by the diff: MET.
  - The diff only changes `readiness::disk_sample`. Its only callers are SOURCE http.rs:68 (`/api/ready`) and lanes/guard.rs:133 (via `HostLoad::sample`, which SOURCE mcp.rs:356 also uses).
  - On Windows, the tests that exercise those paths pass: `http::tests::health_cron_approvals_and_the_tracker_page` (hits /api/ready), `lanes::guard::tests::a_full_lane_budget_or_a_hot_host_is_refused`, and `lanes::guard::tests::proc_parsers_read_load_and_available_memory` (calls `HostLoad::sample(0)`). MEASURED hubfix-gate/win-test.log:345,367,368 and hubfix-gate/win-test2.log:78,106,107.
  - None of the 15 failing tests reaches disk_sample, HostLoad or /api/ready. Every one fails while spawning an external program: `sh` missing ("program not found"), Git `printf` escape semantics, or a `#!/bin/sh` script exec'd directly (os error 193).
  - Their test names first appear (git log -S) in commits before 22a8ecea, and they could not run on Windows before it because the crate did not compile (MEASURED w5-qa/F3/windows/run2/build-A.log:187-200).
- Criteria 1, 2, 4 and 5: unchanged. All pass, per the sections above; criterion 5's Unix-arm gap stays a NOTE.

recommendation: APPROVE
blockers: none

## Follow-ups (not blocking)

Windows hub test failures. Source: MEASURED hubfix-gate/win-test.log:446-550 (run without sh: 155 passed, 15 failed); "w/ sh" means the hubfix-gate/win-test2.log run with Git usr\bin on PATH (167 passed, 3 failed). The commit is where the test name first appears.
1. crates/ferryx-hub/src/chat/buttons.rs:256 `a_content_intel_button_runs_the_configured_command`: "program not found" (`printf`). With sh it still fails at buttons.rs:257, output `ok%sokpublishok86` instead of `okpublish\n86`. (b2e3c53b)
2. crates/ferryx-hub/src/autonomy.rs:640 `a_flush_enqueues_once_and_a_read_proposal_runs_when_the_ledger_is_on`: "program not found"; passes with sh. (4fa29009)
3. crates/ferryx-hub/src/autonomy.rs:701 `spend_stays_pending_and_verified_needs_the_calendar_readback`: a `#!/bin/sh` script exec'd directly (written at autonomy.rs:682), os error 193; still fails with sh. (4fa29009)
4. crates/ferryx-hub/src/cron.rs:235 `a_shell_job_does_not_send_and_a_new_pane_job_does`: "program not found"; passes with sh. (c916056f)
5. crates/ferryx-hub/src/cron_exec.rs:485 `a_relative_script_runs_in_the_imported_working_directory`: "program not found" (`Command::new("sh")` at cron_exec.rs:399); passes with sh. (d2745f8e)
6. crates/ferryx-hub/src/cron_exec.rs:528 `wake_agent_false_skips_the_model_and_a_script_alone_is_the_output`: "program not found"; passes with sh. (4fa29009)
7. crates/ferryx-hub/src/cron_exec.rs:559 `a_monitor_hash_is_committed_only_after_success_and_context_is_prepended`: "program not found"; passes with sh. (4fa29009)
8. crates/ferryx-hub/src/cron_exec.rs:659 `live_job_shapes_match_the_gateway`: the expected "cron script failed with Some(3)" is missing because there is no sh; passes with sh. (b2e3c53b)
9. crates/ferryx-hub/src/inbox_watch.rs:1118 `collection_runs_the_imsg_and_mail_commands_and_a_gateway_row`: a `#!/bin/sh` script exec'd directly (written at inbox_watch.rs:1104-1108), os error 193; still fails with sh. (4fa29009)
10. crates/ferryx-hub/src/schedule.rs:1838 `a_due_job_delivers_and_the_third_failure_opens_a_lane`: zero deliveries without sh; passes with sh. (da1cdeb8)
11. crates/ferryx-hub/src/schedule.rs:1901 `a_disabled_job_does_not_run_unless_forced`: status "failed" without sh; passes with sh. (d2745f8e)
12. crates/ferryx-hub/src/schedule.rs:1922 `run_now_posts_an_originless_job_to_the_imported_home_channel`: status "failed" without sh; passes with sh. (d2745f8e)
13. crates/ferryx-hub/src/schedule.rs:1957 `a_due_originless_job_targets_the_imported_home_channel`: zero deliveries without sh; passes with sh. (d2745f8e)
14. crates/ferryx-hub/src/schedule.rs:1987 `approving_a_problem_does_not_run_a_disabled_job`: status "failed" without sh; passes with sh. (5a9962a7)
15. crates/ferryx-hub/src/tools.rs:265 `a_read_runs_and_a_spend_waits_for_approval`: "program not found" (`Command::new("sh")` at tools.rs:65); passes with sh. (4fa29009)
Suggested direction (owner's choice): gate shell-dependent hub tests with `#[cfg(unix)]`, or make the Windows path portable (run scripts through an explicit interpreter instead of exec'ing a shebang file).

Other follow-ups:
16. Unix-branch coverage gap: no test checks the result of the cfg(unix) `disk_sample` (SOURCE crates/ferryx-hub/src/readiness.rs:46-61). Changing `Some((total, free))` to `None` left all 172 omarchy tests green (MEASURED hubfix-gate/linux-mut-unix.log). The gap predates 22a8ecea.
17. The `/api/ready` test asserts only HTTP 200, not the `disk` field (SOURCE crates/ferryx-hub/src/http.rs:534-544). Windows `"disk":"unknown"` is therefore verified in source only (http.rs:65-74 -> readiness.rs:13-15), not by a test.
18. Product gap outside this diff: on Windows `/api/ready` always reports disk `unknown`, and the lane guard always admits on disk (SOURCE lanes/guard.rs:82). A real Windows sample (e.g. GetDiskFreeSpaceExW) would close it.

VERDICT: confirmed (corrected criterion 3 met: Windows `cargo check --all-targets` exits 0 per hubfix-gate/win-probe-progress.txt, and none of the 15 pre-existing Windows hub test failures reaches the changed disk_sample path; criteria 1, 2, 4 and 5 pass)
