# Native Terminal Full-Height & Overlay Handle Continuation Report

**Agent**: OmO senpi-task child "hephaestus" (st_01a081a6)
**Parent Session**: `01a0819e-72f9-7b47-811b-7239f0ca96e3`
**Root Session**: `01a0819e-72f9-7b47-811b-7239f0ca96e3`
**Model**: Gemini 3.8 Flash (`PI_MODEL=gemini-3.8-flash-high`)
**Date**: 2026-09-08 / 2026-09-09

---

## 1. Executive Summary

This continuation task finalized the removal of the permanent 12px top and 20px bottom height deductions and opaque backing strips on native terminal panes, transitioning the pane handle to a hover-only overlay.

All constraints requested by user and acceptance ledgers are met:
1. **Full-height geometry**: Terminal pane uses 100% of the pane slot (`h-full`, empty inline `marginTop`, empty inline `height`), recovering 32px of previously lost vertical screen real-estate.
2. **Backing strip elimination**: Both `terminal-pane-handle-backing` (12px top) and `terminal-pane-bottom-backing` (20px bottom) permanent opaque backing strips in `TerminalPane.tsx` have been completely removed. (Note: the concurrent error-overlay hunk in `NativeTerminalPane.tsx` near line 2308 is preserved in the working tree but explicitly excluded from this task's owned commit scope, per lead direction).
3. **Hover-only handle overlay**: Hotspot is preserved at exactly 16px (`h-4`, `relativeY <= 16`). Handle bar is exactly 12px (`h-3`). Handle buttons preserve original overflowing 20px sizing (`size-5`) and neutral gray styling (`text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground`).
4. **Attention borders**: Attention frame elements (`attention-frame-bottom`, `attention-frame-corner-left`, `attention-frame-corner-right` with fixed `h-5` 20px corners) overlay the terminal with `pointer-events-none` without reserving layout space.
5. **Ownership & presentation parity**: Native presentation ownership behavior from commit `851b763` is preserved; terminal frames are retained behind overlays without opaque occlusions on errors/reconnections.
6. **Zero regressions**: All 206 scoped tests pass. Real browser component surface proof exercises all 8 scenarios on port 5212 via `Bun.WebView`, with binary observables, action logs, and screenshot artifacts recorded.

---

## 2. Recovered RED Evidence

The original failure proof was recovered directly from the source session transcript:
`/Users/indo/.omo/agent/sessions/--Users-indo-code-project-orca-lite--/2026-09-08T14-43-54-791Z_01a08179-bf27-7632-b67b-5a61b889e8eb.jsonl`

*(Note: Per lead inspection, foreign error-overlay RED is excluded from this scope and documented separately).*

### 1. Height Deduction & Handle Reservation RED Proof
- **Source Transcript**: Line 85, Notification ID `73305e94`
- **Timestamp**: `2026-09-08T14:53:04.528Z`
- **Monitor / Bash ID**: `mon_NTWKA8PZ2FPBB2GV` (Bash ID: `bash_1`)
- **Invocation**:
  ```bash
  bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/TerminalSplitView.paneHandleReach.test.tsx -t "uses the entire pane height|overlays the handle only"
  ```
- **Captured Failure Output (Verbatim)**:
  ```text
  Test Files 1 failed | 0 passed (2)
  Tests 2 failed | 149 skipped (151)
  FAIL  src/components/NativeTerminalPane.test.tsx > NativeTerminalPane geometry reporting contract > uses the entire pane height beneath the handle and bottom overlays
  AssertionError: expected '12px' to be '' // Object.is equality
  FAIL  src/components/TerminalSplitView.paneHandleReach.test.tsx > pane handle reachability over a native terminal > overlays the handle only inside the narrow hotspot without shrinking the terminal
  AssertionError: expected '12px' to be '' // Object.is equality
  Test Files  2 failed (2)
  Tests  2 failed | 149 skipped (151)
  ```
- **Physical Defect**: `NativeTerminalPane.tsx` subtracted a fixed `12px` top margin and `32px` total height (`marginTop: 12px`, `height: calc(100% - 32px)`).

### 2. Permanent Opaque Backing Strips RED Proof
- **Source Transcript**: Line 110, Notification ID `c721a5ae`
- **Timestamp**: `2026-09-08T14:56:36.511Z`
- **Monitor / Bash ID**: `mon_7WWCTFT147850Q2Z` (Bash ID: `bash_4`)
- **Invocation**:
  ```bash
  bun run --cwd ui test src/components/TerminalPane.test.tsx -t "overlays the running DAG" --reporter=basic
  ```
- **Captured Failure Output (Verbatim)**:
  ```text
  FAIL  src/components/TerminalPane.test.tsx > TerminalPane native routing contract > overlays the running DAG indicator without opaque top or bottom strips
  Test Files  1 failed (1)
  Tests  1 failed | 17 skipped (18)
  watcher exited_1 (exit code 1)
  ```
- **Physical Defect**: `TerminalPane.tsx` rendered hardcoded DOM elements `<div data-testid="terminal-pane-handle-backing" style={{ height: 12 }}>` and `<div data-testid="terminal-pane-bottom-backing" style={{ height: 20 }}>`, which permanently occluded the terminal surface underneath.

---

## 3. Current GREEN Evidence

### Mandatory Comprehensive Test Suite (All 7 Scoped & Presentation Files)

Command executed:
```bash
bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/components/TerminalPane.test.tsx src/components/TerminalSplitView.paneHandleReach.test.tsx src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx src/components/NativeTerminalPane.presentation.test.tsx
```

Output:
```text
$ vitest run --maxWorkers=1 src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/components/TerminalPane.test.tsx src/components/TerminalSplitView.paneHandleReach.test.tsx src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx src/components/NativeTerminalPane.presentation.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/NativeTerminalPane.test.tsx (150 tests) 2537ms
 ✓ src/components/NativeTerminalPane.lifecycle.test.tsx (30 tests) 415ms
 ✓ src/components/TerminalPane.test.tsx (18 tests) 101ms
 ✓ src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx (4 tests) 76ms
 ✓ src/components/NativeTerminalPane.presentation.test.tsx (9 tests) 71ms
 ✓ src/components/TerminalSplitView.paneHandleReach.test.tsx (1 test) 38ms
 ✓ src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx (3 tests) 34ms

 Test Files  7 passed (7)
      Tests  215 passed (215)
   Start at  00:56:36
   Duration  7.11s (transform 363ms, setup 416ms, collect 1.57s, tests 3.27s, environment 1.11s, prepare 174ms)
Exit code: 0
```
All 7 files (215 tests) pass with zero failures.

---

## 4. Real Browser Component Surface QA Evidence

Browser testing was performed using `Bun.WebView` (1024x768) communicating with a dedicated Vite dev server spawned via Node on port **5212** with isolated cache `.vite-qa-cont-terminal`.

### Test Architecture

- Config: `ui/qa-cont-terminal.config.mjs` (Vite port 5212, strictPort, isolated cache dir, PostCSS/Tailwind configuration).
- HTML: `ui/qa-cont-terminal.html`.
- Harness: `ui/qa-cont-terminal.tsx` (Real React component tree rendering `TerminalSplitView`, `TerminalPane`, `NativeTerminalPane`, with mock Tauri IPC logging and control toggles for attention, DAG, and bounds error injection).
- Runner: `ui/qa-cont-terminal-runner.mjs`.

### Scenario Execution & Binary Observables (25/25 PASS)

| # | Scenario | Action / Input | Concrete Selector | Binary Observable | Result |
|---|---|---|---|---|---|
| 1 | Idle Geometry | Page load | `[data-testid="terminal-pane-handle-backing"]` | Element is `null` | **PASS** |
| 1 | Idle Geometry | Page load | `[data-testid="terminal-pane-bottom-backing"]` | Element is `null` | **PASS** |
| 1 | Idle Geometry | Page load | `[data-testid="native-terminal-error-backing"]` | Element is `null` | **PASS** |
| 1 | Idle Handle | Page load | `[data-testid="pane-toolbar"]` | `classList` contains `opacity-0` and `pointer-events-none` | **PASS** |
| 1 | Full-Height Classes | Page load | `[data-testid="native-terminal-pane"]` | `marginTop === ""` and `height === ""` and has `h-full` | **PASS** |
| 1 | Full-Height Measurement | Page load | `[data-testid="native-terminal-pane"]` | `termRect.height === leafRect.height` (696px / 696px) | **PASS** |
| 1 | Presentation State | Page load | `[data-testid="native-terminal-pane"]` | `data-native-terminal-presented === "true"` | **PASS** |
| 2 | Hotspot Reach | `mousemove` at `clientY = top + 8px` | `[data-testid="pane-toolbar"]` | `classList` contains `opacity-100` and `pointer-events-auto` | **PASS** |
| 2 | Handle Geometry | Pointer inside hotspot | `[data-testid="pane-toolbar"]` | Bounding client rect height is exactly 12px (`h-3`) | **PASS** |
| 2 | Overflowing Buttons | Pointer inside hotspot | `button[aria-label="Split pane right"]` | Bounding client rect is 20px x 20px (`size-5`) | **PASS** |
| 2 | Terminal Height on Hover | Pointer inside hotspot | `[data-testid="native-terminal-pane"]` | Terminal height unchanged (696px); no reflow | **PASS** |
| 3 | Handle Drag Press | `pointerdown` on handle bar | `[data-testid="pane-toolbar"]` | `cmd_native_terminal_mouse` is NOT dispatched | **PASS** |
| 3 | Terminal Styles on Press | `pointerdown` on handle bar | `[data-testid="native-terminal-pane"]` | `marginTop === ""` and `height === ""` | **PASS** |
| 4 | Pointer Exit Hotspot | `mousemove` at `clientY = top + 40px` | `[data-testid="pane-toolbar"]` | `classList` reverts to `opacity-0` and `pointer-events-none` | **PASS** |
| 4 | Terminal Height Post-Hover | Pointer outside hotspot | `[data-testid="native-terminal-pane"]` | Terminal height unchanged (696px) | **PASS** |
| 5 | Attention Frame Presence | `window.__QA__.toggleAttention()` | `[data-testid="attention-frame-bottom"]` | Bottom frame present with `pointer-events-none` | **PASS** |
| 5 | Attention Frame Corners | Needs attention active | `[data-testid="attention-frame-corner-left"]` | Corner element height is exactly 20px (`h-5`) | **PASS** |
| 5 | Zero Layout Reservation | Needs attention active | `[data-testid="native-terminal-pane"]` | Terminal height remains 696px (no margin or shrink) | **PASS** |
| 6 | Running DAG Badge | `window.__QA__.toggleDagBadge()` | `[data-testid="dag-pane-badge"]` | Badge rendered as DOM sibling under surface container | **PASS** |
| 6 | No DAG Backing Strip | DAG badge visible | `[data-testid="terminal-pane-bottom-backing"]` | Bottom backing strip remains `null` | **PASS** |
| 6 | Terminal Height with DAG | DAG badge visible | `[data-testid="native-terminal-pane"]` | Terminal height remains 696px | **PASS** |
| 7 | Terminal Input Dispatch | `pointerdown` at `top + 100px` | `[data-testid="native-terminal-pane"]` | Routes to `cmd_native_terminal_mouse` (`action: "Press"`, `button: "Left"`) | **PASS** |
| 8 | Foreign Regression Check | Bounds failure triggered | `button[role="alert"]` | Alert rendered with `z-50` overlay (foreign regression guard) | **PASS** |
| 8 | Foreign Regression Check | Error banner displayed | `[data-testid="native-terminal-error-backing"]` | Error backing remains `null` (frame uncovered) | **PASS** |
| 8 | Foreign Regression Check | Error banner displayed | `[data-testid="native-terminal-pane"]` | `data-native-terminal-visible === "true"` preserved | **PASS** |

### Textual Pixel-Read & Geometry Audit (Non-Visual Verification Table)

For verifiers and review systems without multimodal / image-input support, the exact measured DOM bounding rectangles, computed dimensions, and style properties captured in headless execution are provided below:

| Metric / Element | Target Selector | Measured Value | Expected Spec | Verification Status |
|---|---|---|---|---|
| Leaf Viewport Box | `[data-testid="pane-leaf"]` | `x: 0, y: 72, w: 1024, h: 696` | Full available height beneath 32px tab bar | Exact match (696px) |
| Native Host Box (Idle) | `[data-testid="native-terminal-pane"]` | `x: 0, y: 72, w: 1024, h: 696` | `h: 696` (`100%` of leaf slot) | Exact match (0px delta) |
| Top Handle Backing Strip | `[data-testid="terminal-pane-handle-backing"]` | `null` (not in DOM) | Completely removed | Absent (PASS) |
| Bottom Overlay Backing | `[data-testid="terminal-pane-bottom-backing"]` | `null` (not in DOM) | Completely removed | Absent (PASS) |
| Native Host Inline Styles | `[data-testid="native-terminal-pane"]` | `marginTop: "", height: ""` | No inline margin or height offsets | Pure CSS `h-full` |
| Handle Toolbar Box (Idle) | `[data-testid="pane-toolbar"]` | `opacity: 0, pointer-events: none` | Hidden when pointer outside hotspot | Fully non-interactive |
| Handle Toolbar Box (Hover) | `[data-testid="pane-toolbar"]` | `x: 0, y: 72, w: 1024, h: 12` | Exactly 12px height (`h-3`), `opacity: 1` | Exact match (12px) |
| Toolbar Action Buttons | `button[aria-label="Split pane right"]` | `w: 20, h: 20` | Overflowing 20px buttons (`size-5`) | Exact match (20px) |
| Native Host Box (Hover) | `[data-testid="native-terminal-pane"]` | `x: 0, y: 72, w: 1024, h: 696` | `h: 696` (no reflow or resize on hover) | Exact match (0px delta) |
| Handle Drag Gesture | PointerDown on toolbar | `cmd_native_terminal_mouse` = 0 calls | Drag presses do not route to terminal PTY | Isolated (PASS) |
| Native Host Box (Post-Hover) | `[data-testid="native-terminal-pane"]` | `x: 0, y: 72, w: 1024, h: 696` | `h: 696` | Exact match (0px delta) |
| Attention Frame Bottom | `[data-testid="attention-frame-bottom"]` | `h: 2, pointer-events: none` | 2px bottom border, zero layout space | Overlaid (PASS) |
| Attention Frame Corner Left | `[data-testid="attention-frame-corner-left"]` | `w: 2, h: 20` (`h-5`), `pointer-events: none` | Fixed 20px decoration corner | Overlaid (PASS) |
| Native Host Box (Attention) | `[data-testid="native-terminal-pane"]` | `x: 0, y: 72, w: 1024, h: 696` | Full 696px (zero space reserved) | Exact match (0px delta) |
| DAG Badge Placement | `[data-testid="dag-pane-badge"]` | `bottom: 0, right: 20px (right-5)` | DOM sibling under surface container | Sibling overlay |
| Native Host Box (DAG Active) | `[data-testid="native-terminal-pane"]` | `x: 0, y: 72, w: 1024, h: 696` | Full 696px (zero space reserved) | Exact match (0px delta) |
| Terminal Mouse Input | PointerDown at `y = 172` | `cmd_native_terminal_mouse` received | `action: "Press", button: "Left"` | Authoritative match |
| Foreign Error Overlay Banner | `button[role="alert"]` | `z-50, bg-popover, bottom: 12px, right: 12px` | Clickable recovery button over terminal | Actionable |
| Foreign Error Backing | `[data-testid="native-terminal-error-backing"]` | `null` (not in DOM) | Last terminal frame remains uncovered | Uncovered (PASS) |

### Desktop Verification Boundary

> **Explicit Boundary**: The browser component surface proof verifies the DOM layout, geometry calculations, CSS classes, transition states, event routing, and mock Tauri IPC invocation. It does **not** prove native AppKit/macOS compositor pixels (which composite via WKWebView parent/child NSView relationships). Native desktop pixel confirmation must be performed by running `bun tauri dev` on macOS.

---

## 5. Cleanup & Deterministic Teardown Receipts

All timing guesses (`setTimeout` polling, `while (loading)`) were replaced with deterministic navigation/DOM event readiness (`qa:ready` custom event and `MutationObserver` subscriptions on exact state/class changes with bounded timeouts).

### Teardown Receipt
```json
{
  "serverPid": 88377,
  "serverExited": true,
  "serverExitCode": null,
  "serverExitSignal": "SIGKILL",
  "portFreed": true,
  "webviewClosed": true,
  "cacheCleaned": true
}
```

- **Process Teardown**: Node Vite process (PID 88377) terminated and verified exited via child process `"exit"` event.
- **Port Release**: Port 5212 confirmed completely released via TCP socket probing (`net.connect` error indicating no listeners).
- **Vite Cache**: Directory `ui/.vite-qa-cont-terminal` explicitly unlinked and removed.
- **Webview Cleanup**: `Bun.WebView` instance closed. Zero lingering headless webviews or node processes.
- **QA File Preservation & Cleanup**: Reproducible QA test harness (`qa-cont-terminal-runner.mjs`, `qa-cont-terminal.config.mjs`, `qa-cont-terminal.html`, `qa-cont-terminal.tsx`) preserved under `docs/session-continuation-20260908/terminal/qa/`; temporary files in `ui/` (`ui/qa-cont-terminal*`) completely unlinked so `ui/` remains free of untracked test artifacts.

---

## 6. Scope Ownership & Hunk Audit

### A. Owned Changes (Commit Scope: Full-Height Terminal with Hover-Only Handle Overlay)

These changes directly implement the approved removal of the top 12px and bottom 20px permanent loss and transition to the hover-only handle overlay:

1. **`ui/src/components/NativeTerminalPane.tsx`**:
   - Removed `NATIVE_TERMINAL_HANDLE_INSET_PX` (12px) and `NATIVE_TERMINAL_BOTTOM_INSET_PX` (20px) constants.
   - Removed inline `marginTop: ${NATIVE_TERMINAL_HANDLE_INSET_PX}px` and `height: calc(100% - ${...}px)` styling from terminal host container (leaving `style={style}` so the terminal takes full `h-full` height).

2. **`ui/src/components/NativeTerminalPane.test.tsx`**:
   - Removed imports of removed inset constants.
   - Updated geometry test `"uses the entire pane height beneath the handle and bottom overlays"` to assert full height (`marginTop: ""`, `height: ""`, `h-full`) and unsliced bounds IPC.

3. **`ui/src/components/TerminalPane.tsx`**:
   - Removed imports of inset constants.
   - Removed `<div data-testid="terminal-pane-handle-backing">` (12px top) and `<div data-testid="terminal-pane-bottom-backing">` (20px bottom).

4. **`ui/src/components/TerminalPane.test.tsx`**:
   - Updated DAG test `"overlays the running DAG indicator without opaque top or bottom strips"` to assert backing elements are null.

5. **`ui/src/components/TerminalSplitView.tsx`**:
   - Removed import of `NATIVE_TERMINAL_BOTTOM_INSET_PX`.
   - Updated attention corner frames from dynamic inset height to fixed `h-5` (20px) to prevent layout space reservation.
   - 16px hover hotspot (`relativeY <= 16`, `h-4`).
   - 12px handle toolbar (`h-3`).
   - 20px overflowing buttons (`size-5`) in neutral gray.

6. **`ui/src/components/TerminalSplitView.paneHandleReach.test.tsx`**:
   - Updated reachability test to assert full-height terminal before, during, and after hover within 16px hotspot.

7. **Lane-only QA & Documentation Deliverables**:
   - `docs/session-continuation-20260908/terminal/qa/*` (Reproducible runner `qa-cont-terminal-runner.mjs`, config `qa-cont-terminal.config.mjs`, HTML, and harness)
   - `docs/session-continuation-20260908/terminal/REPORT.md`, `qa-evidence.json`, and screenshots

---

### B. Foreign / Unrelated Hunks (Preserved in Working Tree, Excluded from Owned Scope)

In accordance with lead inspection, the following changes currently present in the working tree are **foreign** and must be excluded from this task's commit:

1. **`NativeTerminalPane.tsx` Error-Overlay Hunk (near line 2308)**:
   - **Hunk**: Removal of `<div data-testid="native-terminal-error-backing" ... />` and alert button restyling (`z-50`, `bg-popover`, `hover:bg-accent`).
   - **Origin**: Part of concurrent terminal overlay repair / error-presentation retention work.
   - **Status**: **Preserved** in the working tree but **excluded** from this task's owned change list and commit scope.
   - **Associated Test Hunks Excluded**:
     - `NativeTerminalPane.test.tsx`: Test `"keeps the terminal uncovered when a bounds failure is shown"`.
     - `NativeTerminalPane.lifecycle.test.tsx`: Recovery test assertion on `native-terminal-error-backing`.

2. **`TerminalSplitView.tsx` Shortcut Hints Hunks**:
   - **Hunk**: Data attributes wiring shortcut hint telemetry and keybinding actions:
     - `data-shortcut-scope-active={isActive && !showsDropFeedback}` (on `pane-leaf`)
     - `data-shortcut={tab.kind !== "browser" ? "terminal.splitRight" : undefined}`
     - `data-shortcut={tab.kind !== "browser" ? "terminal.splitDown" : undefined}`
     - `data-shortcut={tab.kind !== "browser" ? "terminal.unsplit tab.close" : "tab.close"}`
   - **Origin**: Shortcut hint feature track.
   - **Status**: **Preserved** in the working tree but **excluded** from this task's owned change list and commit scope.

3. **`ui/src/lib/nativeTerminalVisibility.tsx` Visibility Helper Diff**:
   - **Hunk**: Reverting `visible: owner.visible && (isMacShortcutPlatform() || !occluded)` to `visible: owner.visible && !occluded`.
   - **Status**: **Preserved** read-only. Excluded from owned scope.

4. **`src-tauri/src/native_terminal/surface_host.rs`**:
   - **Hunk**: Deferred resize completion order tests from commit `57980ce`.
   - **Status**: Clean on branch; preserved untouched.

5. **Other Unrelated Working Tree Files**:
   - `ui/src/components/ShortcutHints*`, `RemoteDirectoryPicker*`, `Sidebar*`, release pipeline scripts, and release documentation.
   - **Status**: Preserved read-only. Excluded from owned scope.

### Historical Note on Concurrent Shared-Tree Context (Non-Blocking)

1. **Shared-Tree `ui/src/lib/nativeTerminalVisibility.tsx` Status**:
   - In current shared-tree execution, all 7 files / 215 tests pass cleanly (including `dragFeedbackVisibility.test.tsx` and `NativeTerminalPane.presentation.test.tsx`).
   - The uncommitted modification in `ui/src/lib/nativeTerminalVisibility.tsx` is preserved untouched per read-only constraints and does not block terminal scope verification.

2. **TypeScript Typing Error in `ui/src/components/ShortcutHints.test.tsx`**:
   - **Location**: `ui/src/components/ShortcutHints.test.tsx:143:49` (`TS2769: 'exact' does not exist in type 'ByRoleOptions'`).
   - Untracked foreign test file preserved untouched. Scoped terminal files typecheck with 0 errors.
