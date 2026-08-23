# Performance Fix: P-worktree (F-shell-02, F-shell-03)

## Summary
- **Packet ID**: `P-worktree`
- **Finding IDs**: `F-shell-02`, `F-shell-03`
- **Severity**: Medium
- **Description**: 
  1. In `WorktreeList.tsx` (F-shell-02), replaced linear agent scans (`agents.find(...)`) inside the render loop with an `agentsByPath` `Map<string, ActiveAgent>` computed via `useMemo`. Extracted and memoized individual `WorktreeRow` components using `React.memo` to avoid re-rendering untouched worktree rows on unrelated parent or state updates. Avoided unnecessary object allocation for `displaySummary` when `summary.hasUnread` already matches `unread`.
  2. In `Sidebar.tsx` (F-shell-03), replaced eager `useRef` evaluation (`useRef<Set<string>>(new Set(projects.map(...)))`) with lazy ref initialization to prevent allocating a new Set on every single Sidebar render.

## Files Changed
- `ui/src/components/WorktreeList.tsx` (production implementation)
- `ui/src/components/Sidebar.tsx` (production implementation)
- `ui/src/components/WorktreeList.test.tsx` (RED/GREEN test coverage)
- `ui/src/components/Sidebar.test.tsx` (RED/GREEN test coverage)

## Production Change Details
- **File**: `ui/src/components/WorktreeList.tsx`
  - **Lines 40–125**: Extracted and memoized `WorktreeRow = memo(function WorktreeRow({ ... }) { ... })` with stabilized props (`worktree`, `active`, `agent`, `status`, `unread`, `activitySummary`, `onSelect`, `onDelete`).
  - **Lines 137–142**: Pre-indexed `agents` into `Map<string, ActiveAgent>` with `useMemo(() => ... agentsByPath, [agents])` for O(1) row lookups instead of O(W * A) linear scans.
  - **Lines 159–176**: Rendered memoized `WorktreeRow` components in `WorktreeList`.
- **File**: `ui/src/components/Sidebar.tsx`
  - **Lines 87–100**: Lazily initialized `knownProjectsRef` (`useRef<Set<string> | null>(null)` with null check and fallback initialization) to avoid evaluating `new Set(projects.map(...))` on every render cycle.

## Test Verification

### RED Phase
Added regression tests asserting that `WorktreeList.tsx` does not perform `agents.find` linear search in render and uses Map lookup `agentsByPath`, and that `Sidebar.tsx` does not allocate a new Set as an eager parameter to `useRef`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/WorktreeList.test.tsx src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx
```

**Output Tail (RED failure):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/Sidebar.test.tsx (19 tests | 1 failed) 2281ms
   ✓ Sidebar navigation > uses the 236px parity width by default 72ms
   ✓ Sidebar navigation > keeps only global actions in the sidebar titlebar  431ms
   ...
   × Sidebar navigation > initializes knownProjectsRef lazily without allocating a new Set on every render (F-shell-03) 77ms
     → expected 'import {\n  ChevronRight,\n  FolderGi…' not to contain 'useRef<Set<string>>(new Set'
 ✓ src/components/Sidebar.activity.test.tsx (3 tests) 651ms
 ❯ src/components/WorktreeList.test.tsx (9 tests | 1 failed) 914ms
   ✓ WorktreeList actions > renders worktree dirty status without a manual refresh control  388ms
   ...
   × WorktreeList actions > indexes active agents by path with Map lookup instead of agents.find linear scan (F-shell-02) 116ms
     → expected 'import { GitBranch, LockKeyhole, Tras…' not to contain 'agents.find'
   ✓ WorktreeList actions > renders active agent info mapped to corresponding worktree correctly 3ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/Sidebar.test.tsx > Sidebar navigation > initializes knownProjectsRef lazily without allocating a new Set on every render (F-shell-03)
AssertionError: expected 'import {\n  ChevronRight,\n  FolderGi…' not to contain 'useRef<Set<string>>(new Set'

 FAIL  src/components/WorktreeList.test.tsx > WorktreeList actions > indexes active agents by path with Map lookup instead of agents.find linear scan (F-shell-02)
AssertionError: expected 'import { GitBranch, LockKeyhole, Tras…' not to contain 'agents.find'

 Test Files  2 failed | 1 passed (3)
      Tests  2 failed | 29 passed (31)
```

### GREEN Phase
Applied Map indexing and `WorktreeRow` memoization in `WorktreeList.tsx`, and lazy ref initialization in `Sidebar.tsx`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/WorktreeList.test.tsx src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx
```

**Output Tail (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/Sidebar.test.tsx (19 tests) 1114ms
 ✓ src/components/Sidebar.activity.test.tsx (3 tests) 185ms
 ✓ src/components/WorktreeList.test.tsx (9 tests) 116ms

 Test Files  3 passed (3)
      Tests  31 passed (31)
   Start at  23:45:16
   Duration  4.89s (transform 175ms, setup 314ms, collect 1.19s, tests 1.41s, environment 1.05s, prepare 154ms)
```

## Leftover Risk
- **None**: Functional behavior and UI invariants remain completely unchanged. Worktree selection, creation, deletion, status reporting, activity dots, and project hierarchy behave identically while eliminating redundant allocations and O(W * A) lookup overhead during re-renders.
