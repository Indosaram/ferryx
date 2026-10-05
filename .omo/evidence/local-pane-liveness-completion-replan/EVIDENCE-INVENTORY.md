# EVIDENCE INVENTORY — nine "missing evidence directory" holes (H-1, H-2, H-3, H-4, H-5, H-8, H-9, H-10, H-11)

**Produced:** 2026-10-04, read-only inspection session. **Nothing was compiled, built, or tested** —
no `cargo`, no `bun`/`vitest`, no LSP, no remote host command, no GUI action.
**Written:** exactly one new file, this document. No prescribed path was created, copied, moved,
symlinked, or fabricated.

## Authority and scope

| Item | Value |
|---|---|
| Plan (authority) | `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md` (42,235 bytes) |
| Audit (authority) | `C/.omo/evidence/local-pane-liveness-completion-replan/F1-PLAN-TO-ARTIFACT-AUDIT.md` (164,083 bytes, mtime 2026-10-04 21:33:04) |
| Consolidated hole map | `/Volumes/T9-Mac/project/ferryx/.omo/ulw-execute/local-pane-liveness-remaining-work.md` |
| Execution ledger | `/Volumes/T9-Mac/project/ferryx/.omo/ulw-execute/local-pane-liveness-completion-state.md` (272,002 bytes) |
| Candidate worktree `C` | `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation` |
| Branch / HEAD (verified) | `work/local-pane-liveness-completion-foundation` / **`14744c8a`** — matches the brief |
| Evidence root `E` | `C/.omo/evidence/local-pane-liveness-completion-replan/` |

The audit's verdicts were bound to `70eefafe`. Two of the audit's HEAD-independent claims are
re-checked below against the disk as it stands at `14744c8a`; where the disk has moved, this
document says so instead of repeating the audit.

Two paths named in the task brief do **not** exist as written, and the real ones were used instead:

- The plan is at `ferryx/.omo/plans/…`, **not** at `C/.omo/plans/…` (C has a `.omo/plans/`
  directory, but it is empty of this plan).
- The execution ledger is at `ferryx/.omo/ulw-execute/local-pane-liveness-completion-state.md`,
  **not** at `C/.omo/ulw-execute/…` (C has no `.omo/ulw-execute/` directory at all).

### Method (read-only)

Every existence claim below comes from a real filesystem check run in this session
(`ls`, `find`, `stat`, `grep`, `python3 -c` reading JSON). The checks were:

```
cd E && for p in hosts.json task-1/logs task-2/logs task-2/manifest.json task-3 task-4 \
  task-5 task-6 task-7 task-9/SCENARIO task-10 final; do [ -e "$p" ] && echo "EXISTS $p" || echo "ABSENT $p"; done
find C R -type f \( -name 'request-traces*' -o -name 'per-side-reader-receipts*' \
  -o -name 'state-traces*' -o -name 'headless-result*' -o -name 'hosts.json' \)   # node_modules/.git/target/dist pruned
find E/task-9 -type d -name 'run-*' | wc -l          # → 41
find E -iname '*screenshot*' -o -iname '*.bmp' -o -iname '*.webp'   # → nothing
```

## Result of the existence check (verbatim)

```
ABSENT   hosts.json
ABSENT   task-1/logs
ABSENT   task-2/logs
ABSENT   task-2/manifest.json
ABSENT   task-3
ABSENT   task-4
ABSENT   task-5
ABSENT   task-6
ABSENT   task-7
ABSENT   task-9/SCENARIO
ABSENT   task-10
ABSENT   final
```

`E/` today contains: `F1-PLAN-TO-ARTIFACT-AUDIT.md`, `H-18-…`, `H-20-…`, `H-25-…`, `H-26-…`,
and the directories `task-1/`, `task-2/`, `task-8/`, `task-9/`. **Every one of the nine prescribed
paths is still absent.** No hole has closed itself by the passage of time. What has changed is
`task-9/`, which now holds 41 completed scenario run directories — see H-10.

---

## H-1 — `E/task-3/{logs,request-traces,cleanup.json}`

**Prescribed (plan, Todo 3 QA happy):** "`E/task-3/{logs,request-traces,cleanup.json}`."

**Exists now?** No. `E/task-3/` does not exist. Repo-wide search for `request-traces*` found
nothing anywhere under `C` or `ferryx`.

