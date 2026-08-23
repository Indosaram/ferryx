# Ultrawork Notepad — Correct terminal and browser tab splitting and movement
Started: 2026-08-23T00:12:00+09:00

## Plan (exhaustively detailed)
1. Inventory the existing pane tree, workspace store, drag/drop components, tests, design contract, dirty-tree overlap, and runnable QA surfaces without modifying user-owned work.
2. Run a mass-ulw discovery DAG with independent lanes for state transitions, drag/drop geometry, Orca/reference behavior, and regression-test seams. The lanes are read-only and parallel because they inspect disjoint evidence; a lead synthesis between DAG runs will integrate their outputs.
3. Capture at least three orthogonal root-cause hypotheses with file:line evidence and select the smallest shared seam that explains terminal and browser failures.
4. Delegate a RED-first implementation DAG to Gemini 3.7 Flash. One implementation node owns the smallest coherent overlapping write scope (tests plus production) because state and DnD files cannot safely be edited in parallel; a dependent verification node runs the exact related tests, typecheck/build, and reports evidence.
5. Independently inspect the implementation diff, preserve unrelated pre-existing edits, run LSP diagnostics on changed files, and fix only criterion-related failures through an amended/retried DAG node.
6. Start the browser-served UI surface, drive literal terminal-tab and browser-tab split/move scenarios with Playwright against the real rendered page, capture screenshots, action logs, pane bounding boxes, and teardown receipts. Desktop Tauri interaction is not automated because the user's durable constraint forbids desktop input injection; a browser-rendered surface is used for automated proof and any native-only residue is surfaced as a manual check.
7. Run fresh visual-qa captures for all affected states and dispatch the required independent design-system/functional and visual-fidelity/CJK reviewers in parallel; repair and recapture until both return PASS with no BLOCKING findings.
8. Run the final relevant test scope and build once, record cleanup, self-review the diff against every criterion, persist the implementation/audit result as repo markdown, reconcile todos, and complete the registered goal.

Delegation topology: discovery is a four-node parallel DAG because evidence lanes are independent; implementation is one Gemini node because the write scope overlaps; verification is a dependent Gemini node because it consumes the implementation state; the lead retains synthesis, conflict protection, evidence validation, and final QA judgment.

## Success criteria + QA scenarios
Tier: HEAVY — cross-file shared layout state, drag/drop geometry, terminal/browser lifecycle, and user-requested exactness.

1. Terminal split happy path. Scenario: run the new targeted Vitest test that drags a terminal tab to a concrete edge target, then use Playwright on the browser-served UI with the same drag coordinates. PASS iff a single new split leaf is created, the tab exists exactly once, no overlay occurs, and screenshot plus pane bounding-box JSON are captured. RED: targeted test fails on unchanged implementation for the intended tree/geometry mismatch. GREEN: same test passes after the minimal fix.
2. Browser cross-type boundary. Scenario: drag a browser tab to left/right/top/bottom of an existing terminal split and move it again to another leaf using concrete Playwright locators/coordinates. PASS iff the indicator matches the chosen edge, the emptied source leaf collapses, terminal/browser panes do not overlap, and screenshot plus bounding-box JSON are captured. RED/GREEN: targeted integration test before and after implementation.
3. Adjacent regression. Scenario: run existing same-pane tab reorder, pane drop, and split resize suites. PASS iff command exits 0 with no skipped or weakened tests and existing ordering/resize behavior remains unchanged.
4. Quality and visual integrity. Scenario: run LSP diagnostics on changed files, UI typecheck/build, fresh visual captures, and two independent visual-qa reviewer passes. PASS iff diagnostics contain zero errors, commands exit 0, captures are current, and both reviewers report PASS with empty BLOCKING.

Stop: I'll stop right away when all scenarios are RED→GREEN, the browser-rendered split/move captures pass, diagnostics/tests/build pass, both visual reviewers pass, and every spawned resource has a recorded cleanup receipt.

## Now
Bootstrap complete; preparing the mass-ulw discovery DAG.

### 2026-08-23T00:23:30+09:00 transition
Discovery DAG `dag_71034bc1-9e4e-48c0-bb73-38d0792cda77` completed. Independently reproduced the claimed component RED: `cd ui && bunx vitest run src/components/TerminalSplitView.paneDrop.runtime.test.tsx src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx --maxWorkers=1` -> 2 files failed, 5 tests failed for missing pane-edge zones, pane-edge priority/command, pane source payload, detach+targeted merge, and one-pane direct move. Independently verified reducer/store baseline: 5 files, 67 tests passed.
Now: synthesize the root cause and launch the RED-first Gemini implementation DAG.

