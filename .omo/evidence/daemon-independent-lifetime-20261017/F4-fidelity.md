# F4 Ideal-state fidelity: daemon-independent-lifetime-20261017

- **Reviewer.** F4, independent. Read-only review of the evidence and the source at FINAL_MAIN cfe34b8e.
- **Plan.** .omo/plans/daemon-independent-lifetime-20261017.md. IS rows are at lines 24-30, Success criteria at 282-290, and the Q10b gateway note at line 225.
- **Brief.** brief-final-F1F2F4.md, section 3.
- **Path convention.** Every path below is relative to /Volumes/T9-Mac/project/ferryx-wt/.evidence/daemon-independent-lifetime-20261017/.
- **Labels.** MEASURED means I read the summary.json myself. For each scenario I counted `assertions[].ok`: 0 failed in every PASS scenario of both runs. SOURCE means file:line at cfe34b8e.

## Evidence files
| Platform | summary.json (scenario-level) | index | runId | head (`revisions.head`) | exitCode / RUN_EXIT |
| --- | --- | --- | --- | --- | --- |
| Linux (omarchy) | w5-qa/F3/linux/final/run1/summary.json | w5-qa/F3-linux-summary.json | 20261022t175629z | cfe34b8ead2e9cf4cdd0f590aa1d3c9380b1a17a | 0 / 0 |
| Windows (maho-win) | w5-qa/F3/windows/final2/run2/summary.json | w5-qa/F3-windows-summary.json | 20261022t184142z | cfe34b8ead2e9cf4cdd0f590aa1d3c9380b1a17a | 0 / 0 |
| macOS (this Mac) | none, by the user's waiver ("코드로만 확인", 2026-10-19) | n/a | n/a | n/a | n/a |

- Both runs use v1 rev 24cbfd25 and baseline 493fae45 with clean trees.
- The plan's evidence name `task-12-<platform>-summary.json` is not present on disk. Per the brief, these F3 files stand in for it (F1 r4 N9).
- **Not applicable.**
  - Linux: Q04b, Q06w, Q09.
  - Windows: Q04b, Q08.

  The plan scopes them so (line 231: "Q04b macOS only, Q08 Unix only, Q09 Windows only"; Q06w is the Windows detach guard).
- **todo 12 references** (same statuses, `statusDifferences: []` in both index files):
  - Linux all3b 20261022t022013z @ b08fab25;
  - Windows all4 20261022t123022z @ e7bad8a7.

## IS rows
### IS-1: normal quit and forced GUI termination leave the daemon, PTYs and their commands running
| Platform | Status | Proving scenarios | Evidence | Assertions carrying the claim |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | none run | SOURCE launchd.rs:149-158 (plist RunAtLoad, `KeepAlive {SuccessfulExit=false}`, AbandonProcessGroup); lib.rs:2670 `handle_run_event` and the pin at lib.rs:2709-2796 (quit and window destroy send no request, no `DaemonRequest::Shutdown`); launchd.rs:462-470 (job mutation only for an Absent owner) | n/a |
| Linux | PASS | Q00, Q01, Q02, Q03 | linux run1 summary.json, 20261022t175629z @ cfe34b8e | Q00 "QA OS job reports Running", "daemon pid is the QA job's pid", "daemon executable is the staged runtime copy, not the app bundle"; Q01-Q03 "GUI process exited", "after GUI end: daemon pid and start time unchanged", "after relaunch: every live session kept its identity", "reattached session: nonce … echoed exactly once" |
| Windows | PASS | Q00, Q01, Q02, Q03 | windows run2 summary.json, 20261022t184142z @ cfe34b8e | Q00 "scheduled task is Running", "watchdog runs the staged generation executable", "daemon pid is the QA job's pid"; Q01-Q03 the same four as Linux plus "reattached session: fresh prompt after Ctrl+C" |

### IS-2: relaunch reattaches the same session identities with no replacement shell and no input replay
| Platform | Status | Proving scenarios | Evidence | Assertions |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | none run | SOURCE protocol.rs:670 `DescribeLiveSessions`; qa_barrier.rs:2908 `QaWriteAckHold`; lib.rs:2709-2796 | n/a |
| Linux | PASS | Q01, Q11 | linux run1 | Q01 "after relaunch: every live session kept its identity", "reattached session: write accepted", "nonce … echoed exactly once"; Q11 "same backend id and remote_pid after GUI quit/relaunch", "remote log contains the marker exactly once (no replay of the uncertain write)", "the held Enter was not replayed" |
| Windows | PASS | Q01, Q11 | windows run2 | the same Q01 and Q11 assertions, plus Q11 "the ack was still held when the GUI quit" |

