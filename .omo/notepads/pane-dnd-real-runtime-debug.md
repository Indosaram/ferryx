# Debug Journal — Real Pane Split Drag

Started: 2026-08-23

## Environment snapshot

- Runtime: React 18 + TypeScript 5.7 + Vite 6, Bun 1.4 test runner, Tauri native browser child webview.
- Entry: `cd ui && bun run dev`; desktop entry is the existing Tauri launcher.
- Ports 1420 and 4173 were free at investigation start.
- Working tree is heavily dirty with unrelated user/agent changes; only pane-DnD-owned files are in scope.
- References read: debugging Node runtime, Playwright, setup, investigation; TypeScript rules; frontend layout/interaction/perfection; visual QA.
- Root `.debug-journal.md` belongs to a concurrent Cmd+W task and is intentionally untouched.

## Hypotheses

1. [CONFIRMED] Visual feedback is painted on the 20% collision strip itself, so it cannot cover half the pane. `SplitEdgeDropZone.tsx` and `TabGroupDropSurface.tsx` combine collision geometry with preview paint.
2. [CONFIRMED] The native browser child webview remains visible during DnD, intercepting pointer events and covering DOM feedback except for the HTML browser toolbar. `BrowserPane.tsx` bounds the child view below the toolbar and no workspace drag state reaches `visible`.
3. [SUPPORTED] Actual drop callbacks work when a pane-edge payload reaches `DndContext`; the real-app no-drop symptom is therefore caused by native webview hit-test interception rather than the resolver/store path.

## RED evidence

- Geometry: four edge cases fail because `[data-testid=split-edge-preview]` does not exist; current visual feedback is the 20% hit node.
- Browser drag: drag-start tests fail because `setBrowserVisible(browserId, false)` is never called.
- Registered pane-edge payload path already passes, isolating the failure before command execution.

## Artifacts

- Production files to retain only after GREEN: `SplitEdgeDropZone.tsx`, `TabGroupDropSurface.tsx`, `TerminalSplitView.tsx`, `BrowserPane.tsx` if required.
- Regression tests created/updated by task `st_01a02c78`.
- Temporary browser QA harness/server will be removed; final evidence retained.

## Final fix

- Separated 20% collision strips from 50% visual previews and reused the same component for group/pane targets.
- Workspace drag state now hides native browser child webviews and restores them on end/cancel.
- Same-tab terminal pane-edge drops now call `onSplitPane` for the exact leaf/position rather than entering the cross-tab reducer no-op.
- RED: 6 intended failures across geometry/native visibility; self-drop test also failed with zero `onSplitPane` calls.
- GREEN: focused and adjacent DnD/store/browser tests passed; TypeScript and Vite build passed.
- Fresh browser geometry ratios: left/right 0.5 width x 1.0 height; top/bottom 1.0 width x 0.5 height.
- Temporary QA harness/server removed; screenshot retained at `.omo/evidence/pane-dnd-real-runtime/half-pane-previews.png`.
