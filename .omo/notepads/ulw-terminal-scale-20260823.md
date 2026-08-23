# Ultrawork Notepad — scalable multi-workspace terminal runtime
Started: 2026-08-23T01:03:34Z

## Plan (exhaustively detailed)
1. Inspect the renderer, output, and workspace-switching seams and the dirty-worktree ownership risks.
2. Run a first mass-ulw DAG: three disjoint read-only architecture lanes and one synthesis node that records an implementation boundary report.
3. Reconcile the architecture report with the repo and define a second DAG with serialized, disjoint implementation ownership and a dependent verification wave.
4. Capture failing tests before every behavioral change, then run the delegated RED-to-GREEN implementation wave.
5. Run targeted tests, UI build, native regression tests, and the closest real rendered Ferryx surface; record every artifact and cleanup receipt.
6. Re-read the requested scope, reconcile all tasks, preserve unrelated changes, and report only captured evidence.

## Success criteria + QA scenarios
- Tier: HEAVY. The implementation crosses renderer lifetime, output scheduling, React workspace navigation, and resource boundaries.
- Criterion 1: `bun run --cwd ui test -- terminalHostManager` must first fail for the missing bounded visible-plus-LRU renderer budget, then pass once eviction disposes only frontend renderers while backend session IDs remain intact. Artifact: test transcript.
- Criterion 2: `bun run --cwd ui test -- terminalEvents` must first fail for missing ordered animation-frame batching and inactive replay continuity, then pass. Artifact: test transcript.
- Criterion 3: `bun run --cwd ui test -- workspaceStore` must first fail for known-workspace navigation calling spawn or waiting, then pass with five parked workspaces and 100 sessions. Artifact: test transcript.
- Criterion 4: `bun run --cwd ui build` must pass, then a real Ferryx UI surface must switch a retained tab and reattach an evicted renderer. Artifact: rendered screenshot/action log or an explicit desktop-manual QA procedure if automation is unavailable.
- Criterion 5: `cargo test --manifest-path src-tauri/Cargo.toml terminal::output_hub` or the narrowest existing native output-hub/session suite must pass. Artifact: command transcript.
- Stop: I will stop right away when all criteria have captured PASS evidence (or the unavailable desktop check is transparently documented), the cleanup receipts and plan are current, every DAG worker is terminal, and the user-facing implementation report is delivered.

## Completion audit checklist

| Requirement | Required artifact | Current state |
| --- | --- | --- |
| 100-session renderer budget | Deterministic `terminalHostManager` test proves visible panes + bounded LRU only; eviction disposes frontend renderer and preserves backend ID | PASS — split host-manager suites retain 14 lifecycle tests, including five workspaces × twenty sessions |
| Ordered batching + bounded overload | Host-manager test proves same-frame coalescing, replay-before-live, teardown, and cap-triggered ordered synchronous flush | PASS — final targeted terminal integration suite passed 13 files / 58 tests |
| Inactive continuity | Host-manager test proves inactive xterm writes stop while event bus can replay title/activity/backlog in order on visibility restore | PASS — event-bus direct retained-suffix/title/replay coverage passed in targeted suite |
| Five by twenty immediate navigation | `workspaceStore` test holds 101 sessions and asserts zero spawn/close/wait/event/CWD side effects across known layout selection | PASS — workspace scale proof passed in targeted suite |
| UI regression | Targeted Vitest suites and `bun run --cwd ui build` exit 0 | PASS — final `bun run --cwd ui build`; final `bun run --cwd ui test`: 70 files / 493 tests |
| Native/mobile contract | Narrow Rust output-hub test exits 0 and source review confirms remote uses unchanged bounded hub stream | PASS — `cargo test --manifest-path src-tauri/Cargo.toml output_hub`: 2 tests; `TerminalService::attach` supplies history + receiver and remote websocket sends history before live messages |
| Real surface | User performs desktop terminal/tab switch plus evicted-renderer reattach (automation prohibited by durable user constraint) | USER ACTION REQUIRED — exact procedure at `.omo/evidence/terminal-scale/MANUAL_DESKTOP_QA.md`; no desktop automation or false pass claimed |

## Manual QA scenario (desktop automation prohibited)