### IS-3: an unresponsive or unverifiable endpoint never causes stop, unload, kill or a second owner
| Platform | Status | Proving scenarios | Evidence | Assertions |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | Q04b is macOS-only and not run | SOURCE owner_admission.rs:125-215 (truth table), :812 `admit`; client.rs:2636 `Admit::Start` / :2816 `wait_for_owner_endpoint`; launchd.rs:469 (`owner != Absent` -> zero launchctl); launchd.rs:872 forbids `kickstart -k` (fake-runner tests :956, :972, :1139) | n/a |
| Linux | PASS | Q04, Q05, Q06 (Q04b N/A) | linux run1 | Q04 "client received DAEMON_BUSY for this stall", "job pid unchanged during the stall", "zero supervisor mutations in the GUI log", "after release: daemon pid and start time unchanged"; Q05 "<fault>: typed Unknown logged / zero supervisor mutations / job pid unchanged" for inspect-denied, start-time-mismatch and record-unreadable; Q06 "exactly one daemon process", "owner.json only ever named one new owner" |
| Windows | PASS | Q04, Q05, Q06, Q06w (Q04b N/A) | windows run2 | Q04, Q05 and Q06 as Linux; Q06w "client refused the detached spawn with DAEMON_DETACH_UNAVAILABLE", "no daemon process was started", "zero supervisor mutations" |

Q04b covers the macOS-only loaded-idle kickstart. On Linux and Windows the same "start only for Absent" rule is carried by Q06 (Absent -> exactly one owner) and Q10 "opted-out launch started the owner under the OS job".

### IS-4: app replacement and upgrade preserve sessions; each runtime generation independently owned
| Platform | Status | Proving scenarios | Evidence | Assertions |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | none run | SOURCE runtime_stage.rs:107 `generation_key`, :147 `stage_runtime`, :345 `SignatureInvalid`; supervisor.rs:272 `start_handover_candidate`; control_server.rs:371-420 (RequestUpgrade validation); pty.rs:1353 `keep_writer_from_sending_eof`; launchd.rs:609-640 `migrate_legacy_job` (Absent-only removal) | n/a |
| Linux | PASS | Q07, Q08, Q08b, Q13 (Q09 N/A) | linux run1 | Q07 "A generation: daemon pid and start time unchanged", "B attaches the same sessions", "B generation job registered but not running while A is Alive"; Q08 (184 assertions) "A_B/B_C: owner.json moved straight from predecessor to successor", "candidate accepted nothing on any inherited listener before commit", "a probe flock on both lock files failed at every stage", "successor runs under its own generation job", "successor parent is not the predecessor", "SSH remote_pid unchanged", "local counter continues without restart", "SSH counter continues through the successor without restart"; Q08b "control Hello succeeds across revisions", "V1_HEAD: owner.json moved straight from predecessor to successor"; Q13 "legacy pid and start time unchanged", "no bootout/stop/End sent to the legacy job", "no UpgradeBinary sent to the legacy owner" |
| Windows | PASS | Q07, Q08b, Q09, Q13 (Q08 N/A) | windows run2 | Q07 as Linux; Q08b "Windows enters the pending state", "old owner untouched"; Q09 "typed pending state with the exact count 2", "A pid unchanged (no Shutdown, no terminate)", "stale count answers COUNT_CHANGED", "A exited by RetireWhenIdle when its last terminal closed", "no A-generation respawn past 2 x 1000ms backoff", "confirm with the exact count is accepted and A exits", "pending state with unknown count (button disabled)"; Q13 as Linux plus "legacy task State read for real (Running)" |

Q08 (Unix upgrade) does not apply to Windows by design: ConPTY is not transferable, and the plan bans transfer claims. On Windows the same IS is covered by Q09 (never end the live owner without a count-confirmed user action) and Q08b (cross-revision: pending, not destroyed). Q09 does not apply to Linux; Q08 and Q08b cover Linux.

### IS-5: macOS, Linux and Windows all meet IS-1..IS-4 with native mechanisms
| Platform | Status | Evidence |
| --- | --- | --- |
| macOS | **SOURCE-ONLY (user waiver)** | launchd per-generation jobs: launchd.rs:149-158, :462-500, :609-640; signed helper staging: runtime_stage.rs:10, :147, :345; no QA run |
| Linux | PASS | systemd --user per-generation units; Q00 "QA OS job reports Running", "legacy QA unit is not enabled alongside the generation unit"; IS-1..IS-4 rows above |
| Windows | PASS | per-user scheduled task + watchdog; Q00 "scheduled task is Running", "watchdog runs the staged generation executable"; IS-1..IS-4 rows above |