### 2026-08-23T00:30:00+09:00 RED evidence
- Terminal/pane-handle RED: `TerminalSplitView.paneDrop.runtime.test.tsx` + `TerminalSplitView.paneHandleDrop.runtime.test.tsx` -> 5 intended failures, exit 1.
- Browser cross-type RED: delegated Gemini test `TerminalSplitView.browserPaneDrop.runtime.test.tsx` was independently read and rerun. It faithfully drags inactive `{kind:'browser', id:'browser-source'}` to terminal `{tabId:'terminal-target', leafId:'leaf-target', edge:'right'}` and fails only because `onMoveTabToSplit` has zero calls. Command exit 1.
Now: implement the smallest shared pane-edge DnD wiring fix without changing the already-green reducer/store seam.

### 2026-08-23T00:42:00+09:00 GREEN evidence
- Targeted GREEN: `TerminalSplitView.paneDrop.runtime.test.tsx`, `TerminalSplitView.paneHandleDrop.runtime.test.tsx`, `TerminalSplitView.browserPaneDrop.runtime.test.tsx` -> 3 files, 6 tests passed, exit 0.
- Adjacent regression GREEN: `TerminalSplitView.test.tsx`, `TerminalSplitView.tabDropFallback.test.tsx`, `workspaceStore.tabDrop.test.tsx`, `workspaceStore.paneDrop.runtime.test.tsx`, `layout.paneHandleDrop.runtime.test.ts` -> 5 files, 23 tests passed, exit 0.
- TypeScript: `cd ui && bunx tsc --noEmit` -> exit 0.
- LSP: `TerminalSplitView.tsx`, `PaneEdgeDropZones.tsx`, and browser RED test report no diagnostics. `tabDragTypes.ts` LSP request timed out twice; the independent project TypeScript check is clean, so this is recorded as a tooling timeout rather than a code error.
Now: run the broader related suites/build, then capture the real browser-rendered movement surface.

### 2026-08-23T00:46:00+09:00 broader verification
- Exact related regression suites remain green (23/23) and the project-wide TypeScript check was green before unrelated current test edits were included.
- `bunx vite build` produced the production bundle successfully in 5.88s.
- The repository's `bun test` command incorrectly invokes Bun's incompatible test runner, producing 53 failures (`vi.hoisted`, `vi.mocked`, missing jsdom globals); this is pre-existing script debt and not used as evidence.
- Full correct Vitest run has pre-existing failures outside this change: `terminalHostManager.test.ts` subscription expectation and `NewTabPopover.browserTab.runtime.test.tsx` Tauri invoke mocking, plus one additional current dirty test file in the full run. None touch the pane-edge implementation; targeted/adjacent scopes are green.
- `bun run build` is currently blocked by pre-existing unused imports in dirty `SettingsDialog.test.tsx`; direct Vite production bundling succeeds.

### 2026-08-23T00:59:00+09:00 corrected browser model
- Self-review caught that the first component-only browser GREEN would have called the store's pane-target helper, which intentionally rejects browser tabs and therefore no-ops in real state.
- New failing-first store test captured this no-op before correction: browser source remained in the original group and no adjacent group was created.
- Final behavior uses Orca's two-layer model: browser whole-tab drop on a terminal pane edge resolves visually against the leaf but calls group-level `MOVE_TAB_TO_SPLIT` without `targetPane`; terminal whole tabs and pane handles still use pane-target merging.
- Independent final GREEN: 6 files, 13 tests passed; browser appears exactly once with metadata, target terminal pane tree/session map unchanged, adjacent group created on requested side; TypeScript and fresh LSP diagnostics clean.

### 2026-08-23T01:12:00+09:00 real-surface and visual QA
- First Chromium terminal run failed because the pointer path crossed inactive tabs and previewed another tab before drop. A new RED test locked the real product bug that redundant activation of an already-active pane target destabilized droppable registration; the fix skips that activation and still previews genuinely inactive targets.
- Final asserted Chromium QA at 1440x960: terminal right-edge drop => `paneCount=2`, `splitCount=1`, `groupSplitCount=0`, two 695/694px panes at full 846px content height; browser left/right/top/bottom => `groupSplitCount=1`, `browserViewportCount=1`; browser right then top => one browser viewport, one group split, source group collapsed.
- Evidence: `.omo/evidence/pane-split-movement/action-log.json`, `00-terminal-drag-preview.png`, `01-terminal-split.png`, `03-browser-*-group-split.png`, `04-browser-moved-again-top.png`.
- Visual QA reviewer 1: PASS, BLOCKING none. Reviewer 2 (fidelity/CJK): PASS, BLOCKING none.
- Final affected regression set: 8 files, 17 tests passed; final `tsc --noEmit` and `vite build` exit 0.
- Cleanup receipt: Vite QA server `bash_74` killed; TCP 4173 verified free; Playwright contexts closed by the driver; QA-only `ui/qa.html`, `ui/src/qa/paneSplitQa.tsx`, Python driver, and `__pycache__` removed; evidence artifacts retained intentionally.

