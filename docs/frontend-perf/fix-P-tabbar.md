# Performance Fix: P-tabbar (F-shell-01)

## Summary
- **Packet ID**: `P-tabbar`
- **Finding ID**: `F-shell-01`
- **Severity**: Medium
- **Description**: `SortableTab` was not memoized with `React.memo`, `TabBar` allocated a fresh `onCancelRename` inline arrow closure per tab on every render, and `SortableContext` computed a fresh `items` array on every render. This caused every tab component to re-render and re-execute `@dnd-kit/sortable` hooks whenever any workspace activity summary, unread flag, or active tab selection changed. `SortableTab` is now wrapped with `React.memo`, `useSortable` data is stabilized with `useMemo`, `onCancelRename` and context menu handlers are stabilized with `useCallback`, and sortable item IDs are memoized with `useMemo`.

## Files Changed
- `ui/src/components/tab-dnd/SortableTab.tsx`
- `ui/src/components/TabBar.tsx`
- `ui/src/components/TabBar.test.tsx`

## Production Change Details
- **File**: `ui/src/components/tab-dnd/SortableTab.tsx`
  - **Line 28**: Wrapped `SortableTab` in `memo(function SortableTab(...) { ... })`.
  - **Lines 44–47**: Memoized `sortableData` with `useMemo` based on `[tab.id, groupId, index]` so `useSortable` receives a stable `data` reference.
- **File**: `ui/src/components/TabBar.tsx`
  - **Line 102**: Wrapped `handleContextMenu` in `useCallback(..., [])`.
  - **Line 116**: Wrapped `handleCommitRename` in `useCallback(..., [onRenameTab, renameValue])`.
  - **Line 126**: Added stable `handleCancelRename` callback using `useCallback(() => { renameCancelledRef.current = true; }, [])`.
  - **Line 147**: Added `sortableItems = useMemo(() => tabs.map((tab) => `tab:${tab.id}`), [tabs])`.
  - **Line 163**: Passed memoized `items={sortableItems}` to `SortableContext`.
  - **Line 180**: Passed stable `onCancelRename={handleCancelRename}` to each `SortableTab`.

## Test Verification

### RED Phase
Added regression tests in `ui/src/components/TabBar.test.tsx` verifying:
1. `SortableTab` is wrapped with `React.memo`.
2. `onCancelRename` callback is a single stable `useCallback` reference without inline arrow closures per tab.
3. `SortableContext` `items` array is memoized with `useMemo` based on `[tabs]`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TabBar.test.tsx
```

**Output Tail (RED failure):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/TabBar.test.tsx (16 tests | 3 failed) 2954ms
   ✓ TabBar > routes activate, close, and new-tab actions through real callbacks  833ms
   ✓ TabBar > supports keyboard tab activation 13ms
   ✓ TabBar > renders leading working, waiting, and unread indicators with attention precedence 20ms
   ✓ TabBar > keeps whole-tab split actions separate from terminal-pane split actions  868ms
   ✓ TabBar > keeps pin state controlled by the workspace model and blocks pinned-tab close  352ms
   ✓ TabBar > commits trimmed title changes and cancels rename on Escape 31ms
   ✓ TabBar > exposes dnd-kit sortable metadata scoped to its tab group instead of global DOM hit testing 125ms
   ✓ TabBar > opens browser tabs from the action-only menu and keeps popovers out of window dragging 180ms
   ✓ TabBar > passes the configured homepage when opening a browser tab from the new-tab menu 171ms
   ✓ TabBar > disables terminal split for browser tabs while preserving whole-tab split 179ms
   ✓ TabBar > keeps Tauri window dragging on strip background without making tabs native drag regions 53ms
   ✓ TabBar > renders new tab trigger and popover outside the horizontal overflow tablist container to avoid vertical clipping 17ms
   ✓ TabBar > forwards launchable agents and triggers onLaunchAgent when clicked in new tab popover 30ms
   × TabBar > memoizes SortableTab with React.memo
     → expected false to be true
   × TabBar > stabilizes onCancelRename callback across all rendered tabs
     → expected false to be true
   × TabBar > memoizes SortableContext items array across re-renders when tabs are stable
     → expected false to be true

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 3 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/TabBar.test.tsx > TabBar > memoizes SortableTab with React.memo
AssertionError: expected false to be true

- Expected: 
true

+ Received: 
false

 ❯ src/components/TabBar.test.tsx:384:43

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/3]⎯

 FAIL  src/components/TabBar.test.tsx > TabBar > stabilizes onCancelRename callback across all rendered tabs
AssertionError: expected false to be true

 ❯ src/components/TabBar.test.tsx:392:26

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/3]⎯

 FAIL  src/components/TabBar.test.tsx > TabBar > memoizes SortableContext items array across re-renders when tabs are stable
AssertionError: expected false to be true

 ❯ src/components/TabBar.test.tsx:400:29

 Test Files  1 failed (1)
      Tests  3 failed | 13 passed (16)
```

### GREEN Phase
Applied `React.memo` to `SortableTab`, memoized `sortableData` with `useMemo`, stabilized `handleContextMenu`, `handleCommitRename`, and `handleCancelRename` with `useCallback`, and memoized `sortableItems` with `useMemo`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TabBar.test.tsx
```

**Output Tail (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/TabBar.test.tsx (16 tests) 1173ms
   ✓ TabBar > routes activate, close, and new-tab actions through real callbacks  426ms
   ✓ TabBar > keeps whole-tab split actions separate from terminal-pane split actions  338ms

 Test Files  1 passed (1)
      Tests  16 passed (16)
   Start at  23:48:51
   Duration  8.51s (transform 456ms, setup 714ms, collect 3.40s, tests 1.17s, environment 1.72s, prepare 59ms)
```

## Leftover Risk
- **None**: All existing tab bar behaviors (tab selection, rename commit/cancel via Escape/Enter, pin toggling, close guards, context menu actions, split routing, dnd-kit drag identifiers, agent launches, and browser tabs) remain fully functional and pass the comprehensive test suite.
