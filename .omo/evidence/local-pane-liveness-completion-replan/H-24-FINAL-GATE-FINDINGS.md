# H-24 — explicit final findings for the RED final gates

Closes F1 audit hole **H-24**:

> `H-24 - Full final gates are RED (UI x3, lib on linux) with A/B classification but no recorded explicit final finding.`
> — `.omo/evidence/local-pane-liveness-completion-replan/F1-PLAN-TO-ARTIFACT-AUDIT.md:559` (hole list);
> the audit row itself is `:355`.

**Scope of this artifact.** It records, per RED final gate: the exact command, the revision it ran at, the raw
measured result, the A/B classification with its evidence, and one explicit final finding. It changes no
product, test, script or plan file. **Nothing was compiled or run for this artifact**; every number below is
quoted from a stored artifact or log that this session read, and each is cited. Where a number is not recorded
anywhere, this document says **not recorded** and names where it should have come from.

**Artifact version: v2 (2026-10-05).** v1's **G9 disposition was stale**: it said "repaired, not yet re-verified —
an open verification obligation" because it cited a **forward-looking dispatch line** instead of the measured
result that followed it. G9 is corrected in §3.4, and the same error class was then swept across every non-green
gate in **§5.1**. The ledger this artifact cites is **not in chronological order** (sections are inserted at
different points), so a later line number does **not** imply a later state; every line number below was
re-derived from the ledger text as it stood when this version was written, and the ledger is being edited by
other lanes concurrently (it grew from 3 134 to 3 271 lines during the sweep).

**Revision vocabulary used below** (all on branch `work/local-pane-liveness-completion-foundation`, base
`d82b35e4`; candidate chain read from `git log --oneline d82b35e4..HEAD`, HEAD `89a363a0`):

| Name | Commit | Why it matters here |
| --- | --- | --- |
| base | `d82b35e4` | the immutable A/B baseline |
| `21dea3c0` | round-3 repairs | **the revision the three full-UI gates ran at**; UI source of record |
| `7c0fed30` | transport fixture repair | the only post-`21dea3c0` commit touching `ui/**` (one test file) |
| `abd9e890` | split-journal portability | the revision the three full-`--lib` classifications ran at |
| `70eefafe` | handover adoption fix | the last revision with a measured linux full `--lib` |
| `c34b90fc` | QA producer surface | the revision the task-9 linux full `--lib` (2646/31/6) ran at |
| `314251e0` | QA-feature compile repair | last commit touching the gated QA surface |

`git diff --stat d82b35e4..HEAD -- src-tauri/src/daemon/session_service_machine_tests.rs` is empty (the file is
unchanged by this candidate) — relevant to H-27, recorded there.

---

## 1. The plan's final gates, and which of them are RED

The plan (`/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`,
`## Verification strategy`) names these verbatim final gates and requires "all changed behavior scopes and full
final gates green with nonzero counts" (Todo 8 acceptance) with "a preexisting failure requires A/B proof and
explicit final finding; failures within changed behavior block completion".

| # | Gate (verbatim) | Status at the last measured revision | This document |
| --- | --- | --- | --- |
| G1 | `bun run --cwd ui build` | **PASS ×3** — exit 0 (mac 11.57 s, linux 5.15 s, windows 5.78 s), `task-8/pass3/report.md` UI verdict table | not RED |
| G2 | `bun run --cwd ui test` (mac) | **RED** — exit 1 | §2.1 |
| G3 | `bun run --cwd ui test` (linux) | **RED** — exit 1 | §2.2 |
| G4 | `bun run --cwd ui test` (windows) | **RED** — exit 1 | §2.3 |
| G5 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | **PASS** — linux and mac exit 0 at `70eefafe`; windows exit 0 measured at `39e722ce` (the `70eefafe` diff touches no Windows path) — `task-8/pass3/FINAL-VERDICT-70eefafe.md`, Rust gate tables | not RED |
| G6 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` (linux) | **RED** — exit 101 | §3.1 |
| G7 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` (mac) | **NOT RUN** — structurally, not a pass | §3.2 |
| G8 | `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` (windows) | **NOT RUN** — left in flight, no verdict | §3.3 |
| G9 | `cargo test --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` (linux) | **RED (uncompilable)** at `c34b90fc` → repaired in `314251e0` → **re-verified green** (`qa_barrier` 19/19 at the repaired revision; all four selectors 61/61 in pass 4) | §3.4 |
| G10 | `--test daemon_handover_transfer_contract`, `--lib local_split_reliability_`, `--lib pane_liveness_` | **PASS** — linux at `70eefafe`: 5/0 in 6.21 s, 15/0, 54/0; mac 5/0-class gates measured at `39e722ce` (47/0, 15/0) (`task-8/pass3/FINAL-VERDICT-70eefafe.md`) | not RED |
| G11 | Windows `#![cfg(unix)]` integration targets (`daemon_handover_transfer_contract`, `ipc_hardening_contract`, `daemon_persistence_contract`, `daemon_handover_contract`) | **0 selected = structurally not run, NOT a pass** | §4 |

