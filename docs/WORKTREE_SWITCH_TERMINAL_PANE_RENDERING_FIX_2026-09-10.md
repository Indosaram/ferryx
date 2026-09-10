# Worktree Switch Terminal Pane Blank Rendering & Cursor Artifact Fix

- **Date**: 2026-09-10
- **Scope**: Native terminal rendering pipeline (`render_pass.rs`), surface host lifecycle (`surface_host.rs`), and unit tests (`terminal.rs`).

---

## 1. Symptom & Problem Statement

When switching between worktrees or projects in Ferryx:
1. One or more terminal panes would initially appear blank (rendering only a dark background with no text).
2. A single phantom cursor was rendered at the top-left corner `(0, 0)`.
3. Interacting with the window or resizing the split pane immediately caused the terminal contents to appear properly.

---

## 2. Root Cause Analysis

### A. Phantom Cursor at `(0, 0)` (`render_pass.rs`)
In `src-tauri/src/native_terminal/render_pass.rs`:
```rust
let (x, y) = if viewport_has_value {
    (raw_cursor.viewport_x, raw_cursor.viewport_y)
} else {
    (0, 0)
};
```
When Ghostty's underlying VT cursor is located outside the currently displayed viewport (for instance, when terminal content is scrolled up or when the viewport has not yet caught up), `viewport_has_value` is `false`.
However, `visible: visible` was directly passed through without checking `viewport_has_value`.
As a result:
- The cursor coordinates fell back to `(0, 0)`.
- `visible` remained `true`.
- The renderer drew a visible cursor block at `(0, 0)` over a blank or scrolled area, creating the visual artifact of a stuck cursor at `(0, 0)`.

### B. Unchanged Dimensions Layout & Viewport Anchor (`surface_host.rs`)
When returning to a workspace where the container dimensions did not change:
- `prepare_session_layout` and `reattach_existing_session_with_bounds` evaluated `if session.terminal.dimensions()? != (layout.cols, layout.rows)`.
- If dimensions were identical, the branch was skipped.
- While skipped, sessions that were at the bottom (`is_at_bottom == true`) did not have their viewport explicitly aligned to the bottom (`ScrollViewport::Bottom`).
- While resizing changed cols/rows and triggered the resize branch (which scrolls to bottom or target ratio), an unresized warm return could leave the viewport misaligned until the user triggered a resize gesture.

---

## 3. Architectural Review Points (opencodex/gpt-6-astra)

1. **Do not trigger unconditional PTY resize on attach**:
   - Spawning SIGWINCH on every worktree switch disrupts interactive CLI apps (e.g. `vim`, `less`, `htop`, Claude/Codex TUI).
   - Only resize if dimensions actually differ.
2. **Do not unconditionally force `scroll_viewport(Bottom)`**:
   - Forcing bottom scrolling breaks user scrollback inspection when switching back and forth between worktrees.
   - Only scroll to bottom if `is_at_bottom` is true; otherwise, preserve the user's scrollback position.
3. **Suppress cursor visibility when `viewport_has_value == false`**:
   - If the cursor is outside the viewport, `visible` must evaluate to `false`.

---

## 4. Implementation

### 1. `src-tauri/src/native_terminal/render_pass.rs`
- Updated cursor visibility to:
  ```rust
  visible: visible && viewport_has_value,
  ```

### 2. `src-tauri/src/native_terminal/surface_host.rs`
- In `prepare_session_layout` and `reattach_existing_session_with_bounds`:
  - Hoisted scrollbar state inspection outside the dimension comparison.
  - If dimensions match but `is_at_bottom == true`, explicitly invoke `session.terminal.scroll_viewport(ScrollViewport::Bottom)`.
  - Preserves scrollback ratio if `is_at_bottom == false`.

### 3. `src-tauri/src/native_terminal/terminal.rs`
- Added unit test `off_viewport_cursor_suppresses_visibility`:
  - Feeds 20 lines into a 5-row terminal.
  - Verifies bottom snapshot has `cursor.visible == true`.
  - Scrolls to top and verifies `cursor.visible == false` and `(x, y) == (0, 0)`.

---

## 5. Verification

- `cargo check --manifest-path src-tauri/Cargo.toml` -> exit code 0
- `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::terminal::tests::off_viewport_cursor_suppresses_visibility` -> 1 passed (ok)
- `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::warm_return_reasserts_pty_size_without_replaying_terminal` -> 1 passed (ok)
- `vitest run --maxWorkers=1 src/components/NativeTerminalPane.lifecycle.test.tsx` -> 30 passed (ok)
