# Read-Only Acceptance Audit: Nested Splits & Numeric Shortcuts

## Summary Table

| Requirement | Status | Source Evidence | Test Evidence | Notes / Gaps |
|---|---|---|---|---|
| 1. Horizontal root preserved when adding vertical split | PASS | `ui/src/state/layout.ts:64-74`, `ui/src/components/TerminalSplitView.tsx:103-108,166-178` | `ui/src/state/layout.test.ts:37-56`, `ui/src/components/TerminalSplitView.test.tsx:128-142` | Root `state.split` remains `horizontal`; orthogonal vertical split stored in `nestedSplit` and rendered within primary pane frame. |
| 2. No PTY spawn | PASS | `ui/src/state/layout.ts:58-75`, `ui/src/components/TerminalSplitView.tsx:166-180`, `ui/src/state/workspaceStore.ts:83-95` | `ui/src/components/TerminalSplitView.test.tsx:62-79`, `ui/src/state/workspaceStore.test.tsx:83-100` | Splitting reuses existing session/tab identity or creates mirror pane referencing the same primary `session.backendSessionId`. |
| 3. Pane-local split actions are semantically correct | PASS | `ui/src/components/TerminalSplitView.tsx:80-94,102,122` | `ui/src/components/TerminalSplitView.test.tsx:81-110,112-126` | Pane headers contain split right (`Columns2`), split down (`Rows2`), and unsplit (`X`) controls routing to pane/layout callbacks. |
| 4. Cmd+1..4 selects workspace by index | PASS | `ui/src/lib/shortcuts.ts:70-98`, `ui/src/App.tsx:159-170,187-190` | `ui/src/lib/shortcuts.test.tsx:20-23,114-117`, `ui/src/App.test.tsx:270-281` | `workspace.select1..4` bound to `{ key: "1".."4", mod: true }`, routes to `handleSelectWorktreeByIndex(0..3)` switching workspace. |
| 5. Ctrl+1..4 selects tab by index | PASS | `ui/src/lib/shortcuts.ts:42-69`, `ui/src/App.tsx:172-186,183-186` | `ui/src/lib/shortcuts.test.tsx:24-27,118-121`, `ui/src/App.test.tsx:284-297` | `tab.select1..4` bound to `{ key: "1".."4", control: true }`, routes to `handleSelectTerminalTabByIndex(0..3)` activating tab by 0-based index. |
| 6. Out-of-range is harmless | PASS | `ui/src/App.tsx:160-163,173-176`, `ui/src/lib/shortcuts.ts:25-103` | `ui/src/App.test.tsx:299-307` | Unmapped indexes (e.g. 5) or out-of-bounds tab/worktree array lookups guard with `if (target)` no-ops without state corruption or errors. |
| 7. Existing modifiers/terminal typing remain protected | PASS | `ui/src/lib/shortcuts.ts:122-132,142-159` | `ui/src/lib/shortcuts.test.tsx:76-106` | Strict boolean matching on `metaKey`, `ctrlKey`, `altKey`, `shiftKey`; non-matching keydowns (e.g. regular typing, Ctrl+C) do not prevent default. |

---

## Detailed Findings

### 1. Horizontal Root Preserved when Adding Vertical Split (PASS)
- **Source**: In `ui/src/state/layout.ts` (lines 64–74), when `ENABLE_SPLIT` is received with an orientation differing from `state.split !== "none"`, the reducer updates `nestedSplit: { orientation: action.orientation, tabId: primaryTabId }` while keeping `state.split` unchanged. `ui/src/components/TerminalSplitView.tsx` (lines 103–108, 166–178) inspects `layout.nestedSplit` and renders the nested split within the primary pane container using flex column/row layout.
- **Test**: `ui/src/state/layout.test.ts` lines 37–56 ("keeps a horizontal split when adding a vertical nested split") verifies that `split` stays `"horizontal"` while `nestedSplit.orientation` is `"vertical"`. `ui/src/components/TerminalSplitView.test.tsx` lines 128–142 ("renders an orthogonal nested split without replacing the outer split") verifies 3 panes rendered in DOM.