**Where the equivalent evidence lives.** The reliable-local-split work (Todo 3's code) was verified
by the Task 8 convergence gate, not by a Task 3 evidence lane. Verified paths:

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `logs` | `E/task-8/pass3/linux/logs/local_split_reliability_.log` (+ `local_split_reliability_-list.log`) | `test result: ok. 15 passed; 0 failed` — the exact gate command from the plan's Verification strategy | Todo 3 QA happy (the named `local_split_reliability_` filter) |
| `logs` | `E/task-8/pass3/mac/logs/local_split_reliability_.log`, `E/task-8/pass3/windows/logs/local_split_reliability_.log` | Same filter, other two hosts (windows: 14 passed, per `FINAL-VERDICT.md`) | Cross-host coverage |
| `logs` | `E/task-8/pass3/linux/rust-commands.jsonl` | Structured per-gate record: exact `argv`, `cwd`, `pid`, `rawNativeExit`, `wrapperExit`, `selectedCount`, `assertedLines` | Todo 3 command provenance |
| `logs` | `E/task-8/pass3/FINAL-VERDICT.md`, `FINAL-VERDICT-70eefafe.md`, `FINAL-VERDICT-39e722ce.md` | Per-host gate tables mapping command → exit code → asserted line | Todo 3 + Todo 8 acceptance |
| `logs` | `E/task-8/pass3/linux/logs/{split.log,split-list.log,pane_liveness_.log}`, `…/ab-*.log`, `MUTATION-journal.log` | Split/QA filter runs, A/B baseline-vs-candidate, and the one mutation run | Todo 3 acceptance + QA failure |
| `cleanup.json` | `E/task-8/cleanup.json`, `E/task-8/cleanup-unix.sh`, `E/task-8/cleanup-windows.ps1`, `E/task-8/pass3/CLEANUP-RECEIPTS.md`, `E/task-8/pass3/{linux,mac}/DISK-RECLAIM-RECEIPT.md` | Teardown of owned staging/runtime resources and foreign-root preservation | Todo 3 "Cleanup: owned staging/runtime only" |
| `request-traces` | **no artifact of this kind exists anywhere** | — | — |

**`request-traces` genuinely does not exist.** Nothing in the evidence root records per-IPC-request
traces (the `cmd_terminal_spawn_operation` / create-status-cancel request/response sequence).
The nearest artifacts are the per-gate command records in `rust-commands.jsonl`, which trace
*commands*, not *IPC requests*. Note that this is the same gap the audit records separately as
**H-12** ("`cmd_terminal_spawn_operation` never exercised with a frontend-shaped request") — the
missing request traces and the unexercised command are the same underlying absence.

**Verdict.** `logs` and `cleanup.json`: **closable by organisation** — the artifacts exist and are
the same runs' outputs; what is missing is the plan's per-task location, so the honest closure is a
documented mapping (as in the table above), or relocating those specific files. `request-traces`:
**needs the task re-run** — no artifact of that kind was ever produced, and no amount of moving can
create one.

---

## H-2 — `E/task-4/{logs,per-side-reader-receipts,cleanup.json}`

**Prescribed (plan, Todo 4 QA happy):** "`E/task-4/{logs,per-side-reader-receipts,cleanup.json}`;
actual native retained/abort/suspension scenarios in9."

**Exists now?** No. `E/task-4/` does not exist. Repo-wide search for `per-side-reader-receipts*`
found nothing.

**Where the equivalent evidence lives.** Todo 4's code (retained identity, suspension authority,
handover reader relinquishment) was verified by the Task 8 gate:

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `logs` | `E/task-8/pass3/linux/logs/daemon_handover_contract.log` | `test result: ok. 5 passed; 0 failed` (13.43s) | Todo 4 QA happy (Unix transfer contract) |
| `logs` | `E/task-8/pass3/linux/logs/target-daemon_handover_transfer_contract.log`, `transfer.log`, `transfer-list.log` | The named integration test `daemon_handover_transfer_contract` | Todo 4 acceptance (incarnation survives epoch change) |
| `logs` | `E/task-8/pass3/linux/logs/unix-suspension.log`, `unix-suspension-list.log` | Suspension-attribution filter run | Todo 4 suspension clause |
| `logs` | `E/task-8/pass3/PAIRED-ATTACH-FENCE-PROBE-STATUS.md`, `REPAIR-VERIFICATION-70eefafe.md`, `REPAIR-VERIFICATION-39e722ce.md` | The handover-succession regression found and repaired (base 5/0 6.3s → candidate 1/4 336s → repaired 5/0 6.21s) | Todo 4 acceptance (no dual readers / no discarded workload) |
| `logs` | `E/task-8/pass3/mac/logs/{transfer.log,transfer-list.log}`, `…/windows/logs/{transfer.log,transfer-list.log}` | Same contract on the other hosts | Cross-host coverage |
| `cleanup.json` | `E/task-8/cleanup.json` + `E/task-8/pass3/CLEANUP-RECEIPTS.md` | Teardown receipts for the same hosts | Todo 4 cleanup |
| `per-side-reader-receipts` | **no artifact of this kind exists** | — | — |

