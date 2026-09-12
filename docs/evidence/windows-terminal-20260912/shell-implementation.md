# Windows New-Terminal Shell Selector Implementation & Verification Report

**Date:** 2026-09-12
**Task ID:** st_01a09586
**Status:** COMPLETE (Failing-First Tests & Implementation Verified)
**Deliverable File:** `docs/evidence/windows-terminal-20260912/shell-implementation.md`

---

## 1. Executive Summary

This deliverable repairs the Windows new-tab shell selector regression diagnosed in `docs/evidence/windows-terminal-20260912/shell-diagnosis.md`.

### Defect Confirmation & Fix Summary
1. **Defect 1 (`TabBar.tsx`)**: The `+` ("New tab") menu handler previously constructed only a single static `new-terminal` entry and unconditionally called `onAdd()` without shell arguments. On Windows (`isWindowsPlatform()`), `TabBar` now presents a submenu `New Terminal Profile` offering:
   - `PowerShell` (`pwsh`)
   - `Windows PowerShell` (`powershell`)
   - `Command Prompt` (`cmd`)
   - `WSL` (`wsl`)
   Clicking any of these options forwards the specific shell identifier to `onAdd(shell)`. Generic `New Terminal` remains intact as the default action calling `onAdd()`. Non-Windows platforms (macOS / Linux) retain clean parity with zero extraneous Windows options.
2. **Defect 2 (`TerminalSplitView.tsx`)**: `TerminalSplitViewProps` and internal `TabGroupViewProps` previously typed `onAddTab` as `() => void`, and `TabGroupView` dropped the argument (`onAdd={() => { focusGroup(); onAddTab(); }}`). `TerminalSplitView` now accepts `onAddTab?: (shell?: string) => void` and forwards `shell` faithfully:
   ```tsx
   onAdd={(shell) => {
     focusGroup();
     if (shell !== undefined) {
       onAddTab(shell);
     } else {
       onAddTab();
     }
   }}
   ```
3. **End-to-End Delivery**: Upstream `App.tsx` (`handleAddTerminalTab(shell?: string)`), `workspaceStore.ts`, Tauri IPC `cmd_terminal_spawn`, and `src-tauri/src/terminal/shell.rs` were already intact. Restoring these two seams restores end-to-end Windows shell launching.

---

## 2. Test Plan & Pinned Test Identifiers

Test execution uses the exact real Node runtime vitest command:
```bash
cd /Users/indo/code/project/orca-lite/ui && node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/components/TabBar.test.tsx src/components/TerminalSplitView.shell.test.tsx
```

### Pinned Test Cases
1. `src/components/TabBar.test.tsx`:
   - `TabBar > offers Windows terminal shell options in the new-tab menu on Windows and forwards selected shell to onAdd`
   - `TabBar > does not offer Windows shell options in the new-tab menu on non-Windows platforms`
2. `src/components/TerminalSplitView.shell.test.tsx`:
   - `TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows`
   - `TerminalSplitView Windows shell selection forwarding > forwards pwsh, powershell, and wsl shells from TabBar through TabGroupView to onAddTab on Windows`
   - `TerminalSplitView Windows shell selection forwarding > forwards generic default New Terminal action without shell to onAddTab`
   - `TerminalSplitView Windows shell selection forwarding > forwards shell selection to onAddTab when all tabs are closed (empty layout fallback)`

---

## 3. Faithful Failing-First (RED) Evidence

