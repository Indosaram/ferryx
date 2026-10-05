# Task 8 pass 3 — composed-candidate remote verification

Candidate (this pass): `21dea3c01d1ec423498bc48eb2d29107be75eddf`
Tree: `02488435b60cb416eebc377db7097d53b3a7e992`  Base: `d82b35e4`
Branch: `work/local-pane-liveness-completion-foundation`
Prior immutable candidate: `172baa874f5e320ef08f4ed1dc5f11b391898477` (pass 2, rejected)
Pass-2 predecessor: `5464da0dd65a7a6312d096229d2067ec02735c78`

STATUS: IN PROGRESS — verdicts are appended as each gate lands. Nothing below is inferred.

## Candidate freeze and attribution

- 31 dirty files were staged by explicit path (never `git add -A`) and inspected with
  `git diff --cached --stat` (31 files, +395/-183) before the commit.
- Attribution: backend 17 files, native 2, scripts 5, frontend 7. Every file carries a lane
  report or lane-owned diff as its provenance; the per-file table is in this file below.
- The three UI files omitted from the frontend final report were resolved as frontend-owned and
  included deliberately (see "Source-ownership findings").
- Lock hashes and the Ghostty submodule entry are unchanged from pass 2.

## Platform verdicts

| Gate | Mac | Windows | Linux |
| --- | --- | --- | --- |
| UI build | | | |
| Scoped split/lifecycle | | | |
| Exact QA runner | | | |
| Rust selectors + all-targets + full lib | | | |
| Full UI | | | |

## Gate table

| Exact command | Host | Native exit | Selected | Verdict | Asserted line |
| --- | --- | --- | --- | --- | --- |

## Per-file attribution (all 31 frozen files)

Evidence keys: **R3** = `task-8/round3-backend-repair-report.md` "Files touched by this session" list;
**REC** = `task-8/backend-reconciliation-2026-10-04.md` route table; **NAT** = native lane round-3 delivery
(completion-state.md); **FE** = frontend lane round-3 delivery (completion-state.md); **SCR** = scripts lane
round-3 delivery (completion-state.md); **DIFF** = the lane-owned diff itself, read in this pass.

| # | File | Lane | Evidence | Confidence |
| --- | --- | --- | --- | --- |
| 1 | scripts/lib/qa-scenarios/common-harness.mjs | scripts | SCR (digest baseDir) + DIFF | certain |
| 2 | scripts/lib/qa-scenarios/lifecycle-scenarios.mjs | scripts | SCR (4 lifecycle adapters use selected driver) + DIFF | certain |
| 3 | scripts/lib/qa-scenarios/native-driver.mjs | scripts | SCR (selectNativeDriver) + DIFF | certain |
| 4 | scripts/lib/qa-scenarios/split-scenarios.mjs | scripts | SCR (4 split adapters use selected driver) + DIFF | certain |
| 5 | scripts/qa/pane-liveness.test.mjs | scripts | SCR ("2 regression tests appended") + DIFF (digest + driver tests) | certain |
| 6 | src-tauri/Cargo.toml | backend | R3 (rust-version 1.89 for std file locking) | certain |
| 7 | src-tauri/src/daemon/client.rs | backend | R3 + REC (11 HandshakeOk fixtures) | certain |
| 8 | src-tauri/src/daemon/handover_transaction.rs | backend | R3 (Windows cfg gate removed) | certain |
| 9 | src-tauri/src/daemon/mod.rs | backend | R3 (module ungated on Windows) | certain |
| 10 | src-tauri/src/daemon/protocol.rs | backend | REC section A.2 (test-only local_split pattern; R3 had excluded this file, REC repaired it) | certain |
| 11 | src-tauri/src/daemon/server.rs | backend | R3 (HandoverManager import, HandshakeOk pattern) | certain |
| 12 | src-tauri/src/daemon/split_journal.rs | backend | R3 (fs2 removed, std lock) | certain |
| 13 | src-tauri/src/ipc/agents_reset_event_tests.rs | backend | R3 + REC (handshake fixture) | certain |
| 14 | src-tauri/src/ipc/error.rs | backend | R3 (Display for IpcErrorCode) | certain |
| 15 | src-tauri/src/ipc/file_link_tests.rs | backend | R3 + REC (incarnation, spawn fixtures) | certain |
| 16 | src-tauri/src/ipc/terminal.rs | backend | R3 + REC (paired-proxy incarnation: None) | certain |
| 17 | src-tauri/src/ipc/tests.rs | backend | R3 + REC (17 handshake, 12 spawn, 1 session-details fixtures) | certain |
| 18 | src-tauri/src/native_terminal/input.rs | native | NAT (unused var removed) + DIFF | certain |
| 19 | src-tauri/src/native_terminal/surface_host.rs | native | NAT (accessors, claim, append_receipt, new test) + DIFF | certain |
| 20 | src-tauri/src/remote/tests.rs | backend | R3 + REC (2 handshake, 2 session-details) | certain |
| 21 | src-tauri/src/remote/workspace_api/worktree_authority_tests.rs | backend | R3 + REC (2 local_split: None) | certain |
| 22 | src-tauri/src/terminal/pty.rs | backend | R3 (PathBuf -> str) | certain |
| 23 | src-tauri/src/terminal/service.rs | backend | R3 (mark_running inference) | certain |
| 24 | src-tauri/src/terminal/shell.rs | backend | R3 + REC (inline closures replacing the local binding) | certain |
| 25 | ui/src/App.pairedDaemon.test.tsx | frontend | FE (2nd spawn arg) + DIFF | certain |
| 26 | ui/src/App.remote.test.tsx | frontend | FE (updater mock preserves exports + getCurrentVersion) + DIFF | certain |
| 27 | ui/src/App.tsx | frontend | FE (HMR no fallback shell spawn) + DIFF | certain |
| 28 | ui/src/components/NativeTerminalPane.presentation.test.tsx | frontend | FE (seven-field detach tuple) + DIFF | certain |
| 29 | ui/src/lib/terminalTransport/tauriTransport.ts | frontend | ownership resolved in this pass (below) | certain |
| 30 | ui/src/lib/terminalTransport/types.ts | frontend | ownership resolved in this pass (below) | certain |
| 31 | ui/src/state/workspaceRestore.ts | frontend | ownership resolved in this pass (below) | certain |

