# Code Review: Terminal Pane Click Focus & Keyboard Typing Resolution

**Review Date**: 2026-09-04
**Scope**:
- `src-tauri/src/lib.rs` (`install_macos_terminal_focus_monitor`)
- `ui/src/components/TerminalSplitView.tsx` (`PaneLeafView`, `focusPaneInput`)
- `ui/src/components/NativeTerminalPane.tsx` (`active` prop, `handleCaptureKeyDown`, `ownsInput`, `sendFocus`)
- `ui/src/components/TerminalSplitView.paneFocus.test.tsx`
- `ui/src/components/NativeTerminalPane.test.tsx`

---

## 1. Overall Verdict

**Verdict**: **Approved with Minor Improvements (양호, 일부 엣지케이스 개선 권장)**

The implementation directly addresses the root causes of the keyboard typing failure across multiple layers (DOM click handling, active pane state propagation, global keydown capture ownership, and native macOS event monitoring). The fix is well-targeted, verified by automated unit/integration tests, and does not regress existing terminal workflows.

Two edge cases were identified during review that should be addressed before final production deployment:
1. Focus stealing from terminal search input (`TerminalSearchOverlay`) or other embedded inputs.
2. Inactive pane `hoveredPane` fallthrough in `ownsInput` when `active === false`.

---

## 2. Strengths & Positive Highlights

- **Multi-Layer Defensive Design**:
  Rather than relying solely on native AppKit mouse events or only on React state, the fix combines:
  - Immediate DOM focus on `pointerdown` and `click`.
  - Next-frame (`requestAnimationFrame`) and 40ms timer confirmation to survive WebKit's 22ms mouse-handling blur.
  - Rust-level non-consuming `LeftMouseDown | LeftMouseUp` local event monitoring for native WGPU surfaces.
- **Explicit Active State Synchronization**:
  Introducing the `active?: boolean` prop to `NativeTerminalPane` bridges the gap between Zustand's layout state (`tabLayout.activeLeafId === leafId`) and the terminal's keyboard capture pipeline. Keystrokes falling back from `document.body` now deterministically route to the active pane.
- **Symmetric Backend Focus Notification**:
  `sendFocus(false)` is now dispatched when a pane transitions to inactive, ensuring ghostty backend cursor drawing state remains synchronized.
- **Zero Test Regressions & Solid New Coverage**:
  Existing test suites (135/135 in `NativeTerminalPane.test.tsx`, 21/21 in `TerminalSplitView.test.tsx`, 38/38 in `workspaceStore.test.tsx`) pass cleanly, and dedicated tests verify the new focus restoration behavior.

---

## 3. Findings & Detailed Review

### Finding 1: Potential Focus Stealing from Search Input (Priority: Medium-High)
- **Location**: `ui/src/components/TerminalSplitView.tsx` (`PaneLeafView`)
- **Code**:
  ```tsx
  onPointerDown={(event) => {
    if ((event.target as HTMLElement | null)?.closest("button")) return;
    focusPaneInput();
  }}
  ```
- **Issue**:
  When a user opens the terminal search overlay (`TerminalSearchOverlay`, Cmd+F) and clicks inside the search `<input data-testid="terminal-search-input">`, `closest("button")` returns `null`.
  Consequently, `focusPaneInput()` executes and calls `.focus()` on the hidden terminal `<textarea>`, stealing focus away from the search input.
- **Recommendation**:
  Guard against all interactive input elements:
  ```tsx
  const isInteractiveInput = (target: HTMLElement | null) =>
    Boolean(target?.closest("button, input, select, textarea:not([data-testid='native-terminal-focus-sink']), [contenteditable='true'], [role='search']"));

  onPointerDown={(event) => {
    if (isInteractiveInput(event.target as HTMLElement | null)) return;
    focusPaneInput();
  }}
  ```

---

### Finding 2: Inactive Pane `hoveredPane` Fallthrough in `ownsInput` (Priority: Medium)
- **Location**: `ui/src/components/NativeTerminalPane.tsx` (`handleCaptureKeyDown`)
- **Code**:
  ```tsx
  const ownsInput = targetedPane
    ? targetedPane === containerRef.current
    : active
      ? true
      : hoveredPane
        ? hoveredPane === containerRef.current
        : fallbackSessionId === targetSessionId;
  ```
- **Issue**:
  When Pane 1 has `active: true` and Pane 2 has `active: false`:
  If the user's mouse happens to rest on Pane 2 while typing, Pane 2's `active` is `false`, so it falls through to `hoveredPane ? hoveredPane === containerRef.current : ...`, which evaluates to `true` for Pane 2!
  Both Pane 1 and Pane 2 now claim `ownsInput = true`. Although the first listener to run calls `event.preventDefault()` and stops the other via `event.defaultPrevented`, relying on listener registration order is fragile.
- **Recommendation**:
  If `active` is explicitly provided (`active !== undefined`), strictly obey `active`:
  ```tsx
  const ownsInput = targetedPane
    ? targetedPane === containerRef.current
    : active !== undefined
      ? active
      : hoveredPane
        ? hoveredPane === containerRef.current
        : fallbackSessionId === targetSessionId;
  ```
  This guarantees that inactive panes never claim input over active panes, while preserving the legacy `hoveredPane` fallback for standalone panes where `active` is omitted.

---

### Finding 3: Timer Lifecycle Management (Priority: Low)
- **Location**: `ui/src/components/TerminalSplitView.tsx` (`focusPaneInput`)
- **Code**:
  ```tsx
  window.setTimeout(focusSink, 40);
  ```
- **Note**:
  `focusSink` uses optional chaining (`sink?.focus()`) and `leafRef.current` safely becomes `null` on unmount, so it does not throw or leak memory. However, holding a `focusTimerRef` and clearing pending timeouts on unmount is best practice in React components.

---

### Finding 4: Dual Event Emissions on macOS (Priority: Low / Informational)
- **Location**: `src-tauri/src/lib.rs` (`install_macos_terminal_focus_monitor`)
- **Code**:
  ```rust
  NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp
  ```
- **Note**:
  Monitoring both `LeftMouseDown` and `LeftMouseUp` causes `NATIVE_TERMINAL_FOCUS_EVENT` to be emitted twice per click.
  On the frontend, `onNativeTerminalFocus` cancels previous animation frames and timers (`cancelAnimationFrame`, `clearTimeout`), making this idempotent. Emitting on `LeftMouseDown` is desirable because it focuses the pane before a drag-select begins. Emitting on both `window` and `app_handle` ensures global event delivery across all Tauri listener registries.

---

## 4. Suggested Action Items

1. Update `PaneLeafView` click and pointerdown guards in `TerminalSplitView.tsx` to ignore clicks inside `input`, `textarea:not([data-testid='native-terminal-focus-sink'])`, and `[role='search']`.
2. Update `ownsInput` in `NativeTerminalPane.tsx` to use `active !== undefined ? active : ...`.
3. Re-run tests to confirm 100% pass rate.
