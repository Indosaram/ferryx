# Pane Click Focus and Keyboard Typing Resolution

**Date**: 2026-09-04
**Commit**: In-progress verification
**Status**: Fully Implemented & Verified

## 1. Issue Summary

The user reported:
> "pane을 클릭해도 해당 페인에 타이핑이 안되는 문제가 있음 원인 파악 후 해결"
> (Clicking a pane does not allow keyboard typing into that pane. Identify root cause and fix.)

When switching panes or clicking within an active/inactive terminal pane in a multi-pane split layout, clicking on the pane did not focus the pane's input sink (`<textarea>`), leaving `document.activeElement` at `document.body` or on an unmapped DOM container. As a result, subsequent keystrokes were either swallowed, dropped, or routed to the previously focused pane.

---

## 2. Root Cause Analysis

Investigation identified four primary defects across the UI component layer, keyboard capture pipeline, and native desktop event bridge, plus two critical edge cases discovered during code review:

### Defect 1: Lack of DOM-Level Input Sink Focusing in `PaneLeafView` (`TerminalSplitView.tsx`)
In `TerminalSplitView.tsx`, `PaneLeafView` had:
```tsx
onClick={() => onFocusPane(tab.id, leafId)}
```
- `onFocusPane` only dispatches `FOCUS_PANE` in the Zustand store. It never called `.focus()` on the pane's inner `<textarea data-testid="native-terminal-focus-sink">`.
- When a user clicked on any DOM element of the pane (top toolbar, drag handle, hover hotspot, pane padding, or resize divider), WebKit defaulted focus to the clicked un-focusable `div` or `document.body`.
- If the pane was already active (e.g. clicking the sole pane, or re-clicking the currently active split pane), `tabLayout.activeLeafId === leafId` did not change. The store did not emit an update, `active` prop did not transition, `useEffect([active])` in `NativeTerminalPane` did not run, and the input sink remained permanently unfocused.

### Defect 2: Active-Prop Blindness in `NativeTerminalPane` Keyboard Capture (`handleCaptureKeyDown`)
In `NativeTerminalPane.tsx`:
```tsx
const ownsInput = targetedPane
  ? targetedPane === containerRef.current
  : hoveredPane
    ? hoveredPane === containerRef.current
    : fallbackSessionId === targetSessionId;
```
- `ownsInput` completely ignored the `active` prop.
- When focus was on `document.body` (e.g. after clicking outside or on a DOM divider), `targetedPane` and `hoveredPane` were `null`.
- `fallbackSessionId` fell back to `lastFocusedNativeTerminalSessionId ?? mountedNativeTerminalSessionCounts.keys().next().value`.
- If a user clicked into a new pane, but `lastFocusedNativeTerminalSessionId` still pointed to the previous pane, `ownsInput` evaluated to `false` for the clicked pane and `true` for the old pane. The active pane could not claim the keystrokes.
- In addition, when `active` transitioned from `true` to `false`, `sendFocus(false)` was never called, leaving backend cursor states out of sync.

### Defect 3: WebKit 22ms Mouse Handling Focus Reset vs ProMotion Display Frames
- On macOS with high-refresh displays (Apple Silicon 120Hz ProMotion), one animation frame is ~8.3ms.
- WebKit's asynchronous mouse-handling pipeline resets DOM focus to `document.body` at ~22–24ms after a click.
- `onNativeTerminalFocus` previously scheduled only a single `requestAnimationFrame` (executing at ~8ms). By the time WebKit's 22ms reset fired, the confirmation frame had already finished, leaving the sink blurred on fast clicks/taps.

### Defect 4: Incomplete Event Propagation in macOS Terminal Focus Monitor (`src-tauri/src/lib.rs`)
In `install_macos_terminal_focus_monitor`:
- It emitted `NATIVE_TERMINAL_FOCUS_EVENT` only via `window.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id)`, omitting `app_handle.emit` (which `windows_focus.rs` uses to reach global listeners).
- It listened only to `NSEventMask::LeftMouseUp`, missing immediate focus activation on `LeftMouseDown`.

---

## 3. Code Review Edge Cases & Hardening

During peer code review, two additional operational edge cases and one lifecycle hazard were identified and resolved:

### Edge Case 1: Terminal Search Overlay Focus Stealing
- **Problem**: When a user opens the terminal search overlay (`TerminalSearchOverlay`, Cmd+F) and clicks inside the search `<input data-testid="terminal-search-input">`, clicking inside the pane leaf could trigger `focusPaneInput()`, stealing focus away from the search input.
- **Solution**: Defined `isInteractiveTarget(target: HTMLElement | null)` to identify all interactive elements (`button, input, select, textarea:not([data-testid='native-terminal-focus-sink']), [contenteditable='true'], [role='search']`). `onPointerDown` and `onClick` immediately return without focusing the terminal sink when an interactive element is clicked.

