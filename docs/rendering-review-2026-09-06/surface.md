# Native terminal surface review - 2026-09-06

## Result and evidence boundary

Three actionable source findings, ordered by severity below. Two concern frame/geometry
correctness; the third is a Windows native-target teardown contract violation whose
rendering consequence has **not** been reproduced. None is a runtime-confirmed cause of
the reported macOS intermittent rendering. The older detached-present race is already
guarded in the current source and is not a new finding.

- Reviewed HEAD: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931` plus the working tree.
- `git diff --` for the six requested Rust files, `platform/`, and
  `src-tauri/src/ipc/native_terminal.rs` was empty. These branches are committed code,
  not foreign uncommitted fixes. The directly inspected pane/lifecycle TS files were
  also unchanged from HEAD.
- Foreign atlas/row-cache edits, Cargo patch, vendored HAL and other dirty files were
  read-only. No production/test edits, test executions, builds, app launches, desktop
  input or instrumentation were performed. Proposed REDs below are **new tests**, not
  existing passing/failing results; create them before running the commands and verify
  that the runner executes one test rather than silently matching zero.
- PASS here means only a checked source observation. Native presentation, AppKit layer
  state, Windows HWND behavior and Linux compositor behavior remain runtime-unproven.
- Short Rust paths below are relative to `src-tauri/src/native_terminal/` unless
  explicitly prefixed otherwise; `ipc/` is relative to `src-tauri/src/`, and test
  basenames are relative to `src-tauri/tests/` (TS tests remain in their cited UI
  directories). A bare `:line` continues the immediately preceding file reference.

## 1. HIGH / P1 - Wayland buffer geometry is not one coherent contract

**Source-proven mismatch; Linux GUI reproduction not run.** A resize can produce a
buffer whose dimensions are incompatible with `wl_surface.set_buffer_scale`, or a
surface smaller than its pane. This is about missing/clipped terminal area and protocol
validity, not glyph sharpness.

### Branch and caller evidence

- `src-tauri/src/native_terminal/child_surface.rs:69-87` derives an integer
  `buffer_scale` but rounds `logical_width * buffer_scale`, which need not be a
  multiple of that integer for fractional logical widths.
- `src-tauri/src/native_terminal/platform/linux.rs:378-387` consumes only the
  Wayland geometry's position and scale; its `physical_width`/`physical_height`
  never reach the renderer. `platform/wayland_child.rs:372-393` sends that scale
  to `wl_surface.set_buffer_scale`.
- Separately, `composition.rs:151-155` computes the actual render size from the
  original, potentially fractional `bounds.scale_factor`.
  `surface_host.rs:1924-1932` configures the swapchain with that size;
  `renderer/gpu_context.rs:128-139` passes those extents to WGPU without a
  Wayland adjustment.
- Concrete UI caller: `ui/src/components/NativeTerminalPane.tsx:407-419` snaps
  **physical** edges, not integer Wayland logical dimensions. At scale 2 it can
  legitimately return a width of 400.5. `:654-678` sends those bounds and raw
  `window.devicePixelRatio`; `:1589-1593` invokes set-bounds. IPC
  `src-tauri/src/ipc/native_terminal.rs:505-534` forwards them to `render`, which
  calls `prepare_session_layout` at `surface_host.rs:1720-1721`.
- The desktop composition guard (`surface_host.rs:1867-1869`) checks that a real
  child exists, not buffer divisibility or agreement of the two scale calculations.
  It therefore does not reject these otherwise valid bounds.

### Trigger sequences

1. Native Wayland, scale 2, visible split pane. Resize from width 400 to width
   400.5 logical pixels (a one-physical-pixel change permitted by frontend snapping).
   Host size becomes 801 pixels; `buffer_scale` remains 2. At buffer commit, 801
   is not divisible by 2. The Wayland protocol explicitly requires an integer
   multiple and specifies `invalid_size` otherwise. Actual driver/compositor
   behavior on this application's selected adapter remains unobserved.
2. If the WebView reports scale 1.5, width 400 and height 300 produce a 600x450
   WGPU target, while the Wayland helper expects 800x600 and sends scale 2.
   A 600x450 buffer at scale 2 describes 300x225 logical pixels, not the requested
   400x300. This second trigger is conditional on a real frontend reporting that
   fractional scale; no such runtime report was collected here.

Protocol evidence retrieved during this review:
[Wayland `wl_surface.attach`](https://raw.githubusercontent.com/wayland-mirror/wayland/main/protocol/wayland.xml),
lines 1464-1468 of the retrieved file: surface size uses inverse buffer scale;
non-multiple buffer sizes cause `invalid_size`. No `wp_viewport` is installed by
the inspected child implementation to establish a different logical destination.

### Coverage and faithful RED

`src-tauri/tests/native_terminal_wayland_subsurface_contract.rs:50-63`,
`fractional_scale_rounds_up_to_an_integer_buffer_scale`, checks the helper alone
at scale 1.5 with integral logical dimensions. It cannot catch the host's use of
600x450 instead of 800x600. The same file's integer-scale cases also use integer
logical sizes; none checks an 801-pixel buffer at scale 2.

Cheapest first RED: add
`buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional` to that
existing integration target. Given width 400.5, height 300, scale 2; when converting
through the real Wayland geometry helper; then require both extents to be divisible
by the returned buffer scale. This fails the current arithmetic without a GUI or
mock. A separate follow-on case must compare the actual host layout extent against
the Wayland extent at scale 1.5; do not stop at fixing the helper while leaving the
renderer on the original scale.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
```

