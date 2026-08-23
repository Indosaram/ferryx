# Audit: app-store
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/src/App.tsx, ui/src/state/workspaceStore.ts, ui/src/state/workspaceRuntime.ts, ui/src/state/layout.ts, ui/src/state/paneTree.ts, ui/src/main.tsx
Date: 2026-08-22

## Findings
### F-app-store-01
- Severity: High
- File: ui/src/state/workspaceStore.ts:131
- Mechanism: Monolithic store state in `useReducer` re-renders top-level `WorkspaceApp` on every store action (terminal title/activity OSC ticks, drag pane ratio resize `SET_PANE_RATIO`, tab activation `ACTIVATE_TAB`, pane focus `FOCUS_PANE`). There is no `useSyncExternalStore` and no selector subscription mechanism. `WorkspaceApp` is the sole direct consumer of `useWorkspaceStore`, so every single state mutation forces a full reconciliation of the entire workspace tree (including `Sidebar`, `TerminalSplitView`, and overlay components). Furthermore, derived values `selectAgents(state)`, `selectTabActivitySummaries(state)`, and `selectWorktreeActivitySummaries(state)` run unmemoized on every render of `useWorkspaceStore`, creating new array and record allocations even on unrelated state updates.
- Hot path: yes
- Suggested fix: Migrate `workspaceStore` to an external store with `useSyncExternalStore` or selector hooks (or split high-frequency title/activity and pane-ratio slices from the structural workspace state) so child components re-render only when their specific slice changes, and memoize selector outputs.
- Write scope: ui/src/state/workspaceStore.ts, ui/src/App.tsx
- RED proof:
```ts
// ui/src/state/workspaceStore.ts:131-133
export function useWorkspaceStore({
  workspaceId = DEFAULT_WORKSPACE_ID,
  initialWorktrees = [],
  services = defaultServices,
}: UseWorkspaceStoreOptions = {}) {
  const [state, reactDispatch] = useReducer(workspaceReducer, initialWorktrees, createInitialState);
...
// ui/src/state/workspaceStore.ts:460-464
  return {
    state,
    agents: selectAgents(state),
    tabActivity: selectTabActivitySummaries(state),
    worktreeActivity: selectWorktreeActivitySummaries(state),
...
// ui/src/App.tsx:185-213
  const {
    state,
    agents,
    tabActivity,
    worktreeActivity,
    ...
  } = useWorkspaceStore({ workspaceId: activeProject.workspaceId });
```
Why it is slow: Every dispatch (including continuous `SET_PANE_RATIO` events during divider dragging and high-frequency `SESSION_TITLE_ACTIVITY` ticks from terminal streaming) invokes `useReducer`, causing `WorkspaceApp` to re-execute completely, recalculate hooks, and trigger reconciliation across both `Sidebar` and `TerminalSplitView`.

### F-app-store-02
- Severity: Medium
- File: ui/src/App.tsx:282
- Mechanism: Effect churn on window close guard. The `registerWindowCloseGuard` effect declares `[activeProject.repoRoot, activeProject.workspaceId, state]` as dependencies. Because `state` changes on every terminal activity tick, pane resize, and tab switch, this effect continuously tears down the previous window close guard handler and registers a new IPC listener.
- Hot path: yes
- Suggested fix: Store `state` in a `useRef` (or retrieve current state snapshot on close) and remove `state` from the `registerWindowCloseGuard` effect dependency array so registration happens once per active project.
- Write scope: ui/src/App.tsx
- RED proof:
```ts
// ui/src/App.tsx:282-287
  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      const session = serializeWorkspaceState(activeProject.workspaceId, activeProject.repoRoot, state);
      await saveSession(session);
    });
    return unregister;
  }, [activeProject.repoRoot, activeProject.workspaceId, state]);
```
Why it is slow: On every state transition (title update, tab focus, pane ratio change), the cleanup callback runs and a new close guard is registered, thrashing event handlers on Tauri/window close lifecycle.

### F-app-store-03
- Severity: Medium
- File: ui/src/App.tsx:408
- Mechanism: Churn in shortcut handlers and keyboard listener reattachment during pane resize and state updates. `shortcutHandlers` depends on callbacks (`handleSplitActive`, `handleUnsplitActive`, `handleCyclePaneFocus`, `handleOpenTerminalSearch`, `handleSelectTerminalTabByIndex`, `handleSelectTerminalTab`) that have dependencies on `state.layout.layoutsByTabId`, `state.layout.tabs`, `state.sessions`. During pane resizing (`SET_PANE_RATIO`), `state.layout.layoutsByTabId` updates on every mousemove frame, causing `shortcutHandlers` to recreate on every frame, which in turn causes `useShortcuts` to tear down and re-add the global `keydown` event listener on every animation frame.
- Hot path: yes
- Suggested fix: Refactor shortcut callback references to access state via refs or stable action dispatchers so that `shortcutHandlers` remains referentially stable across layout ratio and session activity updates.
- Write scope: ui/src/App.tsx
- RED proof:
```ts
// ui/src/App.tsx:408-466
  const shortcutHandlers = useMemo(
    () => ({
      "tab.newTerminal": handleAddTerminalTab,
      "tab.newBrowser": () => void createBrowserTab("http://localhost:3000").catch(reportRuntimeError),
      "tab.close": () => {
        if (state.layout.activeTabId) handleCloseTab(state.layout.activeTabId);
      },
      ...
    }),
    [
      createBrowserTab,
      handleAddTerminalTab,
      handleCloseTab,
      handleCyclePaneFocus,
      handleCycleTab,
      handleOpenTerminalSearch,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleUnsplitActive,
      handleZoomIn,
      handleZoomOut,
      handleZoomReset,
      reportRuntimeError,
      state.layout.activeTabId,
      toggleSidebar,
    ],
  );
  useShortcuts(shortcutHandlers);
```
Why it is slow: During continuous mouse dragging of split dividers, each ratio update invalidates handlers, forcing `shortcutHandlers` to change identity and re-register the global DOM keydown listener 60-120 times per second.