No file is unattributed. No file belongs to a lane other than the one above, and no file from shared `main`
or from worktree `W` is present in the diff.

## Source-ownership findings

### The three UI files omitted from the frontend final report (included deliberately)

`ui/src/lib/terminalTransport/types.ts`, `ui/src/lib/terminalTransport/tauriTransport.ts` and
`ui/src/state/workspaceRestore.ts` carry the incarnation-propagation repair but are **not named** in the
frontend lane's round-3 report text in `completion-state.md`. They were nevertheless attributed and included,
on four independent pieces of evidence:

1. The frontend lane's own earlier owned-file list names "types, tauri" as frontend-owned files
   (completion-state.md, plv-frontend delivery entry).
2. `ui/src/state/workspaceStore.ts:244` declares `live: Map<string, { daemonEpoch; running; incarnation? }>`
   and `:2678-2695` compares `liveInfo.incarnation` against `session.incarnation`. That file is
   **frontend-owned and already committed** in 172baa87. The transport/restore files are the only producers of
   those `incarnation` values, so dropping them would leave a committed consumer permanently reading
   `undefined` — the three files are the other half of an already-landed frontend change.
3. File mtimes 11:11:27 / 11:11:28 / 11:11:43 sit inside the frontend lane's round-3 write window
   (11:05:56-11:14:32), interleaved with its other four files.
4. The committed HEAD test `ui/src/App.test.tsx:2663/2701` already feeds `incarnation: "replacement-pty"`
   and `"old-pty"` into the frontend reconciliation fixtures, so the field is part of the frontend contract.

The omission is a **documentation finding** against the frontend lane report, not an ownership ambiguity.
Sweeping them in blind was not the reason for inclusion; the dependency on committed frontend code was.

### `ui/src/App.test.tsx` fixture repair — present, not missing

The reported fixture repair **is** on disk, committed in `172baa87` (not dirty because nothing modified it
afterwards). Verified hunks: `getAccountEnrollmentStatus` mock added; `listTerminalSessions` fixture gains
`incarnation: "replacement-pty"`; the stale-session fixture gains `incarnation: "old-pty"`; and the HMR test
was renamed to "reconciles missing HMR sessions without automatically spawning replacements" asserting
`ensureSessionBackends`/`spawnTerminal` are **not** called.

## Rust gate verdicts — NOT_RUN_BLOCKED by one candidate-branch compile error

Exactly one compiler error exists on every platform (pass 2 had 70/72/25). It is
`error[E0505]` at `src/native_terminal/surface_host.rs:989`; the full diagnostic and the one-line fix are in
`blocking-e0505-repair-brief.md`. Because it blocks compilation of the crate itself (and of the production
`ferryx` lib, per the two handover rows), every dependent Rust gate is recorded `NOT_RUN_BLOCKED` with the
causal log — deliberately not repeated 34 times as in pass 2.

| Exact command | Host | Native exit | Selected | Verdict | Causal log |
| --- | --- | --- | --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | mac | 101 | — | NOT_RUN_BLOCKED (1 err, 51 warn) | logs/local_split_reliability_-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | mac | 101 | — | NOT_RUN_BLOCKED (1 err, 53 warn) | logs/qa_barrier-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | mac | 101 | — | NOT_RUN_BLOCKED (1 err, 32 warn; **lib**, not lib test) | logs/handover-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | mac | — | — | NOT_RUN_BLOCKED | same |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | linux | 101 | — | NOT_RUN_BLOCKED (1 err, 47 warn) | logs/local_split_reliability_-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | windows | 101 | — | NOT_RUN_BLOCKED (1 err, 43 warn) | logs/local_split_reliability_-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | windows | 101 | — | NOT_RUN_BLOCKED (1 err, 45 warn) | logs/qa_barrier-list.log |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | windows | 101 | — | NOT_RUN_BLOCKED (1 err, 17 warn; **lib**) | logs/handover-list.log |

The four required selectors therefore have **no** confirmed nonzero selected count — not because the naming
is wrong, but because the crate does not compile. That is a hole in this pass's evidence, reported as such.

## UI gate verdicts — PASS on all three hosts

| Exact command | Host | Native exit | Selected | Verdict | Asserted line |
| --- | --- | --- | --- | --- | --- |
| `bun run --cwd ui build` | mac | 0 | N/A | RAN_PASSED | `✓ built in 11.57s` |
| `bun run --cwd ui build` | linux | 0 | N/A | RAN_PASSED | `✓ built in 5.15s` |
| `bun run --cwd ui build` | windows | 0 | N/A | RAN_PASSED | `✓ built in 5.78s` |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | mac | 0 | 19 | RAN_PASSED | `Tests  19 passed (19)` |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | linux | 0 | 19 | RAN_PASSED | `Tests  19 passed (19)` |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | windows | 0 | 19 | RAN_PASSED | `Tests  19 passed (19)` |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | mac | 0 | 112 | RAN_PASSED | `Test Files  4 passed (4); Tests  112 passed (112)` |
| (same lifecycle command) | linux | 0 | 112 | RAN_PASSED | `Test Files  4 passed (4); Tests  112 passed (112)` |
| (same lifecycle command) | windows | 0 | 112 | RAN_PASSED | `Test Files  4 passed (4); Tests  112 passed (112)` |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | mac | 0 | 28 | RAN_PASSED | `Test Files  1 passed (1); Tests  28 passed (28)` |
| (same runner command) | linux | 0 | 28 | RAN_PASSED | `Test Files  1 passed (1); Tests  28 passed (28)` |
| (same runner command) | windows | 0 | 28 | RAN_PASSED | `Test Files  1 passed (1); Tests  28 passed (28)` |

### Runner gate: pass-2 defects closed by observation

Pass 2 recorded the exact runner as `native1; 5 failed / 21 passed (26 selected)` on linux and windows, and
`NOT_RUN GUI_BOUNDARY` on mac. Pass 3 records **28 selected, 28 passed, native 0** on all three hosts. The
five tests that failed in pass 2 were:

- `regression H6 source digest: evidence pointer binds to runner bytes` (cwd-relative digest path ENOENT)
- `split-cancel scenario adapter ...`
- `split-attach-stall asserts actionable failure ...`
- `retained-handover and handover-abort adapters ...`
- `suspension-ownership and stale-binding adapters ...`

The last four failed because the adapters dispatched every non-win32 preflight to the real Darwin driver
(osascript ENOENT on linux/windows; real focus/click on mac, which is why the mac gate was refused). All five
are now inside the passing 28. The mac gate runs because `selectNativeDriver` returns the no-op driver for
mock/linux/undefined preflights.

**Scope of this gate (declared, not blurred):** the QA runner command is adapter-level unit evidence. It is
NOT native proof. Task 9 still owns the real native matrix; nothing here substitutes for it.

## Full UI suite — Mac (COMPLETE; answers the pass-2 open question)

`bun run --cwd ui test` on mac: **native exit 1, 6521 selected, Tests 192 failed / 6329 passed,
Test Files 7 failed / 365 passed (372)**. The run **finished on its own** well inside the 2400 s deadline —
no timeout, no SIGKILL. Pass 2 could not answer this: it TIMED_OUT at 1200 s with unknown final count and no
identified active test. The 30 s heartbeat (naming the last started test file) was armed but never needed.

| Failed file (mac) | Classification | Note |
| --- | --- | --- |
| `src/lib/terminalTransport/terminalTransport.test.ts` | **CANDIDATE_CAUSED (A/B proven)** | NEW vs pass 2 — see below |
| `src/App.test.tsx` | AB_CONFIRMED_PRE_EXISTING | base 154 tests / 1 failed, same test name; candidate identical |
| `src/components/NativeTerminalPane.test.tsx` | AB_CONFIRMED_PRE_EXISTING | pass-2 classification |
| `src/components/TerminalSearchOverlay.test.tsx` | AB_CONFIRMED_PRE_EXISTING (protected) | out of scope |
| `src/lib/updater.test.ts` | AB_CONFIRMED_PRE_EXISTING (protected) | out of scope |
| `src/components/TerminalSplitView.paneHandleReach.test.tsx` | AB_CONFIRMED_PRE_EXISTING (mandatory) | protected, not repaired |
| `src/lib/pairedDaemonRollout.test.ts` | AB_CONFIRMED_PRE_EXISTING (mandatory) | protected, not repaired |

Failures that pass 2 recorded and pass 3 **no longer** has: `App.remote.test.tsx`, `App.pairedDaemon.test.tsx`,
`NativeTerminalPane.presentation.test.tsx`, `SettingsDialog.cli.test.tsx` — the frontend round-3 repair closed them.

### Candidate-caused regression introduced by the three omitted UI files (A/B proven)

`src/lib/terminalTransport/terminalTransport.test.ts` > "TauriTerminalTransport listSessions queries tauri
listTerminalSessions" — `AssertionError: expected [...] to deeply equal [...]`, the received objects carrying
an extra `incarnation: null` key.

| Run | Command | Result |
| --- | --- | --- |
| base `d82b35e4` | `bun run --cwd ui test src/lib/terminalTransport/terminalTransport.test.ts` | 11 passed (11), native **0** |
| candidate `21dea3c0` | same | 1 failed \| 10 passed (11), native **1** |

Cause: `ui/src/lib/terminalTransport/tauriTransport.ts` adds `incarnation: s.incarnation ?? null` to the
`listSessions` objects, while the test at `:88` asserts exact deep equality on
`{sessionId, worktreePath, daemonEpoch, running}`. The test file itself is **unchanged** since ancestor
`72d7523c`. This is the concrete cost of including the three UI files the frontend lane report omitted:
the propagation is needed by the committed `workspaceStore.ts` consumer, but the sibling caller test was not
updated. **Routing: frontend lane** — either add `incarnation: null` to the expected objects at
`terminalTransport.test.ts:88`, or drop the field. The verifier edited neither.

### Candidate-introduced unhandled error (no extra test failure)

`[vitest] No "enrollThisMachine" export is defined on the "./lib/tauri" mock` — 6 occurrences on the candidate,
0 at base, 0 in every pass-1/pass-2 full-UI log. `App.test.tsx`'s `./lib/tauri` mock gained
`getAccountEnrollmentStatus` in `172baa87` but not `enrollThisMachine`, so `AccountStep.tsx:28` (reached via
`WelcomeWizard.tsx:10`) now throws. Test counts are identical to base (154 tests / 1 failed), so it adds no
failure — reported for the frontend lane.

## Full UI suite — all three hosts (complete, no timeout anywhere)

| Host | Command | Native exit | Selected | Tests | Test files | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| mac | `bun run --cwd ui test` | 1 | 6521 | 192 failed / 6329 passed | 7 failed / 365 passed (372) | RAN_FAILED (baseline-dominated) |
| windows | `bun run --cwd ui test` | 1 | 6521 | 193 failed / 6323 passed / 5 skipped | 9 failed / 363 passed (372) | RAN_FAILED (baseline-dominated) |
| linux | `bun run --cwd ui test` | 1 | 6521 | 1224 failed / 5297 passed | 99 failed / 273 passed (372) | RAN_FAILED (baseline-dominated) |

**No host timed out.** The mac suite finished on its own inside the 2400 s deadline — pass 2's 1200 s
SIGKILL timeout did not recur. Recorded as a fact; no root cause is claimed for the pass-2 timeout.

### Pass-3 vs pass-2 failure-file classification (`pass3-failure-classification.json`)

| Host | Failed files | Known from pass 2 | **New in pass 3** | Fixed vs pass 2 |
| --- | --- | --- | --- | --- |
| mac | 7 | 6 | `src/lib/terminalTransport/terminalTransport.test.ts` | App.remote, App.pairedDaemon, NativeTerminalPane.presentation, SettingsDialog.cli |
| windows | 9 | 8 | `src/lib/terminalTransport/terminalTransport.test.ts` | App.pairedDaemon, App.remote, NativeTerminalPane.presentation |
| linux | 99 | 98 | `src/lib/terminalTransport/terminalTransport.test.ts` | NativeTerminalPane.presentation |