### 2. No PTY Spawn (PASS)
- **Source**: `ui/src/state/layout.ts` (lines 58–75) and `ui/src/state/workspaceStore.ts` (lines 83–95) handle split enablement without invoking native terminal creation APIs. `TerminalSplitView.tsx` renders identical session objects (`session={primarySession}`) for mirror and nested panes.
- **Test**: `ui/src/components/TerminalSplitView.test.tsx` lines 62–79 ("renders a mirror pane immediately for a single-tab split state") and `ui/src/state/workspaceStore.test.tsx` lines 83–100 ("enables a single-tab mirror split without spawning another backend PTY").

### 3. Pane-Local Split Actions are Semantically Correct (PASS)
- **Source**: `ui/src/components/TerminalSplitView.tsx` lines 80–94 defines `paneActions` (`onSplitRight`, `onSplitDown`, `onUnsplit`) and attaches them to `TerminalPaneFrame` headers via `TabBar`.
- **Test**: `ui/src/components/TerminalSplitView.test.tsx` lines 81–110 ("owns split controls inside each terminal pane and dispatches pane actions") and lines 112–126 ("renders a close split control in each pane and routes it").

### 4. Cmd+1..4 Selects Workspace by Index (PASS)
- **Source**: `ui/src/lib/shortcuts.ts` lines 70–98 registers `workspace.select1` through `workspace.select4` with `{ key: "1".."4", mod: true }`. In `ui/src/App.tsx` lines 159–170 and 187–190, `handleSelectWorktreeByIndex` looks up `state.worktrees[index]` and invokes `handleSelectWorktree`.
- **Test**: `ui/src/lib/shortcuts.test.tsx` lines 20–23, 114–117 and `ui/src/App.test.tsx` lines 270–281 test `Cmd+1..4` triggering `ensureTabForWorktree` with worktrees 0 through 3.

### 5. Ctrl+1..4 Selects Tab by Index (PASS)
- **Source**: `ui/src/lib/shortcuts.ts` lines 42–69 registers `tab.select1` through `tab.select4` with `{ key: "1".."4", control: true }`. In `ui/src/App.tsx` lines 172–186 and 183–186, `handleSelectTerminalTabByIndex` activates `state.layout.tabs[index].id`.
- **Test**: `ui/src/lib/shortcuts.test.tsx` lines 24–27, 118–121 and `ui/src/App.test.tsx` lines 284–297 verify `Ctrl+1..4` activating tab indices 0 through 3.

### 6. Out-of-Range is Harmless (PASS)
- **Source**: `ui/src/App.tsx` lines 160–163 and 173–176 check `if (target)` before dispatching workspace/tab selection. Unregistered keys (such as `5`, `6`, or un-modified numbers) do not match any shortcut binding in `ui/src/lib/shortcuts.ts`.
- **Test**: `ui/src/App.test.tsx` lines 299–307 explicitly tests `Cmd+5`, `Ctrl+5`, and plain `1`/`2` keys ensuring neither `activatePrimary` nor `ensureTabForWorktree` are called.

### 7. Existing Modifiers/Terminal Typing Remain Protected (PASS)
- **Source**: `ui/src/lib/shortcuts.ts` lines 122–132 and 142–159 verify exact modifier flags match expected values (`event.metaKey === expectedMeta && event.ctrlKey === expectedControl && event.altKey === Boolean(binding.alt) && event.shiftKey === Boolean(binding.shift)`). Standard typing or unmodified keypresses, as well as unmapped chords (like `Ctrl+C`), do not match shortcut bindings and are not `preventDefault()`ed.
- **Test**: `ui/src/lib/shortcuts.test.tsx` lines 76–106 asserts that ordinary xterm typing (`key: "x"`) and `Ctrl+C` have `defaultPrevented === false`.
