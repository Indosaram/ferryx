# Pane-to-tab drag regression

## Outcome

Fixed and verified through the real browser drag sensors, collision detection, split-view renderer, and layout reducer. Native desktop input and live PTY continuity still require the manual check below.

Changed implementation files:
- `ui/src/components/TabBar.tsx`: register the full tab strip as a distinct droppable with its current tab count.
- `ui/src/components/tab-dnd/tabDragTypes.ts`: route strip drops to explicit end insertion; indexed tabs retain priority.
- `ui/src/components/TerminalSplitView.tsx`: switch pane preview to a bounded tab over the row, track live pointer movement, and constrain the preview to the viewport.
- `ui/src/components/TerminalSplitView.paneToTabRow.runtime.test.tsx`: seven deterministic regression tests.

## Investigation

- Report: moving a pane to the tab row neither changes its preview into a tab nor detaches it on release.
- Hypothesis 1: the blank tab strip has no droppable registration. Distinguishing evidence: a real pointer drag ends with no tab added in the blank row, while an existing tab can be targeted.
- Hypothesis 2: preview state ignores the current drop target. The existing overlay label is unconditionally `Terminal pane` for pane drags.
- Hypothesis 3: the state reducer rejects a valid detach. Distinguishing evidence: inspect the resulting tab tree and session mapping after the callback runs.

## QA environment and cleanup

- `ui/pane-drag-qa.html`, `ui/src/devtools/PaneDragQa.tsx`, and `ui/pane-drag-qa.mjs` were temporary isolated browser QA files and were removed after verification. Screenshots are retained under `docs/evidence/pane-to-tab-*`.
- The owned Vite process on `127.0.0.1:5173` is stopped during final cleanup.
- Each isolated Chrome/Bun.WebView closed in the runner's `finally` block. No desktop app or user input automation.
- No native PTYs are created by this harness. Native desktop drag and live process continuity require manual verification.

## Results

- Before fix: trusted Chrome mouse input from pane toolbar `(250, 36)` through `(250, 65)` to blank tab row `(500, 16)` displayed `Terminal pane`, width 500px, height 30px. Release left one tab and the original two-leaf split unchanged. Reproduced twice.
- The production App wires `onDetachPaneToTab={detachPaneToTab}`; the missing blank strip collision prevents that callback from running.
- Screenshots: `docs/evidence/pane-to-tab-before-preview.png` and `docs/evidence/pane-to-tab-before-drop.png`.
- The in-kernel WebView cannot issue Chrome CDP commands from its worker thread. A standalone Bun runner supplied trusted mouse input instead; no desktop UI was driven.
- First fix: blank row and existing-tab drops both created an active tab, preserving the moved `leaf-a/session-a` pair and remaining `leaf-b/session-b` pair. Preview changed to tab styling at 224px by 32px.
- Review caught two follow-up defects before handoff: an omitted same-group insertion index means "after source", not "end of strip"; and retaining the pane's original grab offset placed the narrower preview off-screen over a left-side tab.
- Explicit `tabCount` insertion is now verified in Chrome: blank row produces `[main, other, detached]`; dropping over the first tab produces `[main, detached, other]`. Escape via CDP preserves the two original tabs and the original split tree.
- Overlay geometry now constrains the DragOverlay wrapper as well as its child. Live pointer delta is included in the position, and viewport clamping keeps the preview visible near the left edge.
- QA harness correction: wait for the exact hover hit-test state before mouse press. An early run timed out because the press raced the hidden handle's hover activation. No fixed sleeps were introduced.
- State tests passed: `bun run --cwd ui test src/state/layout.test.ts src/state/layout.paneHandleDrop.runtime.test.ts src/state/workspaceStore.tabDrop.test.tsx src/state/workspaceStore.paneDrop.runtime.test.tsx src/state/workspaceStore.browserPaneDrop.runtime.test.tsx` — 25 tests, 5 files, exit 0.
- Initial component regression evidence from the implementation worker: before fix, 2 failed / 1 passed; after fix, 3 passed. These tests execute the real component handlers with mocked DnD registration hooks; the browser QA separately exercises the real sensors and collision detection.
- Final movement regression RED: `expected 250 to be close to 500`, 1 failed / 6 passed. Adding the live transform delta made the seven-test regression pass.
- Final related component suite: `bun run --cwd ui test src/components/TerminalSplitView src/components/TabBar src/components/tab-dnd` — 99 tests, 19 files, exit 0. Together with the state checks: 124 related tests passed.
- LSP diagnostics are unavailable because the shared LSP daemon is unreachable. TypeScript compilation passed as part of the build.
- Final `bun run --cwd ui build` passed after the last source correction: TypeScript and Vite, 1860 modules, exit 0.

## Final browser evidence

The final standalone Bun runner used trusted Chrome CDP mouse/key events and exact DOM-state subscriptions, without fixed sleeps. All three scenarios completed with exit 0:
- Blank row at pointer `(500, 16)`: preview bounds `(436, 0, 128, 32)`, fully visible and centered under the pointer. Release appends and activates the detached tab, order `[main, other, detached]`.
- Existing first tab at pointer `(60, 16)`: preview bounds `(0, 1, 128, 32)`, fully visible with pointer inside. Release inserts immediately after that tab, order `[main, detached, other]`.
- Escape on the row: preview is removed; the original two tabs and two-leaf source split remain intact.
- Both successful drops preserve `leaf-a -> session-a` in the detached tab and `leaf-b -> session-b` in the remaining pane. These are fixture sessions, not live native PTYs.

Evidence images:
- `docs/evidence/pane-to-tab-after-blank-preview.png`
- `docs/evidence/pane-to-tab-after-blank-drop.png`
- `docs/evidence/pane-to-tab-after-existing-preview.png`
- `docs/evidence/pane-to-tab-after-existing-drop.png`
- `docs/evidence/pane-to-tab-after-cancel-preview.png`
- `docs/evidence/pane-to-tab-after-cancel-drop.png`

All changes remain uncommitted in the shared working tree. Unrelated concurrent changes were preserved.

## Native desktop manual check

1. Run the debug app with `bun tauri dev`.
2. Split a terminal and drag a pane from the narrow top handle to the blank tab-row area. Its preview should become a tab under the pointer; release should append and activate that tab.
3. Repeat over an existing tab. The new tab should appear immediately after it.
4. Press Escape while dragging to confirm the split remains unchanged.
5. Confirm existing terminal output and the running process remain intact, then type in both the new tab and the remaining pane.