**`per-side-reader-receipts` genuinely does not exist.** The plan wanted a *pair* of receipts — one
per side of a handover (predecessor relinquishing readers, successor acquiring them). What exists
instead is the contract test's pass/fail record and the succession-regression repair document. That
proves the reader-relinquishment *behavior*; it is not the *receipt pair* the plan prescribed. This
is the same class as the audit's **H-13** (QA live-arm binding) — the proof is behavioral, not
receipted.

**Verdict.** `logs` and `cleanup.json`: **closable by organisation**. `per-side-reader-receipts`:
**needs the task re-run** (or an explicit plan amendment accepting the contract-test log as the
substitute — that is a decision for the plan owner, not something this document may assert).

---

## H-3 — `E/task-5/{logs,receipts,cleanup.json}`

**Prescribed (plan, Todo 5 QA happy):** "`E/task-5/{logs,receipts,cleanup.json}`; actual
stale-binding and EOF native proof in9."

**Exists now?** No. `E/task-5/` does not exist.

**Where the equivalent evidence lives.** Todo 5's code (native attach generation fencing, positive
presentation receipts) was verified by the Task 8 gate and is *exercised per-run* by Task 9:

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `logs` | `E/task-8/pass2/linux-base-presentation.log`, `E/task-8/pass2/windows-base-presentation.log` (637 B), `E/task-8/pass2/mac-candidate-app-remote.log` (9,739 B) | Presentation-path observations per host | Todo 5 presentation receipts |
| `logs` | `E/task-8/pass3/windows/logs/H-sh-synchronized_output_bounds_ipc_waits_for_actual_presentation.log`, `…H-sh-deferred_bounds_retry_does_not_restore_obsolete_width.log`, `…H-sh-bounds_ipc_presents_when_browser_child_is_open.log`, `…H-sh-synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end.log` | The four surface-host presentation/bounds cases named in the plan | Todo 5 acceptance (no synthetic native success) |
| `logs` | `E/task-8/pass3/{linux,mac,windows}/surface_host-repair.patch`, `E/task-8/pass3/blocking-e0505-repair-brief.md` | The presentation-path repair and its blocking compiler defect | Todo 5 QA failure path |
| `receipts` | **inside** `E/task-9/win-pass9/runs/split-happy/task-3-harness/split-happy/run-8e075644-…/result.json` (key `nativeEvidence`) and the other 40 `run-*/result.json` files | The per-run five-field native evidence / presentation receipt, embedded in the run result | Todo 5 receipts (per run) |
| `cleanup.json` | `E/task-8/cleanup.json`, `E/task-9/win-pass20/inner-cleanup.json` + each `run-*/cleanup.json` | Teardown receipts | Todo 5 cleanup |

**Note on `receipts`.** The five-field presentation receipt *is* produced — but as a field inside
each Task 9 run's `result.json`, not as a Task 5 `receipts` file. A file literally named
`E/task-5/receipts*` was never written. Verified: `grep -rl 'presentation.receipt' E/` matches
`E/task-9/win-pass9/runs/split-happy/…/result.json` and the instrumented harness sources.

**Verdict.** `logs` and `cleanup.json`: **closable by organisation**. `receipts`: **needs the task
re-run** to produce a Task 5-scoped receipt artifact; the receipt *data* exists but is owned by
Task 9's runs, and relabelling Task 9's output as Task 5's would misattribute provenance.

---

## H-4 — `E/task-6/{logs,state-traces}`

**Prescribed (plan, Todo 6 QA happy):** "`E/task-6/{logs,state-traces}`; native9 verifies actual UI."

**Exists now?** No. `E/task-6/` does not exist. `grep -ril 'state-trace\|state_trace\|stateTrace' E/`
matches **only** the audit document itself — no state-trace artifact exists anywhere in the
evidence root.

**Where the equivalent evidence lives.**

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `logs` | `E/task-8/logs/linux/ui-lifecycle.log`, `ui-split.log`, `ui-build.log`, `full-ui.log` | The frontend lifecycle/split suites the plan names for Todo 6 | Todo 6 QA happy (scoped canonical UI commands) |
| `logs` | `E/task-8/logs/mac/{ui-lifecycle,ui-split,ui-build,full-ui,full-ui-snapshot}.log`, `E/task-8/logs/windows/{ui-lifecycle,ui-split,ui-build,full-ui}.log` | Same suites, other two hosts | Cross-host coverage |
| `logs` | `E/task-8/pass2/{linux,mac,windows}-final-logs/{ui-lifecycle,ui-split,ui-build,full-ui}.log` | The pass-2 equivalents, retained | Todo 6 QA happy |
| `logs` | `E/task-8/pass2/linux-ui-failures.jsonl`, `linux-ui-failed-files.json`, `mac-ui-failed-files.json`, `mac-full-ui-timeout.json`, `mac-partial-suite-coverage.json` | Classified UI failures and a suite timeout, recorded rather than hidden | Todo 6 QA failure path |
| `logs` | `E/task-8/gate-ui-lifecycle.ps1`, `gate-ui-split.ps1`, `gate-ui-build.ps1`, `gate-full-ui.ps1` | The exact gate invocations | Todo 6 command provenance |
| `state-traces` | **no artifact of this kind exists** | — | — |

