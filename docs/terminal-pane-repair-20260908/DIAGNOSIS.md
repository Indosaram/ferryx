# Confirmed pane error and resize defects

## Evidence boundary

Three read-only diagnosis nodes completed in
`dag_a1bc6899-f931-4467-bac9-db4f861076d3` after switching the failed provider
route to `opencodex/gpt-6-astra`. The lead checked their causal claims against
current source. Source review alone does not identify the error originally
shown by the installed application.

## Full-pane error occlusion

`ui/src/components/NativeTerminalPane.tsx` rendered a full-pane opaque
`native-terminal-error-backing` whenever attachment, bounds or input failed.
The actual macOS terminal view is below WebKit, so this backing hides healthy
terminal pixels as well as failed surfaces.

The regression `keeps the terminal uncovered when a bounds failure is shown`
failed before implementation: an error alert was present, the pane remained
visible, but the full-pane backing was still present. See `evidence/c1-red.log`.

The repair removes only that backing and retains the localized retry alert,
with the existing opaque popover background and overlay stacking token.
The existing recovery test now requires the alert to remain until presentation
without requiring a full-pane cover.

Input error interpretation is a separate concern: the backend writes to the
daemon before querying its receipt. An unsuccessful receipt does not imply
unsuccessful input delivery. The existing frontend condition that permits
automatic replay only for explicit `inputWritten=false` must remain intact.

## Obsolete deferred geometry replay

`src-tauri/src/ipc/native_terminal.rs::cmd_native_terminal_set_bounds` used to
call `render` with its original captured geometry every time synchronized
output woke its presentation wait. Rendering first changes the VT layout and
queues PTY resize, then checks whether presentation must be deferred.

Frontend geometry serialization is local to one React effect. A replacement
owner can reuse the same native session without a real detach; therefore an
older native command can remain outstanding while a newer owner installs a
new width.

The deterministic native test
`deferred_bounds_retry_does_not_restore_obsolete_width` exercises:

1. Present width 800.
2. Start DEC2026 and apply width 640; the old bounds IPC waits.
3. Deliver the explicit synchronized-output reset through the output pump.
4. Complete a newer bounds IPC with width 900.
5. Resume the old command.

Before the repair, the final session width is **640**, not **900**.
`evidence/c2-red.log` captures the assertion and exit 101. This is an executable
regression through real bounds IPC and Ghostty terminal state, not a timing
hypothesis.

The repair applies the supplied geometry once. Readiness retries use
`render_current`, sharing the existing focus-render path under the established
host/session locks. They redraw the currently accepted layout instead of
mutating it back to obsolete geometry. DEC2026 expiry, detach notification,
PTY queue coalescing and scroll-up behavior are not changed.

The native test checks final session/host bounds, VT dimensions and the final
queued PTY size. An injected frame target avoids desktop interaction. This is
not proof of actual desktop pixels or the kernel PTY resize acknowledgement.

## Overlay policy and concurrent ownership

The shared visibility hook historically suppressed all terminal and browser
consumers for any mounted dialog/search overlay. Current macOS terminal
stacking permits keeping terminal surfaces visible, but interaction must remain
blocked while a covering overlay owns input. Browser child webviews must retain
their masking policy.

During this run, another writer changed `nativeTerminalVisibility.tsx`, its
tests and the corresponding pane lifecycle hunks. Those changes were preserved.
The combined test run currently has three old lifecycle assertions that still
expect macOS modal detachment. They need owner-coordinated integration and
regression proof, not silent removal or a false passing verdict.

## Native regression result

`cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal -- --test-threads=4`
completed with **158 passed, 0 failed, 0 ignored**, exit 0. The obsolete-width
test passed, together with existing synchronized-output, focus, detach and
scrolling tests. See `evidence/c2-green.log`.

## Remaining verification

- Independent compiler/build checks and code review.
- Full frontend integration after concurrent ownership is resolved.
- Actual debug desktop resize/overlay scenarios; Accessibility automation is
  currently denied with `osascript is not allowed assistive access (-1728)`.
- Independent code review, cleanup receipts and verified commits.
