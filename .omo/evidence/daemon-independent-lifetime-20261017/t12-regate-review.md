# t12 re-gate (delta only: 134dcc40, previous-gate Blocker 1, the flaky update_pending test)

Scope: `git diff e7bad8a7 134dcc40` on fix/qa-win-all-20261022 (2 files: src-tauri/src/daemon/update_pending.rs and update_pending_tests.rs). Everything the previous gate confirmed (w5-qa/t12-final-gate-review.md) is out of scope.

recommendation: APPROVE (confirmed)
blockers: none

## originalIntent / desiredOutcome / userOutcomeReview
- originalIntent: clear Blocker 1. `update_pending_reads_inventory_and_sends_retire_when_idle_once` failed 2 of 5 runs on maho-win, and the fix must not change product behaviour or weaken any assertion.
- desiredOutcome: a test that passes on every run on maho-win with default parallelism and still goes RED when the "once per runtime dir per GUI run" log contract breaks. The QA all4/all3b results still hold, and the trial merge into main is clean.
- userOutcomeReview: met. The race is real and is removed by construction rather than masked: the test no longer depends on global tracing state. 12 of 12 consecutive scoped runs and the full daemon:: run passed on maho-win. Both mutants go RED, and the runtime path and QA harness are unaffected.

## Check 1: Cause. Confirmed (SOURCE; tracing-core 0.1.36 and tracing 0.1.44, versions taken from src-tauri/Cargo.lock)
- SOURCE ~/.cargo/registry/src/index.crates.io-*/tracing-core-0.1.36/src/callsite.rs:308-330. `DefaultCallsite::register` runs `rebuild_callsite_interest` once, on whichever thread reaches the callsite first, and caches the result in the callsite's global interest.
- SOURCE callsite.rs:544-547 and 562-566. While `has_just_one` holds, the rebuilder is `JustOne` and calls `dispatcher::get_default`, which returns the calling thread's dispatcher. dispatcher.rs:379-397 shows that a thread without a scoped subscriber resolves to the global or NONE dispatcher.
- SOURCE subscriber.rs:675-677. `NoSubscriber::register_callsite` returns `Interest::never()`. callsite.rs:490-506 stores it.
- SOURCE tracing-0.1.44/src/macros.rs:36 and 67. The event macro short-circuits on `interest.is_never()` before any thread-local subscriber is consulted.
- SOURCE dispatcher.rs:479 with callsite.rs:484-488. Interest is rebuilt only when a new Dispatch is registered. So when a parallel subscriber-less test registers the same `info!` callsite after this test's `set_default`, `never` is cached and this test's line is dropped. That produces exactly the gate's "left 0, right 1".
- MEASURED (lane): run-all-fix/30-flaky-fix/maho-win/master.log. repro-old-1..3 (the old capture test with a forced pre-hit from a subscriber-less thread) FAILED 3 of 3, and repro-new-1..3 passed. I did not re-run the repro refs. The source chain alone is sufficient.

## Check 2: Fix correctness. Confirmed (SOURCE: git diff e7bad8a7 134dcc40)
- Product behaviour is unchanged. update_pending.rs:89-95: `enter` calls `enter_and_decide_log` and logs `tracing::info!("{line}")` exactly when the decision is Some. update_pending.rs:136-145: Some is set only on the branch that logged before, where RetireWhenIdle is accepted (`Live`) under `!retire_requested`, and that branch is gated once per runtime dir by `retire_sent()` (update_pending.rs:63-80, unchanged). Warn paths, op order and return values are identical. The only production caller is client.rs:2997 `update_pending::enter`, which is unchanged.
- The logged text is byte-identical. The e7bad8a7 update_pending.rs:124 literal "Sent RetireWhenIdle; the running daemon exits once its terminals close" equals `RETIRE_SENT_LOG` at 134dcc40 update_pending.rs:83, and `"{line}"` formats it verbatim. QA Q09 (scripts/qa/daemon-lifetime/scenarios/q07-q09.mjs:567) counts `/RetireWhenIdle/g === 1`, which still matches. Only the callsite file:line metadata moves, and the fmt layer does not print it.
- Exactly once per runtime dir per GUI run: the same gating as before, and enter logs at most once per call.
- No assertion was weakened. update_pending_tests.rs:174-195 still asserts live_sessions == 3, retire_requested, the pending-line regexes, `op_names == [Inventory, RetireWhenIdle]`, and an unchanged op sequence after the second enter ("no Shutdown is ever sent"). The log assertion moved from a captured count to `logged == [RETIRE_SENT_LOG]` across both calls, which is stricter: it pins the text and the count. One assertion was added: the pending line never names the op.
- No sleep, retry, `#[ignore]` or `--test-threads` appears anywhere in the diff.

