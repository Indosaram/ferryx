# Terminal presentation behind overlays

## Outcome

On macOS, DOM dialogs, terminal search, and pane drop feedback no longer detach
the native terminal below WebKit. They suspend terminal interaction instead.
Already-presented terminal content remains visible when its session exits, until
the pane is unmounted or a replacement session is presented. A pane with no
presented content receives an opaque terminal-theme background.

Windows and Linux retain their existing native-surface yielding behavior, including
exit, because their surfaces cannot use the macOS WebKit layering contract.
Explicit owner visibility remains authoritative on every platform.

This work does not replace the installed release, launch the desktop application,
restart a daemon, create a commit, or change the daemon protocol.

## Implementation

- `ui/src/lib/nativeTerminalVisibility.tsx` separates display visibility from
  interaction availability. Covering dialogs/search and pane drop feedback
  disable interaction on macOS without removing the underlying native surface.
- `ui/src/components/TerminalSplitView.tsx` marks targeted drop feedback as
  occlusion rather than unconditional owner hiding.
- `ui/src/components/NativeTerminalPane.tsx` separates attachment ownership from
  live input ownership. It tracks successful presentation separately from DOM
  mount/visibility, retains a shown macOS surface after exit, and releases the
  outgoing surface after the replacement presentation receipt.
- The input owner is invalidated when an overlay takes focus or the session exits.
  Delayed input failures cannot recover/retry into that departed owner. Delayed
  attachment completion cannot take focus back from a dialog.
- Retained exited surfaces cannot request a new attachment to their dead PTY,
  including through the geometry-error retry action.
- `ui/src/index.css` gives an unpresented native pane an opaque `--terminal`
  background without painting over an adjacent presented surface. The existing
  opaque macOS window backing handles space outside native pane coverage.
- `docs/DESIGN.md` records this bounded platform and presentation contract.

Retention is scoped to the mounted pane's existing presentation, not a persisted
screenshot archive. A cold restored exited pane has no native frame to retain and
uses the opaque fallback. Genuine unmount releases the native surface.

## Regression evidence

Observed failures before the relevant fixes:

- macOS dialog/search overlays changed visibility to false.
- Exit detached the last presented surface immediately.
- Reconnect released the outgoing frame before the replacement was presented.
- An overlay arriving during an input request allowed recovery to reattach.
- A late attachment completion stole the dialog button's focus.
- An unpresented pane computed a fully transparent background.
- Retaining an exited surface outside macOS would cover the disconnect overlay.
- Retained-frame geometry recovery reattached a dead PTY.

These regressions pass after the changes. Additional cases cover explicit owner
hiding, non-macOS overlay yielding, cold exited panes, late first-frame receipts
after exit, genuine unmount cleanup, and input suppression during a modal.

Final shared-tree focused run:

```sh
cd ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/NativeTerminalPane.presentation.test.tsx \
  src/nativeTerminalPresentationBacking.test.ts \
  src/lib/nativeTerminalVisibility.test.tsx \
  src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx
```

Result: 4 test files, 25 tests passed, exit 0.

## Isolated integration validation

Other sessions were editing the same working tree. A verification worktree was
created at `/tmp/ferryx-overlay-qa-1788879608431`, based on `a7e24d3`.
Only this session's changes were applied to that copy. Foreign removals of the
pane inset constants, error-backing changes, and unrelated tests were excluded.

The following 10 suites passed together, 255 tests total, exit 0:

- `nativeTerminalPresentationBacking.test.ts`
- `nativeTerminalPlatformTransparency.test.ts`
- `components/NativeTerminalPane.test.tsx`
- `components/NativeTerminalPane.lifecycle.test.tsx`
- `components/NativeTerminalPane.presentation.test.tsx`
- `components/TerminalSplitView.dragFeedbackVisibility.test.tsx`
- `lib/nativeTerminalVisibility.test.tsx`
- `lib/nativeTerminalLifecycle.test.ts`
- `components/TerminalPane.test.tsx`
- `components/dag/DagPaneBadge.test.tsx`

`bun run build` in that worktree completed successfully after the final source
changes. Both worktrees passed `git diff --check`.

The shared full build was blocked by concurrent removal of
`NATIVE_TERMINAL_BOTTOM_INSET_PX` and `NATIVE_TERMINAL_HANDLE_INSET_PX` while
`TerminalPane.tsx` still imported them. The same incomplete change caused 18
`TerminalPane.test.tsx` failures; 236 other tests passed in that shared run.
An earlier unrelated `RemoteDirectoryPicker.test.tsx` type error disappeared
between compiler runs. None of those foreign changes was reverted or repaired
by this session.

LSP diagnostics were unavailable because the LSP daemon socket was unreachable.
TypeScript compiler validation is provided by the successful isolated build.
Delegation/review providers failed authentication or usage limits, so no
independent reviewer verdict is claimed.

## Browser evidence and its boundary

The fixture at `ui/src/test/terminalPresentationFixture.tsx` mounts the real
`TerminalPane`, `NativeTerminalPane`, and DAG modal components with the current
CSS in an isolated WebKit view. Its Tauri bridge and underlying terminal frames
are simulated; it never contacts the user's daemon.

Six fresh 1280x850 captures and their machine-observed states are in
[`evidence/terminal-overlay-20260909/`](evidence/terminal-overlay-20260909/):

- `01-live-and-cold.png`: two presented frames and one opaque cold exited pane.
- `02-dag-open.png`: both terminal frames retained; all terminal input disabled;
  zero detach calls.
- `03-dag-closed.png`: input restored; still zero detach calls.
- `04-disconnected.png`: exited pane's last frame retained; its input disabled.
- `05-reconnected.png`: replacement frame shown; exactly one outgoing detach.
- `06-moved.png`: both native-frame stand-ins follow the changed pane geometry.
- `states.json`: presentation/input state, computed backgrounds, frame IDs,
  detach counts, and an empty script-error list for every final capture.

The cold pane computed `rgb(40, 44, 52)`, while displayed panes remained
transparent to their underlying frame. The browser and Vite fixture server were
closed after capture. Screenshots were generated, but this session's model could
not visually read image attachments. No actual Metal/AppKit screenshot acceptance
or live desktop compositor verdict is claimed.

## Required desktop acceptance

Use the signed debug application through exactly `bun tauri dev`; do not restart
the background daemon. The shared source's concurrent build issue must first be
resolved by its owner, or use the isolated verification copy with the normal
project debug-build prerequisites.

1. Open and close a running pane's DAG modal. Terminal content should remain
   behind the modal and its dimming layer. Typing into the modal must not reach
   the PTY; closing it must restore normal input.
2. Open terminal search and Settings. Verify their controls remain above the
   terminal and keyboard/IME input stays with the covering surface.
3. Move a live pane between splits and tabs. Confirm no desktop is exposed,
   the drop feedback remains visible, and the terminal follows its destination.
4. Exit a visible agent/session. Confirm the final terminal frame remains beneath
   the disconnected notice. A cold restored exited pane should have a solid
   terminal background instead.
5. Reconnect. Confirm the new frame replaces the old frame without a transparent
   gap. Close the pane and confirm its retained surface disappears.
6. On Windows/Linux, verify overlays and exited-state controls remain visible
   with the existing yielding behavior.

Implementation was delivered uncommitted before the user's follow-up commit
request. That request authorizes committing only this isolated change, not the
unrelated work still in the shared tree. The verification copy remains available
for comparison.
