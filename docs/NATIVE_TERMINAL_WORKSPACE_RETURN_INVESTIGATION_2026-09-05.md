# Native terminal workspace-return investigation

Date: 2026-09-05

This is the pre-fix investigation record; its line references describe that
snapshot. The subsequently authorized implementation and completed checks are
recorded in [the repair report](NATIVE_TERMINAL_WORKSPACE_RETURN_FIX_2026-09-05.md).
The original visible symptom still requires a desktop reproduction.

## Verdict

The reported symptom is terminal text remaining concentrated near the top of a
pane after leaving a workspace and returning. This investigation has not
reproduced that visible symptom or established its root cause.

One separate lifecycle defect was reproduced in isolation: reusing an attachment
can resolve before the original native attach has completed. Three other
rendering defects were identified statically. They should not be presented as
proof of the reported top-of-pane symptom.

No application source or tests were changed. The shared working tree contains
other sessions' active changes; those were inspected where relevant and preserved.

## Findings

### 1. High: attachment reuse resolves before native readiness

Evidence: `ui/src/lib/nativeTerminalLifecycle.ts:183-239`.

The lifecycle adds the session to `attachedSessionIds` before awaiting the native
operation. A second attach for the same session returns an immediately resolved
promise rather than the pending attachment's result.

An in-memory probe executed the current module's lifecycle functions after Bun
TypeScript transpilation. Only the logging import was replaced with a no-op; the
attach, detach, generation, and queue logic were unchanged. The original attach
was held by an explicitly controlled promise, not a sleep or timed race.

Observed output:

```text
original attach started
replacement attach resolved; nativeReady=false
original attach completed
prematureReuse=true
oldDetachExecuted=false
```

This proves premature reuse independently of the native backend. The visible
blank-pane consequence remains a static inference:

- `NativeTerminalPane.tsx:679-704` uses the lifecycle result as attachment readiness.
- `NativeTerminalPane.tsx:1690-1701` reports bounds after that result resolves.
- `surface_host.rs:1691` rejects a session that is not yet attached.
- `NativeTerminalPane.tsx:1614-1621` ignores that detached error without arranging
  an attach-completion-driven bounds retry.
- The retired effect ignores its eventual attach completion.
- Output-driven rendering uses an existing host; only explicit bounds rendering
  creates a missing host (`surface_host.rs:350-355`, `1701-1706`).

Recommended correction: distinguish attachment readiness from attachment intent,
and have reuse await the same pending native operation, including its failure.
The regression should hold the initial native attach, remount the owner, reject
premature bounds, and assert successful bounds presentation after readiness.

### 2. High: a dropped frame is reported as a presented frame

Static evidence:

- `surface_error.rs:12` classifies a swapchain timeout as a dropped frame.
- `surface_host.rs:1938-1946` returns a successful bounds receipt without a frame.
- Actual presentation and child-view reveal happen at `surface_host.rs:1961-1962`.
- `NativeTerminalPane.tsx:1590-1593` caches the geometry and releases outgoing
  surfaces for every successful receipt, regardless of actual presentation.

A timeout on the first frame can therefore release the outgoing pane while its
replacement remains hidden. An existing surface can retain stale content. A new
output, focus, or bounds event can hide this failure by rendering again; no such
event is guaranteed solely by the dropped frame.

Recommended correction: represent actual presentation explicitly and retain a
render request after a dropped frame. Do not retire the outgoing pane until a
frame has really been presented. Verify with injected first-frame acquisition
failure rather than relying on GPU timing.

### 3. High: detach can interleave with host creation after the attachment check

Static evidence: `surface_host.rs:1468-1487`, `1678-1721`, and
`src-tauri/src/ipc/native_terminal.rs:490-495`, `537-542`.

Bounds rendering checks attachment, releases the session lock, prepares layout,
and later creates or accesses the native host. Detach clears attachment and layout
and removes the host under separate lock scopes. Bounds rendering runs on the
main thread, but the detach command executes independently.

A render that passed its attachment check can resume after detach, reinstall
obsolete layout, and create a visible host for a detached session. Frontend
generation guards prevent future stale lifecycle operations but do not cancel a
bounds command already executing in Rust.

Recommended correction: make ownership invalidation and host installation atomic
with respect to each other, using shared serialization or a generation guard
whose validation cannot race with installation. Verify by pausing the render at
the ownership boundary, detaching, then resuming it. This interleaving was not
executed during this investigation.

### 4. Medium: lag recovery does not schedule a repaint

Static evidence: `surface_host.rs:1289-1399`.

