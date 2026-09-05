# Native Terminal Multi-Click Text Selection (Double-Click & Triple-Click)

## Overview
Ferryx native terminal (powered by `libghostty-vt` + `wgpu`) supports mouse text selection gestures. Previously, single-click mouse drag was implemented, but double-clicking to select a word or triple-clicking to select an entire line (matching `cmux`, Ghostty desktop, and macOS Terminal) did not trigger.

## Root Cause
Libghostty-vt interprets multi-click sequences via its `SelectionGesture` state machine. In `ghostty/vt/selection.h` and `SelectionGesture.zig`:
- Repeated mouse presses within `repeat_interval` and `max_distance` increment the internal `left_click_count` (1 = cell/drag, 2 = word selection, 3 = line selection).
- To detect repeat clicks, `SelectionGesture.press` requires:
  1. `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS` (event time in nanoseconds)
  2. `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS` (maximum interval between clicks, typically 500ms)
  3. `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE` (maximum allowable distance between click coordinates)
- In Ferryx:
  - `src-tauri/src/native_terminal/sys/types.rs` only defined options 0 (`REF`), 1 (`POSITION`), and 8 (`GEOMETRY`).
  - `src-tauri/src/native_terminal/selection.rs` only set `OPT_REF` and `OPT_POSITION` on `MouseAction::Press`.
  - Without `TIME_NS`, Ghostty treated every press event as untimed (`time == null`), causing `pressRepeat` to immediately fail with `error.PressRequiresReset`. As a result, every subsequent click reset `left_click_count` to 1 and returned `GHOSTTY_NO_VALUE` (clearing any active selection).

## Implementation Details

### 1. C FFI Constants (`src-tauri/src/native_terminal/sys/types.rs`)
Defined the missing `GhosttySelectionGestureEventOption` constants:
- `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE: c_int = 2`
- `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS: c_int = 3`
- `GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS: c_int = 4`

### 2. Mouse Event DTO (`src-tauri/src/native_terminal/mouse.rs`)
Extended `MouseEvent` with optional `timestamp_ns`:
```rust
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub position: MousePosition,
    pub modifiers: KeyModifiers,
    pub size: Option<MouseRendererSize>,
    #[serde(default)]
    pub timestamp_ns: Option<u64>,
}
```

### 3. Gesture Configuration (`src-tauri/src/native_terminal/selection.rs`)
On `MouseAction::Press`:
- Set `OPT_REPEAT_DISTANCE` to `(cell_width as f64 * 2.0).max(16.0)` pixels.
- Set `OPT_REPEAT_INTERVAL_NS` to `500_000_000` ns (500 ms).
- Set `OPT_TIME_NS` using `event.timestamp_ns` or falling back to Rust's monotonic clock (`std::time::Instant`).
- When `ghostty_selection_gesture_event` returns `GHOSTTY_SUCCESS` (on double-click word or triple-click line), `install_selection` installs the resulting `GhosttySelection` snapshot into the terminal.

### 4. Drag & Release Continuation
- `SelectionGesture` retains the gesture's click count and behavior (`.word` or `.line`) across `Release`.
- If the user double-clicks and drags, `MouseAction::Motion` invokes `dragSelectionWord`, expanding the selection word-by-word.
- If the user triple-clicks and drags, `MouseAction::Motion` invokes `dragSelectionLine`, expanding the selection line-by-line.
- A single click after >500ms or far away resets the sequence and clears the selection.

### 5. Frontend Timestamp Forwarding (`ui/src/components/NativeTerminalPane.tsx`)
In `sendMouse`, DOM `event.timeStamp` is converted to nanoseconds:
```typescript
const timestampNs = Math.round(
  (typeof event.timeStamp === "number" && event.timeStamp > 0
    ? event.timeStamp
    : typeof performance !== "undefined" && typeof performance.now === "function"
      ? performance.now()
      : 0) * 1_000_000,
);
```
Passed in the `cmd_native_terminal_mouse` event payload.

## Verification
- **Rust backend unit tests** (`cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal_double_click`):
  - `native_terminal_double_click_selects_word_and_triple_click_selects_line`: verifies double-click selects word, triple-click selects full line, and subsequent click after timeout clears selection.
  - `native_terminal_double_click_drag_extends_selection_by_word`: verifies double-click followed by drag extends selection word-by-word.
- **Frontend test suite** (`bun run --cwd ui test NativeTerminalPane.test.tsx`):
  - `forwards double-click and triple-click pointerdown events with monotonic timestamps for native text selection`: verifies 3 consecutive clicks pass increasing monotonic nanosecond timestamps.
  - All 128 tests in `NativeTerminalPane.test.tsx` passed.
- **Frontend build** (`bun run --cwd ui build`):
  - Completed with 0 TypeScript/build errors.