1. Start Ferryx in development mode using the repository's documented app command.
2. In one workspace, open three terminal tabs, then switch among them; PASS: each tab returns immediately and its running command continues rather than reopening a shell.
3. Switch away from that workspace after leaving two terminal panes visible, create or visit more inactive terminal tabs than the configured warm-cache limit, then return to the oldest tab; PASS: its existing PTY output is retained/replayed in order, and no duplicate shell/process appears.
4. Repeat with five workspaces that each have at least twenty already-running agents; PASS: selecting a known workspace/tab does not display a loading/spawn delay and each agent's activity/output remains intact.
5. Record a screenshot or screen capture containing the selected workspace/tab and terminal output. No test processes, servers, or browser contexts are created by this manual scenario.

Durable handoff: `.omo/evidence/terminal-scale/MANUAL_DESKTOP_QA.md`.

## Now
Close a verification-discovered pending renderer creation race: an explicit destroy during asynchronous instance creation must invalidate and dispose the late renderer rather than allow it to repopulate the cache.

## Todo
- Inspect dirty-file ownership and existing scale plans.
- Run architecture mass-ulw DAG and reconcile reports.
- Define implementation DAG with disjoint file ownership.
- Capture RED proofs before behavioral implementation.
- Run implementation and dependent verification DAG.
- Run real-surface QA and record cleanup.
- Reconcile evidence and report outcome.