**The only regression in the entire full-UI set, on every platform, was the transport fixture** — and it is
repaired in `7c0fed30`, verified by re-running exactly that suite: mac 11/11, linux 11/11, windows 11/11
(pre-repair candidate: 1 failed | 10 passed on all three).

Protected and mandatory baseline files continue to fail exactly as pass 2 recorded and were **not** repaired:
`TerminalSearchOverlay.test.tsx`, `updater.test.ts` (protected/out of scope) and
`TerminalSplitView.paneHandleReach.test.tsx`, `pairedDaemonRollout.test.ts` (A/B-confirmed pre-existing).

### Repair commits frozen for this pass

| Commit | Subject | Scope |
| --- | --- | --- |
| `764934a4` | `fix(native): clone the completion window before moving it into the render closure` | `surface_host.rs`, 2 insertions / 1 deletion |
| `7c0fed30` | `test(ui): bind the transport listSessions fixture to the incarnation contract` | `terminalTransport.test.ts`, 3 insertions / 2 deletions |

Both were staged by explicit path with the staged diff inspected before each commit, and are kept separate as
instructed. Candidate for the Rust half is therefore `7c0fed30` (tree `c4f62a6c`) applied as a two-patch delta
onto the already-staged pass-3 tree, so every UI byte and the built `ui/dist` are unchanged and no UI scope
was re-run.

## Rust half on the repaired candidate (`764934a4` + `7c0fed30`)

The two-line E0505 repair is **proven by compilation**: for the first time in this effort the Rust selectors
compile. Pass 1, pass 2 and the pre-repair prefix of pass 3 all exited 101 without selecting a single case.

### mac (complete)

| Exact command | Native exit | Selected | Result | Verdict |
| --- | --- | --- | --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | 0 | **15** | selectors listed | RAN_PASSED |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | 101 | — | `12 passed; 3 failed; 2675 filtered out` | RAN_FAILED (in-scope) |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | 101 | — | `46 passed; 1 failed; 2643 filtered out` | RAN_FAILED (in-scope) |

Four failures, all in **candidate-authored** tests, so no baseline A/B is possible or meaningful — no base
version of these tests exists. Routing is to the lane that authored each:

| Test | Panic site | Message |
| --- | --- | --- |
| `daemon::client::local_split_transport_tests::local_split_reliability_isolated_lifecycle_bypasses_general_slot` | `src/daemon/client.rs:6985:13` | `assertion failed: matches!(read_request(&mut connection).await, DaemonRequest::Spawn ...` |
| `daemon::client::local_split_transport_tests::local_split_reliability_lost_create_reply_is_ambiguous_without_retry` | `src/daemon/client.rs:7271:13` | same `DaemonRequest::Spawn` pattern assertion |
| `daemon::handover::spawn_owner_tests::local_split_reliability_handover_private_prepare_after_publication` | `src/daemon/handover.rs:477:9` | `assertion failed: legacy.starts_with(root.path())` |
| `native_terminal::surface_host::tests::pane_liveness_native_binding_host_accessors_discard_detached_presentation` | `src/native_terminal/surface_host.rs:5448:26` | `unexpected acquisition / retry loop` (the test's scripted acquisition queue is exhausted) |

The first two assert that the first request read from the mock connection is `DaemonRequest::Spawn`, so the
fixture's request sequence is the first thing for the backend owner to check; the third is a jail-root path
prefix assertion; the fourth is a fixture-count mismatch in the native lane's own new test.

### Windows-only Rust failures: 5 `split_journal` tests fail on filename validation

Windows `local_split_reliability_` reported **7 passed / 7 failed** where mac and linux reported 12 passed / 3 failed.
The five extra Windows failures are all `daemon::split_journal::tests::local_split_reliability_*`, panicking with
`Io(Os { code: 123, kind: InvalidFilename })` (**ERROR_INVALID_NAME**) at `split_journal.rs:283, :292, :307, :324, :338`.

Root cause (`src-tauri/src/daemon/split_journal.rs:208-212`):

```rust
let temp = self.dir.join(format!(
    ".{JOURNAL_FILE}.tmp-{}-{}",
    std::process::id(),
    std::thread::current().name().unwrap_or("writer"),   // <- test thread name is the full test path
));
```

Rust's test harness names the thread after the test path
(`daemon::split_journal::tests::local_split_reliability_round_trips_entries_after_reopen`), which contains `::`.
A colon is illegal in Windows filenames, so the atomic write's `OpenOptions::open` fails there; mac and linux
permit it. This is a cross-platform defect in the candidate's new journal, not a lock problem
(`File::lock` is portable). Routing: backend/journal lane.

## Candidate chain for this pass

| Commit | Subject | Scope |
| --- | --- | --- |
| `21dea3c0` | round-3 backend/native/frontend/scripts repairs | 31 files, +395/−183 — **UI source of record** |
| `764934a4` | `fix(native): clone the completion window before moving it into the render closure` | surface_host.rs +2/−1 (E0505) |
| `7c0fed30` | `test(ui): bind the transport listSessions fixture to the incarnation contract` | test file +3/−2 (A/B-proven regression) |
| `17295029` | `test(native): script one acquisition for the presentation accessor test` | surface_host.rs test +28/−2 |
| `b0ed4bef` | `test(daemon): reconcile the create stage before asserting the spawn write` | client.rs +28/−1, handover.rs +15/−1 |
| `d97233c1` | `test(daemon): complete the integration fixtures for the new wire fields` | 3 integration targets +9 |

Each commit was staged by explicit path and its staged diff inspected before committing; the tree is clean
at `d97233c1` (tree `90acfc27`). The Rust re-run uses the **same staged trees as the UI runs** (UI bytes
unchanged, `ui/dist` from `21dea3c0` retained) with only these six files replaced, verified by sha256 on
every host — so no UI scope is re-run and the UI evidence stays bound to `21dea3c0`.

## Rust re-run on `d97233c1` (delta applied to the `21dea3c0` trees; UI bytes unchanged)

The delta is the six changed files at `d97233c1`, pushed into each host's existing staged tree and
sha256-verified there, so `ui/dist` and every UI source byte remain the ones the UI gates ran against.
No UI scope was re-run.

| Gate | mac | linux | windows |
| --- | --- | --- | --- |
| `--lib local_split_reliability_ -- --list` | exit 0, **15 selected** | exit 0, **15 selected** | exit 0, **14 selected** |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **15 passed / 0 failed** | **15 passed / 0 failed** | 9 passed / 5 failed |
| `--lib pane_liveness_ -- --list` | exit 0, 47 selected | exit 0, 47 selected | exit 0, 43 selected |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | **47 passed / 0 failed** | **47 passed / 0 failed** | **43 passed / 0 failed** |
| `cargo check --all-targets` | 101 — 2 sites | 101 — 2 sites | 101 — 2 sites |

**Proven by execution:** the daemon create-stage reconcile fix (`b0ed4bef`) turns
`local_split_reliability_` from 12/3 into 15/0 on mac and linux, and the native acquisition-script fix
(`17295029`) turns `pane_liveness_` from 46/1 into 47/0 on mac and linux and 43/0 on windows.

### Remaining Windows-only failures (5) — the `::` filename defect, not yet repaired

`daemon::split_journal::tests::local_split_reliability_*` fail only on Windows with
`Io(Os { code: 123, kind: InvalidFilename })` at `split_journal.rs:283, :292, :307, :324, :338`. Root cause is
recorded above: the atomic-write temp name embeds `std::thread::current().name()`, which is the test path
containing `::`, and a colon is illegal in Windows filenames. Routing: backend/journal lane.

### `all-targets`: two remaining missing-field sites (definitive compiler sweep)

A fresh `cargo check --all-targets` on the delta lists exactly two E0063 sites, both in targets the fixture
worker did not cover:

| Site | Missing |
| --- | --- |
| `src-tauri/examples/ssh_password_fixture.rs:82` (`DaemonRequest::Spawn`) | `local_split` |
| `src-tauri/tests/zero_config_gen5_regression.rs:195` (`DaemonResponse::HandshakeOk`) | `capabilities`, `admission_time_unix_ms` |

(`ssh_password_fixture.rs:40` is a match pattern using `..`, so it is fine.) The three targets the worker
fixed now compile; these two remain. Routing: the same fixture worker.

---

# Pass 4 — resumed verification after the `/Volumes/T9-Mac` remount

Candidate: **`abd9e890`** (`6c69715f` + the split-journal portable-name fix) — tree clean.
Base: `d82b35e4`. All commands below ran on the named host; nothing ran on the control Mac.

## Re-attachment

The previous verifier's local PTY sessions (`bash_25` mac, `bash_26` linux, `bash_29` windows,
`bash_10`, `bash_11`) and every monitor were **gone** — the volume unmount killed them. All three
remote hosts were reachable and **no runner or cargo process from the pass-3 runs was alive**, so
nothing was duplicated; the channels were simply re-established over ssh. The host staging trees are
plain `tar` extractions (not git repos), so the verifier pushes a delta built with
`git archive abd9e890 <paths>` and sha256-verifies it on each host before running.