**`state-traces` genuinely does not exist.** The plan wanted traces of the frontend workspace/restore
*state* transitions (create → persist backend identity → attach → matching presentation, plus the
retry/cancel pending states). No such trace was captured in any pass. What exists is the pass/fail
result of the suites that assert those transitions, plus the classified failure lists. The audit's
**H-15** (no mutation RED evidence for the Task 6 UI failure classes) is the same absence seen from
the mutation side.

**Verdict.** `logs`: **closable by organisation**. `state-traces`: **needs the task re-run** — no
state-trace was ever captured, so there is nothing to relocate.

---

## H-5 — `E/task-7/{logs,headless-result,cleanup.json}`

**Prescribed (plan, Todo 7 QA happy):** "`E/task-7/{logs,headless-result,cleanup.json}`; headless
remains DEFERRED-NATIVE."

**Exists now?** No. `E/task-7/` does not exist. No file matching `headless-result*` exists anywhere
under `C` or `ferryx`.

**Where the equivalent evidence lives.**

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `logs` | `E/task-8/runner.log` and `E/task-8/logs/{linux,mac,windows}/runner.log` | The canonical runner Vitest invocation. **Honest content:** all four record the same failure — `No test files found, exiting with code 1` (`NATIVE_EXIT=1`) on `source-5464da0d`. The runner suite did not pass at that revision. | Todo 7 QA happy (runner command) — recorded as a failure, not a pass |
| `logs` | `E/task-8/runner.mjs`, `E/task-8/pass3/runner3.mjs`, `runner3-rust.mjs`, `runner3-rust2.mjs`, `E/task-8/pass3/run-control.json` | The runner harness and its per-host control record | Todo 7 work product |
| `logs` | `E/task-9/win-pass2/tooling/{run-scenario.bat,win-bat.ps1,win-cleanup.ps1,win-recognizer.ps1,win-scenario6.ps1,win-reaper.ps1,win-manual-probe.ps1}` | The scenario adapters actually used on Windows | Todo 7 "extends actual actions per scenario table" |
| `logs` | `E/task-9/win-pass3/marker-scan-default.txt` (1,182 B) | Marker-scan output | Todo 7 marker recognition |
| `logs` | `E/task-9/win-pass2/WINDOWS-PASS2-VERDICT.md` (23,778 B), `E/task-9/win-pass3/WINDOWS-PASS3-VERDICT.md` (8,267 B), `E/task-9/win/WINDOWS-SCENARIO-VERDICT.md` (12,309 B) | Verdicts of the adapter runs | Todo 7 acceptance |
| `cleanup.json` | `E/task-8/cleanup.json`, `E/task-9/win-pass2/tooling/win-cleanup.ps1`, `E/task-9/win-pass13/cleanup13.ps1`, each `run-*/cleanup.json` | Teardown receipts, including the screenshot-helper temp-directory rule | Todo 7 cleanup |
| `headless-result` | **no artifact of this kind exists** | — | — |

**`headless-result` genuinely does not exist, and the headless run never happened.** Verified:

- No `run-*` directory under `task-9/` has scenario `diagnostic-classifier`; the 41 runs cover only
  `split-happy` (20), `split-attach-stall` (12), `split-cancel` (9).
- The only diagnostic-classifier artifacts are *harness sources*, never a result:
  `E/task-9/win-pass11/diagnostic-classifier.orig.mjs` and `…diagnostic-classifier.session.mjs`.
- `grep -n -i headless E/task-9/REPORT-PASS8.md` and `E/task-8/report.md` return nothing.
- `E/task-9/win-pass2/runs/` contains only the three split scenarios.

This matches the audit's **H-23** ("headless diagnostic-classifier smoke NOT_RUN") and the plan's own
"headless remains DEFERRED-NATIVE". **What produced the equivalent verification instead: nothing.**
The headless lane is dark by design at this point in the run, and no other gate covers it.

**Verdict.** `logs` and `cleanup.json`: **closable by organisation** — with the caveat that the
relocated runner log records a *failure*, and it must be moved as a failure, never presented as a
pass. `headless-result`: **needs the task re-run** — the invocation never executed.

---

## H-8 — `E/task-2/{logs,manifest.json}`

**Prescribed (plan, Todo 2 QA happy):** "`E/task-2/{contracts.md,logs,manifest.json}`; no runtime
resources beyond scoped test fixtures."