### F-app-store-04
- Severity: Low
- File: ui/src/App.tsx:504
- Mechanism: Inline arrow function allocations passed directly to `TerminalSplitView` and `Sidebar` props (`onTitleChange`, `onAddBrowserTab`, `onNavigateBrowserTab`, `onReloadBrowserTab`, `onSplitPane`, `onClosePane`, `onCloseSearch`, `onAddProject`, `onCreateWorktree`, `onOpenSettings`). Even if child components are wrapped in `React.memo`, these props receive fresh function references on every render, defeating shallow prop comparison.
- Hot path: yes
- Suggested fix: Wrap inline handlers in `useCallback` or pass stable dispatch functions.
- Write scope: ui/src/App.tsx
- RED proof:
```ts
// ui/src/App.tsx:508, 521-527
            onTitleChange={(tabId, title, sessionId) => updateSessionTitleActivity(tabId, title, sessionId)}
            onAddBrowserTab={(url) => void createBrowserTab(url ?? "http://localhost:3000").catch(reportRuntimeError)}
            onOpenSettings={() => setIsSettingsOpen(true)}
            onNavigateBrowserTab={(tabId, url) => void navigateBrowserTab(tabId, url).catch(reportRuntimeError)}
            onReloadBrowserTab={(tabId) => void reloadBrowserTab(tabId).catch(reportRuntimeError)}
            onSplitPane={(tabId, leafId, direction, options) => splitPane(tabId, leafId, direction, options).catch(reportRuntimeError)}
            onClosePane={(tabId, leafId) => closePane(tabId, leafId).catch(reportRuntimeError)}
            onCloseSearch={() => setSearchLeafId(null)}
```
Why it is slow: Creates fresh closure objects on every render of `WorkspaceApp`, forcing `TerminalSplitView` and `Sidebar` to re-render even when their data props are unchanged.

### F-app-store-05
- Severity: Low
- File: ui/src/App.tsx:538
- Mechanism: Inactive dialog overlays (`SettingsDialog` and `CommandPalette`) stay mounted in the DOM tree when closed (`open={false}`). While `AddProjectDialog`, `AddWorktreeDialog`, and `WorktreeDeleteDialog` are conditionally mounted (`{isAddProjectOpen ? ... : null}`), `CommandPalette` and `SettingsDialog` are permanently rendered in JSX and undergo reconciliation on every workspace state update.
- Hot path: no
- Suggested fix: Conditionally mount `{isCommandPaletteOpen ? <CommandPalette ... /> : null}` and `{isSettingsOpen ? <SettingsDialog ... /> : null}`, or ensure their root components return `null` immediately when closed and are wrapped in `React.memo`.
- Write scope: ui/src/App.tsx
- RED proof:
```ts
// ui/src/App.tsx:538-546
      <CommandPalette
        open={isCommandPaletteOpen}
        worktrees={state.worktrees}
        tabs={state.layout.tabs}
        onSelectWorktree={handleSelectWorktree}
        onSelectTab={handleSelectTerminalTab}
        onClose={() => setIsCommandPaletteOpen(false)}
      />
      <SettingsDialog open={isSettingsOpen} onClose={() => setIsSettingsOpen(false)} />
```
Why it is slow: Both components are evaluated during every render cycle of `WorkspaceApp`, allocating JSX elements and checking child props even when hidden.

## Non-findings / accepted
- Vite server.watch.usePolling: Configured with `interval: 100` in `vite.config.ts`. Accepted per constraints: it is a dev-only watcher for reliable atomic file replacement detection on macOS and is excluded from the Tauri production build.
- JSON.parse in loadProjects() and loadAgentSettings(): Executed once during component initialization in lazy `useState` initializers (`useState(() => ...)` and `useState(loadAgentSettings)`) and during infrequent window settings change events; not on hot render loops.
- JSON.parse in loadCollapsedProjectIds(): Called only inside the shortcut handler `handleSelectWorktreeByIndex` when triggered by user keyboard shortcuts (`workspace.selectN`), not during render cycles.
- normalizeLayoutInternal in layout.ts: Executed synchronously inside `layoutReducer`. Layout trees are small (1-10 tabs, 1-4 split panes per tab); deduplication and validation operate in $O(N)$ with shallow sets and Map lookups without DOM layout reads or reflows.
- paneTree.ts ratio clamping and tree operations: Pure recursive tree functions (`splitLeaf`, `removeLeaf`, `swapLeaves`, `setRatioAtPath`) with structural sharing and early bailout when nodes are unchanged; no DOM queries or memory leaks.
- useWorkspaceRuntime refresh debouncing: Uses `refreshInFlightRef` to deduplicate in-flight worktree IPC calls during focus and worktree change events.
- main.tsx initialization: Synchronous execution of `installSettingsRuntimeBridge()` and `applyCachedTerminalBackground()` runs once on startup before React mount to prevent theme/style flashing.
- Autosave debounce effect in App.tsx: The 500ms debounce timer for `serializeWorkspaceState` / `saveSession` clears previous timers on every state update, preventing high-frequency persistence during active typing or rapid state changes.

## Scan coverage
- files read: ui/src/App.tsx, ui/src/state/workspaceStore.ts, ui/src/state/workspaceRuntime.ts, ui/src/state/layout.ts, ui/src/state/paneTree.ts, ui/src/main.tsx
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