## New commit frozen

| Commit | Subject | Scope |
| --- | --- | --- |
| `abd9e890` | `fix(daemon): make the split journal temp name portable on Windows` | `split_journal.rs` +77/−5 |

Authored by worker `st_01a105fb`; the verifier inspected the staged diff by direct read before
accepting it. The atomic-write contract is unchanged (same directory, `create_new`, `write_all`,
`sync_all`, `fs::rename` onto the journal path, unix mode 0600, unix directory fsync, temp removal on
error); only the name construction changed — pid + a process-wide `AtomicU64` counter for uniqueness,
with the thread name reduced to a sanitized 48-char debugging tag.

## Gates re-run at abd9e890

| Exact command | mac | linux | windows |
| --- | --- | --- | --- |
| `--lib local_split_reliability_ -- --list` | 0, 15 selected | 0, 15 selected | 0, 14 selected |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | **0, 15 passed / 0 failed** | **0, 15 passed / 0 failed** | **0, 14 passed / 0 failed** |
| `--lib pane_liveness_ -- --list` | 0, 47 selected | 0, 47 selected | 0, 43 selected |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | **0, 47 passed / 0 failed** | **0, 47 passed / 0 failed** | **0, 43 passed / 0 failed** |
| `cargo check --all-targets` | **101** | **101** | **0** |
| `--test daemon_handover_transfer_contract -- --list` | **101** | **101** | 0, 0 selected (`#![cfg(unix)]`) |
| `--test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | NOT_RUN_BLOCKED | NOT_RUN_BLOCKED | 0, 0 passed / 0 failed |
| `--lib split_journal -- --list` / run | — | 0, 7 / **7 passed** | 0, 7 / **7 passed** |

**The five Windows `Io(123, InvalidFilename)` failures are gone** — the Windows split count went from
9 passed / 5 failed to 14 passed / 0 failed, native exit 0.

**Selected count is 14/15/15, not the predicted 16/16/15.** The worker's new test
`local_split_temp_names_are_portable_bounded_and_unique` lives in `daemon::split_journal::tests::` and
therefore does not match the `local_split_reliability_` filter. Proven directly with the
`split_journal` filter instead: 7 selected and 7 passed on **both** linux and windows, the new test
name present and `ok` on both.

**Mutation proof of the new test** (linux): reverting the naming to the pre-fix
`thread_name.unwrap_or("writer").to_string()` gives `mutated_exit=101` with a panic at
`split_journal.rs:421` (RED); restoring the file (sha256 back to `412870e8…`) gives exit 0 (GREEN).

## `all-targets` at abd9e890 — two sites remain, and `--all-targets` under-reports

mac and linux both exit 101 at exactly two E0063 sites:

```
error[E0063]: missing field `local_split` in initializer of `DaemonRequest`
   --> tests/daemon_handover_transfer_contract.rs:109:28

error[E0063]: missing fields `create_only`, `prepared_local_split` and `remaining_ms` in initializer of `SpawnTerminalRequest`
   --> tests/ipc_hardening_contract.rs:110:9