**Exists now?** Partially — and unchanged since the audit. `E/task-2/` exists and contains
**exactly one file**: `contracts.md` (11,956 bytes, mtime 2026-10-04 00:28:20). `logs/` and
`manifest.json` are absent.

**Where the equivalent evidence lives.**

| Prescribed | Real path | What it contains | Requirement satisfied |
|---|---|---|---|
| `contracts.md` | `E/task-2/contracts.md` | **Present** — the frozen DTO/contract publication | Todo 2 acceptance |
| `logs` | `E/task-8/pass3/linux/logs/pane_liveness_.log` (lines 726–736 and following) | Individual `test ipc::pane_liveness_contract::tests::… … ok` lines for the contract tests, under the gate whose summary is `test result: ok. 47 passed; 0 failed` | Todo 2 QA happy (`pane_liveness_contract` filter) |
| `logs` | `E/task-8/pass3/linux/rust-commands.jsonl` (line 7) | Structured record for the `pane_liveness_` gate: exact `argv`, `cwd`, `rawNativeExit: 0`, `selectedCount: 47`, `assertedLines` | Todo 2 command provenance |
| `logs` | `E/task-8/pass3/{mac,windows}/logs/pane_liveness_.log` | Same filter on the other hosts | Cross-host coverage |
| `logs` | `E/task-8/pass3/{linux,mac,windows}/logs/qa_barrier.log`, `qa_barrier-list.log` | `test result: ok. 11 passed` under `--features local-split-qa` | Todo 2 capability-negotiation clause |
| `manifest.json` | **no Task 2 manifest exists** | — | — |

**`manifest.json` genuinely does not exist.** There is a Task 1 *source* manifest
(`E/task-1/manifest.json`, `manifest-r2.json`, `manifest-r3.json`) and a Task 8 convergence manifest
(`E/task-8/manifest.json`), but no Task 2 artifact manifest of the contract increment. The closest
provenance record is the per-gate `rust-commands.jsonl` plus `E/task-8/pass3/run-control.json`
(host/revision), which record *what ran*, not *what the task produced*.

**Verdict.** `logs`: **closable by organisation** — the contract test results exist and are
verifiable line-by-line in the Task 8 pass-3 logs. `manifest.json`: **needs the task re-run** to
produce a Task 2-scoped manifest.

---

## H-9 — `E/task-1/logs/` absent (logs are flat)

**Prescribed (plan, Todo 1):** "`E/task-1/{manifest.json,logs,ownership.json}`."

**Exists now?** `E/task-1/logs/` — **absent**. `E/task-1/manifest.json` — **present**.
`E/task-1/ownership.json` — **present**. The logs exist, flat, in `E/task-1/` itself.

**The requirement is satisfied in substance and differs only in layout.** `E/task-1/` contains 22
flat log/exit files, each a real gate output with its native exit code recorded beside it:

```
canonical-runner-vitest.log   canonical-runner-vitest.exit
mac-coordinator-test.log      mac-coordinator-test.exit
rust-diag.log                 rust-diag.exit
rust-diag-r2.log              rust-diag-r2.exit
rust-diag-r3.log              rust-diag-r3.exit
rust-qa-barrier-r3.log        rust-qa-barrier-r3.exit
scoped-vitest.log             scoped-vitest.exit
scoped-vitest-r2.log          scoped-vitest-r2.exit
ui-build.log                  ui-build.exit
ui-build-r2.log               ui-build-r2.exit
task-1-mac-list-r3.log        task-1-mac-list-r3.exit
```

(22 files, verified by `ls -1 *.log *.exit | wc -l`.) The only difference from the plan is that
these sit one directory level higher than the plan's `logs/` wording. No content is missing, and
nothing else in `E/task-1/` competes for the name `logs`.

