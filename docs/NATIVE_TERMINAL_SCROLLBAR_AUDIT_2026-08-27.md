# Native Terminal and Agent Scrollbar Audit

## Objective

Verify that the native terminal and agent scrollbar uses Ghostty as the authoritative scrollback
source, that click/drag/wheel input moves the same viewport rendered by the native compositor, and
that split panes and backgrounded agent tabs preserve independent scrollbar state.

## Implementation trace

| Requirement | Current implementation | Evidence |
| --- | --- | --- |
| Visible, interactive scrollbar outside the compositor | `NativeTerminalPane` reserves a 12px DOM strip while the native viewport ends at `right-3`. | `ui/src/components/NativeTerminalPane.tsx` |
| Track click and thumb drag | Pointer coordinates map to a clamped Ghostty row offset and invoke `cmd_native_terminal_scroll` with `{ type: "row", offset }`. | `ui/src/components/NativeTerminalPane.tsx` |
| Wheel scrolling | Wheel deltas invoke the same command with `{ type: "delta", rows }`. | `ui/src/components/NativeTerminalPane.tsx` |
| Ghostty scrollback authority | Rust queries `GHOSTTY_TERMINAL_DATA_SCROLLBAR` for `total`, `offset`, and `len`, and uses `ghostty_terminal_scroll_viewport` for movement. | `src-tauri/src/native_terminal/scroll.rs` |
| Native viewport redraw | A successful scroll command schedules the current session's render on the main thread. | `src-tauri/src/ipc/native_terminal.rs` |
| Background output synchronization | The native daemon pump emits `native_terminal_scrollbar` only when a session's Ghostty scrollbar state changes. | `src-tauri/src/native_terminal/surface_host.rs` |
| Pane isolation | Each listener filters the global event by `sessionId`; each native session retains its own terminal and last scrollbar state. | `ui/src/components/NativeTerminalPane.tsx`, `src-tauri/src/native_terminal/surface_host.rs` |

## Automated evidence

Executed against the current workspace on 2026-08-27:

| Command | Result | Covered contract |
| --- | --- | --- |
| `bun run --cwd ui test -- src/components/NativeTerminalPane.test.tsx src/components/TerminalPane.nativeAlways.test.tsx src/lib/tauri.test.ts` | 65 passed | Track click, thumb drag, wheel payloads, event bridge, concurrent native pane mounting. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract -- --test-threads=1` | 13 passed | Scroll behavior wire format, Ghostty viewport movement, attached-session scrollbar metrics. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract -- --test-threads=1` | 16 passed | Scrollbar events, replay-gap reset, split-session isolation, reattachment lifecycle. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_capability_contract -- --test-threads=1` | 6 passed | Ghostty scrollback moves from bottom to top and back. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed | Rust native terminal implementation type-check. |
| `bun run --cwd ui build` | Passed | TypeScript type-check and production UI bundle. |

`git diff --check` also passed for the scoped scrollbar implementation.

## Required desktop verification

Automated contracts cannot prove the actual AppKit child-view hit-testing and pointer path. Perform
these steps in Ferryx and record each result:

1. In a new terminal, run `echo short`; confirm no thumb is visible.
2. Run `seq 1 200`; confirm the gray thumb appears at the bottom of the right-side strip.
3. Click near the track top; confirm early rows appear and the thumb moves to the top.
4. Drag the thumb from top through middle to bottom; confirm terminal history follows continuously.
5. Wheel upward and downward; confirm rendered Ghostty content and thumb move together.
6. Split the pane, run `seq 1 300` in one leaf and `echo short` in the other; confirm only the
   long-output leaf shows a thumb and scrolling one leaf leaves the other unchanged.
7. Start an agent or a long-running output command, switch to another tab, wait for more output,
   then return; confirm the returned pane shows current output and an accurate thumb position.

## Completion condition

The objective is complete only after all seven desktop observations pass. No desktop input
automation was used for this verification because it is intentionally performed by the user.

## Height-resize flicker correction

### Reported behavior

Increasing pane height made the thumb repeatedly grow and shrink and could visibly flicker the
native terminal. The browser track grows immediately, but Ghostty changes its visible row count
only when the asynchronous native bounds update reaches the surface host. Repeated
`ResizeObserver` callbacks were also able to queue competing bounds IPC requests.

### Correction

`NativeTerminalPane` now keeps at most one bounds request in flight per mounted pane. While that
request is pending, any number of observer callbacks are coalesced to the newest geometry; only
that newest geometry is dispatched after the current request settles. A successful bounds update
immediately refreshes Ghostty's `total`, `offset`, and `len` metrics for the thumb. A local
revision guard prevents an older metrics query from overwriting a newer push event.

### Regression evidence

- `coalesces rapid height increases until the prior native resize completes` proves a 600px resize
  followed by 700px and 800px measurements dispatches only 600px then the final 800px geometry.
- `refreshes the thumb metrics after a height resize changes visible rows` proves a Ghostty
  visible-row increase changes the thumb height without waiting for future PTY output.
- `bun run --cwd ui test -- NativeTerminalPane.test.tsx`: 42 passed.
- `bun run --cwd ui test -- TerminalPane.nativeAlways.test.tsx tauri.test.ts`: 35 passed.
- `bun run --cwd ui build`: passed.

At the final verification attempt, the Rust native-terminal contract command could not compile
because the independently modified `src-tauri/src/native_terminal/renderer/pipeline.rs` contains
literal `\\n` escape text in Rust source. This is outside the scrollbar change and must be repaired
before a freshly built desktop application can provide final native-surface evidence.

### Focused desktop recheck

Create scrollback with `seq 1 200`, then repeatedly increase and decrease the pane height or drag
the split divider. The thumb must change smoothly toward its new size, without oscillating; the
native terminal surface must not flash. Then click and drag the thumb once to confirm the existing
scroll interaction still follows output.