Normal Output schedules the render coordinator. Lagged resets and replays the
grid, restores pane dimensions, and updates the watch sender, but does not
schedule rendering. Gap has the same scheduling omission. No production repaint
consumer of `subscribe_session_update` was identified in the inspected code.

If a previously scheduled render has already drained and output becomes quiet,
the corrected engine grid need not reach the visible surface until another render
trigger. A pending output render can conceal the omission.

Recommended correction: schedule recovery-driven rendering through the ordinary
coordinator. Verify by draining pending work, delivering recovery, and asserting
that the new grid is presented without another output or input event.

## What explains the workspace-return symptom, and what does not

### Normal return does not necessarily replay history

`cmd_native_terminal_attach` first tries
`reattach_existing_session_with_bounds` (`src-tauri/src/ipc/native_terminal.rs:464`).
When both the stream task and pump task are still live, the existing terminal
state is reused (`surface_host.rs:984-1087`). Detach preserves those tasks and the
terminal, while discarding surface geometry (`surface_host.rs:1468-1487`).

It is therefore incorrect to describe every workspace return as replaying the
ring buffer at 80 by 24. Cold attachment and failed-stream recovery are different
paths. Current cold attachment already carries daemon PTY dimensions and supports
dimension-tagged history segments.

### PTY and native-grid dimension divergence is a relevant, unconfirmed hypothesis

Native layout resizes the local terminal first and asynchronously queues a daemon
PTY resize (`surface_host.rs:905-947`,
`src-tauri/src/ipc/native_terminal.rs:404-439`). Reattachment only queues a PTY
resize when the local terminal dimensions change (`surface_host.rs:1027-1086`).
It does not independently compare the daemon's current PTY dimensions.

Consequently, if the PTY is at a different size while the local grid already
matches the returning pane, the return path need not repair that divergence.
An independently resizing client is one concrete conditional source: the remote
terminal handlers call `session_backend.resize` directly
(`src-tauri/src/remote/server.rs:1000-1005`, and the corresponding Resize branch
in `handle_terminal_grid_socket`). A failed asynchronous resize is another
conditional source; the dispatcher logs failure without reverting the local grid.

An application drawing for fewer PTY rows could occupy only the upper part of a
taller native grid. Neither a mismatched PTY size, a failed resize, nor a remote
resize was observed for the reported incident. This is a hypothesis, not a
diagnosis or a claim that remote access was in use.

### Obsolete geometry remains a secondary ordering hypothesis

Bounds coalescing is local to each React effect
(`NativeTerminalPane.tsx:1564-1587`). An old effect and its replacement can each
have an outstanding command. The bridge carries no owner generation or geometry
revision. A late obsolete command could restore an old rectangle, but actual
cross-handler execution inversion was not demonstrated.

### The symptom needs a grid-versus-render comparison

`renderer/renderer.rs:284-406` draws snapshot cells at their cell positions inside
a surface-local viewport. It does not redistribute text to fill the pane height.
Text occupying the top of a tall pane can therefore originate in terminal state,
not necessarily glyph rasterization. That does not exclude a renderer defect;
the decisive evidence is the terminal snapshot and geometry at the same moment
as the visible failure.

## Verification and next decisive observation

Completed:

- Traced the frontend lifecycle, bounds IPC, native attachment and render paths.
- Inspected the current relevant foreign diffs without modifying them.
- Executed the deterministic lifecycle readiness probe described above.
- Reviewed layout calculation, preserved-session reattachment, replay recovery,
  PTY resize dispatch, and remote resize callers.

Not performed:

- No desktop launch, UI automation, or manipulation of the user's running panes.
- No reproduction of the reported top-of-pane display failure.
- No GPU timeout injection, detach/render barrier test, or recovery repaint test.
- No full test suite or build; this is an investigation, not an implementation.

For the next debug reproduction, use exactly `bun tauri dev` and have the user
switch away from the affected workspace and back. At the failure, compare the
same backend session's DOM rectangle, native bounds, local grid dimensions,
daemon PTY dimensions, cursor row, and first nonempty snapshot rows. Record
whether changing pane size restores the layout. Do not use a destructive reset
or replace the agent session to obtain evidence.

Existing frontend traces are written to `/tmp/ferryx-switch-debug.jsonl`
(`src-tauri/src/ipc/debug.rs:32-50`). They include bounds and lifecycle events,
but the event named `terminal.surface.presented` is not proof of GPU presentation
because of finding 2. The current trace does not contain all of the grid/PTy
comparison data above; targeted instrumentation would be needed for a conclusive
diagnosis.
