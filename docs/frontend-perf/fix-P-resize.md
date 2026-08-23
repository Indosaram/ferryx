# Performance Fix: P-resize (F-terminal-03, F-terminal-04)

## Summary
- **Packet ID**: `P-resize`
- **Finding IDs**: `F-terminal-03`, `F-terminal-04`
- **Severity**: Medium
- **Description**: 
  1. Fixed `F-terminal-03`: Eliminated N-way `terminalHostManager.applySettings(settings)` refits on local pane mounts. `TerminalPane` now calls `terminalHostManager.applyInstanceSettings(session.id, settings)` so that mounting or updating a pane only applies settings to that pane's terminal instance rather than iterating across all open terminals.
  2. Fixed `F-terminal-04`: Removed duplicate `ResizeObserver` and chained 2-frame `requestAnimationFrame` fits from `TerminalPane`. Resizing and backend PTY resize IPC is owned by the single `ResizeObserver` on `TerminalHostManager`, preventing 3x redundant layout recalculations per resize.

## Files Changed
- `ui/src/components/TerminalPane.tsx` (production fix: targeted instance settings + eliminated duplicate ResizeObserver)
- `ui/src/lib/terminalHostManager.ts` (production fix: added `applyInstanceSettings` and styled `hostElement` on creation)
- `ui/src/components/TerminalPane.test.tsx` (RED/GREEN test suite)

## Production Change Details
- **File**: `ui/src/lib/terminalHostManager.ts`
  - **Lines 80–88**: Absolute positioning and full sizing styles applied to `hostElement` upon creation.
  - **Lines 223–230**: Added `applyInstanceSettings(sessionId: string, settings: EffectiveTerminalSettings)` targeting only the matching session instance.
  - **Lines 232–239**: Guarded `applySettings(settings)` fit pass with client dimension check.
- **File**: `ui/src/components/TerminalPane.tsx`
  - **Lines 23–25**: Updated settings effect to call `terminalHostManager.applyInstanceSettings(session.id, settings)` instead of global `applySettings(settings)`.
  - **Lines 35–62**: Removed internal `ResizeObserver`, removed `scheduleStableFit` chained rAF loops, and removed redundant `fitMountedTerminal` helper.

## Test Verification

### RED Phase
Added tests in `ui/src/components/TerminalPane.test.tsx` ensuring:
1. `TerminalPane` does not create a duplicate `ResizeObserver` instance on pane mount.
2. `TerminalPane` applies settings only to its own instance (`applyInstanceSettings`) and does NOT invoke global `applySettings` across all instances.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TerminalPane.test.tsx
```

**Output (RED failure):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/TerminalPane.test.tsx (3 tests | 2 failed) 41ms
   × TerminalPane mounted sizing > mounts the terminal element and focuses when active without duplicate ResizeObserver in pane 21ms
     → expected [ { callback: [Function], …(2) } ] to have a length of +0 but got 1
   × TerminalPane mounted sizing > applies settings only to its own instance on mount and does not invoke global applySettings across all instances 4ms
     → expected "spy" to be called with arguments: [ 'session-new', …(1) ]

Number of calls: 0

   ✓ TerminalPane mounted sizing > fills unused terminal rows with the active terminal theme background 15ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/TerminalPane.test.tsx > TerminalPane mounted sizing > mounts the terminal element and focuses when active without duplicate ResizeObserver in pane
AssertionError: expected [ { callback: [Function], …(2) } ] to have a length of +0 but got 1

- Expected
+ Received

- 0
+ 1

 ❯ src/components/TerminalPane.test.tsx:86:27
     84|     expect(element.parentElement).toBe(mount);
     85|     // Sizing/ResizeObserver is owned by terminalHostManager; pane must not create a duplicate observer
     86|     expect(resizeRecords).toHaveLength(0);
       |                           ^
     87|     expect(focus).toHaveBeenCalled();
     88|   });

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[1/2]⎯

 FAIL  src/components/TerminalPane.test.tsx > TerminalPane mounted sizing > applies settings only to its own instance on mount and does not invoke global applySettings across all instances
AssertionError: expected "spy" to be called with arguments: [ 'session-new', …(1) ]

Number of calls: 0

 ❯ src/components/TerminalPane.test.tsx:106:43
    104| 
    105|     await waitFor(() => expect(manager.getOrCreate).toHaveBeenCalledOnce());
    106|     expect(manager.applyInstanceSettings).toHaveBeenCalledWith("session-new", expect.objectContaining({ fontSize: 13 }));
       |                                           ^
    107|     expect(manager.applySettings).not.toHaveBeenCalled();
    108|   });

⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯⎯[2/2]⎯


 Test Files  1 failed (1)
      Tests  2 failed | 1 passed (3)
   Start at  23:43:50
   Duration  1.89s (transform 57ms, setup 154ms, collect 658ms, tests 41ms, environment 569ms, prepare 67ms)
```

### GREEN Phase
Implemented the production changes in `ui/src/lib/terminalHostManager.ts` and `ui/src/components/TerminalPane.tsx`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/TerminalPane.test.tsx src/lib/terminalHostManager.test.ts
```

**Output (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/lib/terminalHostManager.test.ts (4 tests) 126ms
 ✓ src/components/TerminalPane.test.tsx (3 tests) 50ms

 Test Files  2 passed (2)
      Tests  7 passed (7)
   Start at  23:47:02
   Duration  2.22s (transform 247ms, setup 275ms, collect 503ms, tests 177ms, environment 896ms, prepare 90ms)
```

## Leftover Risk
- **None**: Single `ResizeObserver` lifecycle remains tied to the host element managed by `TerminalHostManager`. Font/theme updates and window resizes continue to resize xterm and dispatch backend PTY resize IPC without redundant multi-frame layout churn.
