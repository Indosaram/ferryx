# Ferryx Native Terminal Drag-Selection Resolution

## 1. Issue Summary
In Ferryx native terminal (libghostty-vt + WGPU native surface), dragging with the mouse to select text was either failing or not rendering the active selection highlight in the viewport.

## 2. Root Cause Analysis
1. **Selection Screen vs Viewport Row Offset**:
   - `selection_range` in `src-tauri/src/native_terminal/selection.rs` converted Ghostty grid points using `GHOSTTY_POINT_TAG_SCREEN`.
   - `GHOSTTY_POINT_TAG_SCREEN` coordinates are absolute row indices spanning the full history buffer (including scrollback).
   - The renderer (`instances.rs`) expects 0-indexed viewport row coordinates `[0, visible_rows)`.
   - Without adjusting for the current scrollbar offset (`start_screen.y - offset`), any history or scrollback offset shifted selection coordinates out of the visible row range, causing `selection_range` checks to drop or misplace highlights.
2. **Mouse Tracking Bypass for Left-Click Selection**:
   - When interactive CLI agents (like TUI/omo) enable terminal mouse tracking, mouse motions are forwarded as PTY escape sequences rather than OS text selection.
   - `cmd_native_terminal_mouse` needed strict routing (`is_plain_left_selection`) to guarantee native selection gestures take priority when the user performs standard left-click drag selections without modifier keys.
3. **Execution Environment & Dev Runner**:
   - The user's running environment was previously executing the installed release `.app` bundle from `/Applications/Ferryx.app` which lacked the latest source fixes.
   - Per explicit workflow constraints, launching via `bun tauri dev` compiled and mounted the debug binary with the corrected selection and scrollback offset logic.

## 3. Key Fixes Applied
- `src-tauri/src/native_terminal/selection.rs`:
  - Queried `super::scroll::query_scrollbar(handle)` to obtain `offset` and `len`.
  - Normalized `start_screen.y` and `end_screen.y` by clamping and subtracting `offset` to produce correct 0-based viewport row bounds `(start_col, start_row, end_col, end_row)`.
  - Screen rows outside the visible viewport range are clamped to `(0, 0)` or `(cols - 1, len - 1)`.
- `src-tauri/src/ipc/native_terminal.rs`:
  - Verified `is_plain_left_selection` logic ensuring left-mouse button drag actions directly invoke `select_attached_native_terminal_with_mouse`.
- `ui/src/components/NativeTerminalPane.tsx`:
  - Verified window pointer event lifecycle: `onPointerDown` initiates `pointerDragRef`, throttles `Motion` events to the backend on `pointermove`, and releases cleanly on `pointerup`.

## 4. Verification
- Cargo contract test: `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract test_pointer_drag_selection_renders_visible_selection_artifact_png` (passed with visible gold selection background RGBA validation).
- Frontend Vitest suite: `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx` (all pointer press, drag selection, and mouse tracking tests passed).
- User manual E2E check in live `bun tauri dev` environment confirmed text drag selection is fully functional.