## Check 3: Determinism on maho-win. Confirmed (MEASURED: run-all-fix/40-regate-maho-win/master.log)
- Setup: a fresh worktree `C:\Users\sook\ferryx-wt\t12-regate-20261022` was created from my own bundle. The bundle sha256 is 96f25a23e3b489342bf922049580ff95470923c80acd3af6295ef613c6d459e1 by Mac shasum and also by maho-win certutil (40-regate-maho-win/sha.txt). HEAD was 134dcc40f6b7, zig 0.16.0 (`C:\Users\sook\ferryx-wt\.zig-0.16.0`) was first on PATH, and `CARGO_TARGET_DIR=C:\Users\sook\ferryx-wt\.target-t12-regate-20261022`. Parallelism was the default throughout.
- `daemon::update_pending`: scoped-01..12 were 12 consecutive EXIT=0 runs, each "11 passed; 0 failed" (40-regate-maho-win/scoped-NN.out.txt).
- `daemon::`: daemon-all EXIT=0, "394 passed; 0 failed; 1 ignored" (daemon-all.out.txt).
- Every non-test failure I hit was environment, not the test, and each was established:
  - run1 (run1-scoped-01-nodist.err.txt): the build script panicked with "resource path `..\ui\dist` doesn't exist". My setup ran only `bun install`. Fixed with `bun run build` (uibuild.txt).
  - run2 scoped-01..03 (run2-enospc-*.err.txt): LNK1318 PDB error, then "os error 112" (disk full; C: dropped to 0.2 GB). The cause was a full test build with incremental artifacts in a fresh target dir, on a disk that had 10 GB free. I deleted my own incremental and stale ferryx_lib objects and re-ran with CARGO_INCREMENTAL=0.
  - A stall after a passing test: MSVC `vctip.exe` (telemetry spawned by link.exe) outlives the link, and PowerShell 5.1 `Start-Process -Wait` waits for it. I killed my own orphaned vctip four times (vctip-kills.txt). Test output was already complete when each stall started.
- The lane's "scoped-01 exit 1 with no test output": 30-flaky-fix/maho-win/scoped-01.err.txt is 1731 lines and ends inside the compiler-warning stream. It has no `Finished` and no `Running` line, so cargo exited during the build of the test binary, before any test ran. scoped-02 (the next run) needed a full 3m53s rebuild of the same crate. So scoped-01 died mid-build (likely the same class of interruption: environment or resources) and was not a test failure. The exact killer is not recoverable from the lane's logs because stderr has no error line. In my 12 runs I saw no test-time failure of any kind.

## Check 4: Mutation. Confirmed (MEASURED, run myself)
- mut-never (`log = Some(RETIRE_SENT_LOG)` removed): EXIT=101, FAILED at update_pending_tests.rs:192 with left `[]`, right `[line]` (mut-never.out.txt:19-22).
- mut-every (sets the log whenever `retire_requested`, so on the second entry too): EXIT=101, FAILED at :192 with left `[line, line]` (mut-every.out.txt:19-22).
- restored: EXIT=0, 11 passed. RESTORED DIRTY=2: the two files are tauri-generated src-tauri/gen/schemas/*.json written by the build script, not source.
- These match the lane's own mut-never and mut-twice logs.

## Check 5: Trial merge and runtime path. Confirmed (SOURCE)
- `git merge-tree --write-tree origin/main fix/qa-win-all-20261022` gives tree d4b6cd75 with exit 0 and no conflicts (origin/main is b57f553d).
- 134dcc40 changes only the two update_pending files relative to e7bad8a7. It touches no scripts/qa harness file. The runtime change is a behaviour-identical refactor (check 2), and b08fab25 is an ancestor. So the all4/all3b QA results at e7bad8a7/b08fab25 still hold for 134dcc40.

## Check 6: Cleanup (MEASURED: run-all-fix/40-regate-maho-win/90-cleanup.txt)
- Mine: worktree `t12-regate-20261022` removed and pruned, ref `refs/t12-regate/head` deleted, target dir, bundle, run scripts and the remote evidence dir deleted (evidence copied to the Mac first). Result: `EXISTS wt=False target=False bundle=False evidence=False`, and free space is 10.54 GB. The remote cleanup script is gone, and the local /tmp scripts are deleted.
- The lane's leftover `C:\Users\sook\ferryx-wt\flaky-fix-20261023\src-tauri` (empty) still exists. `rmdir` fails with a sharing violation ("another process is using it"), and no process command line references it, so it is not deletable now. Most likely some process has it as its working directory. I did not touch foreign processes.
- maho-win `git worktree list` after my cleanup: ferryx [main], daemon-lifetime-w5-t12-20261018, w5qa-baseline-20261019, w5qa-r3-20261020, w5qa-v1-20261019. All are foreign, and none is mine.

## Slop / programming pass (my own)
- The production extraction `enter_and_decide_log` is the minimal seam that removes global tracing state from the assertion. It is private, has a single caller plus the test, and adds no parsing or normalization. Acceptable.
- NOTE (not a blocker): `assert_eq!(logged.concat().matches("RetireWhenIdle").count(), 1)` is implied by `logged == [RETIRE_SENT_LOG]`, so it is slightly redundant, but it mirrors Q09's counting rule. The unit test no longer covers the one-line wiring in `enter` (`if let Some(line) = log { info!(...) }`). That wiring is verified by source here and end-to-end by QA Q09 against the real GUI log.
- No tautological, deletion-only or implementation-mirroring test was added.

## Evidence gaps / not verified
- The lane's repro-old/repro-new refs were not re-run by me (I relied on the lane's logs plus the source chain).
- The exact process that ended the lane's scoped-01 mid-build is not identifiable from its logs.
- No code-review report for this delta was provided to me, so I could not confirm the reviewer's skill-perspective coverage. My own pass is above.
- No Linux or macOS run of the delta; the scope was maho-win only. The change is platform-neutral Rust.
- Checked artifacts: w5-qa/t12-final-gate-review.md (reference only); run-all-fix/30-flaky-fix/{README.md, 00-cause-source.md, 90-maho-win-cleanup.txt, maho-win/master.log, prep.log, scoped-01/02 and build err logs}; run-all-fix/40-regate-maho-win/* (mine); the tracing-core and tracing sources in the cargo registry; q07-q09.mjs:567.

VERDICT: confirmed
