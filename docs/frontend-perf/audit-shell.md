# Audit: shell
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/src/components/Sidebar.tsx, ui/src/components/WorktreeList.tsx, ui/src/components/TabBar.tsx, ui/src/components/WorkspaceHeader.tsx, ui/src/components/CommandPalette.tsx, ui/src/components/NewTabPopover.tsx, ui/src/components/AgentCards.tsx, ui/src/components/tab-dnd/SortableTab.tsx, ui/src/components/tab-dnd/TabGroupDropSurface.tsx, ui/src/components/tab-dnd/PaneEdgeDropZones.tsx, ui/src/components/tab-dnd/SplitEdgeDropZone.tsx, ui/src/components/ui/IconButton.tsx, ui/src/components/ui/SectionHeader.tsx, ui/src/components/ui/StatusDot.tsx
Date: 2026-08-22

## Findings
### F-shell-01
- Severity: Medium
- File: ui/src/components/TabBar.tsx:128
- Mechanism: `TabBar` passes unstable inline closure `onCancelRename` and newly computed `SortableContext` items array on every render. `SortableTab` is not memoized with `React.memo`. When terminal activity summaries, unread states, or active tabs change, every tab in the tab strip re-renders, re-running `@dnd-kit/sortable` hooks (`useSortable`) with fresh `data` object allocations.
- Hot path: yes
- Suggested fix: Wrap `SortableTab` in `React.memo`, memoize the `onCancelRename` handler with `useCallback` or pass a stable function reference, memoize the `tabs.map(t => \`tab:${t.id}\`)` item list using `useMemo`, and stabilize `useSortable` data props.
- Write scope: ui/src/components/TabBar.tsx, ui/src/components/tab-dnd/SortableTab.tsx
- RED proof:
```tsx
// ui/src/components/TabBar.tsx:128
<SortableContext items={tabs.map((tab) => `tab:${tab.id}`)} strategy={horizontalListSortingStrategy}>
  <div className="flex min-w-0 items-stretch overflow-x-auto scrollbar-none" role="tablist">
    {tabs.map((tab, index) => {
      const active = tab.id === activeTabId;
      return (
        <SortableTab
          key={tab.id}
          tab={tab}
          groupId={groupId}
          index={index}
          active={active}
          unread={Boolean(unreadTabIds?.[tab.id] && !active)}
          activity={activityByTabId?.[tab.id]}
          isRenaming={renamingTabId === tab.id}
          renameValue={renameValue}
          onRenameValueChange={setRenameValue}
          onCommitRename={handleCommitRename}
          onCancelRename={() => {
            renameCancelledRef.current = true;
          }}
          onActivate={onActivate}
          onClose={onClose}
          onContextMenu={handleContextMenu}
        />
      );
    })}
  </div>
```
Every render of `TabBar` allocates a new `items` array and a new `onCancelRename` arrow function for every tab, causing all `SortableTab` child instances to re-render even when their individual tab state, activity, and selection have not changed.

### F-shell-02
- Severity: Medium
- File: ui/src/components/WorktreeList.tsx:57
- Mechanism: `WorktreeList` and its individual list item rows are not memoized. In addition, `WorktreeList` performs an O(W * A) linear search (`agents.find(...)`) in the render loop across all worktrees and creates a new `displaySummary` object on every render for every worktree with an activity summary. Any change to a single worktree dirty status, active selection, or terminal activity re-renders every worktree row in all expanded projects.
- Hot path: yes
- Suggested fix: Pre-index active agents by `worktreePath` into a lookup map before mapping or in `Sidebar`, avoid per-item object allocation `{ ...summary, hasUnread }`, and memoize individual worktree row components with `React.memo`.
- Write scope: ui/src/components/WorktreeList.tsx, ui/src/components/Sidebar.tsx
- RED proof:
```tsx
// ui/src/components/WorktreeList.tsx:57
{worktrees.map((worktree) => {
  const active = worktree.path === activePath;
  const agent = agents.find((candidate) => candidate.worktreePath === worktree.path);
  const status = statuses[worktree.path];
  const primary = isPrimaryWorktree(worktree);
  const canDelete = !primary;
  const summary = activityByWorktreePath?.[worktree.path];
  const hasUnread = !active && Boolean(summary?.hasUnread || unreadWorktreePaths?.[worktree.path]);
  const displaySummary = summary ? { ...summary, hasUnread } : undefined;
  const aggregateIndicator = resolveActivityIndicator(displaySummary);
  const indicator: StatusDotState | null = aggregateIndicator ?? (summary === undefined && agent ? agent.state : null);
```
On every worktree list render, `agents.find` iterates through the agent list for every worktree, `displaySummary` allocates a fresh object for each active worktree, and every row DOM node is re-rendered without item-level change gating.

