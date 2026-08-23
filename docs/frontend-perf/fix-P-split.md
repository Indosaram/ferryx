# Fix: P-split (F-terminal-05, F-terminal-06)

## Summary
- **Packet ID**: `P-split`
- **Finding IDs**: `F-terminal-05`, `F-terminal-06`
- **Severity**: Medium
- **Description**: 
  1. **F-terminal-05**: Replaced quadratic lookup inside dnd-kit `collisionDetection` (`droppableContainers.find(...)` repeated for every container comparison in `.filter()` and `.sort()`) with a single-pass `Map<UniqueIdentifier, unknown>` created once per collision evaluation pass, achieving O(1) container lookups on every drag movement.
  2. **F-terminal-06**: Hoisted the inline fallback session object literal previously allocated on every render of `PaneRenderer` to a static, module-level constant `FALLBACK_SESSION`. Wrapped `PaneRenderer` and `PaneLeafView` with `React.memo` to preserve referential equality and prevent downstream `TerminalPane` effect churn during split view layout updates.

## Files Changed
- `ui/src/components/TerminalSplitView.tsx` (production implementation)
- `ui/src/components/TerminalSplitView.test.tsx` (RED/GREEN test suite)

## Production Change Location
- **File**: `ui/src/components/TerminalSplitView.tsx`
- **Key Changes**:
  - **Lines 51–59**: Defined and exported module-level constant `FALLBACK_SESSION: TerminalSession`.
  - **Lines 61–79**: Defined and exported `workspaceCollisionDetection: CollisionDetection` which constructs a `containerDataById = new Map<UniqueIdentifier, unknown>()` once at invocation, performing O(1) lookups during `.filter()` and `.sort()`.
  - **Line 183**: Referenced `workspaceCollisionDetection` directly for `collisionDetection`.
  - **Line 651**: Wrapped `PaneRenderer` in `React.memo`.
  - **Line 656**: Used `sessions[sessionId] ?? FALLBACK_SESSION` instead of allocating a fresh object literal per render.
  - **Line 714**: Wrapped `PaneLeafView` in `React.memo`.

## RED Test Run

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TerminalSplitView.test.tsx
```

**Output Tail (RED failure):**
```
 ❯ src/components/TerminalSplitView.test.tsx:320:26
    318| 
    319|       // Verify that droppableContainers.find is not present in collision detection
    320|       expect(source).not.toMatch(/droppableContainers\.find/);
       |                          ^
    321|       // Verify Map is constructed for droppable lookups in collision detection
    322|       expect(source).toMatch(/containerDataById/);

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/4]⎯

 FAIL  src/components/TerminalSplitView.test.tsx > TerminalSplitView group and pane rendering > F-terminal-05 and F-terminal-06 perf optimizations > F-terminal-05: workspaceCollisionDetection builds an O(1) container map and does not call find() on droppableContainers during sort/filter
AssertionError: expected 'undefined' to be 'function' // Object.is equality

Expected: "function"
Received: "undefined"

 ❯ src/components/TerminalSplitView.test.tsx:327:50
    325|     it("F-terminal-05: workspaceCollisionDetection builds an O(1) container map and does not call find() on droppableContainers during sort/filter", () => {
    326|       const workspaceCollisionDetection = (TerminalSplitViewModule as Record<string, any>).workspaceCollisionDetection;
    327|       expect(typeof workspaceCollisionDetection).toBe("function");
       |                                                  ^
    328| 
    329|       const droppableContainers = [

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/4]⎯

 FAIL  src/components/TerminalSplitView.test.tsx > TerminalSplitView group and pane rendering > F-terminal-05 and F-terminal-06 perf optimizations > F-terminal-06: source check proves stable module-level FALLBACK_SESSION is defined and used in PaneRenderer
AssertionError: expected 'import {\n  DndContext,\n  DragOverlay,\n...' to match /export const FALLBACK_SESSION: TerminalSession =/

- Expected: 
/export const FALLBACK_SESSION: TerminalSession =/

+ Received: 
"..."

 ❯ src/components/TerminalSplitView.test.tsx:388:22
    386| 
    387|       // Verify module-level FALLBACK_SESSION definition
    388|       expect(source).toMatch(/export const FALLBACK_SESSION: TerminalSession =/);
       |                      ^
    389|       // Verify PaneRenderer uses FALLBACK_SESSION rather than inline object allocation
    390|       expect(source).toMatch(/sessions\[sessionId\] \?\? FALLBACK_SESSION/);

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[3/4]⎯

 FAIL  src/components/TerminalSplitView.test.tsx > TerminalSplitView group and pane rendering > F-terminal-05 and F-terminal-06 perf optimizations > F-terminal-06: exports stable FALLBACK_SESSION and maintains reference identity when session is missing
AssertionError: expected undefined to be defined
 ❯ src/components/TerminalSplitView.test.tsx:396:32
    394|     it("F-terminal-06: exports stable FALLBACK_SESSION and maintains reference identity when session is missing", () => {
    395|       const FALLBACK_SESSION = (TerminalSplitViewModule as Record<string, any>).FALLBACK_SESSION;
    396|       expect(FALLBACK_SESSION).toBeDefined();
       |                                ^
    397|       expect(FALLBACK_SESSION.id).toBe("");
    398|       expect(FALLBACK_SESSION.lifecycle).toBe("working");

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[4/4]⎯

 Test Files  1 failed (1)
      Tests  4 failed | 10 passed (14)
   Start at  23:44:12
   Duration  1.05s (transform 138ms, setup 79ms, collect 449ms, tests 155ms, environment 195ms, prepare 27ms)
```

## GREEN Test Run

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TerminalSplitView.test.tsx
```

**Output Tail (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/TerminalSplitView.test.tsx (14 tests) 947ms

 Test Files  1 passed (1)
      Tests  14 passed (14)
   Start at  23:46:52
   Duration  7.40s (transform 340ms, setup 443ms, collect 2.45s, tests 947ms, environment 1.80s, prepare 1.14s)
```

## Pre-existing Failures (Unrelated to P-split)
The separate test files `TerminalSplitView.paneDrop.runtime.test.tsx` and `TerminalSplitView.paneHandleDrop.runtime.test.tsx` have pre-existing test failures for future/other packet features (pane toolbar drag payloads and edge zones). Per task instructions, these files were left untouched.

## Leftover Risk
- **None**: Drag drop prioritization semantics remain identical (`dropPriority(activeData, data)` is preserved in exact order) while benefiting from O(1) map container lookup. `FALLBACK_SESSION` provides a stable default session object across renders without triggering spurious `TerminalPane` update cycles.
