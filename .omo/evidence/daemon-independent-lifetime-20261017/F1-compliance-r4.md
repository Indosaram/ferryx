# F1 Plan compliance audit, round 4: daemon-independent-lifetime-20261017

- **Reviewer.** F1 r4, independent. It is source plus evidence-file review. I built and tested nothing on this Mac.
  - The only remote commands I ran were read-only listings on maho-win (`Get-ChildItem` of the supervisor dir, task XML `<Command>` lines, `Get-ScheduledTask`).
  - The F2 platform clippy, which is recorded in F2-review-r4.md, was the only build.
- **Target.** FINAL_MAIN cfe34b8e (`cfe34b8ead2e9cf4cdd0f590aa1d3c9380b1a17a`).
  - The r3 base was 66f388f3.
  - `git log 66f388f3..cfe34b8e` holds the daemon-lifetime T12 fixes merged at 3f6c1f78, 22a8ecea (hub `cfg(unix)`), and unrelated hub, relay, billing and agent-control work from other sessions.
- **Plan.** /Volumes/T9-Mac/project/ferryx/.omo/plans/daemon-independent-lifetime-20261017.md. Must have / Must NOT at lines 34-58, todo 12 at 223-232, Final wave at 263-276, Success criteria at 282-290.
- **Brief.** brief-final-F1F2F4.md, section 1.

## recommendation
**APPROVE-SOURCE-ONLY-MACOS.**
- Every Must have and Must NOT line still holds at cfe34b8e.
- Every destructive call site is still reachable only from Absent, from an owner-requested handover, or from an explicit user command.
- The new hub code (crates/ferryx-hub, src-tauri/src/hub_runtime.rs) adds no launchctl, systemctl or schtasks call and no daemon Shutdown sender.
- Every PENDING-QA row from r3 is closed for Linux and Windows by the F3 final runs at cfe34b8e (both RUN_EXIT=0).
- macOS stays source-only by the user's waiver ("코드로만 확인", 2026-10-19).
- One open item (O1) remains, a QA-harness residue on maho-win. It is not a product defect and not a Must have/Must NOT violation. It does not meet the literal todo 12 / F3 "no ... dir remains" criterion for the supervisor dir. Details are in section 5.

## blockers
None against a Must have or Must NOT line.

O1 is an open item against the todo 12 / F3 cleanup criterion. I record it as an open item, not a product blocker, for the reasons in section 5:
- it is harness-owned;
- it is inert (no task is registered, and every executable it names is deleted);
- the cleanup is a single, exactly specified harness change.

## originalIntent
Quitting, force-quitting, relaunching or updating Ferryx must never kill running terminals on macOS, Linux or Windows. The daemon runs from its own staged runtime under its own OS job. It is never stopped, unloaded or replaced while its owner is Alive or Unknown. Verification proves the same live PTY and remote processes, not registry IDs.

## desiredOutcome
At the final commit:
- every Must have holds and no Must NOT is violated;
- every destructive call site is reachable only from Absent or an explicit user command;
- the r3 PENDING-QA rows close with passing Linux and Windows runs from the final commit;
- QA leaves nothing behind on the hosts.

## userOutcomeReview
The user gets the outcome they asked for on Linux and Windows. This is measured by running the real app from cfe34b8e on omarchy and maho-win:
- **Quit, kill and OS-attributed termination** (Q01-Q03): same daemon pid and start time, and a fresh nonce echoed exactly once.
- **Stalls and probe faults** (Q04/Q05): zero supervisor mutations.
- **A->B replacement** (Q07): same sessions.
- **Linux A->B->C upgrades** (Q08): local and SSH sessions moved or retained, with counters continuing.
- **Windows protocol mismatch** (Q09): a typed pending state with the exact count; only an explicit, count-confirmed action ends the old owner.
- **Opt-out and headless** (Q10/Q10b).
- **Uncertain writes** (Q11): never replayed.
- **Legacy migration** (Q13): never stops the live legacy owner.