### F-shell-03
- Severity: Low
- File: ui/src/components/Sidebar.tsx:77
- Mechanism: `knownProjectsRef` passes `new Set(projects.map((project) => project.workspaceId))` as an eager argument to `useRef`. In React, `useRef` ignores subsequent arguments after initial mount, but the argument expression (array `.map()` allocation and `new Set()` creation) is evaluated on every single render of `Sidebar`.
- Hot path: no
- Suggested fix: Lazily initialize `knownProjectsRef` using `useRef<Set<string> | null>(null)` and populate it inside an effect or on first access if null.
- Write scope: ui/src/components/Sidebar.tsx
- RED proof:
```tsx
// ui/src/components/Sidebar.tsx:77
const knownProjectsRef = useRef<Set<string>>(new Set(projects.map((project) => project.workspaceId)));
```
`projects.map(...)` and `new Set(...)` execute synchronously on every render of `Sidebar` even though `useRef` discards the instantiated Set on every render after mount.

### F-shell-04
- Severity: Low
- File: ui/src/components/CommandPalette.tsx:64
- Mechanism: `CommandPalette` renders filtered worktrees and terminal tabs without list windowing/virtualization. When searching in repositories with large numbers of worktrees or workspaces with numerous tabs, all matched entries are rendered into the DOM simultaneously.
- Hot path: no
- Suggested fix: Bound rendered search results with a slice limit (e.g. `filteredWorktrees.slice(0, 50)`) or add virtualization if worktree counts grow large.
- Write scope: ui/src/components/CommandPalette.tsx
- RED proof:
```tsx
// ui/src/components/CommandPalette.tsx:64
<div className="max-h-[55vh] overflow-y-auto p-2 scrollbar-sleek">
  {filteredWorktrees.length > 0 ? (
    <section aria-label="Worktrees" className="mb-2">
      <h2 className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Worktrees</h2>
      {filteredWorktrees.map((worktree) => (
        <button
          key={worktree.path}
          type="button"
          onClick={() => {
            onSelectWorktree(worktree);
            onClose();
          }}
          className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-accent"
        >
```
All filtered worktrees are mapped directly into DOM buttons without bounding result count or virtualization.

## Non-findings / accepted
- Sidebar pointer resize (`Sidebar.tsx:100`): `handlePointerMove` reads `event.clientX` delta from `dragRef.current.startX` without performing DOM layout reads (`getBoundingClientRect`, `offsetWidth`, `scrollTop`). No layout thrashing occurs during resizing.
- Accordion & width persistence (`Sidebar.tsx:69, 219, 245`): `JSON.parse` for collapsed projects and sidebar width runs only on initial mount inside lazy `useState` initializer callbacks; `JSON.stringify` runs only on discrete user click interactions.
- Titlebar window drag handlers (`TabBar.tsx:103`, `WorkspaceHeader.tsx:21`): `getCurrentWindow().startDragging()` is invoked on `onPointerDown` only; no pointer-move listeners, rAF loops, or layout reads exist.
- Drop zone surface registration (`TabGroupDropSurface.tsx:13`, `SplitEdgeDropZone.tsx:26`): Drop targets are limited to 4 fixed edge zones per container and register with `@dnd-kit/core` without layout reads during drag movements.
- Vite development watcher (`vite.config.ts`): `server.watch.usePolling` is development-only for reliable macOS atomic file change detection and is completely excluded from production desktop builds.
- Animated status indicators (`StatusDot.tsx:14`): Animated spinners and pulses utilize CSS animation classes (`animate-spin`, `animate-pulse`) with zero JavaScript thread or rAF loop overhead.

## Scan coverage
- files read: ui/src/components/Sidebar.tsx, ui/src/components/WorktreeList.tsx, ui/src/components/TabBar.tsx, ui/src/components/WorkspaceHeader.tsx, ui/src/components/CommandPalette.tsx, ui/src/components/NewTabPopover.tsx, ui/src/components/AgentCards.tsx, ui/src/components/tab-dnd/SortableTab.tsx, ui/src/components/tab-dnd/TabGroupDropSurface.tsx, ui/src/components/tab-dnd/PaneEdgeDropZones.tsx, ui/src/components/tab-dnd/SplitEdgeDropZone.tsx, ui/src/components/ui/IconButton.tsx, ui/src/components/ui/SectionHeader.tsx, ui/src/components/ui/StatusDot.tsx
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
