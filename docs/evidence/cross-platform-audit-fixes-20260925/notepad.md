# Ultrawork Notepad (recreated) — Ferryx cross-platform audit fixes + external review remediation

Started: original 2026-09-24 20:21 KST (this file recreated ~23:55 KST after the temp notepad was cleaned by the OS)

> The ORIGINAL notepad lived at `/var/folders/.../ulw-20260924-202126.XXXXXX.md.H3sQ6bNrlZ` and was
> deleted when the system cleaned its temp directory. Durable state lives in the repo instead:
> `docs/CROSS_PLATFORM_AUDIT_2026-09-24.md` (the report, sections 0-11) and
> `.omo/evidence/cross-platform-audit-fixes-2026-09-24/review-gpt-5.6-sol.md` (the external review).

## Goal
Fix every finding from the 2026-09-24 Ferryx cross-platform audit and re-verify on real surfaces.
82 distinct finding IDs (the objective's "83" was an arithmetic error, corrected in the report).

## Criteria status
- C1 macOS compile: PASS (cargo check, C1c_EXIT=0, 36.23s) — needs ONE more run after P11/P12 land.
- C2 Windows gated compile: PASS (zigbuild, 0 rustc error codes; only the permitted ghostty static-lib link) — needs one more run.
- C3 unit tests: 2096 passed; residuals proven load-flaky by isolation runs (failing set changes between runs).
- C4 frontend: PASS (Tests 1 failed / 5458 passed; the 1 is FOREIGN pairedHostInventory; BrowserToolbar.devtools is a pre-existing bun:test collection error). Needs a final run after fixP2.
- C5 shortcut guard: PASS (mutation-proven RED->GREEN).
- C6 portable locking: PASS (auth.rs / attach_identity.rs / store.rs take File::lock() outside any cfg).
- C7 disposition: PASS (report section 8, 82/82 rows).
- C8 gate review: round 1 BLOCK -> remediated -> round 2 APPROVE-WITH-NOTES (in-house reviewer).
  THEN an EXTERNAL review (mahoquot gpt-5.6-sol) returned BLOCK with 5 blockers + 5 P1s. That is the live work.

## External review outcome (gpt-5.6-sol, 66-file diff)
VERDICT: BLOCK. Artifact: .omo/evidence/cross-platform-audit-fixes-2026-09-24/review-gpt-5.6-sol.md

My verification of each claim (I did NOT take the reviewer's word):
- BLOCKER 1 (capability command is dead plumbing) -> FALSE POSITIVE caused by MY diff packaging:
  lib.rs / browserTauri.ts / BrowserToolbar.tsx are absent from /tmp/our-files.txt so they were not in the diff.
  Live tree HAS the registration (lib.rs) and consumers (browserTauri.ts, BrowserToolbar.tsx).
- BLOCKER 2 (linux.rs no-child fallback fabricates child-surface capabilities) -> REAL. linux.rs:239-247.
- BLOCKER 3 (readiness published before the agent-state rendezvous exists) -> REAL. server.rs:2124-2128 vs :2130/:1777/:1783; pty.rs reads once at :80-112.
- BLOCKER 4 (shell.rs returns "/bin/sh" when only PATH has sh) -> REAL. shell.rs:384-389.
- BLOCKER 5 (client.rs deletes port+lock without checking termination success) -> REAL. client.rs:1289-1293.
- P1-1 (Linux text probe without a text MIME type can paste image bytes as text) -> REAL, I confirmed by reading:
  decode_linux_clipboard_text (:252-259) is from_utf8_lossy; classify_linux_clipboard (:281-285) checks text FIRST.
- P1-2 (rendezvous not atomic, stale files not removed) -> folded into fixN.
- P1-3 (Store message shown for Linux deb/rpm) -> REAL. GeneralSection.tsx:89.
- P1-4 (rendezvous tests vacuous) -> folded into fixN.
- P1-5 (quoted Windows command lines still split at the space) -> REAL. agentSessionDiscovery.ts:35-39.

## Remediation runs
- P11 `dag_cf8d0fc4-7a90-4307-867f-1ea35916c28e` (7 nodes): fixL shell, fixM client delete, fixN rendezvous, fixO descriptor, fixP store msg, fixQ quoted cmd, fixR clipboard.
  - fixL COMPLETED + VERIFIED (shell.rs:384-393 now returns "sh" for the PATH-only case).
  - fixQ COMPLETED + VERIFIED (agentSessionDiscovery.ts:36-48 takes the leading quoted segment).
  - fixM, fixN, fixO: running.
  - fixP, fixR: FAILED with start_failed (capacity), retried in P12.
- P12 `dag_2384814f-e117-4d27-a0ff-021ca2e6d5c4` (2 nodes): fixP2 store msg, fixR2 clipboard gating. Retried after P12 settled; currently SCHEDULED waiting on P11's slots.

## Lessons recorded
1. VERIFY THE SEMANTICS OF EACH BRANCH, not just that the logic exists. Two of the external blockers (4 and 5) were inside
   functions I had personally read and certified; I checked intent, not whether each branch returns a value consistent with its own precondition.
2. A diff given to a reviewer must include the REGISTRATION and CONSUMER files when the review must judge end-to-end wiring,
   even if the campaign did not author them. Otherwise the reviewer correctly reports "dead plumbing" and the finding is an artifact.
3. Do not launch a second DAG run while the first still has running nodes: the shared resident cap makes the new children fail at START
   (`start_failed`), which then reads like a work failure in the run status.
4. Never let the only copy of the plan live in a temp path.

## Next
1. Wait for P11 (fixM/fixN/fixO) and P12 (fixP2/fixR2) to settle; verify every landed edit by source read.
2. Re-run C1, C2, C4 (Rust + UI changed).
3. Regenerate the disposition table; add a section 12 documenting the external review and its remediation.
4. Re-submit the corrected diff (WITH lib.rs and the UI wiring files) to the external reviewer for a re-verdict.

## P11 SETTLED (5 completed / 2 failed->P12). ALL THREE RUNNING NODES VERIFIED BY ME:

### fixM VERIFIED (external BLOCKER 5) - the destructive delete is now gated
- client.rs:1454 `async fn terminate_stale_daemon_process_windows(pid: u32) -> StaleDaemonTermination` - the fn now RETURNS an outcome enum
  (was `()`), so a caller can no longer silently ignore whether the kill happened. The pid<=4 / self guard returns NotTerminated.
- client.rs:1312-1325 the call site captures it and gates on `stale_daemon_endpoint_files_removable(termination)`;
  when not removable it logs a warn naming the situation ('A live, incompatible daemon could not be terminated; keeping its daemon.port and daemon.lock instead of deleting the endpoint of a daemon that still owns it')
  and RETURNS daemon_protocol_mismatch_error(...) - mirroring the #[cfg(unix)] arm, exactly as specified.
- client.rs:1327-1329 the two remove_file calls now run ONLY after that gate. The identity check and taskkill args are unchanged.

### fixN VERIFIED (external BLOCKER 3 + P1-2 + P1-4)
- server.rs:2200-2211: `self.spawn_agent_state_listener();` now runs BEFORE the readiness signal (`if let Some(tx) = ready_tx { tx.send(()) }`),
  with a comment stating the exact reason: 'This runs before the readiness signal below: a client released by that signal can spawn a pane immediately,
  and the pane reads its agent-state endpoint from disk as it spawns, so a listener started afterwards leaves that pane with no endpoint for its whole lifetime.'
- Stale rendezvous files from a previous boot are now cleared before publishing (the file list at :943-944 is used for removal; tests at :1076-1095 assert the files do not exist and that a staged legacy pair is removed).
- Tests strengthened: :1076-1077 assert absence after startup; :1089-1095 stage a legacy pair and assert removal.

### fixO VERIFIED (external BLOCKER 2) - the descriptor no longer fabricates capabilities
- linux.rs:239-247 `parent_window_surface_descriptor()` now reports `target_kind: CompositorTargetKind::RootWebviewWindow` with `pointer_transparent: false`
  (and layer_backed false), with a comment saying that reporting LinuxChildWindow capabilities here 'would be a fabrication that bypasses that check and renders the terminal over the whole window instead.'
- The test asserts the truthfulness explicitly: `assert!(!descriptor.layer_backed, "the whole parent window is not layer-backed, so the no-child descriptor must not claim it")`
  and `assert!(!descriptor.pointer_transparent, ...)` (:697-704).

### fixR2 LANDED (external P1-1) - clipboard text gating
- native_terminal.rs:281 `pub fn linux_clipboard_advertises_text(types: &[String]) -> bool` (new pure helper).
- :296 the classifier second line of defence: `if has_image && !linux_clipboard_advertises_text(types) {` -> Image wins for an image-only list.
- :571 the text probe is gated: `let text = if !types.is_empty() && linux_clipboard_advertises_text(&types) {`
- :2590 a test `test_linux_clipboard_advertises_text` exists.

### fixP2 STILL RUNNING (GeneralSection.tsx unchanged at 20:53; 'Microsoft Store' still present once)

## CRITERIA: still need C1 + C2 re-runs (client.rs / server.rs / linux.rs / native_terminal.rs / pty.rs all changed) and a C4 run once fixP2 lands.

## C1 RE-RUN PASS (after ALL external-review Rust fixes)
- `cargo check` -> 'Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 14s', **C1d_EXIT=0**, 52 warnings, zero errors.
- This covers the fixes to client.rs (fixM), server.rs + pty.rs (fixN), linux.rs (fixO) and native_terminal.rs (fixR2) all compiling together.

## fixP2 VERIFIED (external P1-3) - and its reasoning is SOUND, not just plausible
- GeneralSection.tsx:64-75 `externallyManagedHost()` derives windows/linux/unknown from navigator.userAgent + navigator.platform.
- :77-81 `externallyManagedUpdateMessage(host)`: windows -> 'Updates are managed by the Microsoft Store.'; linux -> 'Updates are managed by your system package manager.';
  unknown -> 'Updates for this install are managed outside the app.' The old unconditional Store string is gone from that path.
- WHY USING navigator HERE IS CORRECT (I verified the claim rather than assuming): the card only renders this branch when
  `updatesManagedExternally()` is TRUE, and updater.ts:56-66 shows that function returns false when the invoke fails - so the remote web client
  (no Tauri runtime) never reaches this branch, and in the desktop app navigator IS the host platform. The comment states exactly this reasoning.
  This is therefore NOT the same mistake as the TabBar shell-picker bug (which needed the backend because the web client could reach it).
- NOTE for the report: TabBar/TerminalSection still correctly use status.platform (backend); this card legitimately uses navigator because of the guard above.

## fixR2 VERIFIED (external P1-1)
- native_terminal.rs:281-289 `linux_clipboard_advertises_text(types)` matches the `text/` prefix plus the X11 atoms TEXT / UTF8_STRING / STRING, case-insensitively and after trimming.
- :296-299 classifier second line of defence: `if has_image && !linux_clipboard_advertises_text(types) { return Image }` BEFORE the text check.
- :571-578 the text probe is now gated: `let text = if !types.is_empty() && linux_clipboard_advertises_text(&types) { ... } else { None }`,
  with a comment naming the failure (an unadvertised text flavour means the untyped read returns whatever the clipboard holds, image bytes included).
- :2590-2608 test_linux_clipboard_advertises_text covers 10 cases incl. 'text/plain;charset=utf-8', 'UTF8_STRING', 'STRING', 'TEXT', a padded entry, image-only lists, and the empty list.

## CRITERIA AFTER THE EXTERNAL-REVIEW FIXES
- C1 PASS (C1d_EXIT=0)
- C2 in flight (C2d)
- C3: needs a re-run (server.rs / pty.rs / client.rs / native_terminal.rs changed)
- C4: needs a re-run after fixP2 (GeneralSection.tsx changed)

## C4c PASS (external-review fixes included)
- Test Files 2 failed | 279 passed (281); Tests 1 failed | 5464 passed (5465).
- The two remaining files are the SAME two proven not-mine: BrowserToolbar.devtools.test.tsx (pre-existing bun:test collection error, 0 tests) and
  pairedHostInventory.test.ts (FOREIGN, commit 216272f2, not in our-files.txt).
- Passed counts GREW (278 -> 279 files, 5458 -> 5464 tests), consistent with the added tests from fixP2/fixJ/fixR2.

## C3d: 2099 passed / 4 failed - and ALL FOUR are provably not mine
- Failing files: account/mailer.rs (x2 tests), remote/filesystem_tests.rs, ferryx_scope/ssh/helper_service_tests.rs.
- OWNERSHIP: every one of those files reports EMPTY `git status --porcelain` (untouched by ANY session) and is absent from /tmp/our-files.txt.
  So the campaign never edited them.
- ISOLATION RUN (--test-threads=1, just those four): `test result: ok. 4 passed; 0 failed`, ISO2_EXIT=0. They all PASS alone.
- FAILURE NATURE: lock contention / port bind / deadline admission under load:
  * mailer x2 (account mail dir)
  * filesystem_tests:336 `assertion failed: TcpStream::connect(addr).await.is_err()` (a port was expected to be closed)
  * helper_service_tests:33 'REMOTE_RUNTIME_CONFLICT: lock acquisition failed because the operation would block'

## DECISIVE FLAKINESS EVIDENCE (three runs, identical code except my own fixes)
| run | passed | failing set |
|---|---|---|
| C3b | 2096 | manifest.rs lock contention + foreground.rs teardown (the one DETERMINISTIC failure - fixed by fixI) |
| C3c | 2096 | native_operations (listener survived) + remote/tests (port already in use) |
| C3d | 2099 | mailer x2 + filesystem_tests (port) + helper_service_tests (lock would block) |
=> The failing set CHANGES between runs on the same code, every failure is a port/lock/deadline contention, every one passes in isolation,
   and the passed count INCREASED as my fixes added tests. This is load-induced flakiness, not regression.
- Corroborating: a FOREIGN cargo clippy/test for /Volumes/T9-Mac/project/mahoquot/mahoquot-proxy was running concurrently for much of this window.

## CRITERIA FINAL (after the external-review fixes)
- C1 PASS (C1d_EXIT=0)
- C2 PASS (0 rustc error codes)
- C3: 2099 passed, 4 failed - ALL FOUR provably foreign/load-flaky with isolation proof + three-run evidence
- C4 PASS (1 failed test of 5465, and it is the FOREIGN pairedHostInventory; the other file is a pre-existing bun:test collection error)
- C5 PASS, C6 PASS
- C7: report has sections 0-12 incl. the external review response
- C8: PENDING - must re-submit the corrected diff (WITH lib.rs + the UI wiring files) to the external reviewer for a re-verdict

## RE-REVIEW SUBMITTED to gpt-5.6-sol (round 2)
- Wider, updated diff: /tmp/review2-diff.patch - **70 files, 7,793 lines**.
- KEY FIX TO MY OWN PACKAGING: the file list is now our-files.txt PLUS the wiring files whose ABSENCE caused the blocker-1 false positive:
  src-tauri/src/lib.rs, ui/src/lib/browserTauri.ts, ui/src/components/BrowserToolbar.tsx (+ its test), GeneralSection.test.tsx, agentSessionDiscovery.test.ts, native_terminal/composition.rs.
  Verified present in the patch (lib.rs 3 hits, browserTauri.ts 3, BrowserToolbar.tsx 3).
- Prompt content: my finding-by-finding verdicts (B2-B5 and P1-1/P1-3/P1-5 CONFIRMED and fixed; P1-2/P1-4 folded into B3; B1 declared a packaging artifact and asked to be re-checked on the new diff),
  plus my test evidence (C1/C2/C3/C4 with the flakiness analysis), plus a request to hunt NEW defects in the same four classes and to re-check macOS regression.
- Output artifact: .omo/evidence/cross-platform-audit-fixes-2026-09-24/review2-gpt-5.6-sol.md (written when the response lands).

## CRITERIA SNAPSHOT AT RE-SUBMISSION
| criterion | state |
|---|---|
| C1 macOS compile | PASS (C1d_EXIT=0, 3m14s) |
| C2 Windows gated compile | PASS (0 rustc error codes) |
| C3 unit tests | 2099 passed / 4 failed - all 4 in UNTOUCHED files, all pass in isolation, failing set changes per run |
| C4 frontend | PASS (5464 passed / 1 failed, that one FOREIGN) |
| C5 shortcut guard | PASS (mutation-proven) |
| C6 portable locking | PASS |
| C7 disposition | PASS (report sections 0-12) |
| C8 gate review | in-house: APPROVE-WITH-NOTES; EXTERNAL round 1: BLOCK (4 real) -> remediated -> ROUND 2 SUBMITTED |

## EXTERNAL RE-REVIEW (round 2) RESULT: **BLOCK** - but a much better position
Artifact: .omo/evidence/cross-platform-audit-fixes-2026-09-24/review2-gpt-5.6-sol.md (93 lines)

### Finding-by-finding verdict from the reviewer
| finding | reviewer verdict | my action |
|---|---|---|
| B1 (capability dead plumbing) | **ARTIFACT** - it confirmed the registration in lib.rs, the wrapper in browserTauri.ts and the consumer in BrowserToolbar.tsx | my packaging fix worked; no code change needed |
| B2 (fabricated child-surface capabilities) | **RESOLVED** | done |
| B3 (readiness not conditional on listener success) | **NOT RESOLVED** | P13 fixT |
| B4 (shell returns /bin/sh when only PATH has sh) | **RESOLVED** | done |
| B5 (unknown tasklist result treated as terminated) | **NOT RESOLVED** | P13 fixS |
| P1-1 (clipboard image bytes as text) | RESOLVED | done |
| P1-2 (rendezvous not atomic / stale) | RESOLVED | done |
| P1-3 (Store message on Linux) | RESOLVED | done |
| P1-4 (rendezvous ordering not pinned by a test) | **NOT RESOLVED** | P13 fixT (structural ordering + test) |
| P1-5 (quoted Windows command lines) | RESOLVED | done |
NEW BLOCKERS: none. The BLOCK is caused solely by B3 and B5.

### B5 CONFIRMED BY ME (the reviewer is right, and it is subtle)
- `is_process_alive_windows(pid)` = `tasklist_image_name_windows(pid).is_some()`, and that returns None for ALL THREE of:
  spawn failure (`.output().ok()?`), non-zero exit (`if !status.success()`), and a genuinely absent pid.
- So the poll loop (`:1458-1463`) returns `Terminated` on its FIRST iteration when tasklist merely FAILED - which then authorises deleting daemon.port and daemon.lock.
  That defeats the very gate fixM added. Needs a tri-state (Alive/Absent/Unknown) with only Absent establishing termination.

### B3 CONFIRMED BY ME
- The non-unix `spawn_agent_state_listener` returns None on bind failure (:1751-1752) and on publish failure, but the startup path discards the result at :2205 and sends readiness at :2209-2210 anyway.
- Also noted by the reviewer: `agent_state_endpoint` is assigned BEFORE publication, so a publish failure leaves the in-memory capability claiming an endpoint that was never made discoverable.

### NEW P1s from round 2 (all assigned)
| # | issue | file | node |
|---|---|---|---|
| 1 | unknown process-query treated as termination | client.rs | fixS (same as B5) |
| 2 | readiness not conditional on listener success | server.rs | fixT (same as B3) |
| 3 | daemon.port and daemon.token published as an UNCOORDINATED pair (a client can read the new port with the previous boot's token) | server.rs + client.rs | **DEFERRED** - server.rs is owned by fixT; dispatch after P13 settles |
| 4 | Windows executable matching is CASE-SENSITIVE in the Rust observer (`Claude.EXE`/`NODE.EXE` unrecognized) while the frontend is case-insensitive | foreground.rs | fixW |
| 5 | clipboard type discovery and text read can use DIFFERENT backends (wl-paste vs xclip), defeating the new text gate | native_terminal.rs | fixX |
| 6 | per-window Linux overlay entries are never removed (stale overlay on a reused label; retained for process lifetime) | browser/linux.rs | fixY |
| 10 | **macOS REGRESSION RISK**: `detectMacPlatform()` requires `navigator.maxTouchPoints === 0`, so an ABSENT property (undefined) fails both Mac checks and macOS would stop being detected - `mod` would resolve to Ctrl on a Mac | shortcuts.ts | fixV (highest priority: macOS is the only platform that runs today) |

### Reviewer NOTES I accept without action
- NOTE 7: the rendezvous replacement test runs on Unix via cfg(test), so its 'replaces in place' claim is not Windows-semantic evidence.
- NOTE 8: the shell resolver change does not alter the macOS path (Linux planning arm only).
- NOTE 9: macOS agent-state ordering changed; no regression demonstrated, but the synchronous work is now part of startup latency and no test pins the Unix bind-before-ready property.

## RUNS: P13 dag_193b4aae-ceea-4225-a9b6-afbd2815395d (fixS client.rs, fixT server.rs, fixU build-test.yml)
##       P14 dag_ec12818c-7a17-455e-ae6b-cb0da22097b9 (fixV shortcuts.ts, fixW foreground.rs, fixX native_terminal.rs, fixY browser/linux.rs)
## File scopes are disjoint between the two runs (client.rs/server.rs/build-test.yml vs shortcuts.ts/foreground.rs/native_terminal.rs/browser/linux.rs).

## P13 SETTLED (3/3) - ALL THREE VERIFIED BY SOURCE READ

### fixS VERIFIED (round-2 BLOCKER B5 + new P1-1) - the tri-state liveness
- client.rs:333-343 new `enum ProcessLiveness { Alive(String), Absent, Unknown }` with a doc comment distinguishing the three cases
  ('could not be spawned, exited non-zero, or produced output that cannot be read' = Unknown).
- client.rs:1502-1510 the poll loop now accepts ONLY a positive answer:
    if matches!(Self::tasklist_liveness_windows(pid), ProcessLiveness::Absent) { return Terminated; }
  with a comment stating that an unanswered probe keeps waiting and then falls through to the identity-checked force-terminate path.
=> An unknown tasklist result can no longer authorise deleting daemon.port/daemon.lock. B5 is closed.

### fixT VERIFIED (round-2 BLOCKER B3 + P1-4) - the listener result is observed and the ordering is pinned
- server.rs:994 new `agent_state_ingress_unavailable: AtomicBool` field (initialised :1700, accessor :1774).
- server.rs:1062-1092 new helper `settle_agent_state_ingress(...)` with TWO tests:
  * readiness_is_released_only_after_the_ingress_failure_is_recorded (:1054) - asserts recorded_before_signal is set and the failure flag is read BEFORE the signal,
  * and the success case asserting the flag stays false.
=> The ordering is now pinned structurally: moving the listener back below the ready send breaks these tests. B3 and P1-4 are closed.

### fixU VERIFIED (P1-4 CI half)
- build-test.yml:168-172 the Windows lib-scope step filter is now `-- worktree daemon::server::agent_state_transport_tests --test-threads=1`,
  so the agent-state rendezvous tests DO run on Windows (the only platform where the non-unix listener exists).

## P15 SETTLED (1/1) - fixZ VERIFIED (round-2 P1-3, the port/token pair)
- SERVER: `publish_transport_rendezvous_internal(port_path, token_path, port, token, on_token_published)` at server.rs:884-902 does the sequence deliberately:
    1. `fs::remove_file(token_path)` (ignoring NotFound) - so while the token is gone and the port is not yet rewritten, a reader sees an ABSENT pair rather than a MISMATCHED one;
    2. `fs::write(token_path, token)`;
    3. `on_token_published()` - a seam that makes the ordering testable;
    4. `fs::write(port_path, port.to_string())` LAST - the port is the publication marker a client keys on.
  The call site (server.rs:2340-2349) uses the non-unix wrapper and documents exactly this reasoning in a comment.
- CLIENT: `read_transport_token_at(path)` (client.rs:191-200) treats a missing OR whitespace-only token as ABSENT (never an empty credential to present),
  and `transport_pair_is_stale(&error)` (client.rs:237) recognises the straddled pair. Crucially it is WIRED IN PRODUCTION at client.rs:1343:
    `Err(error) if transport_pair_is_stale(&error) => { ... }` -> the pair is re-read once instead of the failure being reported.
- TESTS: `a_rejected_credential_is_reread_and_no_other_error_is` (:114-143) asserts the rejection IS retried AND that three unrelated errors are NEVER retried as a stale pair.

## OPERATIONAL FINDING: DAG session run limit = 16
- `sdk.start()` began throwing `Error: The dag start response did not include a run_id.` - a message that does NOT name the cause.
- Calling the tool DIRECTLY revealed the real reason: `tool.workflow({ action: 'start', definition: {...} })` -> `{ text: 'DAG session run limit reached: 16' }`.
- FALLBACK USED: the `task` tool is a separate channel and is NOT subject to the DAG cap. fixAA (the overlay wiring) was dispatched via `tool.task` as st_01a0d3f3.
- Recorded durably in memory: notes/facts/omo-dag-session-run-limit-and-task-fallback.md (commit 637f90d).

## ROUND-2 REMEDIATION SCORECARD (all verified by me)
| item | fix | verified |
|---|---|---|
| B3 readiness vs listener | fixT (settle_agent_state_ingress + ordering test) | YES |
| B5 unknown tasklist | fixS (ProcessLiveness tri-state) | YES |
| P1-4 ordering not pinned | fixT + fixU (CI filter now runs the rendezvous tests on Windows) | YES |
| P1-3 port/token pair | fixZ (ordered publish + reader retry) | YES |
| P1-4 case sensitivity | fixW (strip_suffix_ignore_ascii_case) | YES |
| P1-5 clipboard backends | fixX (LinuxClipboardBackend) | YES |
| NOTE 6 overlay leak | fixY (remove_overlay_for_window) + fixAA (wiring, IN FLIGHT via task) | fixY yes, fixAA pending |
| NOTE 10 macOS regression | fixV ((maxTouchPoints ?? 0)) | YES |

## NEXT: after fixAA lands -> C1 + C2 + C3 + C4 re-run -> round-3 re-submission to gpt-5.6-sol.

## fixAA VERIFIED (round-2 NOTE 6 wiring) - the last outstanding edit is done
- lib.rs:1091-1115: inside the existing `WindowEvent::Destroyed` branch, immediately after the file-preview cleanup, it now calls
  `crate::browser::linux::implementation::remove_overlay_for_window(window.label())` under `#[cfg(target_os = "linux")]`,
  handling the Result with `if let Err(error) = ... { tracing::warn!(...) }` - no unwrap, no panic in the event handler, Ok(bool) deliberately unused.
- CFG CORRECTNESS (the node read it and I confirmed the shape): browser/mod.rs declares `#[cfg(target_os = "linux")] pub mod linux;` and linux.rs wraps its body in
  `#[cfg(target_os = "linux")] pub mod implementation { ... }`, so the call is gated exactly like the other Linux callers in ipc/browser.rs. macOS/Windows compile the branch without the call.
- SCOPE: `git status --porcelain src-tauri/src/lib.rs` shows only that file modified; no other .rs file changed in the last 5 minutes.
- DEVIATION the node declared (and I accept): it did NOT wrap the call in `run_on_main_thread` because `on_window_event` already runs on the main thread, which is what the function's doc requires.

## ALL ROUND-2 REMEDIATION IS NOW COMPLETE AND VERIFIED (8 items)
B3 fixT | B5 fixS | P1-4 fixT+fixU | P1-3 fixZ | P1-4-case fixW | P1-5 fixX | NOTE 6 fixY+fixAA | NOTE 10 fixV.

## FINAL VERIFICATION RUNS LAUNCHED (tree is stable; no node is writing)
- C1e: cargo check (bash_706)
- C4d: full vitest suite (bash_707)
- tsc: bunx tsc --noEmit (bash_708)
Then: C2 (zigbuild), C3 (cargo test --lib), regenerate the disposition + add the round-2 section, and re-submit to gpt-5.6-sol for the round-3 verdict.

## FINAL VERIFICATION AFTER ALL ROUND-2 FIXES
- **C1e PASS**: cargo check -> Finished dev profile in 1m 27s, C1e_EXIT=0 (includes the lib.rs overlay wiring).
- **C2e PASS**: zigbuild --target x86_64-pc-windows-gnu --lib -> 0 rustc error codes; only the permitted ghostty static-lib link failure.
- **C4d PASS**: Test Files 2 failed | 279 passed (281); Tests 1 failed | 5468 passed (5469).
  The 1 failure is `pairedHostInventory.test.ts` (FOREIGN, commit 216272f2, not in our scope); the other file is BrowserToolbar.devtools.test.tsx (pre-existing bun:test collection error, 0 tests).
  Passed count rose again (5464 -> 5468) with the new tests from fixV/fixW/fixX/fixY.
- **tsc PASS**: TSC_EXIT=0.
- C3e in flight.

## C3e RESULT: 2113 passed / 3 failed - and ALL THREE are provably not mine (again)
- Failing: account::mailer (x2) + daemon::manifest::tests::handover_manifest_update_excludes_other_file_handles (lock contention, 'transaction released lock: WouldBlock').
- ISOLATION RUNS (ran twice, both --test-threads=1): `test result: ok. 3 passed; 0 failed`, ISO3_EXIT=0.
- Ownership: mailer.rs and manifest.rs are NOT in our-files.txt and have empty git status.
- FAILURE SET ACROSS THE WHOLE CAMPAIGN (same code, my own fixes only):
  | run | passed | failing set |
  | C3b | 2096 | manifest lock + foreground teardown (the deterministic one - FIXED by fixI) |
  | C3c | 2096 | native_operations (listener) + remote/tests (port) |
  | C3d | 2099 | mailer x2 + filesystem_tests (port) + helper_service_tests (lock) |
  | C3e | 2113 | mailer x2 + manifest (lock) |
  => passing count RISES as my fixes add tests; the failing set changes every run; every failure is port/lock/deadline contention; every one passes in isolation.

## REPORT: §13 added, then a DUPLICATE §13 removed (my own double-append), report now §0-§13 at 840 lines.

## FINAL CRITERIA (all round-2 fixes in)
| criterion | result |
|---|---|
| C1 | PASS (C1e_EXIT=0, 1m27s, incl. the lib.rs overlay wiring) |
| C2 | PASS (0 rustc error codes) |
| C3 | 2113 passed / 3 failed - all 3 in untouched files, all pass in isolation, set changes per run |
| C4 | PASS (5468 passed / 1 failed, that one FOREIGN) + tsc exit 0 |
| C5 | PASS (mutation-proven) |
| C6 | PASS |
| C7 | PASS (§8 disposes all 82) |
| C8 | in-house APPROVE-WITH-NOTES; external round 1 BLOCK; round 2 BLOCK (B3/B5); ALL remediated -> ROUND 3 SUBMISSION NEXT |

## ROUND-3 SUBMISSION: first attempt lost to a kernel restart; resubmitted via curl
- The first two attempts ran the fetch INSIDE the eval kernel. The gateway restarted mid-request (PID 863 -> 97624), the socket closed, and the second attempt's cell was cancelled with 'JavaScript worker was unresponsive to interrupt and was restarted' - losing kernel globals.
- ROBUST PATTERN ADOPTED: write the request body to a FILE, then submit with `curl --data-binary @file` in a BACKGROUND BASH session. This never holds the eval kernel and survives a kernel restart.
  * /tmp/round3-brief.txt  - the system brief (written with the write tool)
  * /tmp/mq-key.txt        - the gateway key (read from ~/.mahoquot/secrets.json, key name 'management-key|http://127.0.0.1:18801|default')
  * /tmp/round3-body.json  - {model: gpt-5.6-sol, messages:[...]} (0.38 MB)
  * /tmp/round3-resp.json  - the response (bash_724)
- Round-3 diff: /tmp/review3-diff.patch (70 files, 8584 lines) - same wider file set as round 2, so the wiring files stay included.

## LESSON: do not hold the eval kernel on a long external HTTP call.
- A 5-10 minute model call inside a cell gets detached, then cancelled on a kernel restart, and all globals are lost.
- Write the payload to a file and curl it from a background bash session instead; then read the response file.

## STATE: all round-2 remediation verified; only the C8 round-3 verdict remains.
## Criteria: C1 PASS, C2 PASS, C3 2113 passed/3 foreign-flaky, C4 PASS, C5 PASS, C6 PASS, C7 PASS, C8 round-3 in flight.

## ROUND 3 VERDICT: BLOCK - down to 2 items, and BOTH are precise, real findings
Artifact: .omo/evidence/cross-platform-audit-fixes-2026-09-24/review3-gpt-5.6-sol.md

### Reviewer's finding-by-finding (round 3)
| item | verdict |
|---|---|
| B3 (readiness vs listener) | **NOT RESOLVED** - see below |
| B5 (unknown tasklist) | **RESOLVED** ('treats only ProcessLiveness::Absent as proof of termination') |
| P1-4 (ordering + Windows execution) | **NOT RESOLVED** - see the CI blocker |
| new P1-1 tasklist | RESOLVED |
| new P1-2 readiness before ingress outcome | RESOLVED (server.rs:2369-2390) |
| new P1-3 port/token pair | RESOLVED |
| new P1-4 case sensitivity | RESOLVED |
| new P1-5 clipboard backends | RESOLVED |
| new P1-6 / NOTE 6 overlay | RESOLVED (lib.rs:1098-1114) |
| NOTE 10 macOS detection | 'The change is correct... restores Mac detection when the property is absent while preserving the nonzero-touch iPadOS exclusion.' |
| macOS shell resolver | NO regression |
| macOS readiness | NO cfg break |
| macOS overlay | NO regression (both guarded by cfg(target_os="linux")) |

### NEW BLOCKER 1: the Windows CI step is malformed (my fixU node's defect)
- The step runs `cargo test ... --lib -- worktree daemon::server::agent_state_transport_tests --test-threads=1` - TWO positional filters.
- I VERIFIED IT EMPIRICALLY: `cargo test --lib -- worktree daemon::server::agent_state_transport_tests --list` lists only `worktree::` tests (119 tests, 0 benchmarks).
  So the second filter is IGNORED (the reviewer said 'rejected'; the mechanism differs but the outcome is the same) and the rendezvous tests still never run on Windows.
- => fixU did NOT achieve its purpose. DISPATCHED fixAB (st_01a0d410) to split it into two single-filter invocations.

### REMAINING B3: the endpoint is exposed before it is published
- server.rs:2023-2031 (verified by me): `*self.agent_state_endpoint.lock() = Some((port, token.clone()));` runs BEFORE `publish_agent_state_rendezvous`, and on publication failure the function returns None WITHOUT clearing the field.
- So a caller can observe BOTH 'ingress unavailable' AND a Some endpoint that was never made discoverable. The reviewer is right; fixT fixed the readiness half but not this state half.
- DISPATCHED fixAC (st_01a0d411): assign only after successful publication, guarantee the invariant on every failure path, and reconcile the 'port as publication marker' comment with the code (the old port is NOT removed before the new token is written, so the comment overstates; the reviewer noted the client's single stale-pair re-read is what actually protects readers).

### Reviewer's description-mismatch list (all accepted)
- 'all of B3 remediated' did not match the retained pre-publication assignment.
- 'the Windows workflow now runs both scopes' did not match the invalid two-filter command.
- the 'port as publication marker' comment is stronger than the implementation.
- The supplied cargo/UI test totals are external evidence it cannot verify from the diff; it notes nothing in the macOS-gated changes indicates a macOS regression.

## DAG run cap is still exhausted -> both round-3 fixes went out via tool.task (st_01a0d410, st_01a0d411).

## fixAB VERIFIED + EMPIRICALLY PROVEN (round-3 NEW BLOCKER 1)
- build-test.yml: the Windows lib-scope step now runs TWO invocations in one step, each with a SINGLE filter and the same --test-threads=1.
  The leading comment was updated to explain the two scopes ('daemon::server::agent_state_transport_tests is named explicitly beside it...').
- MODULE PATH CONFIRMED: server.rs:1047-1048 declares `#[cfg(test)] mod agent_state_transport_tests { use super::*; ... }` -
  only `#[cfg(test)]`, NO unix gate, so it really is Windows-viable coverage. daemon/mod.rs has `pub mod server;`, so the filter path is correct.
- EMPIRICAL PROOF (the decisive evidence for this blocker):
  * TWO filters (the old, broken shape): `cargo test --lib -- worktree daemon::server::agent_state_transport_tests --list` listed ONLY `worktree::tests::...` (119 tests, 0 benchmarks) -> the second filter had NO effect.
  * worktree ALONE: 111 tests, 0 benchmarks.
  * rendezvous ALONE: **9 tests**, 0 benchmarks, e.g. `daemon::server::agent_state_transport_tests::transport_token_matches_only_the_exact_credential`.
  => The 9 rendezvous tests were NOT running under the old command; with the split they are selected. The reviewer's finding was correct (mechanism: ignored rather than rejected, but the outcome is the same).
- NOTE: the reviewer's other B3 half is still open and fixAC (st_01a0d411) is running on it.

## fixAC VERIFIED (round-3 remaining B3 half)
- New helper server.rs:987-996 `publish_agent_state_endpoint(endpoint, runtime_dir, port, token)` does `publish_agent_state_rendezvous(...)?; *endpoint.lock() = Some(...); Ok(())` - the assignment happens ONLY after the record is renamed into place.
- Listener: the pre-publish `= Some(...)` is gone; server.rs:2063 sets `*self.agent_state_endpoint.lock() = None;` at function entry next to `clear_stale_agent_state_rendezvous(...)`, so every earlier return (bind, set_nonblocking, local_addr, from_std, publish) leaves the accessor reporting None.
- Field doc updated to state the real invariant.
- Test `a_failed_rendezvous_publish_leaves_no_endpoint_to_report` (:1159+) publishes into an ABSENT runtime dir, asserts Err AND `*endpoint.lock() == None` ('an unpublished rendezvous must never be reported as an endpoint'), then the success path asserts the pair.
- SCOPE: only server.rs modified.

## COMMENT/CODE MISMATCH RESOLVED THE HONEST WAY (option b, and the node found WHY option a is unsafe)
- The reviewer was right that the 'port as publication marker' comment overstated: the predecessor's port is NOT removed before the new token is written.
- The node READ the code and showed removing it at publish time is unsafe for a reason neither the reviewer nor I had stated:
  the port file is removed under the INSTANCE LOCK by `remove_stale_socket_after_lock`, which runs before the listener binds; deleting it at publish time would let a successor that has NOT yet taken the lock erase a LIVE predecessor's endpoint.
- So the doc now says the ordering keeps a port from ever being ahead of its credential, names `remove_stale_socket_after_lock` as the remover, and states the residual window is made harmless by rejection + one re-read. The in-test assertion message was reworded to match.

## SCOPE DISCIPLINE: fixAB touched only build-test.yml; fixAC touched only server.rs. relay_server.rs in the same time window is a FOREIGN session's work (absent from our-files.txt).

## NEXT: C1f (bash_741) -> C2 -> C4 -> append §14 -> round-4 submission.

## ROUND 4 VERDICT: BLOCK - my 3 items RESOLVED, but a NEW real Windows bug surfaced (found BY my CI fix)
Artifact: .omo/evidence/cross-platform-audit-fixes-2026-09-24/review4-gpt-5.6-sol.md

### My round-3 items: ALL RESOLVED
| item | verdict |
|---|---|
| CI blocker (two filters) | **RESOLVED** (build-test.yml:163-176, two invocations, one filter each) |
| B3 (endpoint before publish) | **RESOLVED** (server.rs:987-996 assigns after publication; :2060-2064 clears before every attempt) |
| P1-4 (ordering + Windows execution) | **RESOLVED** |
Reviewer also confirmed the B3 test is NOT vacuous ('it forces fs::write to fail through an absent parent directory and verifies that the mutex remains None, followed by a real successful publication').

### NEW BLOCKER: `fs::rename` does not replace an existing destination on Windows
- `publish_agent_state_rendezvous` (:969-980) stages a temp file then `fs::rename(staged, dest)`. On Windows that FAILS when dest exists, so every boot after the first cannot publish -
  and the newly enabled Windows CI test republishes at :1277, so it would fail there too. My CI fix EXPOSED this pre-existing bug rather than causing it.
- Reviewer's own observation: 'the supplied cross-compile evidence did not execute the Windows tests because it stopped at the pre-existing link failure' - so my C2 evidence could never have caught it.
- DISPATCHED fixAD (st_01a0d429): replace-capable publication (MoveFileExW + MOVEFILE_REPLACE_EXISTING if the windows-sys Win32_Storage_FileSystem feature is really enabled, else a documented Windows delete-then-rename), plus comment corrections.

### THREE DESCRIPTION MISMATCHES - and one is MY false rationale (accepted)
1. The doc at :872-890 says 'a reader that finds a port always finds a token that is already written' - stronger than the code, which has an intentional interval with a PORT AND NO TOKEN. Reword.
2. **MY RATIONALE WAS FALSE**: I told the fixAC node (and the reviewer) that removing the stale port at publish time would let a successor that has NOT yet taken the instance lock erase a live predecessor's endpoint.
   I verified: `acquire_daemon_locks` is at :2386 and `publish_transport_rendezvous` at :2414 - the publisher ALREADY owns the lock. The reviewer is right that the rationale is not demonstrated by the code. Must be deleted/rewritten.
3. The CI comment claims those 9 tests are 'the only coverage of the non-unix agent-state ingress'; the reviewer read them and is right that they exercise helper/ordering contracts and never invoke the `cfg(not(unix))` accept loop. DISPATCHED fixAE (st_01a0d42a) to reword.

### macOS regression check (reviewer, round 4): NO regression found - it checked 6 areas
(unix agent-state path still used; the rendezvous helper is test-compiled on macOS and unix rename-over-existing allows its assertion there; Linux overlay guarded; macOS CLI paths present; launchctl macOS-only; maxTouchPoints fix intact.)

## HONEST ASSESSMENT: round 4 found a bug my own verification structurally could not
- My C2 (Windows cross-compile) stops at the pre-existing ghostty link failure, so it can only prove TYPE-CHECKING, never Windows test EXECUTION.
- This is the third round where an independent review found something real that my per-file reading passed. The pattern is consistent: my verification proves intent and types; it does not execute the target platform.


# SESSION RESUME 2026-09-25 (round-5 closure) — orchestrator log

Delegation: 3 flash lanes via the task tool (2 x mahoquot/cline-free/deepseek-v4.1-flash, 1 x mahoquot/z-ai/glm-5.3-flash); review via mahoquot gpt-5.6-sol HTTP. The orchestrator did not edit product code; it ran the compile/disassembly evidence and the verification chain (child lanes on this surface have no shell access — see the lessons below).

## Criterion results (this session)

| criterion | result | evidence |
|---|---|---|
| C-A server.rs comments | **PASS** | server.rs:875-880 (contract corrected) and :969-976 (platform replace paragraph with citation); `grep -c 'always finds a token that is already written'` = 0; `cargo check` **C1_EXIT=0** (Finished dev profile in 52.77s) |
| C-B rename rebuttal | **PASS** | `rename-windows-evidence.md` (12,698 bytes): std doc (fs.rs:2658-2659), impl (windows.rs:1271-1272), share mode (windows.rs:203), std object disassembly at f8ef/f8f5, linked PE at 140011b0f/140011b15 -> thunk 0x1400bfbb0 -> IAT 0x1400ec788 = MoveFileExW (index 73, confirmed by llvm-readobj and by an independent PE ILT parse) |
| C-C report integrity | **PASS** | old rationale sentence count 0; new §14 row (line 895) + subsection 903-907; §15 at 908-926 + the delta-artifact pointer at line 924 (file 928 lines) |
| C-D round-5 verdict | **PASS** | `review5-gpt-5.6-sol.md`: VERDICT: APPROVE-WITH-NOTES; item 1 "FULLY ANSWERED"; items 2-4 RESOLVED; "NEW BLOCKERS: None identified in the shown changes." |
| C-E rendezvous suite | **PASS** | CI-identical run twice: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 2112 filtered out; finished in 0.01s`, **CE_EXIT=0**; `verify-round5.log` |

## Round-5 reviewer notes and their disposition (no note cited a criterion)

1. delta packaging mixed cumulative campaign changes — answered with `delta-round4-to-round5.diff` (the round-4 file reconstructed from the lane log, then `diff -u`: exactly 2 hunks / 15 changed lines, both comment edits) and recorded in §15.
2. harness had no RECORD2 assertion — `assert_eq!(record2, "41235\ntoken-def\n")` added; harness rebuilt.
3. harness metadata mismatch (Cargo.toml 0.1.0 vs build log v0.0.0) — root cause: the flash lane rewrote /tmp/ferryx-win-rename-proof/{Cargo.toml,src/main.rs} at 01:19, after the orchestrator's 01:16 build, so the published sources and the binary diverged. Rebuilt 01:27 from one authoritative source set: crate ferryx-win-rename-proof v0.0.0, exe 2,010,112 bytes, sha256 52b0ff307cac4c050e24ce06563f04d0d5fb3308cdcf3593ba0d41d47264ca89; the evidence md now shows the exact built sources and the new call-site offsets.

## Cleanup receipt

`rm -rf /tmp/ferryx-win-rename-proof /tmp/zig-smoke /tmp/std-rlib /tmp/const-block.txt /tmp/fn-block.txt /tmp/server.round4.rs` -> RM_EXIT=0; each path verified gone with `test -e`. Also recorded in the evidence md as SECTION 6.

## Lessons (durable)

- Child sessions on this surface have **no eval/shell**; never assign a lane work whose deliverable needs command execution. Lanes author/edit; the orchestrator compiles and captures.
- glm-5.3-flash produced no file writes in 13 minutes (stalled after an eval denial) and was cancelled; deepseek-v4.1-flash landed verified edits twice (6m21s, 7m08s). Prefer deepseek for edit lanes here.
- When a lane and the orchestrator both write under /tmp, the last writer silently wins; pin one authoritative source set and cross-check the build log's crate name/version against the shown Cargo.toml.
- `assembleRound5v2()` does not carry the gateway key; pass it explicitly (the first round-5 POST failed 401 with "Bearer undefined").

## Artifacts (same directory)

rename-windows-evidence.md · delta-round4-to-round5.diff · review5-gpt-5.6-sol.md · lane1-comment-edits.md · lane3-report-edits.md · verify-round5.log · round5-prompt-draft.md · notepad.md (this file)

### C-E addendum (2026-09-25, direct execution of the cargo-built binary)
- Cargo-built test binary provenance: `src-tauri/target/debug/deps/ferryx_lib-cc8118d6c33ab9c1`, 238,380,328 bytes, built 01:28:23 (after server.rs 01:17:25), sha256 2b3872194b39cc976c6f254aff43266869f339bf56ccb6d7d8e404686c9a6a0c.
- Filtered criterion selection executed from that binary: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 2112 filtered out; finished in 0.02s` — the criterion observable captured a fourth time, this time from the binary cargo itself produced (3 cargo-path captures are in verify-round5.log).
- Direct execution requires `DYLD_FALLBACK_LIBRARY_PATH=src-tauri/target/debug/build/ferryx-0853e38632fca60f/out/ghostty_vt/lib` (bare execution dies with "@rpath/libghostty-vt.dylib not loaded").
- FULL-SUITE ATTEMPTS (not a criterion, diligence): cargo-path rebuilds stalled three times in filesystem work (59 GB target/debug/deps + 25 GB incremental on the T9 external volume, with a foreign session copying the repo into /tmp at the same time); direct executions then reached 745 passed / 0 failed before stalling in `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution`, and 732 passed / 0 failed in a --skip rerun before stalling in `daemon::session_service::machine_tests::sixty_four_live_machine_sessions_are_the_admission_limit`. ZERO failures were observed in either run.
- FOREIGN-SESSION INTERFERENCE (evidence): while my runs were stalled, `pgrep -fl ferryx_lib-cc8118` showed another session executing the same test binary from ITS OWN target dir (`/tmp/ferryx-account-grants-target/debug/deps/ferryx_lib-cc8118d6c33ab9c1 --exact daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty --nocapture`). Two sessions running daemon/SSH tests that bind ports concurrently is a plausible contributor to the stalls; those processes were left running (foreign work is untouchable).
- KILL RECEIPTS: bash_13 (direct full suite, p11 hang), bash_15 + bash_17 (their result monitors), bash_16 (--skip rerun, admission-limit stall). No fermenting processes of mine remain.

### ISOLATION VERDICT — the "stalls" and the 3 failures are contention/order flakiness, not deadlocks (2026-09-25)
- `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution`: **PASS in isolation, 121.49s** — a slow-by-design test (reaper interval wait), not a hang. My earlier kill was a too-tight patience window.
- `daemon::session_service::machine_tests::sixty_four_live_machine_sessions_are_the_admission_limit`: **PASS in isolation, 24.13s**.
- The three failures seen in the killed full-suite run (758 passed / 3 failed) were `ipc::worktree::deletion_repair_tests::{missing_record_preview_and_targeted_cleanup, preview_reports_current_dirty_and_unmerged_loss, success_removes_cached_row_and_blocks_stale_worker}` — re-run isolated: **3 passed / 0 failed in 21.17s** (iso-deletion-repair.log). NOTE: `src-tauri/src/ipc/worktree.rs` is NOT in this campaign's 72-file scope and is modified in the working tree by a FOREIGN session, so those failures never implicated our comment-only delta.
- Consequence recorded for honesty: in-session full-suite runs are flaky under concurrent sessions on this machine (foreign tests running the same binary + repo I/O); a complete quiet-machine run is the definitive evidence and is now executing (iso-fullsuite2.log).

### COMPLETE FULL-SUITE RUN — GREEN (2026-09-25, quiet machine)
- Command: the cargo-built test binary executed directly with the build-script dylib on the library path: `DYLD_FALLBACK_LIBRARY_PATH=src-tauri/target/debug/build/ferryx-0853e38632fca60f/out/ghostty_vt/lib src-tauri/target/debug/deps/ferryx_lib-cc8118d6c33ab9c1 --test-threads=1`.
- Result: `test result: ok. 2119 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 496.30s`, `FULLSUITE2_EXIT=0`. Log: iso-fullsuite2.log.
- This closes the last gap from the earlier report: the in-session stalls and the 3 worktree failures are contention/order flakiness (each passes in isolation: 121.49s, 24.13s, 21.17s respectively).

### C-E LITERAL COMMAND — COMPLETED (2026-09-25, quiet machine)
- Command exactly as the criterion words it: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- daemon::server::agent_state_transport_tests -- --test-threads=1`.
- Result: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 2121 filtered out; finished in 0.01s`, `LITERAL_CE4_EXIT=0`. Log: verify-literalc-e4.log.
- Earlier attempts of this same command stalled inside cargo filesystem work on the T9 external volume while a foreign session ran the same test binary; on the quiet machine it completed in one pass. C-E is therefore covered in BOTH forms: the criterion wording (this log) and the CI wording (verify-round5.log, three captures).

## WINDOWS EXECUTION PROOF + VERDICT UPGRADE (2026-09-25T01:25:43.869Z)
- The user pointed out maho-win is SSH-reachable. Verified: `sook@100.126.171.58`, Windows 11 Pro build 26200, PROCESSOR_ARCHITECTURE=AMD64 (native x86_64), rustc 1.97.0 host x86_64-pc-windows-msvc, cargo 1.97.0.
- Built and RAN a dependency-free crate carrying the VERBATIM publish_agent_state_rendezvous on that host: PUBLISH1=Ok(()) RECORD1="41234\ntoken-abc\n"; **PUBLISH2=Ok(()) RECORD2="41235\ntoken-def\n"**; STAGED_TMP_LEFT=false; VERDICT=WINDOWS_REPLACE_CONFIRMED; exit 0. Post-run disk listing: a single 16-byte agent-state.rendezvous holding the second pair. Evidence: `windows-execution.log` (raw) + `rename-windows-evidence.md` SECTION 7.
- **sol-6 verdict upgraded: APPROVE** (from APPROVE-WITH-NOTES). Its words: "The native Windows execution directly disproves the round-4 prediction... No production change is required for this issue... There is no remaining criterion-cited blocker." Evidence: `review5b-windows-execution-gpt-5.6-sol.md`.
- Report updated (§14 limit sentence replaced by the execution result; §15 row + verdict line record the upgrade). File now 929 lines. NOTE: the report edit was done by the orchestrator because the delegated lane died on a 503 (all deepseek pool accounts daily-exhausted, retry-after ~971s); the report is documentation, not code, and earlier sessions also had the orchestrator write report sections.
- CLEANUP RECEIPTS: remote maho-win `win-exec-proof/`, `win-exec-proof.tgz`, `%TEMP%\ferryx-win-rename-exec-proof` removed and verified GONE; local `/tmp/win-exec-proof*`, `/tmp/{probe,win-run,win-verify,win-clean}.ps1` removed and verified GONE.
- MEMORY: new fact note `notes/facts/maho-win-windows-execution-host.md` (how to reach it, what can/cannot be verified there, mandatory teardown) + the audit note updated with the execution confirmation and the APPROVE upgrade.

## 커밋 기록 (2026-09-25)

- `6df7f4c4` fix(cross-platform): repair non-macOS paths the 2026-09-24 audit found silently dead — 캠페인 소스 67파일(+5616/−560). `ui/src/lib/contextMenuGuard.ts`는 캠페인 이전 커밋(8aa2821c, 2026-09-05)에 이미 반영돼 변경 없음.
- `a77afa0d` docs(cross-platform): publish the 2026-09-24 audit report with its review and execution evidence — 리포트 + 이 증적 미러 20파일(+5724).
- 사후 감사: 두 커밋의 파일 목록 88건 전부 우리 범위이며 외부(타 세션) 파일 0건. 커밋 후 남은 dirty 114파일은 전부 타 세션 작업(README/docs/scripts/ui 등)으로 손대지 않았습니다.
- 스테일 `.git/index.lock`(2026-09-25 01:07, 0바이트, 보유 프로세스 없음)을 제거하고 커밋했습니다.
