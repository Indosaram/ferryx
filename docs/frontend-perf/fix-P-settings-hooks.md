# Performance Fix: P-settings-hooks (F-settings-03)

## Summary
- **Packet ID**: `P-settings-hooks`
- **Finding ID**: `F-settings-03`
- **Severity**: Medium
- **Description**: In `SettingsDialog.tsx`, `useTerminalSettings()` and dialog state hooks previously ran unconditionally before the `if (!open) return null` early exit. When `SettingsDialog` was mounted with `open={false}`, `useTerminalSettings()` triggered an asynchronous IPC request (`getTerminalPreferences()`) and registered global window listeners (`orca:terminal-settings`, `storage`). Any terminal setting updates across the app triggered background re-renders of the hidden dialog. Gated dialog hook execution behind an inner component (`SettingsDialogBody`) that mounts only when `open` is `true`.

## Files Changed
- `ui/src/components/SettingsDialog.tsx` (production fix: gated `SettingsDialogBody` mounting behind `open` check)
- `ui/src/components/SettingsDialog.test.tsx` (RED/GREEN test coverage for deferred hook execution and source structure)
- `ui/src/components/SettingsDialog.workspace.test.tsx` (RED/GREEN test coverage for closed dialog render behavior)

## Production Change Details
- **File**: `ui/src/components/SettingsDialog.tsx`
- **Lines 88–93**: Updated `SettingsDialog` to return `null` immediately when `open` is falsy, rendering `<SettingsDialogBody {...props} />` only when `open` is true:
  ```tsx
  export function SettingsDialog({
    open,
    ...props
  }: SettingsDialogProps) {
    if (!open) return null;
    return <SettingsDialogBody {...props} />;
  }
  ```
- **Lines 95–125**: Extracted `SettingsDialogBody` containing `useTerminalSettings()`, `useState<SettingsSection>("general")`, and the `Escape` keydown `useEffect` handler so they are only invoked while the dialog is mounted/open.

## Test Verification

### RED Phase
Added regression tests in `ui/src/components/SettingsDialog.test.tsx`:
1. Asserted that rendering `<SettingsDialog open={false} />` does not invoke `getTerminalPreferences` IPC or register `orca:terminal-settings` / `keydown` event listeners.
2. Asserted source structure ensures `SettingsDialog` returns null before invoking `useTerminalSettings()`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/SettingsDialog.test.tsx src/components/SettingsDialog.workspace.test.tsx
```

**Output Tail (RED failure):**
```
 FAIL  src/components/SettingsDialog.test.tsx > SettingsDialog > F-settings-03: deferred hooks when closed > does not invoke useTerminalSettings or register listeners when open=false
AssertionError: expected "spy" to not be called at all, but actually been called 1 times

Received: 

  1st spy call:

    Array []

Number of calls: 1

 ❯ src/components/SettingsDialog.test.tsx:483:49
    481|       render(<SettingsDialog open={false} onClose={vi.fn()} />);
    482| 
    483|       expect(native.getTerminalPreferences).not.toHaveBeenCalled();
       |                                                 ^
    484|       const registeredTerminalListeners = addEventListenerSpy.mock.calls.filter(
    485|         ([event]) => event === "orca:terminal-settings" || event === "keydown",

 FAIL  src/components/SettingsDialog.test.tsx > SettingsDialog > F-settings-03: deferred hooks when closed > source structure gates useTerminalSettings behind open check via inner component
AssertionError: expected 'import { useCallback, useEffect, useMemo, useState } from "react";...' to match /function SettingsDialogBody[\s\S]*?useTerminalSettings\(\)/

 ❯ src/components/SettingsDialog.test.tsx:498:22
    496| 
    497|       expect(source).toMatch(/export function SettingsDialog\([\s\S]*?\)\s*\{[\s\S]*?if\s*\(!(?:open|props\.open)\)\s*return null;/);
    498|       expect(source).toMatch(/function SettingsDialogBody[\s\S]*?useTerminalSettings\(\)/);
       |                      ^

 Test Files  1 failed | 1 passed (2)
      Tests  2 failed | 16 passed (18)
```

### GREEN Phase
Implemented `SettingsDialogBody` and the `open` gate in `SettingsDialog.tsx`.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && bun run test src/components/SettingsDialog.test.tsx src/components/SettingsDialog.workspace.test.tsx
```

**Output Tail (GREEN pass):**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/SettingsDialog.test.tsx (17 tests) 705ms
 ✓ src/components/SettingsDialog.workspace.test.tsx (2 tests) 98ms

 Test Files  2 passed (2)
      Tests  19 passed (19)
   Start at  23:50:39
   Duration  3.10s (transform 128ms, setup 232ms, collect 937ms, tests 804ms, environment 551ms, prepare 80ms)
```

## Leftover Risk
- **None**: Gating the body component on `open` does not alter dialog behavior or styling when open. When closed, all hooks, IPC requests, and global event listeners are completely avoided.