macOS is unproven at runtime by the user's choice. Its rows are source-only.

The only thing the user did not get exactly as specified is the "leave nothing behind" promise on Windows: QA task-definition files accumulate in the user's real `%LOCALAPPDATA%\Ferryx\supervisor` (O1).

## 1. Must have (SOURCE at cfe34b8e; MEASURED where noted)
| Must have | Evidence | Verdict |
| --- | --- | --- |
| Baseline recorded | task-1-baseline.txt (493fae45). The F3 runs record `baselineRev.head` 493fae45 (MEASURED in both summaries). | OK |
| Admission Alive/Unknown/Absent under the admission lock; ordinary start only on Absent | owner_admission.rs:125/133 `classify_owner(_with)`, :812 `admit`, :914 `ensure_if_absent_with`. GUI setup lib.rs:1411 `supervisor::ensure_if_absent()`. Opt-in ipc/agent_integration.rs:100. No change to these files in 66f388f3..cfe34b8e (`git diff --stat` lists none of owner_admission.rs, supervisor.rs, launchd.rs, control_server.rs, handover*.rs). MEASURED: Q04/Q05/Q06 PASS on Linux and Windows. | OK |
| HandoverCandidate is the only start while Alive | supervisor.rs:272 `start_handover_candidate` (unchanged). MEASURED: Linux Q08 "candidate accepted nothing on any inherited listener before commit" and "a probe flock on both lock files failed at every stage" ok. | OK |
| Frozen control channel; protocol not bumped | protocol.rs:15 `= 5`; :18 `= 6` only under `qa-protocol-bump`. The only protocol.rs change in range adds the optional `host_id` to `DaemonRequest::AgentControl` (`serde(default, skip_serializing_if)`), which is additive and data-protocol only. control.rs is unchanged. MEASURED: Q08b "control Hello/Inventory succeeds across revisions" ok on both. | OK |
| SSH/paired never moved; predecessor drains | Unchanged source. MEASURED: Linux Q08 "SSH-owning generation still alive while it serves the SSH session", "SSH remote_pid unchanged", "SSH counter continues through the successor without restart" ok. | OK |
| Signed macOS helper; staging verifies | runtime_stage.rs:53/:345 `SignatureInvalid` (unchanged) | OK (source) |
| Immutable generation-keyed runtime | runtime_stage.rs:107 `generation_key`. MEASURED: Q00 "daemon executable is the staged runtime copy, not the app bundle" ok on both. | OK |
| One OS job per generation; newest autostarts | launchd.rs:462-500; supervisor_linux.rs, supervisor_windows.rs `ensure_with`/`register_for_login_with`. MEASURED: Q00 "daemon pid is the QA job's pid" ok; Q10 "newest generation registered for login before opt-out" ok. | OK |
| Legacy migration never stops Alive/Unknown | supervisor_windows.rs:446-510 (`/Delete` at :478 only for an Absent owner). MEASURED: Q13 "no bootout/stop/End sent to the legacy job", "legacy pid and start time unchanged", "legacy job lost login autostart without being stopped" ok on both. | OK |
| Unix upgrade keeps predecessor until commit | control_server.rs:371-420; pty.rs:1353 `keep_writer_from_sending_eof`. MEASURED: Linux Q08 PASS (184 assertions, 0 failed), including the four transfer-failure variants. | OK |
| Windows: live owner ended only by an explicit user count | control_server.rs:323-324 `UPGRADE_UNSUPPORTED`. update_pending.rs:166 `restart_terminals` is the only control Shutdown sender. The range change in update_pending.rs only adds `log_line` and splits `enter` into `enter_and_decide_log` (134dcc40, 3f6c1f78); no new send. MEASURED: Windows Q09 "A pid unchanged (no Shutdown, no terminate)", "stale count answers COUNT_CHANGED", "confirm with the exact count is accepted and A exits" ok. | OK |
| Live-session inventory | protocol.rs:670 `DescribeLiveSessions`, handover.rs:558 `count_live`. MEASURED: Q12 PASS on both. | OK |
| Regression tests first, then QA Q00-Q13 | **Closed (was PENDING-QA in r3).** MEASURED: F3 Linux 20261022t175629z and Windows 20261022t184142z at cfe34b8e, RUN_EXIT=0, every applicable Q PASS (section 4). | OK |