**Verdict. Closable by organisation — pure layout.** The honest answer is that the deliverable is
present and only its directory nesting deviates. Because the flat paths are already cited by other
artifacts (the audit's own rows, `E/task-8/` cross-references) and another session may read them,
**this document recommends documenting the mapping over reorganising the files**, and therefore
**no files were moved.**

---

## H-10 — `E/task-9/SCENARIO/run-UUID/{actions.jsonl,screenshot.png,result.json,cleanup.json}`

**Prescribed (plan, Todo 9):** "`E/task-9/SCENARIO/run-UUID/{actions.jsonl,screenshot.png,result.json,cleanup.json}`.
No production app/daemon touched."

**Exists now?** `E/task-9/SCENARIO/` — **absent**. But the run shape it describes **does exist**,
under a different tree, and it has grown substantially since the audit (which found "zero scenario
directories, zero action logs, zero screenshots, zero result/cleanup files").

### What actually exists under `task-9/` today

**41 `run-<UUID>/` directories**, all complete. Verified: every one contains all three of
`actions.jsonl`, `result.json`, `cleanup.json` — `find` over all 41 reported **zero** missing files.

| Actual path pattern | Count | Example |
|---|---|---|
| `E/task-9/win-pass<N>/runs/<scenario>/task-3-harness/<scenario>/run-<UUID>/` | 34 | `win-pass2/runs/split-happy/task-3-harness/split-happy/run-0fec03df-0126-4360-a9d2-1b01330ae357/` |
| `E/task-9/win-pass<N>/<variant>/task-3-harness/<scenario>/run-<UUID>/` (no `runs/` level) | 7 | `win-pass11/profileiso-run1/task-3-harness/split-happy/run-96e29e01-7fb5-43ec-9f87-379d5c5a35a9/`, `win-pass10/split-happy/task-3-harness/split-happy/run-8464f56f-9050-42bb-8a66-72bb9f59ba74/` |
| **Total** | **41** | |

Passes carrying run directories: `win-pass2`, `win-pass4`, `win-pass5`, `win-pass6`, `win-pass7`,
`win-pass8`, `win-pass9`, `win-pass10`, `win-pass11`.

**Verdict distribution across the 41 runs** (from each `result.json`): **23 `BLOCKED`, 18 `FAIL`,
0 `PASS`.** Scenario spread: `split-happy` 20, `split-attach-stall` 12, `split-cancel` 9.
Host spread: **all 41 are `win32` (Windows 10.0.26200 x64) — zero Mac runs.**

Later passes changed shape rather than adding run directories. Honest record of the alternatives:

- `E/task-9/win-pass12b/`: `runner-actions-r1.jsonl`, `runner-actions-r2.jsonl`,
  `runner-result-r1.json`, `runner-result-r2.json` (result verdict `BLOCKED`, scenario `split-happy`).
- `E/task-9/win-pass13/`, `win-pass17/`, `win-pass18/`, `win-pass19/`: harness sources and stage
  scripts only — no result, no actions, no cleanup.
- `E/task-9/win-pass20/`: `inner-actions.jsonl`, `inner-result.json` (verdict `BLOCKED`, scenario
  `split-happy`), `inner-cleanup.json`, `inner-verifier-probe.jsonl`, `delegation.json`,
  `inner-app.{stdout,stderr}.log`, `relaunch.out`, `run20.ps1`, `stage20.mjs`, `verifier-probe.mjs`.

### Mapping to the prescribed shape

| Prescribed file | Actual equivalent | Status |
|---|---|---|
| `actions.jsonl` | `<…>/run-<UUID>/actions.jsonl` (41 of them) and `win-pass20/inner-actions.jsonl` | **Exists** — different nesting only |
| `result.json` | `<…>/run-<UUID>/result.json` (41), `win-pass20/inner-result.json`, `win-pass12b/runner-result-r*.json` | **Exists** — different nesting only |
| `cleanup.json` | `<…>/run-<UUID>/cleanup.json` (41) and `win-pass20/inner-cleanup.json` | **Exists** — different nesting only |
| `SCENARIO/` level | The scenario name is a *directory level* in the actual tree (`runs/<scenario>/…`), not a fixed `SCENARIO` parent | **Layout deviation** |
| `screenshot.png` | **Does not exist in any run.** | **Missing — see below** |

### `screenshot.png` — stated plainly

**No screenshot exists in any run so far.** Verified three ways:

- `find E/task-9 -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg'` returns exactly **one** image:
  `E/task-9/win-pass2/captures/recognizer-selftest.png`.
- `find E -iname '*screenshot*' -o -iname '*.bmp' -o -iname '*.webp'` returns **nothing**.
- The single PNG is a *recognizer self-test*, not a capture of an owned window: it lives in a
  `captures/` directory contains only that one file; the sibling `win-pass2/manual-probe/` holds
  `daemon.log` and `fixture-setup.receipt.jsonl`, and no `run-*/result.json` contains the string
  `screenshot` (checked programmatically).

This is why the independent image-reader lane is dark: the plan's recognition requirement ("an
independent visual inspector views the image and writes digest/run/operation/bounds-bound recognition
evidence") cannot be met because **no image was ever produced**. The plan anticipated exactly this
("Task7 provides a capture-ready event and bounded inspection handshake, so recognition is not
required before the image exists") — the handshake exists; the capture never fired.

### Honest statement about scope

- **3 of the 9 scenarios have any run at all**: `split-happy`, `split-attach-stall`, `split-cancel`.
- **The other 6 have no run directory anywhere**: `diagnostic-classifier`, `split-concurrent`,
  `retained-handover`, `handover-abort`, `suspension-ownership`, `stale-binding`. The plan requires
  Mac to run all nine and Windows to run three plus platform suspend/resume contract tests.
- **Zero Mac runs exist.** `maho-mac` is offline (user-deferred), per the remaining-work map.

**Verdict. Mixed, and it must not be reported as anything else.**

- `actions.jsonl`, `result.json`, `cleanup.json` **for the three Windows scenarios that ran**:
  **closable by organisation** — the files exist and are the same runs' outputs. The honest closure
  is a **mapping document** (as above), **not a copy**: the plan's `SCENARIO` level does not exist as
  a directory, so copying 41 run trees into a fabricated `SCENARIO/` hierarchy would invent a
  structure no run produced. **No copy was made.**
- `screenshot.png`: **needs the task re-run** — the capture was never taken.
- The 6 unrun scenarios and the entire Mac matrix: **need the task re-run** — these are the audit's
  H-21 (0 of 12 native scenario executions) and the Mac-deferred group, and they are gated on the
  `maho-win` delegation stall and the `maho-mac` outage, not on evidence organisation.

---

## H-11 — `E/hosts.json` absent at the plan's path

**Prescribed (plan, Verification strategy):** "Task1 reserves fresh absolute paths on each host and
stores them in **E/hosts.json**."

**Exists now?** `E/hosts.json` — **absent**. `E/task-1/hosts.json` — **present**, 7,517 bytes,
mtime **2026-10-03 23:04:37**.

**The first clause of the hole stands; the second clause does not reproduce against the disk.**
This is the one place where the audit's text and the artifact disagree, so it is recorded exactly.

The audit says (H-11, and again at row H1): "the plan's *evidence* and *runtime* host roots are not
recorded in the staging map (only source + ghostty + ui/dist)."

Reading `E/task-1/hosts.json` directly shows `hosts.<host>.remotePaths` records **all four** roots
for **all three** hosts:

```
HOST maho-win  parent C:\Users\sook\ferryx-pane-completion          status FREE
               source   …\source        binary  …\source\src-tauri\target\debug\ferryx.exe
               evidence …\evidence      runtime …\runtime
HOST maho-mac  parent /Users/I552267/ferryx-pane-completion         status FREE
               source   …/source        binary  …/source/src-tauri/target/debug/ferryx
               evidence …/evidence      runtime …/runtime
HOST omaki     parent /home/indo/ferryx-pane-completion             status FREE
               source   …/source        binary  …/source/src-tauri/target/debug/ferryx
               evidence …/evidence      runtime …/runtime
```

The file also carries `nativeGuiPermission`, `signingHostSplit`, per-host `actualIdentity`,
`diskSpace`, `toolchains`, `ghostty`, `signingPackaging` and `exitCaptureValidation`.

**Why this matters for honesty.** The file's mtime (2026-10-03 23:04:37) **predates the audit**
(2026-10-04 21:33:04), and `E/task-1/hosts.json` is not tracked by git (`.omo/evidence` is outside
the index), so the file was not edited between the audit and now. That means the audit's
"evidence/runtime roots unrecorded" clause is **not reproducible against `hosts.json` at either
time**. The audit's own parenthetical — "only source + ghostty + ui/dist" — describes a *narrower
staging echo*, most likely `E/task-1/receipts-r3.json`'s `remoteStaging` block (which the audit cites
at row H1: "`T1/receipts-r3.json` `remoteStaging` echoes both"), not `hosts.json` itself.

This document reports both readings rather than picking one: **the prescribed path is wrong (real
deviation), and the substantive data the audit called unrecorded is in fact present** in
`E/task-1/hosts.json.remotePaths`, verified by direct JSON read.

**Verdict. Closable by organisation.** The content is complete at `E/task-1/hosts.json`; the only
defect is that the plan's wording names `E/hosts.json`. Closure is either (a) an amendment to the
plan's path, or (b) a documented mapping — as here. A copy was **not** made, because the file is
untracked evidence owned by another session's task and duplicating it would create a second,
divergent source of truth for host roots.

---

## Summary table

| Hole | Prescribed path exists? | Equivalent evidence found at | Closable by organisation / needs re-run |
|---|---|---|---|
| **H-1** `E/task-3/{logs,request-traces,cleanup.json}` | No | `logs` → `E/task-8/pass3/{linux,mac,windows}/logs/local_split_reliability_.log` + `rust-commands.jsonl`; `cleanup.json` → `E/task-8/cleanup.json`; `request-traces` → **none** | `logs` + `cleanup.json`: **closable by organisation**. `request-traces`: **needs re-run** (never produced) |
| **H-2** `E/task-4/{logs,per-side-reader-receipts,cleanup.json}` | No | `logs` → `E/task-8/pass3/linux/logs/daemon_handover_contract.log` + `transfer.log` + `unix-suspension.log`; `cleanup.json` → `E/task-8/cleanup.json`; `per-side-reader-receipts` → **none** | `logs` + `cleanup.json`: **closable by organisation**. `per-side-reader-receipts`: **needs re-run** |
| **H-3** `E/task-5/{logs,receipts,cleanup.json}` | No | `logs` → `E/task-8/pass2/{linux,windows}-base-presentation.log`, `E/task-8/pass3/windows/logs/H-sh-*.log`; `receipts` → embedded in `E/task-9/…/run-*/result.json` `nativeEvidence`; `cleanup.json` → `E/task-8/cleanup.json` | `logs` + `cleanup.json`: **closable by organisation**. `receipts`: **needs re-run** for a Task 5-scoped artifact |
| **H-4** `E/task-6/{logs,state-traces}` | No | `logs` → `E/task-8/logs/{linux,mac,windows}/{ui-lifecycle,ui-split,ui-build,full-ui}.log`; `state-traces` → **none** | `logs`: **closable by organisation**. `state-traces`: **needs re-run** |
| **H-5** `E/task-7/{logs,headless-result,cleanup.json}` | No | `logs` → `E/task-8/runner.log` (+ per-host), `E/task-9/win-pass2/tooling/*`, `E/task-9/win-pass{2,3}/WINDOWS-PASS*-VERDICT.md`; `headless-result` → **none**; `cleanup.json` → `E/task-8/cleanup.json` | `logs` + `cleanup.json`: **closable by organisation** (runner log records a *failure*). `headless-result`: **needs re-run** |
| **H-8** `E/task-2/{logs,manifest.json}` | No (`contracts.md` only) | `logs` → `E/task-8/pass3/linux/logs/pane_liveness_.log` lines 726+ (the 11 `pane_liveness_contract_*` tests); `manifest.json` → **none** | `logs`: **closable by organisation**. `manifest.json`: **needs re-run** |
| **H-9** `E/task-1/logs/` | No — logs are flat | `E/task-1/` itself: 22 flat `.log`/`.exit` files (listed above); `manifest.json` + `ownership.json` present | **Closable by organisation** — pure layout; requirement satisfied in substance. Recommend documenting, not moving |
| **H-10** `E/task-9/SCENARIO/run-UUID/{…}` | No (`SCENARIO/` absent) | 41 `run-<UUID>/` dirs under `E/task-9/win-pass{2,4,5,6,7,8,9,10,11}/…` each with `actions.jsonl` + `result.json` + `cleanup.json` (23 BLOCKED / 18 FAIL / 0 PASS, all Windows); `screenshot.png` → **none** (only `win-pass2/captures/recognizer-selftest.png`) | `actions`/`result`/`cleanup` for the 3 Windows scenarios: **closable by organisation** (as a mapping, not a copy). `screenshot.png`, the 6 unrun scenarios, and the Mac matrix: **need re-run** |
| **H-11** `E/hosts.json` | No (real file is `E/task-1/hosts.json`, 7,517 B, mtime 2026-10-03 23:04:37) | `E/task-1/hosts.json.remotePaths` — source, binary, **evidence**, **runtime** for maho-win, maho-mac, omaki; plus `signingHostSplit`, `nativeGuiPermission`, `exitCaptureValidation` | **Closable by organisation** — path deviation only; the audit's "evidence/runtime roots unrecorded" clause is not reproducible against this file |

## Bottom line

- **All nine prescribed paths are absent.** None has appeared since the audit.
- **Four holes are fully closable by organisation**: H-9 (pure layout), H-11 (path deviation only),
  and the `logs`/`cleanup.json` halves of H-1–H-5 and H-8 — those artifacts exist as the same runs'
  outputs and only need a documented mapping (or relocation) into the prescribed location.
- **Five sub-deliverables genuinely need the task re-run**, because no artifact of that kind was ever
  produced and no relocation can create one: `request-traces` (H-1), `per-side-reader-receipts`
  (H-2), Task 5 `receipts` as a Task 5-owned artifact (H-3), `state-traces` (H-4), `headless-result`
  (H-5), and the Task 2 `manifest.json` (H-8).
- **H-10 is the largest real gap.** 41 Windows runs exist and their three data files are complete, so
  the *shape* is nearly satisfied — but **no screenshot was ever captured in any run**, all 41 runs
  are `BLOCKED` or `FAIL` (zero PASS), only 3 of 9 scenarios have run at all, and there are zero Mac
  runs. The image-reader lane is dark for the plain reason that there is no image to read.
- **No evidence was fabricated, copied, moved, or symlinked.** This document is the only file written.

**Nothing was compiled, built, or executed in producing this inventory** — no `cargo`, no `bun` or
`vitest`, no LSP, no remote host command, no GUI or desktop action. Every existence and content claim
above comes from a read-only filesystem check (`ls`, `find`, `stat`, `grep`, `python3 -c` JSON reads)
performed in this session.
