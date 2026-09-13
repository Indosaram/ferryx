# Wheel regression seam: pre-implementation review

Date: 2026-09-13. Source inspected; no test execution or source fix.

## Existing tests do not cover the failing adapter

`src-tauri/tests/native_terminal_input_boundary_contract.rs`:

- `scroll_boundary_maps_each_behavior_variant_to_engine` at line 304
  converts a scroll enum and calls `term.scroll_viewport` directly.
- `attached_scrollbar_boundary_tracks_native_viewport_position` at line 361
  uses an owned in-memory attachment and calls
  `scroll_attached_native_terminal` directly.
- `scroll_behavior_serde_wire_contract_matches_frontend_payloads` at line 404
  checks the existing rows-only JSON representation.

These tests exercise real terminal state or serialization, but bypass
`cmd_native_terminal_scroll`'s wheel policy and context construction.
They can pass while that command still substitutes pane-center coordinates
and default modifiers. Passing this test target alone therefore cannot
prove native-input-02 repaired.

`src-tauri/src/native_terminal/wheel.rs` already has direct engine tests
for tracking, Shift override, alternate-screen arrows and primary viewport
scrolling. They pass modifiers straight into `compute_wheel_outcome` and
cannot detect their loss in the command adapter.

## Required regression placement

Keep the existing C002 commands and assertions, but place the new regression
at the command's actual production dispatch seam:

1. Dispatch the frontend's wheel context with a noncentral cell and Ctrl.
2. Use the same context construction and dispatch path as
   `cmd_native_terminal_scroll`, not a test-only reimplementation.
3. Enable actual Ghostty mouse reporting and assert exact encoded bytes,
   including button, modifier and coordinates.
4. With primary-screen history, repeat with Shift and assert viewport
   movement with no PTY write.
5. Feed two owned sessions and assert the untouched sibling's viewport
   and output sequence remain unchanged.

If the command needs a small extraction to make this possible, the extracted
function must be the one production invokes. The old adapter behavior must
fail the same assertions before repair. A pure enum test, a mock returning
the desired bytes, or a direct engine call is insufficient.

The in-memory attachment helper creates its own channel and pending task;
its caller uses an exact subscribed session-update signal before output.
This inspected fixture does not connect to a user daemon. This statement
does not authorize running the entire test target without inspecting its
remaining cases and build/profile isolation.

## Preserve existing engine behavior

Current clean tracked `wheel.rs` already maps negative rows to wheel-up/
ArrowUp, positive rows to wheel-down/ArrowDown, and primary scrolling to
`ScrollViewport::Delta(rows)`. Tracking and alternate-screen output repeat
between one and five ticks. Git reported no working diff for this file
at HEAD `b7ad45163e6d90f2c6ae4410a821e57f7198f5e0`.

An earlier compacted description of reversed direction and single-event
output is not current source evidence. Do not add a redundant direction
fix or alter tick policy without a separate failing scenario.

The still-confirmed defects lie in native hit delivery, frontend delta
normalization and command context loss. Real HWND wheel delivery and
opposite-direction viewport movement remain unverified and require the
registered Windows debug runtime scenarios.