### Edge Case 2: Inactive Pane `hoveredPane` Fallthrough
- **Problem**: In `NativeTerminalPane.tsx`, when Pane 1 is `active === true` and Pane 2 is `active === false`, if the user's cursor hovered over Pane 2, Pane 2 evaluated `active ? true : ...` as `false` and fell through to `hoveredPane === containerRef.current`, claiming `ownsInput = true`.
- **Solution**: Updated `ownsInput` to strictly evaluate `active !== undefined ? active : (hoveredPane ? ...)`. If `active === false` is explicitly specified, the pane unconditionally denies input ownership regardless of hover state.

### Lifecycle Hazard: Unmount Timer Cancellation
- Added `focusFrameRef` and `focusTimerRef` in `PaneLeafView` to store pending animation frames and timeouts, and cleanly cancel them on unmount.

---

## 4. Final Implementation Architecture

### A. DOM-Level Immediate and Multi-Stage Focus (`TerminalSplitView.tsx`)
In `PaneLeafView`:
1. Maintained a ref to the leaf container element (`leafRef`).
2. Implemented `focusPaneInput`:
   - Immediately invokes `onFocusPane(tab.id, leafId)`.
   - Queries `leafRef.current.querySelector('textarea[data-testid="native-terminal-focus-sink"]')` and calls `.focus()`.
   - Confirms focus on the next animation frame (`requestAnimationFrame`).
   - Confirms focus after 40ms (`setTimeout`) to override WebKit's 22ms async focus reset.
   - Cleans up existing pending frame/timer before scheduling new ones.
3. Attached `onPointerDown` and `onClick` handlers to both `pane-leaf` and `pane-toolbar` guarded by `isInteractiveTarget`.

### B. Direct Active-Pane Input Ownership and State Sync (`NativeTerminalPane.tsx`)
1. **Active Input Ownership**:
   Updated `ownsInput` in `handleCaptureKeyDown`:
   ```tsx
   const ownsInput = targetedPane
     ? targetedPane === containerRef.current
     : active !== undefined
       ? active
       : hoveredPane
         ? hoveredPane === containerRef.current
         : fallbackSessionId === targetSessionId;
   ```
   When `active === true`, the active pane unconditionally owns fallback input. When `active === false`, it never intercepts input.
2. **Symmetric Backend Focus Sync**:
   Updated the `active` effect:
   - When `active === true`: focuses `inputRef.current` and calls `sendFocus(true)`.
   - When `active === false`: calls `sendFocus(false)`.
3. **Timer-Backed Native Focus Confirmation**:
   In `onNativeTerminalFocus`, added a 40ms timer (`focusTimer`) alongside `requestAnimationFrame` to ensure fast clicks on native terminal child surfaces survive WebKit's async blur.

### C. macOS Event Monitor Parity (`src-tauri/src/lib.rs`)
1. Updated `install_macos_terminal_focus_monitor` to emit `NATIVE_TERMINAL_FOCUS_EVENT` to both `window` and `app_handle`.
2. Expanded the event monitor mask to `NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp`.

---

## 5. Verification Evidence

1. **Automated Unit & Integration Test Suites**:
   - `ui/src/components/TerminalSplitView.paneFocus.test.tsx`: 6/6 passed (pointerdown/click focus, toolbar click focus, action button guard, search input guard, active pane re-click).
   - `ui/src/components/TerminalSplitView.test.tsx`: 20/20 passed.
   - `ui/src/components/NativeTerminalPane.test.tsx`: 117/117 passed (including `active === true` focus dispatch, `active === false` blur dispatch, `active === true` document.body fallback keydown claiming, and `active === false` hovered isolation).
   - `cargo check --manifest-path src-tauri/Cargo.toml`: 0 errors.
   - `bun run --cwd ui build`: 0 errors, bundle compiled successfully.

2. **Manual E2E Verification Protocol for Desktop**:
   - Launch application with: `bun tauri dev`
   - Open a tab with split terminal panes (Cmd+D or split button in pane toolbar).
   - Click in Pane 1; type text and verify characters render.
   - Click anywhere in Pane 2 (toolbar, drag handle, or terminal surface); immediately type without double-clicking.
   - Verify typing immediately routes to Pane 2.
   - Open search overlay in Pane 2 (Cmd+F); click into the search input; verify typing goes into the search input and is not stolen by the terminal sink.
   - Click outside the terminal (e.g. sidebar or tab bar); click back once into any terminal pane; verify typing immediately works on the first keystroke.