## 2. Destructive call sites, re-run at cfe34b8e (SOURCE)
Search (non-test files): `git grep -nE '"unload"|"bootout"|"kickstart"|"-k"|"/End"|"/Delete"|"--now"|"restart"|"stop"' cfe34b8e -- src-tauri/src crates`, plus a separate grep for `DaemonRequest::Shutdown|ControlRequest::Shutdown|OP_SHUTDOWN`, `supervisor::(reload|uninstall)\(` and `systemctl|schtasks|launchctl` over `crates` and `hub_runtime.rs`.

| Site | Reachable from | Verdict |
| --- | --- | --- |
| launchctl `unload -w` (launchd.rs:690) | Only `supervisor::uninstall()` (supervisor.rs:306). A `supervisor::(reload\|uninstall)\(` grep over src-tauri/src at cfe34b8e is empty. | Unreachable (N1 carried) |
| `bootout` | No product match | OK |
| `kickstart` (launchd.rs:485, :491) | `ensure_generation_job` returns early unless `*owner == OwnerState::Absent` (launchd.rs:469). Plain kickstart; the test at :872 forbids `-k`. | OK |
| `kickstart -k` | Only the QA self-test patch and forbidden-arg assertions | OK |
| systemctl `disable --now` (supervisor_linux.rs:687) | Only Linux `backend::uninstall` <- `supervisor::uninstall()`, which has no caller. The line moved from :672 to :687 because c3ea7010 added hub env forwarding above it; the body is unchanged. | Unreachable (N1 carried) |
| systemctl stop/restart | No product call. The only `"stop"`/`"restart"` hits are hub chat commands (crates/ferryx-hub/src/chat/*, control.rs:17), agent-control `Stop`, ferryx_scope launcher, and relay routes, which are agent-turn and hub-control verbs, not daemon/job verbs. | OK |
| schtasks `/Delete` (supervisor_windows.rs:478) | `migrate_legacy_task_with`, only with owner Absent and State Ready/Disabled | OK |
| schtasks `/Delete` (supervisor_windows.rs:1071) | Windows `uninstall`, guarded by `owner_absent()` (:1062-1067); no caller | OK |
| schtasks `/End` | Only supervisor_windows.rs:2203, the `#[ignore]` test's QA-task Drop | OK |
| `DaemonRequest::Shutdown` | Server arm server.rs:6649. There is no sender: client.rs:679 is a label map, and lib.rs:2709-2796 pins "quit and window destroy send no Shutdown". | OK (N2 carried) |
| control `Shutdown` | Sent only by update_pending.rs:173 inside `restart_terminals` <- client.rs:3003 <- ipc/daemon_update.rs:33 `cmd_daemon_restart_previous_terminals` (registered at lib.rs:1725) <- ui updaterDaemonPending.ts:63. That is the user's count-confirmed "Restart terminals now". | Explicit user command. OK |
| `supervisor::reload` | No caller | OK |
| **New: hub** (`hub_runtime.rs`, crates/ferryx-hub) | `ferryx hub stop/start` (hub_runtime.rs:138, :227-258) talks only to the hub's own control socket and marks a DB flag. Its error path says "the daemon was not restarted". The grep for `systemctl\|schtasks\|launchctl` in crates/ and hub_runtime.rs is empty. The only `kill` is `libc::kill(pid, 0)` (existence probe, meta/supervisor.rs:132) and `child.kill()` of the hub's own spawned watcher child (inbox_watch.rs:942). The hub starts after readiness and "a hub failure is logged and the daemon keeps serving panes" (cli.rs:3047-3062). | Not a daemon/job destroyer. OK |
| **New: c3ea7010 hub start after handover** (cli.rs:3053-3062) | It only moves the hub spawn out of `if announce_readiness`. No lifecycle call. | OK |

**Conclusion.** Each destructive site is reachable only from Absent, from an owner-authorized handover, or from an explicit user command, or it is unreachable. Nothing new in 66f388f3..cfe34b8e adds a site.

## 3. Diff 66f388f3..cfe34b8e against the plan's guardrails (SOURCE)
- **Product daemon files touched in range:**
  - client.rs (agent-control Wait timeout);
  - protocol.rs (optional `host_id`);
  - server.rs (agent observer wiring, relay status flags, `note_human_keystrokes`);
  - session_service.rs (agent watch);
  - supervisor_linux.rs (hub env forwarding, with hub secrets kept in the runtime env file);
  - supervisor_windows.rs (QA-only `FERRYX_QA_DISABLE_CONTROL`, under `local-split-qa` only);
  - update_pending.rs and its tests (log line and decision split).
- **Guardrail-relevant changes:**
  - `QA_FORWARDED_ENV` gains `FERRYX_QA_DISABLE_CONTROL` only under `#[cfg(feature = "local-split-qa")]`. Its test asserts it is never forwarded into a user task (supervisor_windows.rs:1343-1347). This respects "QA uses only isolated debug bundles".
  - The Linux `forwarded_key` now also forwards `FERRYX_HUB` and `FERRYX_HUB_*` non-secret keys into generation units. This changes the env of the staged generation, not its lifecycle. No Must NOT line is touched.
  - No change to admission, the launchd/systemd/schtasks state machines, handover, or the control channel.
- **Harness (scripts/qa/daemon-lifetime, 18 files, +1035/-82):**
  - Teardown still kills only recorded pids. replace.test.mjs and guards.test.mjs add "a reused pid is never signalled" and `KILL_REFUSED` cases.
  - `jobs.mjs` `remove()` uses `schtasks /End` and `/Delete` only on QA-named tasks (`listQaJobs` filters `TaskName -like '<base>*'`).
  - Pattern kills: none added.

## 4. PENDING-QA rows from r3: closure (MEASURED)
| Evidence | runId | head (summary.revisions.head) | RUN_EXIT / exitCode | Statuses |
| --- | --- | --- | --- | --- |
| w5-qa/F3/linux/final/run1/summary.json (index: w5-qa/F3-linux-summary.json) | 20261022t175629z | cfe34b8e…, buildTree dirty [] | 0 / 0 | PASS: Q00-Q05, Q06, Q07, Q08, Q08b, Q10, Q10b, Q11, Q12, Q13 (15). N/A: Q04b, Q06w, Q09. 0 failed assertions (my own count over every scenario's `assertions[].ok`). |
| w5-qa/F3/windows/final2/run2/summary.json (index: w5-qa/F3-windows-summary.json) | 20261022t184142z | cfe34b8e…, buildTree dirty = CRLF-only Cargo.toml + 2 gen schemas | 0 / 0 | PASS: Q00-Q05, Q06, Q06w, Q07, Q08b, Q09, Q10, Q10b, Q11, Q12, Q13 (16). N/A: Q04b, Q08. 0 failed assertions. |

Both runs record v1 rev 24cbfd25 and baseline 493fae45 with clean trees. The statuses equal the todo 12 references:
- Linux all3b 20261022t022013z @ b08fab25 and run4 @ 3f6c1f78;
- Windows all4 20261022t123022z @ e7bad8a7.

`statusDifferences: []` in both index files. I re-derived all4's head and exit myself.

| r3 row | r3 status | r4 status | Closing evidence |
| --- | --- | --- | --- |
| Must have "QA Q00-Q13" | PENDING-QA | **CLOSED** (Linux, Windows) | Table above |
| IS-1 | PENDING-QA | **CLOSED** (L, W) | Q00-Q03 PASS on both |
| IS-2 | PENDING-QA | **CLOSED** (L, W) | Q01, Q11 PASS on both |
| IS-3 | PENDING-QA | **CLOSED** (L, W) | Q04, Q05, Q06 PASS on both. Q06w PASS on Windows. Q04b is macOS-only (N/A). |
| IS-4 | PENDING-QA, AT RISK (Linux Q08 FAIL) | **CLOSED** (L, W) | Linux Q07, Q08 (184/184), Q08b, Q13 PASS. Windows Q07, Q08b, Q09, Q13 PASS. The r3 Q08 FAIL is gone: all3b, run4 and final all PASS. |
| IS-5 | PENDING-QA | **[~]**: Linux and Windows PASS, macOS SOURCE-ONLY (user waiver) | F4-fidelity.md |
| IS-6 | PENDING-QA | **CLOSED** (L, W) | Q10, Q10b PASS on both (gateway excluded per plan line 225) |
| IS-7 | PENDING-QA | **CLOSED** (L, W) | Q12 PASS on both, plus identity assertions in every scenario |
| N7 (Linux Q08 triage) | open | **CLOSED** | Q08 PASS in three consecutive Linux runs |
| N9 (no task-12-<platform>-summary.json) | open | **Mapped, not produced** | The brief names the F3 summary files as the evidence. No file is literally named `task-12-<platform>-summary.json` (MEASURED `ls`). F4 maps each IS row to the F3 paths. NOTE only. |
| Release app/daemon unchanged | n/a | **OK** | Linux after.json: both AppImage sha256 unchanged, release daemon absent before and after. Windows after.json: 0 uninstall entries, owner.json absent before and after, `Ferryx-DaemonQa.xml` sha unchanged. |

## 5. Ruling on the Windows supervisor-dir XML leftover (open item O1)
**Facts (MEASURED).**
- Windows final2 before.json: `supervisorDir.entries = 39`. after.json: `41`.
- The two new files are `Ferryx-ferryx-qa-20261022t184142z-2026.1018.1-516169bd.xml` and `...-2026.1013.2-967dfe90.xml`.
- Today's listing of `%LOCALAPPDATA%\Ferryx\supervisor` on maho-win (my read-only `Get-ChildItem`) shows **40 `Ferryx-ferryx-qa-*.xml` files from 2026-10-21 onward**, plus the foreign `Ferryx-DaemonQa.xml` (2026-10-15). So every Windows QA run since T12 started has left one to three of these files.
- Both of this run's files have `<Command>C:\Users\sook\ferryx-wt\qaruns\20261022t184142z\data\daemon-runtime\<key>\ferryx.exe`. That qaruns dir was removed by teardown (`Test-Path` False). `Get-ScheduledTask -TaskPath \Ferryx\` finds 0 `ferryx-qa*` tasks.

**Who writes them (SOURCE).**
- The product writes them. supervisor_windows.rs:99-107 `supervisor_dir` = `%LOCALAPPDATA%\Ferryx\supervisor` and `task_xml_path` = `<dir>\<task name with \ -> ->.xml`.
- `import_definition` (:352-366) writes the file before every `schtasks /Create /XML`. `register_for_login_with` (:508, :563) rewrites it.
- The QA run's GUIs and daemons are product code. With `FERRYX_DAEMON_SUPERVISOR_NAME=ferryx-qa-<runId>`, they register `Ferryx\ferryx-qa-<runId>-<key>` tasks and record these files.
- The harness deliberately keeps the real LOCALAPPDATA (guards.mjs:164-169: "Windows keeps LOCALAPPDATA for the same reason (task definitions)").
- The harness removes the matching files only partly:
  - scripts/qa/daemon-lifetime/lib/jobs.mjs:206-211 `remove(name)` deletes the task (`/End`, `/Delete`).
  - It deletes the supervisor-dir XML **only for the legacy task** (`if (name === task(null))`).
  - For generation tasks (`<base>-<key>`) it deletes the task but leaves its `<name>.xml`.
- Linux has the same pattern, and it is handled: jobs.mjs:190-198 removes the QA unit file explicitly ("disable/stop leave that file behind ... Remove the QA-named file itself").

**Is it the product's normal record?** In production, yes. A recorded definition is kept while its task exists:
- `recorded_generation_keys` (:570-590) and `registered_executables` (:594-610) read it, so runtime GC keeps a runtime a registered task still launches.
- Product uninstall removes the file together with the task (:1068-1076: `/Delete` then `remove_file(task_xml_path)`).

The product therefore treats "task deleted, file kept" as an inconsistent state. These files describe tasks that no longer exist, named with a QA run id, pointing into a deleted QA run dir. **They are QA residue.**

**Ruling.**
- This is a residue of the QA harness. It violates the literal todo 12 acceptance and the F3 requirement "no ferryx-qa-<runId> job, unit, task, process or dir remains" (plan :230, :272). The files are a per-run artifact named with the run id, in a dir outside the run's own dirs.
- It is **not** a product defect and not a Must have/Must NOT violation:
  - no task or process remains;
  - the release app and daemon are untouched;
  - the files are inert to production. They live under the QA name prefix `Ferryx-ferryx-qa-…`, and production's `recorded_generation_keys` filters on the `Ferryx-Daemon-` prefix, so a production GUI never sees them.
- It has one side effect on later QA runs: `registered_executables` reads every `*.xml` in the dir regardless of prefix, so any later run's runtime GC treats these dead paths as "registered". The paths no longer exist, so this protects nothing and deletes nothing.
- **Status: OPEN item O1 (harness), not a product blocker.** The all4 reference and every earlier Windows run left the same kind of file. It is a long-standing harness gap, not a regression at cfe34b8e.

**Exact cleanup the harness needs** (do not run it by hand; I deleted nothing):
1. In jobs.mjs `remove(name)` on Windows, after `/Delete`, remove the record for every QA task name, not only the legacy one. Replace the `if (name === task(null))` guard with a guard that the name starts with `Ferryx\<base>` (QA-only), then delete `Join-Path $env:LOCALAPPDATA ('Ferryx\supervisor\' + name.Replace('\','-') + '.xml')`.
2. In the teardown `leftovers` step (run.mjs:391-396), add a Windows check that `Get-ChildItem $env:LOCALAPPDATA\Ferryx\supervisor -Filter 'Ferryx-<base>*.xml'` is empty. A residue then fails the run instead of passing silently.
3. Add one windows-all-fix.test.mjs case. A generation task `remove()` must issue the Remove-Item for `Ferryx-ferryx-qa-<RUN>-<key>.xml`, and must not issue it for a name without the QA base.
4. A one-time host cleanup the user may authorize. Only `Ferryx-ferryx-qa-2026102*.xml` files, the 40 listed, should be removed; `Ferryx-DaemonQa.xml` and anything else must stay. Command:
   ```powershell
   Get-ChildItem "$env:LOCALAPPDATA\Ferryx\supervisor" -Filter 'Ferryx-ferryx-qa-*.xml' | Where-Object { -not (Get-ScheduledTask -TaskPath '\Ferryx\' -TaskName ($_.BaseName -replace '^Ferryx-','') -ErrorAction SilentlyContinue) } | Remove-Item
   ```
   It skips any file whose task still exists.

## 6. Notes (non-blocking, carried or new)
- **N1, N2 (carried).** Uninstall paths and the data-protocol Shutdown arm have no caller or sender.
- **N6 (carried).** The Linux `backend::reload` is dead (`supervisor::reload()` has no caller).
- **N8 / worktrees.**
  - maho-win `git worktree list` (MEASURED after my cleanup): ferryx [main] 824b84e9, daemon-lifetime-w5-t12-20261018, w5qa-baseline-20261019, w5qa-r3-20261020, w5qa-v1-20261019. All are from earlier lanes and are not mine.
  - omarchy: see F2-review-r4.md.
- **N9.** The evidence name is mapped (section 4).
- **N10 (carried).** The Q10b literal deviation (CLI spawn) was accepted in r1.
- **N11 (new).** Windows F3 ran under environment changes on maho-win (final2/PROGRESS.md Steps 2-3, 6):
  - CMake 4.4.4 first on Machine PATH;
  - HKCU `CMAKE_GENERATOR` removed;
  - `CMAKE_POLICY_VERSION_MINIMUM=3.5`;
  - a temporary `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, removed after the run;
  - the QA cargo targets deleted for disk space.

  These are build-environment changes, not product or harness code. They do not affect what the scenarios assert.
- **N12 (new).** The 22a8ecea Windows build break shows that main was unbuildable on Windows from 0a63b707 until cfe34b8e. The daemon-lifetime proof is at cfe34b8e, which builds; F2 r4 covers this.

## Slop / overfit pass (remove-ai-slops and programming criteria, applied directly; skill files not loaded)
- **Range tests.** No test in the daemon-lifetime part of 66f388f3..cfe34b8e is deletion-only or tautological:
  - `unit_forwards_hub_env_and_keeps_hub_tokens_out_of_the_unit` asserts the rendered unit and the secret split;
  - the update_pending tests assert the op sequence and the decided log line;
  - the QA-env test asserts that the key is never forwarded into a user task.
- **134dcc40.** It replaces a racy captured-subscriber assertion with the returned decision. It removes nondeterminism and keeps the behaviour pinned. Good.
- **NOTE.** update_pending_tests pins the exact `log_line()` text, but that line is machine-consumed (QA Q09 parses it), which is allowed.
- **Code review report.** No separate code-review report was provided for this range. F2-review-r4.md carries the skill-perspective pass.

## Checked artifact paths
- Plan lines 1-80, 120-292.
- brief-final-F1F2F4.md, F1-compliance-r3.md, F2-review-r3.md.
- w5-qa/F3-linux-summary.json, w5-qa/F3/linux/final/{PROGRESS.md, before.json, after.json, run1/summary.json}.
- w5-qa/F3-windows-summary.json, w5-qa/F3/windows/final2/{PROGRESS.md, before.json, after.json, run2/summary.json}, w5-qa/F3/windows/PROGRESS.md.
- w5-qa/windows/run-all-fix/all4/summary.json, w5-qa/linux/run-all-fix/all3b/summary.json, w5-qa/t12-final-gate-review.md, w5-qa/F3/hubfix-gate-review.md (existence).
- Source at cfe34b8e:
  - supervisor_windows.rs:40-110, 330-420, 500-620, 1050-1085;
  - launchd.rs:462-500;
  - hub_runtime.rs:120-300;
  - cli.rs:3040-3070;
  - meta/supervisor.rs:110-140;
  - scripts/qa/daemon-lifetime/lib/jobs.mjs:1-240, run.mjs:355-400, lib/guards.mjs:160-175.
- Diff 66f388f3..cfe34b8e for daemon/*, lib.rs, main.rs, readiness.rs, scripts/qa/daemon-lifetime/lib/jobs.mjs.
- maho-win (read-only): the supervisor dir listing, the two run-20261022t184142z XML `<Command>` lines, and the `Get-ScheduledTask` count.

## Not verified, with the command that settles it
1. **macOS lifecycle (IS-1..IS-7 on launchd).** Waived by the user. If wanted: `node scripts/qa/daemon-lifetime/run.mjs --platform mac --all` on this Mac with an isolated debug bundle.
2. **O1 fixed.** After the harness change, run one Windows `--all`. Then `Get-ChildItem $env:LOCALAPPDATA\Ferryx\supervisor -Filter 'Ferryx-ferryx-qa-<runId>*'` must be empty, and the entry count must equal before.json's.
3. **Whether the harness's Q13 legacy record file is removed.** It is in jobs.mjs:208-211. Its file `Ferryx-ferryx-qa-<runId>.xml` is absent for 20261022t184142z in the listing, so it worked. The earlier `Ferryx-ferryx-qa-20261022t064140z.xml` predates that fix. No command needed.

VERDICT: APPROVE-SOURCE-ONLY-MACOS
