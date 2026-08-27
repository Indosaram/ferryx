# Native Terminal Scrollbar

## Behavior

Native terminal and agent panes now reserve a visible 12px right-side strip for a vertical
scrollback bar. The native compositor is above the WebView, so the strip is excluded from the
native viewport; it remains visible and interactive rather than being painted over.

Ghostty supplies the authoritative `total`, `offset`, and viewport `len`. The thumb is hidden
when no scrollback exists, sized as `len / total`, and positioned by the current viewport offset.
It updates from the native output stream, after wheel scrolling, and after a drag finishes.

## Interaction

- Click the track to move the visible terminal viewport to the corresponding history row.
- Drag the thumb to scrub through scrollback.
- Mouse wheel scrolling and live output refresh the same thumb position.
- The control uses a semantic vertical `scrollbar` role with current/minimum/maximum values.

## Verification

- `NativeTerminalPane.test.tsx` verifies no-scrollback hiding, event-driven thumb position,
  track click mapping, and row-scroll IPC.
- `native_terminal_input_boundary_contract` verifies the native scrollbar state changes between
  live-bottom and top-of-history positions.

## Required desktop QA

1. Start Ferryx and run `seq 1 200` in a native terminal pane.
2. Confirm the thin gray scroll thumb appears at the bottom right.
3. Click the top of the track and confirm early lines appear; drag the thumb and confirm history
   follows continuously.
4. Scroll the mouse wheel in either direction and confirm both terminal content and thumb move.
5. Confirm a fresh short-output terminal has no scrollbar, and split panes show independent bars.