Recommended repair boundary: derive a single platform presentation geometry used by
both buffer configuration and child placement. For the current integer-scale Wayland
implementation, quantize its logical size consistently and derive buffer dimensions
and cell metrics from that same scale. Do not apply this quantization to macOS/Win32.

**Real-surface acceptance:** on a Linux native-Wayland debug app, record selected WGPU
backend, DPR, outgoing scale/extent and compositor errors; split two panes containing
distinct numbered lines, resize across odd/even physical widths at 2x, then exercise
1.5 only if that DPR is actually reported. Require both pane edges/content to remain
visible with no `invalid_size` disconnect. X11 success would not verify this branch.

## 2. MEDIUM / P2 - One-shot renders lose their final repaint on acquisition timeout

**Source-proven missing retry conditional on `SurfaceError::Timeout`; no timeout was
induced at runtime.** Scheduled output and initial bounds have recovery, but direct
scroll/focus/overlay renders do not. A quiet terminal can retain the previous image
after its VT viewport or visual state has already changed.

### Branch and caller evidence

- `surface_host.rs:1942-1973` obtains a frame, retries Lost/Outdated once, and
  returns a non-presented receipt on Timeout. `surface_error.rs:12` intentionally
  classifies Timeout as a dropped frame, not terminal failure; the receipt starts
  with `presented: false` at `surface_host.rs:156`.
- `surface_host.rs:1751-1758` returns that receipt directly from `render` without
  scheduling anything. The same is true of direct `set_focus` at `:1783-1794`.
- Strong macOS caller: `src-tauri/src/lib.rs:541-575` changes scroll state and calls
  `render` in the native scroll monitor, ignoring its receipt. It consumes the
  native event at `:577`; there is no DOM wheel retry for that event.
- Portable IPC caller: `src-tauri/src/ipc/native_terminal.rs:835-877` changes the
  viewport, then calls `render`. UI scroll handlers at
  `ui/src/components/NativeTerminalPane.tsx:1027-1035` and `:1915-1923` refresh
  scrollbar metadata, not the image, and never inspect `presented`.
  Focus's UI caller at `:629-636` only updates the IME anchor. Sibling overlay and
  attention IPC paths at `ipc/native_terminal.rs:921-931` and `:971-981` discard
  even the receipt via `.map(|_| ())`.
- The existing fix is narrower: `surface_host.rs:369-373,385-389` requeues a
  non-presented **scheduled** frame. It is not reached by those one-shot renders.
  Bounds UI at `NativeTerminalPane.tsx:1596-1602` has its own rAF retry, so the
  allegation that every initial Timeout leaves a blank pane is disproven.

### Trigger sequence