Ledger corroboration for the "not RED" rows: `/Volumes/T9-Mac/project/ferryx/.omo/ulw-execute/local-pane-liveness-completion-state.md:2881`
("UI: build exit 0 ×3; split 19/19 ×3; lifecycle 112/112 ×3; runner 28/28; full UI mac 6521 selected / …").

---

## 2. G2–G4 — `bun run --cwd ui test`, RED on all three hosts

### 2.1 Command and revision (all three hosts)

- **Command:** `bun run --cwd ui test` (the plan's verbatim full-UI gate; no file arguments).
- **Revision:** candidate **`21dea3c0`** ("round-3 backend/native/frontend/scripts repairs … **UI source of
  record**"), stated in `task-8/pass3/report.md` §"Full UI suite — all three hosts" and in the same file's
  commit table. Staged trees were sha256-verified per host (`task-8/pass3/report.md` §"Rust half on the
  repaired candidate").
- **Why that revision still speaks for the UI today:** after `21dea3c0`, exactly one commit touches `ui/**` —
  `7c0fed30` (`git log --name-only --pretty=format:'@@%h %s' 21dea3c0..HEAD -- ui` returns only
  `ui/src/lib/terminalTransport/terminalTransport.test.ts`). No UI **source** file changed, so the UI source
  bytes the suite ran against are still the candidate's UI bytes; only that one test file was later edited.

### 2.2 Raw results, as measured (not summarised)

Read from the raw logs, not from the report's tables:

| Host | Log | Raw exit | Selected | Tests | Test files | Duration |
| --- | --- | --- | --- | --- | --- | --- |
| mac | `task-8/pass3/mac/logs/full-ui.log` | 1 (`error: script "test" exited with code 1`) | 6521 | `192 failed \| 6329 passed (6521)` | `7 failed \| 365 passed (372)` | 453.52 s |
| linux | `task-8/pass3/linux/logs/full-ui.log` | 1 | 6521 | `1224 failed \| 5297 passed (6521)` | `99 failed \| 273 passed (372)` | 393.45 s |
| windows | `task-8/pass3/windows/logs/full-ui.log` | 1 (`NATIVE_EXIT=1`) | 6521 | `193 failed \| 6323 passed \| 5 skipped (6521)` | `9 failed \| 363 passed (372)` | 637.33 s |

**No host timed out** (`"timedOut": false` for all three in `task-8/pass3/pass3-failure-classification.json`);
the mac suite finished on its own, so pass-2's 1200 s SIGKILL did not recur
(`task-8/pass3/report.md`, same section).

Per-file failure counts, recomputed from the raw logs by counting `FAIL  <file>` lines:

| Host | Failing files | Largest contributors |
| --- | --- | --- |
| mac | 7 | `NativeTerminalPane.test.tsx` 186; `App.test.tsx`, `TerminalSearchOverlay.test.tsx`, `TerminalSplitView.paneHandleReach.test.tsx`, `pairedDaemonRollout.test.ts`, `updater.test.ts`, `terminalTransport.test.ts` 1 each |
| linux | 99 | `NativeTerminalPane.test.tsx` 186; `App.test.tsx` 154; `RemoteUI.test.tsx` 104; `accountSession.test.ts` 54; `App.notifications.test.tsx` 50; … (99 files, 1465 `FAIL` lines) |
| windows | 9 | `NativeTerminalPane.test.tsx` 186; `agentStateExtension.test.ts`, `App.test.tsx`, `SettingsDialog.test.tsx`, `TerminalSearchOverlay.test.tsx`, `TerminalSplitView.paneHandleReach.test.tsx`, `pairedDaemonRollout.test.ts`, `updater.test.ts`, `terminalTransport.test.ts` 1 each (194 `FAIL` lines) |

(`NativeTerminalPane.test.tsx` contributes 186 of the mac and windows totals, i.e. 186/192 and 186/193.)

### 2.3 A/B classification and its evidence

**Pass-2 per-failure-file A/B** — `task-8/pass2/{linux,windows,mac}-ab-classification.jsonl`, one record per
failing file, each carrying `baseNative`, `baseSelected`, the candidate failure blocks and the matched base
assertions. Entry counts, recomputed from the three files:

| Host | Entries | `AB_CONFIRMED_PRE_EXISTING` | `MIXED_*` | `CANDIDATE_CAUSED` |
| --- | --- | --- | --- | --- |
| linux | 99 | 97 | 1 (`MIXED_UNATTRIBUTED`, `src/App.test.tsx`) | 1 (`NativeTerminalPane.presentation.test.tsx`) |
| windows | 11 | 7 | 2 (`MIXED_UNATTRIBUTED`: `App.test.tsx`, `App.remote.test.tsx`) | 2 (`App.pairedDaemon.test.tsx`, `NativeTerminalPane.presentation.test.tsx`) |
| mac | 10 | 6 | 2 (`MIXED_CHANGED_ASSERTION`: `App.test.tsx`, `App.remote.test.tsx`) | 2 (same two as windows) |

Human-readable roll-up of the same data: `task-8/pass2/baseline-classification.md` (three per-host tables) and
`task-8/pass2/mandatory-ab-classification.md` (the two mandatory files).

**The two mandatory protected files, A/B-confirmed pre-existing on all three hosts**
(`task-8/pass2/mandatory-ab-classification.md`): `TerminalSplitView.paneHandleReach.test.tsx` (base and
candidate both expect `h-3` and receive `h-5`; base `1 selected / 1 failed`, native 1 on both hosts) and
`pairedDaemonRollout.test.ts` (base and candidate both resolve the same capabilities response with
`directoryBrowseV1/futureV9`; base `5 selected / 1 failed / 4 passed`, native 1). The same file records mac
reproducing both assertions independently.

**Pass-3 delta** (`task-8/pass3/pass3-failure-classification.json`): `NEW_IN_PASS3` =
`src/lib/terminalTransport/terminalTransport.test.ts` on **all three** hosts;
`pass2HadButPass3DoesNot` = 4 files on mac (`App.remote`, `App.pairedDaemon`,
`NativeTerminalPane.presentation`, `SettingsDialog.cli`), 3 on windows, 1 on linux.

**Candidate-caused in the UI set — exactly one, and it is repaired:**
`terminalTransport.test.ts` ("TauriTerminalTransport listSessions queries tauri listTerminalSessions"),
`AssertionError: expected [...] to deeply equal [...]` with the received objects carrying an extra
`incarnation: null` (`task-8/pass3/linux/logs/full-ui.log` shows the diff verbatim). A/B
(`task-8/pass3/report.md`): base `d82b35e4` 11 passed (11) / native **0** vs candidate `21dea3c0`
1 failed | 10 passed (11) / native **1**. Cause: `ui/src/lib/terminalTransport/tauriTransport.ts` adds
`incarnation: s.incarnation ?? null` while the test at `:88` asserts exact deep equality. **Repaired in
`7c0fed30`** (test file, +3/−2) and re-verified by re-running that suite: mac 11/11, linux 11/11, windows
11/11 (`task-8/pass3/report.md`).

**Environment-shaped and non-test-failure signals in the same logs** (raw, not inferred):
- mac **and** windows emit an uncaught `Error: [vitest] No "enrollThisMachine" export is defined on the
  "./lib/tauri" mock` originating in `src/components/onboarding/AccountStep.tsx:28` via `WelcomeWizard.tsx:10`
  (`task-8/pass3/mac/logs/full-ui.log`, `…/windows/logs/full-ui.log`). Candidate-introduced (0 at base, 0 in
  pass 1/2 logs), **adds no test failure** — test counts are identical to base on mac (154 tests / 1 failed) —
  and was routed to the frontend lane (`task-8/pass3/report.md`).
- windows also emits uncaught `listen EACCES: permission denied /tmp/ferryx-ext-test.sock` from
  `src/lib/agentStateExtension.test.ts:93` — a host/socket permission condition, and the file it breaks is one
  of the nine windows failing files.

### 2.4 Explicit final findings — G2, G3, G4

- **G2 (`bun run --cwd ui test`, mac) — FINAL FINDING: accepted as pre-existing-dominated, does not block
  acceptance of the changed behavior, with a named residual risk.** Evidence: 192 failed / 6329 passed over
  7 failing files; 6 of the 7 were already classified pre-existing at pass 2 with base A/B, the 7th
  (`terminalTransport.test.ts`) was the sole candidate-caused failure and is repaired in `7c0fed30` with
  11/11 ×3 re-verification. **Residual risk (named):** (a) the full suite has **no green run at any revision** —
  the criterion "full final gates green" is therefore **not literally satisfied** by this gate; (b) the repaired
  revision was re-verified only on that one file, so no full-UI re-run exists at any post-`7c0fed30` revision;
  (c) `src/App.test.tsx` was labelled `MIXED_CHANGED_ASSERTION` at pass 2 and is **not** fully attributed
  failure-by-failure — it is carried as an unclosed classification, not folded into "pre-existing".
- **G3 (`bun run --cwd ui test`, linux) — FINAL FINDING: accepted as pre-existing-dominated, does not block
  acceptance of the changed behavior, with the same three named residuals.** Evidence: 1224 failed / 5297
  passed over 99 failing files; pass-2 A/B classifies 97 of the 99 `AB_CONFIRMED_PRE_EXISTING`, 1
  candidate-caused (`NativeTerminalPane.presentation.test.tsx`, since fixed — it is in
  `pass2HadButPass3DoesNot`), 1 `MIXED_UNATTRIBUTED` (`src/App.test.tsx`, base itself exits 1 with 154 selected).
  The linux count is the largest because the linux host fails whole files on environment-shaped families
  (`notificationCenter` `localStorage` absence, remote/paired suites), which is why 1224 tests fail inside 99
  files rather than a broader file set.
- **G4 (`bun run --cwd ui test`, windows) — FINAL FINDING: accepted as pre-existing-dominated plus one named
  environment condition, does not block acceptance of the changed behavior.** Evidence: 193 failed / 6323
  passed / 5 skipped over 9 failing files; pass-2 A/B on the windows set gives 7 `AB_CONFIRMED_PRE_EXISTING`,
  2 candidate-caused (both since fixed and in `pass2HadButPass3DoesNot`), 2 `MIXED_UNATTRIBUTED`
  (`App.test.tsx`, `App.remote.test.tsx`; `App.remote.test.tsx` no longer fails at pass 3). **Named environment
  condition:** the `EACCES` on `/tmp/ferryx-ext-test.sock` in `agentStateExtension.test.ts`.

**None of G2–G4 is green, and none of these findings converts the plan's "full final gates green" criterion to
satisfied.** What they establish is narrower and precise: no *candidate-caused* UI failure remains after
`7c0fed30`, on any host.

---

## 3. G6–G9 — the full `--lib` gate

### 3.1 G6 — linux full `--lib`: exit 101, 2646 / 31 / 6

**Command and revision.** `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` on the
linux host, at candidate **`c34b90fc`** (`task-9/compile/PHASE1-COMPILE-VERDICT.md`; ledger
`…completion-state.md:101` and `:2201`). The run is stored raw:
`task-9/compile/linux/logs/09-full-lib.log:3785` and its extracted failure list
`task-9/compile/linux/logs/09-full-lib-failures.txt`.

**Raw result, verbatim:** `test result: FAILED. 2646 passed; 31 failed; 6 ignored; 0 measured; 0 filtered out;
finished in 457.89s`, raw exit **101**, 2683 selected
(`task-9/compile/PHASE1-COMPILE-VERDICT.md` gate table; `PHASE1B-REGRESSION.md` row
"`--lib -- --test-threads=1` | **101** | 2683 | `FAILED. 2646 passed; 31 failed; 6 ignored` (457.89 s)").

**A/B classification, verified as a set in this session** (not taken from a summary):

| Component | Count | Evidence |
| --- | --- | --- |
| pre-existing | **30** | identical to the `70eefafe` still-failing set — see the set comparison below |
| candidate-caused | **0** | ledger `:2201` ("**Zero new candidate-caused failures**"); `PHASE1B-REGRESSION.md` |
| flaky joiner | **1** | `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked` |
| unclassified | **0** | the two sets differ by exactly the flaky joiner |

Set arithmetic, recomputed from the raw artifacts in this session:
`09-full-lib-failures.txt` contains 32 `FAIL | test <name> … FAILED` lines covering **31 unique** test names
(one name, `daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty`, is reported
twice, the second time with its own `0 passed; 1 failed; 2682 filtered out` run line), plus the summary line.
Comparing those 31 names against the 30 names listed in `task-8/pass3/linux/DELTA-70eefafe.md` §"Still failing
(30)": **every one of the 30 is present** and the **only** name not in that set is the flaky joiner. Zero
unclassified, exactly as the ledger states.

**Base evidence for the 30.** They are the `abd9e890` classification's pre-existing set, each decided by a
base-`d82b35e4` A/B on the same host with the same command (`task-8/pass3/linux/CLASSIFICATION-linux.md`:
candidate `2629/37/6` vs base `2554/31/6`, "pre-existing 30 … Every entry is decided by the base A/B on the
same host with the same command — no entry is inferred"), and they are unchanged through
`39e722ce → 70eefafe` (`task-8/pass3/linux/DELTA-39e722ce.md`, `DELTA-70eefafe.md`).

**The flaky joiner, separately identified.** `task-9/compile/linux/logs/13-flaky-rep-{1,2,3}.log` (isolated
repetitions): rep 1 `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2682 filtered out;
finished in 4.22s`; rep 2 `ok. 1 passed; 0 failed; … 0.54s`; rep 3 `ok. 1 passed; 0 failed; … 0.55s` — i.e.
**1 of 3**, panicking in `tests/support/machine_input_cancellation.rs:31`. Ledger `:101` and `:2201` record the
same fact ("the joiner is the flake Task 8 already recorded (reproduced: 1 of 3 reps …); neither QA commit
touches `src/remote/`").

**Cross-revision context** (why "all 30 residual" and "31 failed" are both correct): at `70eefafe` the same
command gave `2647 passed; 30 failed; 6 ignored` with "all 30 residual failures pre-existing with A/B evidence,
**zero unclassified**" (ledger `:2578`; `task-8/pass3/FINAL-VERDICT-70eefafe.md`); the delta `39e722ce →
70eefafe` was "left 1 / joined 0" (`task-8/pass3/linux/DELTA-70eefafe.md`). At `c34b90fc` the same 30 fail plus
the flaky joiner = 31. Cumulative trajectory recorded in the same verdict file: `abd9e890` 2629/37/6 →
`39e722ce` 2638/31/6 → `70eefafe` 2647/30/6, candidate-caused remaining 6 → 1 flaky → **0**.

**Explicit final finding — G6.** *Accepted as pre-existing with A/B evidence and zero unclassified, plus one
separately identified flaky test that is not candidate-caused; it does not block acceptance of the changed
behavior, but it does not discharge the literal "full final gates green" criterion.* Named residual risk: the
gate is RED and will stay RED on this host until the 30 pre-existing failures are repaired by their own owners
(a missing `ferryx-remote-helper`/`ferryx-cli` binary, fixture timeouts, `\\?\` path handling and similar
environment-shaped families); the flaky joiner (`input_is_cancelled_when_grant_is_revoked`, 1 of 3) is
nondeterministic and would need a dedicated A/B on a quiet host before it could be called anything but flaky.

### 3.2 G7 — mac full `--lib`: NOT RUN, and not a pass

**Why it is not run.** The mac full `--lib` has **no verdict at any candidate revision**. The best log,
`task-8/pass3/mac/logs/I-full-lib-HUNG-at-p11-reaper.log`, is 2 285 lines / 168 657 bytes with **no
full-suite `test result:` line** (its nine `test result:` lines all belong to trailing single-test sections —
see the table below); its last two `test` lines are
`test paired_host::client::tests::gatefix_journal_error_uses_the_same_safe_projection ... ok` followed by
`test paired_host::client::tests::gatefix_p06_relay_ticket_failure_surfaces_typed_error_without_ws_retry ...`
— an unterminated line, i.e. the run was killed mid-test. The `2693` selected total comes from the compile
gate in the same revision's gate log (`task-8/pass3/mac/rustH-gates.PARTIAL-DISK.log`:
`GATE mac lib-compile native=0 selected=2693 errors=0`), and `running 2693 tests` appears at
`I-full-lib-HUNG-at-p11-reaper.log:726`.

**On the `1079 / 2693` reading — the raw artifact disagrees with the summary, and the raw artifact wins.**
The ledger (`:2584`, `:2637`, `:2729`), `task-8/pass3/report.md:677` and
`FINAL-VERDICT-39e722ce.md:242` all state "hung at 1079/2693 on `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution`".
The raw logs do not show that test hanging on the candidate: in `I-full-lib-HUNG-at-p11-reaper.log:1890` it
reads `test ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution ... ok`, and
`FINAL-VERDICT-70eefafe.md` itself retracts the claim ("the candidate run did **not** hang on
`ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution` — it **passed** it … and the log was
still advancing … when I killed it"). **What each side actually shows, per raw log:**

| Log | Full-suite section | Where it ended |
| --- | --- | --- |
| candidate `I-full-lib-HUNG-at-p11-reaper.log` | `running 2693 tests` (`:726`); the p11 reaper test `... ok` (`:1890`) | killed at an unterminated `paired_host::client::tests::gatefix_p06_relay_ticket_failure_surfaces_typed_error_without_ws_retry ...` line (last line) |
| base `full-lib-base-HUNG.log` | `running 2609 tests` (`:1104`) | killed at `test ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution ... error: test failed, to rerun pass --lib` (`:2224`) — the p11 reaper test, no verdict printed |
| base `full-lib-base-HUNG2.log` | `running 2608 tests` (`:652`) | second base attempt **skipped** that test and was killed later, at `remote::machine_operation_journal::contention_tests::journal_mutation_deadline_retains_admission_until_worker_drains ... error:` (`:2447` carries the SIGTERM/`--skip test_p11_…` message) |

**Therefore:** the *base* side does show a kill at the p11 reaper test in one of its two attempts, but the
candidate **passed** that test — so the phrase "stalls at the SAME test as base" is **not established from the
raw artifacts**; what is established is that both sides were killed before finishing, at different points.
**Not established:** the exact test at which the candidate mac run was killed and the number of tests it had
completed — **not recorded** in any stored artifact; it should have come from the killed run's own progress
counter, which no log preserves (the logs carry `test ... ok` lines only, no periodic progress line). The
`1079` figure appears **only** in summary prose, never in a raw log.

**What *is* established from raw artifacts: no killed mac full-suite run ever printed a full-suite verdict.**
Each of the three killed logs contains nine `test result:` lines, but every one of them belongs to a trailing
**single-test** section appended by the same gate script (`running 1 test` with `2692`/`2608` filtered out), not
to the full-suite section (`running 2693`/`2609`/`2608` tests) — so the full suite has **no** verdict in any of
them. The two *completed* mac runs did print one: `full-lib-pass3.log:3909`
`test result: FAILED. 2635 passed; 49 failed; 6 ignored; 0 measured; 0 filtered out; finished in 761.06s`
(candidate `abd9e890`) and `full-lib-base.log` `2557 passed; 44 failed; 6 ignored; … 665.66s` (base) — those two
are the `abd9e890` classification runs, whose candidate-caused set (6) and pre-existing set (43) are recorded
with per-test base A/B in `task-8/pass3/mac/CLASSIFICATION-mac.md`. Note the selected totals differ by revision
(base 2609/2608, candidate 2690/2693), which is the candidate's own added tests, not a filtering change.

**Explicit final finding — G7.** *Structurally not run: **not a pass**, and it must never be reported as green.*
It blocks nothing that was actually measured, but it leaves the mac half of the plan's "Mac/Windows compile and
affected tests mandatory" and the mac evidence for IS-1/IS-2/IS-3 **unmeasured at the candidate revision** — an
acceptance gap, disclosed as a gap. Named residual risk: the mac host's condition at the time (load 31.8–33.8 on
12 cores, 1.6 GiB free, then ssh-unreachable — `task-8/pass3/FINAL-VERDICT-70eefafe.md` "Host health for Task 9")
is an **environment** cause; the ledger's "structural reason, not disk" (`:2637`) is a claim about *where it
stalls*, and that specific claim is, per §3.2 above, **not established from the raw artifacts**. Closing this
gate needs a quiet mac host and a window long enough for the full suite at roughly 47× the linux runtime
(`pane_liveness_` 62.96 s mac vs 1.35 s linux, same verdict file).

### 3.3 G8 — windows full `--lib`: NOT RUN (left in flight)

**Command and revision.** `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` on
maho-win at **`c34b90fc`**; log `…\task9-c34b90fc\win-06-full-lib.log`, left in place by dispatch decision
(ledger `:2202`, "LEFT IN FLIGHT BY DISPATCH DECISION, not a product signal, do not wait on it"; ledger `:44`
and `:2140` record the frozen reading, 397 866 B / 1796 test lines, byte-frozen since 23:28:59 — a 31-minute
freeze — stalled on
`ssh::bridge_live_loopback_openssh_connection` — the same test Task 8 recorded hanging on that host).

**Raw result: not recorded** — the run never printed a `test result:` line, so it has **no verdict**. The last
full windows `--lib` with a verdict is at **`abd9e890`**: `2201 passed; 70 failed; 8 ignored`
(`task-8/pass3/windows/CLASSIFICATION-windows.md`), classified **5 candidate-caused / 65 pre-existing /
0 unclassified** under a strict rule ("`pre-existing` requires an explicit `test NAME ... FAILED` line in the
base log; a test that merely *started* at base is not credited either way"), with the base full run bounded at
2174 of 2280 tests. One row of that set is the H-27 test — see `H-27-INTERRUPTED-SPAWN-FINDING.md`.

**Explicit final finding — G8.** *Not run: no verdict exists at the candidate revision; the last measured
windows full `--lib` is one revision earlier and is dominated by pre-existing failures (65 of 70) whose
families are named in `CLASSIFICATION-windows.md` (missing `ferryx-remote-helper`/`ferryx-cli` binaries, Windows
long-path `\\?\` handling, fixture timeouts), with one unresolved row carried to H-27.* It does not block
acceptance of the changed behavior, and it does not discharge the criterion.

### 3.4 G9 — `qa_barrier` under `--features local-split-qa`: RED at `c34b90fc`, repaired in `314251e0`, **re-verified green by execution**

**Command and revision.** `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1`,
on linux at **`c34b90fc`** — the third of the plan's verbatim Rust gate commands.
**Raw result at `c34b90fc`: NOT_RUN_BLOCKED** — the feature combination did not compile (linux exit **101**,
18 error blocks / 16 unique sites; windows exit 101 with **no binary produced**), so **zero tests were selected and
no verdict exists**. Recorded in `task-9/compile/PHASE1-COMPILE-VERDICT.md` and in the ledger's gate table
(`…completion-state.md:2258`, row "`--lib --features local-split-qa qa_barrier` | linux | — |
**NOT_RUN_BLOCKED — a previously-green gate (11/0 at `70eefafe`) is now uncompilable**").
**A/B classification: candidate-caused, proven** — reverting the 17 modified files to `70eefafe` and deleting
the two new modules makes the same command exit **0** (6m13s); restoring `c34b90fc` returns it to **101**
(`PHASE1-COMPILE-VERDICT.md` §2; ledger `:2246` heading and `:2249` "reverting the 17 modified files").

**Latest measured state: GREEN, established twice by execution after the repair.**

| Measurement | Revision | Result | Citation |
| --- | --- | --- | --- |
| pass-2 verifier | **`314251e0`** (the repair) | all compile gates PASS; `qa_barrier` **19/19**, `qa_producers` 10/10, `qa_liveness` 13/13, `qa_split_producers` 14/15; **57 selected** | ledger `:2088`–`:2096` (heading "PASS-2 VERDICT (verifier `st_01a10761`): Phase 1 GREEN, Phase 2 blocked by the fixture gap"), restated at `:2157`–`:2158` |
| pass-3 verifier | `1df40271` | `qa_barrier` 101 / **23 selected** / 1 failing test — a **new** candidate-caused, test-only regression (`qa_split_producers::tests::retry_and_batch_controls_are_read_only_for_this_run`, `ipc/terminal.rs:3876`), A/B-proven to that one file | ledger `:1994` (verdict heading), `:2004` (101 / 23 selected), `:2010` (single-file revert → 0), `:2134` (the failing test named) |
| pass-4 verifier | **`91d447e1`** (after that regression was repaired) | "Four QA selectors \| **61 selected, all pass**"; raw `task-9/compile-pass4/linux/logs/05-run-qa_barrier.log` = `test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 2730 filtered out; finished in 0.38s`, with `qa_producers` 10/10, `qa_liveness` 13/13, `qa_split_producers` 15/15 in the sibling logs and `04-list-*.log` listing 23/10/13/15 | ledger `:1909`–`:1918`; raw logs under `task-9/compile-pass4/linux/logs/` |

**The Rust surface has not changed since the pass-4 measurement**: `git diff --name-only 91d447e1 89a363a0 -- src-tauri`
returns nothing (this session), and the ledger records the same for the earlier step (`:1837`,
"`git diff --name-only 91d447e1 15042b0f` → 5 scripts, 0 Rust; `qa_barrier.rs` blob identical"). The pass-4 verdict
therefore still describes the current candidate's Rust bytes.

**Why v1 of this artifact got it wrong — the supersession trap.** v1 cited `…completion-state.md`'s
*"Still owed: re-verification of `314251e0` — the four QA selectors with `--list` (48 gated tests)"* (now at
`:3268`, and restated in the correction section at `:367`). That line is a **forward-looking dispatch obligation
written before those runs landed**, not a result. **The ledger is not in chronological order** — sections are
inserted at different points — so a later line number does not mean a later state. **Cite the measured result,
not the obligation that preceded it.**

**Explicit final finding — G9.** *Candidate-caused RED at `c34b90fc`, repaired in `314251e0`, and **re-verified
green by execution**: `qa_barrier` 19/19 at the repaired revision (pass-2 verifier), then all four QA selectors
green again in pass 4 at `91d447e1` (61 selected; `qa_barrier` 23/23). **Accepted — not an open obligation, and
not a blocking RED.*** Context retained: the gate had been **11/0 at `70eefafe`** before the QA producer surface
landed, so this was a regression of a previously-green gate and the A/B proof of its cause stands. Named residual
risk: the QA-feature *scenario* path (the nine native scenarios on a QA binary) remains **separately unresolved** —
its verdicts live in Task 9's passes (pass 4: "Windows scenarios | FAIL exit 4 `NATIVE_AUTOMATION_UNSUPPORTED`",
ledger `:1919`; later passes reached the scenario's own assertion and reported `SPLIT_RIGHT_NOT_FOUND`, `:528`–`:578`),
**none of them green** — that is Task 9's open work, not this gate's, and no verdict here should be read as covering it.

---

## 4. G11 — zero-selected targets on Windows are NOT a pass

Four integration targets carry `#![cfg(unix)]`, so on Windows they compile to nothing: the plan's own
`--test daemon_handover_transfer_contract` and `--test ipc_hardening_contract` therefore select **0** tests
there. Recorded in `FINAL-VERDICT-70eefafe.md` ("`daemon_handover_transfer_contract` / `ipc_hardening_contract`
| 0 | **NOT a pass** — targets are `#![cfg(unix)]`, they compile to nothing on Windows |
structurally-not-applicable") and in the ledger (`:233`, `:239`, "0 selected on Windows for the four unix
integration targets is NOT a pass"; the ledger also notes `zero_config_gen4_audit.rs` and
`zero_config_gen5_regression.rs` are **not** unix-gated).

**Explicit final finding — G11.** *Zero selected is not a pass and is not counted as Windows coverage.* The
Windows column of these gates is **not run**, so any statement that the candidate passes
`daemon_handover_transfer_contract` "on all platforms" would be false on its face; the mac+linux runs (5/0)
are the only measurements of it.

---

## 5. Per-gate one-line dispositions (the record H-24 asked for)

| Gate | Host | Revision | Raw | Disposition |
| --- | --- | --- | --- | --- |
| `bun run --cwd ui test` | mac | `21dea3c0` | exit 1; 192F/6329P (6521); 7 files | accepted pre-existing-dominated; does not block changed behavior; residuals: no green full-UI run ever, `7c0fed30` re-verified on one file only, `App.test.tsx` MIXED not fully attributed |
| `bun run --cwd ui test` | linux | `21dea3c0` | exit 1; 1224F/5297P (6521); 99 files | accepted pre-existing-dominated (97/99 A/B-confirmed, 1 fixed candidate-caused, 1 MIXED); same residuals |
| `bun run --cwd ui test` | windows | `21dea3c0` | exit 1; 193F/6323P/5skip (6521); 9 files | accepted pre-existing-dominated + named env condition (`EACCES /tmp/ferryx-ext-test.sock`); 2 MIXED not fully attributed |
| `cargo test --lib -- --test-threads=1` | linux | `c34b90fc` | exit 101; 2646/31/6 | accepted: 30 pre-existing (A/B, set-verified) + 1 separately identified flake; **zero unclassified**; does not discharge "gates green" |
| `cargo test --lib -- --test-threads=1` | mac | `70eefafe` / `39e722ce` | **no full-suite `test result:` line in any killed run** | **NOT RUN — not a pass**; acceptance gap, environment-caused; the "same test stalls at base" claim is not established from raw logs (§3.2) |
| `cargo test --lib -- --test-threads=1` | windows | `c34b90fc` | **no verdict** (left in flight) | **NOT RUN — not a pass**; last verdict is `abd9e890` 2201/70/8 with 65 pre-existing |
| `cargo test --lib --features local-split-qa qa_barrier` | linux | `c34b90fc` | exit 101 (compile) | **candidate-caused RED, repaired in `314251e0`, re-verified green** (`qa_barrier` 19/19; pass 4: all four selectors 61/61) — **accepted**, not an open obligation |
| 4 × `#![cfg(unix)]` integration targets | windows | — | 0 selected | **structurally not run; not a pass** |

### 5.1 Supersession sweep — every non-green gate checked for a later measured result

Run because one disposition (G9) had gone stale. For each gate: searched the ledger and the evidence tree for a
**later measurement that supersedes** the state recorded in §§2–4. **The evidence tree holds no full-`--lib` log
and no full-UI log later than those cited there** (`find` over `E/task-8/pass2`, `E/task-8/pass3` and `E/task-9`:
the newest full-UI logs are pass-3, mtimes 2026-10-04 06:37 / 08:31 / 08:49; the only full-`--lib` logs are the
pass-3 set plus `task-9/compile/linux/logs/09-full-lib.log`, mtime 2026-10-04), and no file in the tree carries a
later linux full-`--lib` summary.

| Gate | State in this artifact | Later result? | Latest result, at revision | Citation |
| --- | --- | --- | --- | --- |
| UI mac | RED 192F/6329P | **no later run — genuinely owed** | pass-3, at `21dea3c0` | `task-8/pass3/mac/logs/full-ui.log`; ledger `:2944` |
| UI linux | RED 1224F/5297P | **no later run — genuinely owed** | pass-3, at `21dea3c0` | `task-8/pass3/linux/logs/full-ui.log`; ledger `:2944` |
| UI windows | RED 193F/6323P/5skip | **no later run — genuinely owed** | pass-3, at `21dea3c0` | `task-8/pass3/windows/logs/full-ui.log`; ledger `:2944` |
| linux full `--lib` | exit 101; 2646/31/6 | **no later run exists** — the figure recorded here **is** the latest, and the ledger's own summary repeats it ("Linux 전체 `--lib` \| 2646/31/6 — 31건 전부 기존결함, 미분류 0") | `c34b90fc` | `task-9/compile/linux/logs/09-full-lib.log:3785`; ledger `:2264` (c34b90fc row) and `:2641`/`:2647` (the `70eefafe` row, 2647/30/6) |
| mac full `--lib` | NOT RUN (no full-suite verdict) | **no later result exists at any revision** | none | ledger `:2647`, `:2700`, `:2792` |
| windows full `--lib` | NOT RUN (left in flight) | **no later result; and the state itself was reclassified** from "in flight" to a **HANG** (log byte-frozen at 397 866 B for 31 minutes, on the same `ssh::bridge` test Task 8 recorded hanging) | last verdict `abd9e890` 2201/70/8 | ledger `:42`–`:46`, `:2265`, `:2202`–`:2204` |
| `qa_barrier` (QA features) | "repaired, not yet re-verified" | **CHANGED — re-verified green twice** | 19/19 at `314251e0`; 61/61 across all four selectors at `91d447e1` | §3.4; ledger `:2088`–`:2096`, `:1909`–`:1918` |
| 4 × `#![cfg(unix)]` targets on Windows | 0 selected | **no later run** | not a pass | ledger `:234` |
| runner vitest (QA adapter suite — adjacent to the plan's gates, not one of its four verbatim Rust gates) | 28/28 at `21dea3c0` (quoted in §2's context) | **no later measurement of the current count** — the expected total was raised to 86 (`:483`) and then 87 at `89a363a0` (`:411`) with **no execution recorded since**; the latest *measured* run is **64/64, exit 0** at `757f8414` | `757f8414` | ledger `:1368` (64/64), `:483` (86), `:411` (87) |

**Result of the sweep:** one disposition changed (**G9**, corrected in §3.4); one state was refined rather than
changed (windows full `--lib`: still not run, and now positively a hang rather than a slow run); every other
non-green gate is confirmed as **genuinely owed** with the citation above, not merely assumed so.

**Concurrency caveat.** The ledger is shared and was being edited while this sweep ran (3 134 → 3 271 lines
during this turn; an H-24 correction section was inserted at `:356`–`:379`). Every line number above was
re-derived from the ledger text at that moment. If a lane rewrites the ledger again, **re-derive the citation
rather than reusing the number**.

**Nothing in this document is unclassified-by-omission.** Two items remain explicitly unclosed and are named
above rather than folded in: the `MIXED_*` UI files (per-failure attribution incomplete) and the mac full
`--lib` test-at-kill reading (`1079/2693` appears only in summary prose; **not recorded** in any raw log).

---

## 6. What was and was not done here

- **Read-only.** No file outside this artifact was created or modified. `src-tauri/**`, `ui/**`, `scripts/**`
  and the plan were not touched.
- **Nothing was compiled, tested, launched or connected.** No cargo, no bun/vitest, no LSP, no remote host, no
  GUI, no daemon.
- **Every number above** was read from a stored artifact or log in this session; the two derived figures
  (per-host `FAIL <file>` counts, and the 31-vs-30 set comparison) were recomputed from those raw files and are
  labelled as recomputed. Where the ledger/evidence prose and a raw log disagree (§3.2), the disagreement is
  stated and the raw artifact is preferred.