```

`SpawnTerminalRequest` gained those three fields at `src-tauri/src/ipc/terminal.rs:490-495`;
`ipc_hardening_contract.rs` is **not in the candidate diff** (unchanged since base), so the break is
purely the widened type. `daemon_handover_transfer_contract.rs` *is* in the diff but only its
`run_v5_handover_case` assertions were extended; its `Spawn` constructor at :109 was not updated.

**Finding: one `--all-targets` run does not list every broken target.** Cargo stops scheduling new
targets once one fails, so the reported site set depends on which target failed first — the pass-3 log
showed 4 sites in three *other* files, this run shows 2. A definitive answer required a per-target
sweep (`linux/rustC-targets.log`): **81 test targets → 79 OK / 2 FAIL**, **12 examples → 12 OK**
(including `ssh_password_fixture`, which the worker repaired). The repo holds 81
`src-tauri/tests/*.rs` and 12 `src-tauri/examples/*.rs` files, so the sweep covered every target.

Pass 3's two recorded sites (`examples/ssh_password_fixture.rs:82`,
`tests/zero_config_gen5_regression.rs:195`) are now **fixed**, as are
`daemon_handover_contract.rs`, `daemon_persistence_contract.rs` (×2) and
`zero_config_gen4_audit.rs`. The two sites blocking now are a different pair the worker did not reach.

## Linux-only gates re-run at abd9e890

| Gate | Result |
| --- | --- |
| `--lib --features local-split-qa qa_barrier` | 0, 11 selected, **11 passed / 0 failed** |
| `--test daemon_handover_contract` | 0, 5 selected, **5 passed / 0 failed** |
| `--test daemon_persistence_contract` | **101**, 15 selected, 14 passed / **1 failed** |
| `--lib suspension` | 0, 12 selected, **12 passed / 0 failed** |
| `--test zero_config_gen4_audit` | 0, 3 selected, **0 passed / 0 failed / 3 ignored** |
| `--test zero_config_gen5_regression` | **101**, 6 selected, 2 passed / **4 failed** (A/B-proven pre-existing) |
| `--test ipc_hardening_contract` | **101** — E0063 compile failure |

### `daemon_persistence_contract` — the single failure, verbatim

```
thread 'test_daemon_output_sequence_contiguity_and_replay_gap' (1638024) panicked at tests/daemon_persistence_contract.rs:1098:9:
assertion `left == right` failed
  left: 0
 right: 1
test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.97s
```

The two other panics in that log (`:669` "injected harness owner panic", `:698` "exercise harness
unwind cleanup") are intentional and those tests report `ok`.

### `daemon_handover_transfer_contract` list gate — the compile failure, verbatim

```
error[E0063]: missing field `local_split` in initializer of `DaemonRequest`
   --> tests/daemon_handover_transfer_contract.rs:109:28
    |
109 |             .send_request(&DaemonRequest::Spawn {
    |                            ^^^^^^^^^^^^^^^^^^^^ missing `local_split`

For more information about this error, try `rustc --explain E0063`.
error: could not compile `ferryx` (test "daemon_handover_transfer_contract") due to 1 previous error
```

The dependent run gate is `NOT_RUN_BLOCKED` (prerequisite compile/list gate failed) — never a pass.

### `zero_config_gen5_regression` — 4 failures, A/B-proven PRE-EXISTING

Candidate linux: `FAILED. 2 passed; 4 failed` (31.10 s). Base `d82b35e4` linux:
`FAILED. 2 passed; 4 failed` (29.50 s) — **the same four test names with the same messages**:

- `cli_refusal_exits_without_replacing_owner_and_original_pin_redeems` (base :230:51, cand :233:51) —
  `Elapsed(())`
- `cli_success_uses_daemon_pin_and_leaves_it_redeemable_after_exit` (base :250:51, cand :253:51) —
  `Elapsed(())`
- `no_daemon_socket_fails_missing_relay_url_without_spawning_daemon` (base :271:5, cand :274:5) —
  `ACCOUNT_LOGIN_REQUIRED: PIN issuance was retired…`
- `stopped_relay_then_off_allows_real_daemon_local_view_pairing` (base :147:18 = cand :147:18) —
  `Relay is unreachable or registration failed: Timed out waiting for relay registration ACK`

The line numbers differ by 3 because the candidate inserted 3 fixture lines. Evidence:
`linux/logs/gen5-base.log`.

## Full `--lib` classification

| Host | Candidate | Base d82b35e4 | candidate-caused | pre-existing | flaky | unclassified |
| --- | --- | --- | --- | --- | --- | --- |
| linux | 2629 passed / **37 failed** / 6 ignored | 2554 passed / **31 failed** / 6 ignored (1 skipped) | **6** | 30 | 1 | **0** |
| mac | 2635 passed / **49 failed** / 6 ignored | 2557 passed / **44 failed** / 6 ignored (2 skipped) | **6** | 43 | 0 | **0** |
| windows | 2201 passed / **70 failed** / 8 ignored | bounded at 2174/2280 tests | **5** | 65 | 0 | **0** |

**The mac and linux candidate-caused sets are exactly the same six tests** (set comparison: nothing
mac-only, nothing linux-only), and all six were proven deterministic with a two-repetition isolated A/B
per side on linux (`linux/ab-classify2.log`, `linux/logs/ab/`, 32 runs).

### Windows candidate full-lib (re-run — the pass-3 log had been truncated)

`test result: FAILED. 2201 passed; 70 failed; 8 ignored; 0 measured; 1 filtered out; finished in 1095.61s`
(`windows/logs/full-lib.log`, `windows/rustB-windows.log`). The `1 filtered out` is the
`ssh_bridge_live_loopback_openssh_connection` test that hung; it is reported NOT_RUN, not a pass.

### linux candidate-caused (6) — each passes at base and fails at candidate, 2 reps each side

| Test | Panic site | Message |
| --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | `src/ipc/tests.rs:313:6` | `UnsupportedCapability: "Attach requires the persisted seven-field pane binding; pass attachTuple"` |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | `src/ipc/tests.rs:3426:6` | same |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | `surface_host.rs:5961:9` | `assertion failed: receipt.presented` |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | `surface_host.rs:6183:13` | `assertion failed: latest.presented` |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | `surface_host.rs:6269:17` | `assertion failed: result.unwrap().presented` |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | `surface_host.rs:6072:9` | `assertion failed: receipt.presented` |

All four `surface_host` test names exist at base (`git grep` = 1 each) and `surface_host.rs` grew
+1373/−49 in the candidate, so the break is the candidate's, not a renamed or absent test.

### flaky — must not be used as an A/B oracle without repetition

`remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`: base
[PASS, FAIL] vs candidate [FAIL, PASS]. Nondeterministic on **both** sides, so it is **not**
candidate-caused; it is a flaky test for the owning lane. A single-run A/B would have mislabelled it —
the two-rep repeat is what caught it.

## Hangs found (reported, never suppressed)

| Host / side | Hung at | Treatment |
| --- | --- | --- |
| linux base | `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution` | SIGTERM after 1036 tests; log kept as `linux/logs/full-lib-base-HUNG.log`; rerun with `--skip` |
| mac base | the same test | SIGTERM after 1035 tests; `mac/logs/full-lib-base-HUNG.log`; rerun with `--skip` |
| mac base (2nd) | `remote::machine_operation_journal::contention_tests::journal_mutation_deadline_retains_admission_until_worker_drains` | SIGTERM after 1707 tests; `mac/logs/full-lib-base-HUNG2.log`; both now skipped |
| windows candidate | `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection` | killed only this session's pids; `windows/logs/full-lib-HUNG.log`; rerun with `--skip` |

The test that hangs at base on linux **and** mac **passes at the candidate on both**; the test that
hangs on Windows **fails** (does not hang) on linux and mac at the candidate with
`bridge_tests.rs:759 "Loopback SSH must be available for live OpenSSH bridge test"`. Both hangs are
therefore environment stalls, not candidate logic — and the skipped tests are reported as NOT_RUN, not
as passes.

## Interventions this session (full disclosure)

- **mac `df` was 127 MiB free (100%)** — the first priority run aborted with "No space left on device"
  for every gate. Reclaimed only this effort's superseded staging (`source` 5.1G,
  `task8-172baa87`, `task8-5464da0d`) → 5.2 GiB, then purged `source-21dea3c0/target` (17G, a
  regenerable build artifact) → 18 GiB. Receipt: `mac/DISK-RECLAIM-RECEIPT.md`. Nothing outside
  `/Users/I552267/ferryx-pane-completion` was touched.
- A naive `tar | tar` tree copy was filling the disk (375 MB into 1.3 GB free); aborted and replaced
  with `cp -al` hardlink cloning (608 MB, instant). Candidate tree integrity re-verified by sha256
  **after** the overlay: `split_journal.rs`, `protocol.rs`, `ipc/terminal.rs`, `ipc/tests.rs` all
  match `abd9e890` exactly, so hardlink aliasing did not corrupt the candidate.
- **windows `C:` free was 17.6 GB** with `source-21dea3c0` at 57.95 GB — reclaimed the superseded
  `ferryx-pane-completion\source` (24.21 GB) → 40.4 GB. Receipt:
  `windows/DISK-RECLAIM-RECEIPT.md`. The eight long-lived `cargo.exe` processes whose parents are
  `rustup.exe` and which are not descendants of this dispatch's runner were left untouched.

## Verification pitfalls this session cost me (for the next verifier)

1. PowerShell over `ssh -Command` mangles quotes — always ship a `.ps1` and invoke
   `powershell.exe -NoProfile -File`.
2. cargo global options must follow the subcommand: `cargo test --manifest-path …`, never
   `cargo --manifest-path … test …`.
3. A monitor sentinel must not live in a log a previous attempt already wrote — a stale
   `ALL_RUSTB_DONE` fired a **false completion**. Use a fresh per-run sentinel file.
4. `grep -m1 '^test result:'` picks the *first* result line, which in a multi-harness run is a
   per-harness line, not the suite total. Use the last, or the `FAILED. N passed; M failed` line.
5. An A/B script must give each (side, rep, filter) its **own** log file; appending them together makes
   the exit code and the reported result line disagree and silently over-claims.
6. A single-run A/B over-claims on flaky tests — always repeat.
7. `--all-targets` under-reports broken targets — sweep targets one at a time.
8. `cp -al` hardlink-clones instantly but **aliases** files: safe only if you then rm+rewrite every
   changed path, and verify the other tree's hashes afterwards.

## Windows full-lib classification (pass 4)

The Windows candidate full-lib was re-run (the pass-3 log had been truncated by the volume loss):
`test result: FAILED. 2201 passed; 70 failed; 8 ignored; 0 measured; 1 filtered out; finished in 1095.61s`.
The `1 filtered out` is `ssh_bridge_live_loopback_openssh_connection`, which **hung** and was killed —
reported NOT_RUN, never a pass.

The base `d82b35e4` full-lib was run against a properly rebuilt base tree but was **bounded at 2174 of
2280 tests** because it was spinning on `worktree::disk_tests::disk_scan_plain_folder_has_no_worktrees`
— the same test the candidate run also spun on for ~6 minutes. Partial log:
`windows/logs/full-lib-base-PARTIAL.log`.

**Classification (strict rule).** `pre-existing` requires an explicit `test NAME ... FAILED` line at
base; `candidate-caused` requires an explicit `test NAME ... ok` line; anything else stays
`unclassified`. The rule is load-bearing — **9 tests had started at base with no verdict line** when
the run was bounded, so a looser rule would have silently mislabelled them.

| Classification | Count |
| --- | --- |
| candidate-caused | **5** |
| pre-existing | **65** |
| unclassified | **0** |
| **total** | **70** |

The one test the base full run never reached,
`worktree::tests::plain_folder_without_git_registers_and_guards_worktrees`, was resolved by a
two-repetition scoped A/B on **both** sides: it is `FAILED. 0 passed; 1 failed` on base **and**
candidate in both reps (`windows/ab2-windows.log`), so it is pre-existing. Its base panic is another
Windows long-path defect — `assertion left == right`, `left: "\\\\?\\C:\\Users\\sook"` vs
`right: "\\\\?\\C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpr6fraO"` at `src/worktree/mod.rs:49`.

### Windows candidate-caused (5)

| Test | base | candidate |
| --- | --- | --- |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | ok | FAILED |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | ok | FAILED |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | ok | FAILED |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | ok | FAILED |
| `daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent` | ok | FAILED (`Other("Timed out waiting for PTY reader shutdown")` at `session_service_machine_tests.rs:344:63`) |

### Cross-platform candidate-caused set

The **four `surface_host` presentation failures are candidate-caused on all three hosts**. The two
`ipc::tests` attach failures are candidate-caused on mac and linux and **cannot exist on Windows**:
`src-tauri/src/ipc/mod.rs:57-58` gates the whole module behind `#[cfg(all(test, unix))]` (verified —
0 grep hits for those names in the Windows log, 4 hits each in the mac log).

---

# Pass 5 — verification at `39e722ce` (repair commits)

Authoritative file: **`FINAL-VERDICT-39e722ce.md`**. Candidate bytes sha256-verified on all three hosts
before any run.

## State changes proven by execution

| Test / gate | `abd9e890` | `39e722ce` (mac / linux / windows) |
| --- | --- | --- |
| `cargo check --all-targets` | 101 (mac, linux) | **0 / 0 / 0** |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | FAILED | FAILED(mac, env-shaped) / **PASS** / n/a (`cfg(unix)`) |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | FAILED | **PASS / PASS** / n/a |
| the four `native_terminal::surface_host::tests::*` presentation tests | FAILED ×3 hosts | **PASS / PASS / PASS** |
| `pane_liveness_` | 47/1F (mac), 47/0 (linux), 43/0 (win) | **47/0 / 47/0 / 43/0** |
| `local_split_reliability_` | 15/0 / 15/0 / 14/0 | **15/0 / 15/0 / 14/0** |

Two NEW tests pass: `paired_session_incarnation_is_stable_per_actor_and_distinct_across_reincarnation`
and `paired_describe_reports_the_incarnation_the_attach_fence_proves` (mac + linux).

## linux full `--lib` delta

`abd9e890` `2629 passed; 37 failed; 6 ignored` → `39e722ce` `2638 passed; 31 failed; 6 ignored`.
**7 left** (the six candidate-caused + the previously-flaky `input_is_cancelled_when_grant_is_revoked`),
**1 joined** (`terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread`),
30 still failing — and set-comparing those 30 against the `abd9e890` classification shows **zero**
unclassified entries.

## New classifications this pass

| Target / test | Verdict | Evidence |
| --- | --- | --- |
| `ipc_hardening_contract` (first-ever run) | **PRE-EXISTING** — base `1 passed; 6 failed` == candidate `1 passed; 6 failed` | `linux/logs/ab-hardening-{cand,base}.log` |
| `terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread` (joiner) | **candidate-side FLAKE** — base [PASS, PASS], candidate [FAIL, PASS] | `linux/wedged-ab.log` |
| `daemon_handover_transfer_contract` (first-ever run) | **PENDING base A/B** — quiet `1 passed; 4 failed`, same as loaded | `linux/newtargets-ab.log` |
| mac `ipc-hist` | **RAN_FAILED, environment-shaped** — 61s spawn timeout on a host at load 22.3; linux passes in 1.63s | `mac/logs/I-ipc-hist.log` |
| mac full `--lib` | **NOT_RUN** — hung at 1079/2693 on the same test that hung at base; disk fell to 3.3 GiB | `mac/logs/I-full-lib-HUNG-at-p11-reaper.log` |

---

# Pass 6 — the blocking handover regression is FIXED at `70eefafe`

## Decisive gate: `daemon_handover_transfer_contract`

| Revision | Raw exit | Result | Runtime |
| --- | --- | --- | --- |
| base `d82b35e4` | **0** | `ok. 5 passed; 0 failed` | **6.30 s** |
| candidate `39e722ce` (pre-fix) | **101** | `FAILED. 1 passed; 4 failed` | **336.47 s** |
| **candidate `70eefafe` (fixed)** | **0** | **`ok. 5 passed; 0 failed`** | **6.21 s** |

Command (all three): `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` on linux.
`70eefafe` ran at **load 2.52** (quiet). The two tests that previously failed
(`test_v5_ownership_transfer_is_the_default_without_the_flag`,
`test_v5_zero_session_loss_and_immediate_predecessor_exit`) are both `ok`.
Evidence: `linux/logs/J-handover-xfer.log`, `linux/rustJ-gates.log`.

## Root cause — my hypothesis was WRONG, the author's holds

I hypothesized changed relinquish/abort ordering. **Wrong.** The successor's per-session adoption loop
resolved the workspace owner from `self.session_metadata` — the **successor's own empty registry** —
for a session it never spawned; the lookup always missed, the closure returned `Err`, and the transfer
future aborted before `commit_started`, so the replacement daemon never acquired locks or bound its
socket. One statement explains all three failure texts and the 336 s runtime.

## The flagged `workspace_id` assertion — CONFIRMED HOLDING by execution

`daemon_handover_transfer_contract.rs:633` asserts
`post_handover.workspace_id.as_deref() == Some(ws_id)` ("the transferred session must retain its
workspace identity"). It is **exercised**: `run_v5_handover_case` is called at `:495` and `:503`,
both inside the five passing tests, so the assertion ran and held.

Reading agrees: describe's local arm (`session_service.rs:2666-2672`) returns `Some(m.workspace_id)`
from `session_metadata`, and the fix installs the predecessor's exported `StoredSessionMeta` (which
carries `workspace_id`) into that map — so once the successor installs the adopted record, describe
reports the workspace. The `#[cfg(test)]` site at `:2545` is unrelated.

## Verifier-side harness lessons (repeated)

- `setsid` alone was **not** enough: the first `70eefafe` run completed the decisive gate and then died
  at the section boundary when the launching ssh PTY closed. Re-launched the remaining gates as a
  separate script writing `rustK-gates.log`; only the decisive gate's own log is cited from the first run.
- A leaked test daemon from the killed mac full-lib run (PID 25736, PPID 1, cwd
  `/private/tmp/fx-v05-gF78ez`) was found and cleaned: killed only that pid, removed the two
  `fx-v05-*` fixture roots, and verified the canonical user daemon (`/tmp/rorca-501/daemon.sock`,
  Oct 2) was untouched.