**IS-5 is `[~]`.** It is PASS on Linux and Windows, and SOURCE-ONLY on macOS by the user's waiver. It is not a full PASS, because no macOS lifecycle scenario was run.

### IS-6: autostart opt-out and headless use work without tying the running daemon to the GUI
Per the plan (line 225, Q10b), the headless proof uses the local CLI. **The remote HTTP gateway is excluded**, because it binds the fixed port 43821 that a live release daemon may hold. That exclusion is applied here.

| Platform | Status | Proving scenarios | Evidence | Assertions |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | none run | SOURCE supervisor.rs:522-530 macOS `remove_autostart` (RunAtLoad false by file rewrite, "No launchctl call"); ipc/agent_integration.rs:100-103 (opt-in `ensure_if_absent`, opt-out `remove_autostart`) | n/a |
| Linux | PASS | Q10, Q10b | linux run1 | Q10 "opt-out removed login autostart", "opt-out toggle never stops the running owner: daemon pid and start time unchanged", "opted-out launch started the owner under the OS job, not as a GUI child", "owner parent is not the GUI"; Q10b "no QA GUI process running", "Hello pid equals the QA job pid", "CLI client listed sessions through the daemon", "one local session spawned headless", "Hello pid unchanged after the CLI exits" |
| Windows | PASS | Q10, Q10b | windows run2 | the same assertions, plus Q10 "task re-import kept watchdog and daemon pids" |

### IS-7: verification proves the same PTY/child processes and live output, not registered IDs
| Platform | Status | Proving scenarios | Evidence | Assertions |
| --- | --- | --- | --- | --- |
| macOS | SOURCE-ONLY (user waiver) | none run | SOURCE handover.rs:558-569 `count_live` (local only if `reader_open`, ssh/paired only if `connected`); protocol.rs:670 `DescribeLiveSessions` | n/a |
| Linux | PASS | Q12, plus identity checks in every scenario | linux run1 | Q12 "exited local PTY reports readerOpen=false (or is gone)", "exited entry is not counted live", "a disconnected SSH entry is excluded from Inventory", "remote_pid never placed in local_child_pid", "control Inventory live_local equals the live local entries"; Q00 "seeded local session is live with a child pid"; "every live session kept its identity" in Q01-Q03, Q07, Q08 |
| Windows | PASS | Q12, plus identity checks | windows run2 | the same Q12 assertions and identity checks |

## Summary table
| IS | macOS | Linux | Windows | Row |
| --- | --- | --- | --- | --- |
| IS-1 | SOURCE-ONLY | PASS | PASS | [~] |
| IS-2 | SOURCE-ONLY | PASS | PASS | [~] |
| IS-3 | SOURCE-ONLY | PASS | PASS | [~] |
| IS-4 | SOURCE-ONLY | PASS | PASS | [~] |
| IS-5 | SOURCE-ONLY | PASS | PASS | **[~]** |
| IS-6 | SOURCE-ONLY | PASS (gateway excluded per Q10b) | PASS (gateway excluded) | [~] |
| IS-7 | SOURCE-ONLY | PASS | PASS | [~] |

No Linux or Windows row is without a passing scenario and its evidence file, so there is no FAIL.

## Open items
- **O1 (harness residue, Windows).**
  - What remains: the Windows run left two `Ferryx-ferryx-qa-20261022t184142z-*.xml` task-definition records in `%LOCALAPPDATA%\Ferryx\supervisor` (39 -> 41 entries; final2 before.json/after.json). There are 40 such files from QA runs since 2026-10-21 (read-only listing).
  - Source: they are written by the product (supervisor_windows.rs:99-107, :352-366). The harness's `remove()` deletes only the legacy record (scripts/qa/daemon-lifetime/lib/jobs.mjs:206-211).
  - Effect: no task, process or release state remains, so no IS row loses its proof. The files are QA residue against the todo 12 / F3 "no ... dir remains" cleanup line.
  - The exact harness fix and a safe one-time cleanup command are in F1-compliance-r4.md section 5.
- **N9.** There is no file literally named `task-12-<platform>-summary.json`. The F3 summaries are the mapped evidence.

## Not verified, with the command that settles it
1. **The macOS column (all seven rows).** `node scripts/qa/daemon-lifetime/run.mjs --platform mac --all` with an isolated debug bundle on this Mac. The user waived it.
2. **IS-6 gateway headless use.** It is excluded by the plan. The existing remote gateway tests cover it: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::`.
3. **O1 closure.** After the harness fix, one Windows `--all`. Then `Get-ChildItem $env:LOCALAPPDATA\Ferryx\supervisor -Filter 'Ferryx-ferryx-qa-<runId>*'` must be empty.

VERDICT: PASS-PARTIAL (macOS source-only)
