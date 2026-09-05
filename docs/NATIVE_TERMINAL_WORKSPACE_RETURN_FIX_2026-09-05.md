# Native terminal workspace-return repair

Date: 2026-09-05

## Outcome

Implemented the confirmed attachment, presentation, ownership, and recovery
defects found in the [workspace-return investigation](NATIVE_TERMINAL_WORKSPACE_RETURN_INVESTIGATION_2026-09-05.md).
Warm reattachment now also reasserts the pane's dimensions to the daemon PTY,
even when the retained native grid already has those dimensions.

These fixes do not establish that the reported screen with text concentrated
near the top has been reproduced or eliminated. That visible incident was not
reproduced during this work. In particular, no incident-time mismatch between
daemon PTY dimensions and the native grid was observed.

## Changes

- `ui/src/lib/nativeTerminalLifecycle.ts`: reuse awaits the original pending
  attachment and propagates its failure. An attachment arriving after detach
  started waits for that detach and performs a new attach. This increment and
  its regression tests were committed as `8ed71b3`.
- `src-tauri/src/native_terminal/surface_host.rs` and
  `src-tauri/src/ipc/native_terminal.rs`: bounds receipts carry `presented`.
  A dropped frame returns `false`; only the path that presents the frame and
  reveals the child surface returns `true`.
- `ui/src/components/NativeTerminalPane.tsx`: a dropped-frame response keeps the
  outgoing pane alive, leaves geometry uncached, and requests another bounds
  render on the next animation frame. Unmount cancels that retry.
- The host lock now covers the attachment check through layout, host creation,
  and presentation. Detach, close, and teardown acquire host ownership before
  the session lock. Detached layout work is rejected rather than restoring
  obsolete geometry.
- Lagged and Gap recovery schedule the ordinary render coordinator. Scheduled
  renders retain a follow-up request after a dropped frame. Follow-up dispatch
  goes through an async task because Wry dispatches inline on the main thread;
  direct recursive dispatch would recurse through repeated acquisition failures.
- Warm reattachment preserves the existing terminal snapshot while notifying
  the PTY resize sink even when local grid dimensions have not changed.

## Regression evidence

The initial failing runs demonstrated three attachment lifecycle failures, two
dropped-frame frontend failures, and three backend failures: detached layout
restoration, recovery repaint scheduling, and missing warm-return PTY resize.
The final checks below passed after the fixes.

Frontend command:

```bash
bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/lib/nativeTerminalLifecycle.test.ts
bun run --cwd ui build
```

Result: 3 test files and 157 tests passed in one final run; TypeScript and Vite
build completed with exit code 0. The logged `mount attach failed` messages
belong to intentional attachment-error test cases, not failed tests.

Backend commands:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib bounds_receipt_serializes_actual_presentation_status -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract offscreen_render -- --test-threads=1
cargo check --manifest-path src-tauri/Cargo.toml
```

Result: 27 surface-host tests, 1 IPC serialization test, 18 surface-host contract
tests, and 7 real WGPU offscreen-render tests passed, for 53 backend tests.
The combined final monitor completed with exit code 0, including `cargo check`.
An earlier combined run passed 26 tests but timed out while `cargo check` was
waiting for the shared build-directory lock; it was not counted as a successful
build. The final run used a longer monitor deadline and included the additional
ownership guard test.

The new ownership test holds the same host guard used by production rendering,
starts detach on another thread, and confirms detachment after releasing that
guard. The recovery test subscribes to the update channel before delivering
Lagged and Gap messages and asserts the coordinator's follow-up request; it
replaces the former polling loop. These are ownership and scheduling checks,
not an end-to-end native-window recovery test.

Language-server diagnostics reported no errors in the changed Rust and frontend
files. `git diff --check` passed. Existing compiler warnings remain in unrelated
input, font-manager, notification, PTY, and worktree code; they were not suppressed
or changed for this repair.

## Browser and GPU checks

An isolated Bun WebView running system WebKit loaded the real
`NativeTerminalPane` component and its lifecycle module through a temporary Vite
server. Only the Tauri IPC boundary was simulated. The first incoming bounds
response was `presented: false`; the next response was held by an explicitly
controlled promise until the outgoing pane's retention had been checked.

Observed result:

```json
{
  "retainedUntilPresentation": true,
  "incomingAttempts": 2,
  "detachedAfterPresentation": true,
  "pane": { "width": 800, "height": 568 },
  "visible": "true"
}
```

The recorded command sequence was outgoing bounds, incoming bounds, dropped
response, incoming retry, held retry response, and outgoing detach after release.
There was no DOM error alert. The temporary browser and server were closed.

This proves the frontend's real-browser lifecycle behavior with controlled IPC
responses. Separately, the offscreen tests executed the real WGPU renderer,
including ANSI/CJK, glyph blending, residual viewport pixels, Retina scale,
emoji, and overlays. Neither check injects a swapchain timeout into a live
Ferryx native child window, and neither is a screenshot reproduction of the
user's reported workspace-return failure.

## Remaining desktop confirmation

Use exactly `bun tauri dev` for a debug desktop run. On the affected terminal,
switch to a different workspace and return repeatedly, including after the
terminal becomes quiet. Confirm that the pane is not blank or stale and that
text no longer remains concentrated near the top. Preserve the existing agent
session; do not reset it for this check.

If the symptom recurs, capture the same backend session's DOM bounds, native
bounds, local grid dimensions, daemon PTY dimensions, cursor position, and grid
snapshot at the failure. Also record whether resizing the pane repairs it. A
late geometry command from an older mounted owner remains an unproven ordering
hypothesis; this patch does not add cross-owner geometry revisions.

The user's desktop was not driven or manipulated. No Windows or Linux desktop
run, release build, or release installation was performed. The final tests ran
against the shared working tree; unrelated sessions' staged and unstaged changes
were preserved and excluded from this repair's commit scope.