### Stage A: Menu Entries Missing in Production `TabBar.tsx`
When running the new test assertions against unchanged production `TabBar.tsx`, `menuItems()` lacked any Windows shell entries or submenus.

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/components/TabBar.test.tsx src/components/TerminalSplitView.shell.test.tsx
```

**Output:**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/TabBar.test.tsx (19 tests | 1 failed) 196ms
   ...
   × TabBar > offers Windows terminal shell options in the new-tab menu on Windows and forwards selected shell to onAdd 6ms
     → expected undefined to be defined
   ✓ TabBar > does not offer Windows shell options in the new-tab menu on non-Windows platforms 3ms
 ❯ src/components/TerminalSplitView.shell.test.tsx (4 tests | 3 failed) 534ms
   × TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows 267ms
     → expected null not to be null
   × TerminalSplitView Windows shell selection forwarding > forwards pwsh, powershell, and wsl shells from TabBar through TabGroupView to onAddTab on Windows 201ms
     → expected null not to be null
   ✓ TerminalSplitView Windows shell selection forwarding > forwards generic default New Terminal action without shell to onAddTab 57ms
   × TerminalSplitView Windows shell selection forwarding > forwards shell selection to onAddTab when all tabs are closed (empty layout fallback) 9ms
     → expected null not to be null

 Test Files  2 failed (2)
      Tests  4 failed | 19 passed (23)

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 4 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/TabBar.test.tsx > TabBar > offers Windows terminal shell options in the new-tab menu on Windows and forwards selected shell to onAdd
AssertionError: expected undefined to be defined
 ❯ src/components/TabBar.test.tsx:536:23
    534|       for (const target of shells) {
    535|         const found = findEntryRecursively(items, target.label);
    536|         expect(found).toBeDefined();
       |                       ^

 FAIL  src/components/TerminalSplitView.shell.test.tsx > TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows
AssertionError: expected null not to be null
 ❯ src/components/TerminalSplitView.shell.test.tsx:136:26
    134|         (typeof entry.label === "string" && entry.label.includes("Command Prompt")),
    135|     );
    136|     expect(cmdEntry).not.toBeNull();
       |                          ^
```

### Stage B: Parameter Dropped by `TerminalSplitView.tsx`
With `TabBar.tsx` patched to emit Windows shell items but `TerminalSplitView.tsx` unpatched, `TabBar` invoked `onAdd("cmd")`, but `TerminalSplitView:773` discarded the argument (`onAdd={() => { focusGroup(); onAddTab(); }}`), passing `[]` instead of `["cmd"]`.

**Output:**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/TerminalSplitView.shell.test.tsx (4 tests | 2 failed) 262ms
   × TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows 175ms
     → expected "spy" to be called with arguments: [ 'cmd' ]

Received:
  1st spy call:
- [
-   "cmd",
- ]
+ []
Number of calls: 1

   × TerminalSplitView Windows shell selection forwarding > forwards pwsh, powershell, and wsl shells from TabBar through TabGroupView to onAddTab on Windows 60ms
     → expected "spy" to be called with arguments: [ 'pwsh' ]

Received:
  1st spy call:
- [
-   "pwsh",
- ]
+ []
Number of calls: 1

   ✓ TerminalSplitView Windows shell selection forwarding > forwards generic default New Terminal action without shell to onAddTab 21ms
   ✓ TerminalSplitView Windows shell selection forwarding > forwards shell selection to onAddTab when all tabs are closed (empty layout fallback) 6ms
 ✓ src/components/TabBar.test.tsx (19 tests) 237ms

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/TerminalSplitView.shell.test.tsx > TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows
AssertionError: expected "spy" to be called with arguments: [ 'cmd' ]

 Test Files  1 failed | 1 passed (2)
      Tests  2 failed | 21 passed (23)
```

---

## 4. Passing Verification (GREEN) Evidence

Once both minimal fixes were applied to `TabBar.tsx` and `TerminalSplitView.tsx`:

**Command:**
```bash
cd /Users/indo/code/project/orca-lite/ui && node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/components/TabBar.test.tsx src/components/TerminalSplitView.shell.test.tsx
```

**Output:**
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/TerminalSplitView.shell.test.tsx (4 tests) 1174ms
   ✓ TerminalSplitView Windows shell selection forwarding > forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows  824ms
 ✓ src/components/TabBar.test.tsx (19 tests) 795ms

 Test Files  2 passed (2)
      Tests  23 passed (23)
   Start at  20:59:08
   Duration  21.76s (transform 2.50s, setup 2.61s, collect 10.86s, tests 1.97s, environment 3.31s, prepare 1.73s)
```