### 2026-08-23T01:16:00+09:00 final self-review
- Single responsibility: production changes remain within DnD command routing and `TerminalSplitView` orchestration; new leaf-zone component isolates reusable edge targets.
- Boundary purity: all DnD payloads are narrowed from `unknown` with structural guards; no `any`, ignore directives, or non-null assertions were introduced in production.
- Variant handling: tab/pane source and every drop target use exhaustive switches.
- Regression proof: reverting pane-edge mounting/routing, browser two-layer fallback, or active-target suppression makes the corresponding new tests fail.
- File size: `TerminalSplitView.tsx` was already a 900+ line modified legacy module before this task; this surgical fix adds no new architectural layer and splitting it would be an unrelated high-risk refactor. `tabDragTypes.ts` is 252 nonblank lines, at the threshold; future growth should extract command resolution by source type.
- Tier remains HEAVY because the work crossed DnD geometry, two layout layers, terminal PTY ownership, browser lifecycle, and real-surface interaction.

## Todo
- Protect unrelated dirty-tree edits and inventory exact source/test seams.
- Run discovery DAG and synthesize root-cause evidence.
- Capture RED tests for terminal and browser movement.
- Run Gemini implementation DAG for the shared fix.
- Verify diagnostics, targeted regressions, and production build.
- Drive browser-rendered split and movement scenarios.
- Run independent visual QA reviewer passes.
- Tear down all QA resources and record receipts.
- Persist final evidence report and reconcile the goal.

## Findings
- 2026-08-23: The repository already has substantial unrelated modifications, including the pane/tab files likely in scope. Every edit must preserve those changes and be based on current file contents rather than HEAD.
- 2026-08-23: Existing targeted seams include `ui/src/state/layout.paneHandleDrop.runtime.test.ts`, `ui/src/state/workspaceStore.paneDrop.runtime.test.tsx`, `ui/src/state/workspaceStore.tabDrop.test.tsx`, `ui/src/components/TerminalSplitView.paneDrop.runtime.test.tsx`, `ui/src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx`, and `ui/src/components/tab-dnd/SplitEdgeDropZone.test.tsx`.
- 2026-08-23: `ui/DESIGN.md` already defines the application design contract; this task changes layout mechanics and interaction behavior, not the visual token system.
- 2026-08-23: Desktop UI automation is prohibited by the user's durable workflow constraint; browser-rendered QA is permitted and native-only checks must be explicitly handed to the user.
- 2026-08-23: Root-cause evidence now distinguishes the reducer from the DnD wiring. `ui/src/state/layout.ts` and `ui/src/state/paneTree.ts` pass 67 focused movement tests, while `ui/src/components/TerminalSplitView.paneDrop.runtime.test.tsx` and `TerminalSplitView.paneHandleDrop.runtime.test.tsx` fail before reducer execution.
- 2026-08-23: `ui/src/components/tab-dnd/PaneEdgeDropZones.tsx` exists but `PaneLeafView` in `TerminalSplitView.tsx` does not mount it, so each leaf exposes zero pane-edge targets.
- 2026-08-23: `isWorkspaceDropData`, `resolveWorkspaceDropCommand`, and `dropPriority` in `tabDragTypes.ts` omit `pane-edge`, so overlapping group-edge targets win and no pane-targeted command can execute.
- 2026-08-23: `PaneDragData` currently contains `{tabId, leafId}` while the failing contract expects explicit `{sourcePaneId, sourceSessionId}`; this prevents the composed detach-then-targeted-merge path and disables one-leaf direct movement.
- 2026-08-23: Browser-specific stale ownership is not the primary cause; browser and terminal whole-tab moves share the same group-level store path, and the missing pane-edge integration blocks both upstream.

## Learnings
- Shared pane movement must be proven at both the pure layout transition seam and the rendered DnD surface; either alone can miss coordinate/tree integration defects.