## Findings
- Skills selected: `mass-ulw` for required DAG orchestration; `programming` for Rust/TypeScript correctness; `frontend` because renderer lifecycle changes affect React runtime behavior; `visual-qa` for the required real rendered surface; `ast-grep` for structural code discovery; `memory-discipline` to distinguish durable project decisions from ephemeral execution state.
- The repository is already heavily dirty across terminal, workspace, and UI files, plus extensive `.omo/senpi-task` state. Existing edits must be treated as externally owned unless a DAG worker establishes otherwise.
- User's known product requirement is 5 workspaces x 20+ agent terminals with fast navigation; live session continuity and remote mobile support are non-negotiable.
- Delegation topology: three parallel read-only `unspecified-low` architecture lanes own renderer, output, and navigation analysis; an `unspecified-high` synthesis lane depends on all three and validates a safe serialized write order. The split pays off because all three initial lanes have read-only, disjoint source scopes; implementation will serialize any lane touching `terminalHostManager.ts`.
- Prior performance sweeps exist (`BACKEND_PERFORMANCE_SWEEP.md`, `FRONTEND_PERFORMANCE_SWEEP.md`) and must be treated as historical claims, not proof for this new renderer-budget requirement.
- Independently confirmed current source facts: `TerminalHostManager` has an unbounded `instances` map; `TerminalEventBus` publishes each decoded chunk immediately; `workspaceStore` retains parked layouts; no rAF batching or warm-cache budget exists; Rust `TerminalOutputHub` remains the bounded source used by terminal service and remote delivery.
- Architecture DAG `dag_31dc4a5b-7972-4189-95f3-cfe13ef3f634` is terminal. Its scope and reports were independently read and source claims checked. Its useful boundary is: output batching and renderer cache must serialize because both modify `terminalHostManager.ts`; workspace fast-path regression work is disjoint. The pre-existing `TerminalSplitView.tsx` diagnostic (`defaultAgentId` prop mismatch) is unrelated and blocks any clean blanket TypeScript claim unless separately fixed by its owner.
- Implementation DAG `dag_9680162c-7904-418c-b82c-5687602e112b` was independently reconstructed from node prompts, artifacts, source, and tests. It delivered the required seams, but source review found the active renderer's rAF queue was not bounded during a throttled frame and the new tests used `as any` escape hatches. The responsible completed nodes were revived with scope-limited corrective instructions; their original claims remain unaccepted until rechecked.
- Completion-audit result at this point: criteria 1–5 are NOT achieved. The current source documents a 128 KiB pending-output limit but did not enforce it in the callback; no final targeted test/build/native output has been independently rerun after the corrective edits; real desktop QA remains deliberately manual because the user prohibits desktop manipulation.
- Fresh independent evidence after the pending-output correction: targeted terminal scale UI test selection passed (10 files/56 tests); renderer host suite passed (12 tests including 100 sessions); events suite passed (10 tests); workspace suite passed (6 files/31 tests); `bun run --cwd ui build` passed; native `cargo test --manifest-path src-tauri/Cargo.toml output_hub` passed 2 tests. Full UI suite later passed 67 files/491 tests. Native compile emitted three pre-existing unused writer-lease warnings under `src-tauri/src/worktree/manager.rs`, unrelated to this work.
- Final source reread found a separate lifecycle risk: `destroy` deletes `pendingSpawns`, but an already awaited `getOrCreate` can later insert its resolved renderer. This is a potential post-close cache resurrection that violates strict visible-only lifecycle ownership. A dedicated one-node implementation DAG now owns the smallest invalidation fix and RED/GREEN test.
- Pending-renderer race closure: final `TerminalHostManager` source tracks concurrent create generations only while needed, invalidates/destroys a renderer that resolves after `destroy`, cannot let an old promise erase a newer pending promise, and removes generation tracking after active creations settle. Deterministic tests cover both destroy-during-create and destroy/new-create/old-resolve overlap; final host-manager command passed 14 tests.
- Final independent verification (after the generation-cleanup correction): targeted terminal suite passed 10 files / 58 tests; `bun run --cwd ui build` passed; full UI suite passed 67 files / 493 tests; `cargo test --manifest-path src-tauri/Cargo.toml output_hub` passed 2 tests; `git diff --check` passed. Rust emitted three pre-existing `WriterLeaseGuard` dead-code warnings under `src-tauri/src/worktree/manager.rs`; no terminal-scale source change was made there.
- All four terminal-scale DAGs are terminal: architecture `dag_31dc4a5b-7972-4189-95f3-cfe13ef3f634` (4/4), implementation `dag_9680162c-7904-418c-b82c-5687602e112b` (4/4), coverage `dag_32d086bf-0442-4f8e-8eed-978ab27deb74` (2/2), and renderer teardown `dag_db9b25b2-206b-4692-bb01-64ce48683980` (1/1). Their outputs were treated as claims and independently re-read/re-run before acceptance.
- Manual desktop QA disposition: desktop automation remains deliberately unavailable because the owner prohibits agent-driven desktop input. The complete PASS/FAIL procedure and evidence request is durable at `.omo/evidence/terminal-scale/MANUAL_DESKTOP_QA.md`; user execution is still required to accept real-surface behavior.
- Final maintainability closure: lifecycle code is split into `terminalHostManager.ts` (237 pure LOC, lifetime/cache ownership), `terminalInstanceFactory.ts` (164 pure LOC, xterm instance ownership), and `terminalOutputScheduler.ts` (52 pure LOC, ordered output batching). The terminal lifecycle test suite is split into helper/core/output/LRU/scale files (180/112/141/83/104 pure LOC). The final split-source command evidence is targeted 13 files / 58 tests, build pass, full UI 70 files / 493 tests, native output-hub 2 tests, and clean whitespace check. The result record is `.omo/evidence/terminal-scale/IMPLEMENTATION_RESULT.md`.
- Reactivation viewport offset follow-up: a bottom-following xterm could be left one row above its newest output after delayed font/resize fitting. `fitTerminal` now preserves deliberately scrolled-up history but aligns a pre-fit bottom-following viewport after every existing fit path. RED: `TerminalPane` test failed because `scrollToBottom()` was not called. GREEN: focused terminal verification passed 6 files / 22 tests; full UI verification passed 71 files / 498 tests; `bun run --cwd ui build` and native `output_hub` tests passed. Durable record: `.omo/evidence/terminal-scale/TERMINAL_VIEWPORT_OFFSET_FIX.md`; desktop-only Scenario D is in `MANUAL_DESKTOP_QA.md`, while 5×20 Scenario C is deferred at the owner's request.
- Cmd+W and tab-close confirmation follow-up: the native Window > Close Window accelerator intercepted Cmd+W before the WebView. Cmd+W now reaches File > Close Tab through `menu_close_tab`; explicit window close uses Cmd+Shift+W. Settings → General persists disabled-by-default `confirmCloseTab`, which makes the shared tab-close intent require explicit confirmation. Focused UI green: 9 files / 91 tests; native `cargo check` and output-hub tests pass. Durable record: `.omo/evidence/cmd-w-tab-close-confirmation.md`; desktop-only Scenario E is added to `terminal-scale/MANUAL_DESKTOP_QA.md`.

## Learnings
- Renderer count, not PTY count, must be bounded; session lifetime and UI renderer lifetime are separate concerns.