Given an attached, already presented terminal with scrollback, no pending scheduled
frame and no further PTY output: the final scroll changes the VT offset; acquisition
returns Timeout (also possible after Lost -> reconfigure -> Timeout); direct render
returns `presented: false`; caller ignores it. No work is queued to present the changed
snapshot. Later input/output, focus or geometry may incidentally heal it. If an overlay
timer produces another frame, it can also heal it; that is not a guarantee of this path.

### Coverage and faithful RED

`surface_error.rs:26-30`, `native_surface_timeout_drops_frame_without_terminal_failure`,
covers classification only. `surface_host.rs:2376-2405`,
`render_schedule_coordinator_stays_pending_until_frame_completion`, covers coordinator
transitions, not this caller. The frontend test
`NativeTerminalPane.lifecycle.test.tsx:130-160`,
`retains the outgoing pane until a dropped frame is retried and presented`, covers
set-bounds retries only. None of these was run in this review.

Proposed RED name:
`native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped`.
Drive the one-shot host path with a narrow deterministic acquisition/dispatch seam:
given attached session and idle coordinator; when one direct render receives Timeout;
then observe exactly one pending retry dispatch without another update event. Do not
call `schedule_render` in the test to manufacture the outcome and do not substitute a
standalone policy test for integration with `render`. Implementing that seam/test is
future work; no existing fault-injection seam was found in the inspected host.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped -- --exact --nocapture
```

Recommended repair: share dropped-frame rearming across direct and scheduled host
renders, retaining the ownership guard and releasing the host lock before a dispatch
that Wry might execute inline. Do not replace the guard or add independent retry loops
to each IPC caller. Preserve fatal-error handling rather than retrying OOM forever.

**Real-surface acceptance:** in the macOS debug app, show numbered scrollback then stop
output. Arm a single acquisition-Timeout injection for the next native scroll render
and subscribe to present completion before scrolling once. Require the new visible rows
without another input/resize/output. Repeat Lost -> Timeout -> success. Run the same
isolated trigger through portable scrollbar IPC when a Windows/Linux GUI is available.
Wait on acquisition/present events with bounded deadlines, not fixed sleeps.

## 3. MEDIUM / P2 - Windows detach attempts child HWND destruction off its owning thread

**Source-proven native lifetime/API violation; not a demonstrated reappearing pane.**
The hidden child can survive host removal because Windows disallows this destruction.
The preceding `ShowWindow(SW_HIDE)` means this is not proof that it remains visible.

- Creation is main-thread dispatched by `ipc/native_terminal.rs:528-534` through
  `surface_host.rs:1730-1732,1867` to `platform/windows.rs:233-258`.
- Cleanup from `NativeTerminalPane.tsx:1802-1806` goes through the serialized
  lifecycle queue, but IPC `native_terminal.rs:484-490` calls `detach_session`
  directly in an **async** command. Close does likewise at `:494-501`.
- `surface_host.rs:1498-1514` removes the host on that caller's thread while holding
  the hosts mutex; `:1518-1531` does the same for close. Neither dispatches cleanup
  to the UI thread.
- `platform/windows.rs:328-336` calls `ShowWindow` and `DestroyWindow` inline and
  ignores the destruction result. Its visibility flag and the host ownership lock
  do not change which OS thread executes those calls.
- Dependency source confirms async execution rather than assuming it: locked
  `tauri-macros-2.6.3/src/command/wrapper.rs:378-392` uses
  `respond_async_serialized`; locked `tauri-2.11.5/src/ipc/mod.rs:371-388` spawns the
  command future on Tauri's async runtime. These files were read in the local Cargo
  registry, not edited. The versions were checked in `src-tauri/Cargo.lock`.

Trigger: attach/present a Windows pane, unmount it or hide it for an overlay, receive
detach IPC on a runtime worker, remove the host, and enter the Windows target destructor.
[Microsoft's DestroyWindow contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-destroywindow),
retrieved during this review, states: a thread cannot destroy a window created by a
different thread. Reattaching then allocates another native child; the old host is
gone from Rust but native destruction has not been established. Potential UI stalls
from synchronous cross-thread window calls under the hosts lock are **not** promoted
to a confirmed deadlock here.

Coverage: `native_terminal_child_surface_contract.rs:93-113` tests a pure visibility
value, not HWND destruction. `surface_host.rs:2180-2203` checks field order using a
mirror struct, not the production target or OS thread. Host-isolation tests at
`native_terminal_surface_host_contract.rs:717-778` use no WGPU child at all.

Cheapest faithful RED is Windows-native, not a mocked visibility test: a proposed
`native_terminal_windows_teardown_contract` integration harness creates the actual
child on its pumping owner thread, records its handle/owner, then drops it from a
worker as production detach does. Subscribe to `WM_NCDESTROY` before triggering the
drop; require owner-thread destruction with a bounded event wait. Include an
owner-thread cleanup path after a failed assertion so the RED itself leaks nothing.

```sh
# Windows runner only; this target/test must first be added.
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_windows_teardown_contract child_is_destroyed_on_owner_thread_when_detached_by_worker -- --exact --nocapture
```

Recommended repair: transfer the removed host/target to main-thread teardown, retaining
surface-before-target drop order and avoiding a synchronous worker-to-main wait while
holding `hosts`. Audit both detach and close through the same cleanup boundary.

**Real-surface acceptance:** on Windows, repeatedly switch A/B tabs and open/close a
masking overlay with distinct pane contents. Track child creation and owner-thread
`WM_NCDESTROY`, require one live native child per attached pane, keep the main event
loop responsive, and verify no old pane covers the replacement or chrome. This was
not run on the macOS workstation; historical Windows success is not evidence for it.

## Checked lifecycle, macOS layer geometry, and existing fixes

1. **Cold attach -> first bounds -> reveal.** IPC attach `:452-477` first tries a
   retained live stream, otherwise awaits daemon attach. Host attach initializes VT
   state/pump; it does not create a compositor. The pane awaits attach then calls
   `reportBounds` (`NativeTerminalPane.tsx:1703-1716`). `render` creates a host only
   after `lock_attached_hosts`; the target starts hidden. Successful WGPU present
   precedes reveal (`surface_host.rs:1987-1989`). Initial invalid geometry is already
   tolerated in `:979-1008` and `:1011-1111`; tests at `:3178` and `:3226` name those
   cases. This is committed fix `2446f6d`, not a newly diagnosed blank-pane cause.
2. **Detach during presentation.** `render`, `set_focus`, and scheduled rendering
   hold `hosts` through ownership checking and present. `detach_session` acquires
   that same lock before clearing attached/layout state and removing the host.
   Either the current frame finishes before detach, or later rendering observes a
   detached session. An old snapshot cannot be captured before detach and then used
   to resurrect a removed host through these paths. `surface_host.rs:2065-2108`
   tests the detached-layout and ownership-lock seams; these are source-checked tests,
   not native GUI proofs.
3. **Warm reattach.** `reattach_existing_session_with_bounds` checks both tasks are
   live (`:1021-1030`), retains accumulated VT state, restores layout and reasserts
   PTY size even if dimensions match (`:1095-1109`). It does not replay daemon history.
   Frontend per-session queue/generation checks (`nativeTerminalLifecycle.ts:30-58,
   149-177,189-250`) prevent a stale queued detach overtaking the current attach.
   `native_terminal_surface_host_contract.rs:781-853` covers retained background
   output; `surface_host.rs:2024-2061` covers matching-size PTY reassertion.
4. **Output arriving during a frame.** Coordinator transitions in
   `surface_host.rs:192-259` preserve one follow-up. Scheduled callbacks re-read
   current session state under locks, not an earlier captured frame. The pump's
   Output/Lagged/Gap paths all schedule (`:1329-1331,:1396-1398,:1423-1425`).
   Commit `faad966` added the checked ownership guards, presented receipt/retry
   handling, Lagged/Gap scheduling and same-size warm-return resize notification.
   Treat these as pre-existing fixes, not open findings.
5. **Surface loss/resize.** `render_snapshot` updates native geometry/config before
   acquisition; size changes reconfigure, and Lost/Outdated cause one same-size
   reconfiguration/reacquisition. A second Lost/Outdated is a reported error, not a
   successful frame; no claim is made that it self-heals without another trigger.
   Timeout retry differs by caller as finding 2 details. No device-loss/full-host
   rebuild was exercised.
6. **Actual macOS layer path, source only.** `platform/macos.rs:283-294` converts
   DOM bounds using the real superview's height/flipped state and sets the child
   NSView frame; `composition.rs:66-84` is the conversion. The normal NSView backing
   layer is not the drawable: current `vendor/wgpu-hal/src/metal/surface.rs:63-107`
   installs an observer CAMetalLayer sublayer. `layer_observer.rs:141-164` propagates
   contents scale and sets its frame from root bounds. `platform/macos.rs:326-348` configures
   the detected Metal sublayer after surface creation; it does not send drawableSize
   to the ordinary backing layer. WGPU configure sets the real drawable size at
   `vendor/wgpu-hal/src/metal/surface.rs:149-171`. The host uses a local origin of
   (0,0) at `surface_host.rs:1935-1940`, not the pane's window offset twice.
7. **macOS teardown.** Unlike Windows, `platform/macos.rs:532-548` dispatches off-thread
   unparent/release to the main queue. The only production `reveal_after_present`
   call is `surface_host.rs:1988`; its callers are main-thread render paths guarded
   by host ownership. The raw-pointer async reveal branch alone therefore does not
   establish a current reachable late-unhide race. Ast-grep found viewport calls in
   two files (the platform forwarding wrapper plus two host sites), all production
   calls supplying `Some(bounds)`. Windows/Linux `update_viewport(None)` being a
   no-op is not this detach bug: production detach removes the host instead.
8. **VT capture/lifecycle distinction.** Requested `render_pass.rs` captures a
   fresh libghostty render state/grid; it is not the WGPU present loop. Its caller
   is `terminal.rs:322-323`, reached by `session_render_snapshot` while sessions are
   locked. Requested `lifecycle.rs` allocates/clears callbacks/frees the VT handle;
   `terminal.rs:120-124` owns teardown. Detach does not destroy that VT or daemon
   pump; close does. No additional stale-pane defect was established in these files.

## Disproven hypotheses and limits

- **Disproven in current source:** missing ownership recheck before presentation;
  unconditional geometry resurrection while detached; follow-up lost merely because
  output arrives during render; warm return always replays old history; initial
  dropped bounds frame immediately releases the outgoing pane; macOS renders into
  the root WebviewWindow; macOS layer configuration only touches an inert backing
  layer. These conclusions concern the bounded callers above, not every possible
  externally constructed IPC ordering or future caller.
- **No new macOS layer-geometry defect established.** The pure AppKit geometry test
  at `native_terminal_composition_contract.rs:362-421` does not observe NSView,
  CALayer, KVO timing or the display. A real acceptance pass must inspect child frame,
  backing bounds/contentsScale, Metal frame/drawableSize, and presented pane content
  while resizing/moving between 1x/2x displays. This audit did not do that.
- The foreign Cargo patch (`src-tauri/Cargo.toml:162-163`) selects vendored HAL.
  A direct comparison of its Metal `surface.rs` with registry wgpu-hal 24.0.4 was
  identical. `layer_observer.rs` differs by the added superclass `dealloc` call at
  `:191`. That is an existing foreign cleanup fix, not this report's work; it does
  not alter the reviewed geometry propagation. Atlas changes were not attributed
  to surface scheduling or independently validated in this lane.
- Windows actual children and Linux Xlib/Wayland children report their real
  capabilities; uninitialized/Xcb/no-child/unsupported cases are rejected by the
  descriptor guard, not silently rendered into a shared root target. These source
  branches are not Windows/Linux GUI PASS results. The `3fa25a1` history records
  earlier GUI verification, but those historical claims were not rerun here.
- No tests were executed, so there are no RED/GREEN receipts or current test PASS
  claims. Existing tests read here use pure state or fake IPC and do not substitute
  for native presentation. No independent reviewer tool was available in this child;
  final checks were re-reads of alleged branches plus callers/guards and the scoped
  diff. The parent synthesis can accept/reject findings before any implementation.
- Only this report was created. No debug artifacts, journal, processes, native
  resources or test fixtures were created to clean up. The report is uncommitted
  in a shared working tree.
