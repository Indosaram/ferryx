# Native terminal split-pane focus routing — 2026-08-29

## Symptom

Clicking a native terminal pane did not move keyboard ownership. Input continued
to go to another split pane.

## Root cause

Every visible `NativeTerminalPane` installs a document-level keyboard fallback.
On macOS, the pointer-transparent WGPU child surface can leave the click absent
from the DOM: no `pointerdown`, `mousedown`, or `click` event reaches React, and
the old hidden textarea remains focused. The first pane listener then consumes
the next BODY-targeted key event before sibling listeners can route it.

Expanding DOM click listeners did not fix the live app because those events were
not emitted. Runtime traces showed both pane listeners capturing each key while
`input.sent` stayed on one backend session.

## Initial mitigation and limitation

The first working mitigation checked the hovered visible `.terminal-host` when
the keyboard fallback ran. It corrected backend routing, but ownership was not
established until the first key and WebKit hover state could lag behind a click.
The user observed slow switching and occasional two-click transitions, so this
was not accepted as the final fix.

## Final fix

A macOS local `NSEvent` monitor observes `LeftMouseUp` without consuming it.
The original event pointer is always returned, preserving the existing React
selection, drag, and wheel path. For events belonging to the Ferryx main window,
the monitor converts AppKit bottom-left window coordinates to the logical
top-left coordinates used by native terminal bounds.

`NativeTerminalSurfaceHostState` maps that point to one attached backend session
using half-open viewport bounds. The backend emits `native_terminal_focus`, and
the matching `NativeTerminalPane` immediately focuses its hidden input sink.
There is no renderer call, async IPC roundtrip, hover lookup, debounce, or wait
for the first key in this ownership path.

Runtime measurement showed that a `LeftMouseDown` event reached the pane at
0ms, its animation-frame confirmation ran at 4–5ms, and WebKit still moved
focus back to BODY at 22–24ms. The ownership event therefore moved to
`LeftMouseUp`, the natural boundary after WebKit's mouse-down focus handling.
The pane also confirms sink focus on the next animation frame. Its normal
`onFocus` path then calls
`cmd_native_terminal_set_focus(true)`, which redraws the active cursor without
waiting for typed input. Repeated native focus events coalesce to one frame and
the pending callback is cancelled on unmount.

## Immediate Korean IME input after switching

The deferred sink focus exposed another race when Korean input started
immediately after clicking a different pane. The first Hangul keydown could
arrive while `document.activeElement` was still BODY and before WebKit emitted
`compositionstart`. The document fallback treated that non-ASCII key as a plain
printable character and sent the isolated jamo directly to the PTY. Subsequent
keys entered normal composition, producing visible jamo separation.

The BODY keyboard fallback now handles non-ASCII printable keys by focusing the
matching sink without preventing or forwarding the key. WebKit can start IME
composition with that first key, and only the completed composition text is
sent at `compositionend`. ASCII printable fallback and control chords retain
their existing direct paths.

The final runtime fix moved native ownership emission from `LeftMouseDown` to
`LeftMouseUp`. Measured debug traces showed mouse-down focus confirmation at
4–5ms followed by WebKit resetting focus to BODY at 22–24ms. With mouse-up
ownership, the final debug trace showed focus confirmation at 88ms,
`composition.start` at 377ms, and the first Hangul key at 378ms. No isolated
jamo was sent; the first PTY input arrived only after `composition.end` as one
completed character. The user verified the behavior in the app launched with
exactly `bun tauri dev`.

## Verification

- Native bounds contract covers pane interiors, origins, exact split boundaries,
  outside points, and detached panes. Bounds are half-open, so a divider point
  maps to exactly one session.
- Frontend regression starts with the left sink focused and emits native click
  ownership for the right backend session. It then simulates WebKit moving focus
  to BODY and verifies the deferred frame restores the right sink.
- Immediate-Hangul regression dispatches a BODY-targeted `ㄱ` before the deferred
  frame, verifies no isolated jamo is sent, then completes composition as `가`
  and verifies that composed text reaches the clicked backend session.
- `NativeTerminalPane.test.tsx`: 71 passed.
- `NativeTerminalPane.lifecycle.test.tsx`: 7 passed.
- Frontend TypeScript and production Vite build passed.
- Release-app validation must confirm each single click produces
  `terminal.surface.focus.native` for the clicked backend session before its
  first `terminal.surface.input.sent` event.

## Shared-workspace note

`NativeTerminalPane.tsx` and its test already contained concurrent paste and
drag/drop work. The focus fix was kept surgical, but these shared files were not
committed independently to avoid absorbing another session's changes.