### Full `TerminalSplitView.test.tsx` Regression Run
```bash
cd /Users/indo/code/project/orca-lite/ui && node node_modules/vitest/vitest.mjs run --maxWorkers=1 src/components/TerminalSplitView.test.tsx
```
**Result:** 22 passed (22), 0 failed.

---

## 5. Source Code Changes

### 1. `ui/src/components/TabBar.tsx`
- Added `WINDOWS_SHELL_OPTIONS`:
  ```ts
  const WINDOWS_SHELL_OPTIONS = [
    { id: "new-terminal:pwsh", shell: "pwsh", label: "PowerShell" },
    { id: "new-terminal:powershell", shell: "powershell", label: "Windows PowerShell" },
    { id: "new-terminal:cmd", shell: "cmd", label: "Command Prompt" },
    { id: "new-terminal:wsl", shell: "wsl", label: "WSL" },
  ] as const;
  ```
- Added private platform detection helper `isWindowsPlatform()` matching repo style:
  ```ts
  function isWindowsPlatform(): boolean {
    if (typeof navigator === "undefined") return false;
    const platform = typeof navigator.platform === "string" ? navigator.platform : "";
    const userAgent = typeof navigator.userAgent === "string" ? navigator.userAgent : "";
    return platform.toLowerCase().startsWith("win") || userAgent.includes("Windows");
  }
  ```
  No test-only production props; tests stub `navigator.platform` / `navigator.userAgent` directly through the runtime seam.
- In `handleNewTabClick`:
  ```tsx
  if (isWindows) {
    items.push({
      kind: "submenu",
      label: "New Terminal Profile",
      items: WINDOWS_SHELL_OPTIONS.map((option) => ({
        kind: "item",
        id: option.id,
        label: option.label,
      })),
    });
  }
  ```
- In `actions`:
  ```tsx
  if (isWindows) {
    for (const option of WINDOWS_SHELL_OPTIONS) {
      actions[option.id] = () => onAdd(option.shell);
    }
  }
  ```

### 2. `ui/src/components/TerminalSplitView.tsx`
- Updated `TerminalSplitViewProps.onAddTab` type to `(shell?: string) => void`.
- Updated `TabGroupViewProps.onAddTab` type to `(shell?: string) => void`.
- Updated `TabGroupView` `TabBar.onAdd` callback:
  ```tsx
  onAdd={(shell) => {
    focusGroup();
    if (shell !== undefined) {
      onAddTab(shell);
    } else {
      onAddTab();
    }
  }}
  ```

### 3. `ui/src/components/TabBar.test.tsx`
- Updated `menuItem` to recurse into submenus.
- Added tests asserting all Windows shells are listed and forward correct strings to `onAdd(shell)`.
- Added tests asserting non-Windows environments show zero Windows shell options.

### 4. `ui/src/components/TerminalSplitView.shell.test.tsx`
- Added end-to-end UI tests exercising `TerminalSplitView` -> `TabGroupView` -> `TabBar` -> native popup menu action -> callback forwarding to `onAddTab(shell)`.
- Asserted `cmd`, `pwsh`, `powershell`, and `wsl` forwarding.
- Asserted generic `New Terminal` forwarding without shell parameter.
- Asserted empty-layout fallback forwarding.

---

## 6. Verification Checklist

- [x] Zero LSP diagnostics / type errors on all edited files (`TabBar.tsx`, `TabBar.test.tsx`, `TerminalSplitView.tsx`, `TerminalSplitView.shell.test.tsx`).
- [x] Tests run with real Node vitest runner (`node node_modules/vitest/vitest.mjs`).
- [x] No fixed sleeps or polling delays in any test code.
- [x] Strict scope adherence: only owned files touched; pre-existing changes in `src-tauri/src/daemon/client.rs` preserved untouched.
- [x] No git commits or daemon changes executed.
