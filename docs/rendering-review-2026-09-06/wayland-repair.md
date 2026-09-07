# D3 repair: coherent Wayland presentation geometry

## Outcome and boundary

Implemented the scoped D3 repair in
`/Users/indo/code/project/orca-lite-rendering-20260906`. Both required tests have
right-reason RED receipts and final GREEN receipts. The actual presentation
geometry resolver feeds the production host's cell density, stored grid/layout,
renderer configuration, and child placement. Native Wayland GUI QA remains
unresolved; nothing here demonstrates compositor pixels or absence of a native
`invalid_size` error on Linux.

Read the D3/Q4 contract in `review.md` and `REPAIR_PHASE.md` before editing.
Original task base: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931` (`b8f82d7`).
The concurrent UI commit advanced worktree HEAD to
`bce59b45d6d2d43f46cc63e35a5fb9b1161ca145`; the owned paths have no committed
changes between that HEAD and the original base. No source baseline was copied
from the main tree. No staging, commit, merge, push, or desktop control was done
by this worker. The concurrent Windows implementation diff is foreign and was
left untouched.

## Owned implementation

Only these six worktree paths were edited:

- `src-tauri/src/native_terminal/child_surface.rs`
- `src-tauri/src/native_terminal/composition.rs`
- `src-tauri/src/native_terminal/platform/mod.rs`
- `src-tauri/src/native_terminal/platform/linux.rs`
- `src-tauri/src/native_terminal/surface_host.rs`
- `src-tauri/tests/native_terminal_wayland_subsurface_contract.rs`

`WaylandSubsurfaceGeometry` now rounds the logical left/top and right/bottom
edges, subtracts the snapped edges, then multiplies by the integer buffer scale.
This preserves common pane edges and makes buffer divisibility intrinsic rather
than correcting an already-rounded physical size. The existing nearest-integer
scale policy is retained: DPR 1.5 selects scale 2. No viewport extension is added.
Collapsed logical extents, physical overflow, and unrepresentable protocol scales
are rejected. Existing negative-origin clamping is retained.

`SurfacePresentationGeometry::resolve` converts that geometry into integral
logical bounds with the selected buffer scale. `PlatformCompositorTarget`
selects this policy only for an actually created Linux Wayland child. macOS,
Windows, X11, and unsupported/fallback targets retain the identity policy;
`SurfaceCompositionLayout::compute` and `ChildSurfaceGeometry` are unchanged.

The production chain is:

1. `NativeTerminalSurfaceHostState::render` obtains the real host/target and
   resolves request bounds before deriving physical cell metrics or grid size.
2. `prepare_session_layout` stores those bounds, metrics, and layout, resizes
   the terminal grid, and emits the existing PTY resize notification.
3. Direct, focus, and scheduled renders consume that stored layout.
   `render_snapshot` derives renderer density from the same bounds, configures
   the actual swapchain from layout width/height, and updates the child viewport.
4. The Linux child path derives position/scale from the same canonical bounds
   and forwards them to the existing `WaylandChild::set_geometry` protocol calls.
5. Both attach paths consult an existing host's geometry policy, preventing a
   warm reattach from restoring raw-DPR layout before scheduled output. Bounds
   errors stay inside the existing logged attach fallback, not an early failure.

Before a native host exists, attach layout remains provisional. The first real
render selects the actual target and replaces that layout before presentation.
Repeated resolution is idempotent. No renderer/atlas, IPC, UI, other-platform
implementation, vendor, or manifest edits are part of this repair.

## RED/GREEN evidence

Artifacts are under
`/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/D3/`.
Every command below ran from the isolated worktree. Each exact RED executed
one test and exited 101 on the expected assertion, before production edits.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
```

| Contract | RED | Final GREEN |
|---|---|---|
| 400.5x300, DPR 2 | Width 801, scale 2; remainder assertion `1 != 0` | Width 802, height 600, scale 2; both extents divisible; 1 passed |
| 400x300, DPR 1.5 | Actual bounds-request layout 600x450 versus child 800x600 | Production resolver plus bounds-request layout 800x600 equals child; scale 2; grid 40x15 for fixed 20x40 metrics; 1 passed |

The second test's only geometry-path adjustment after RED is consuming the
new production resolver before the existing bounds-request layout. It does not
substitute a hand-computed host size. The additional shared-edge regression
uses the real font metrics and `prepare_session_layout`, checking the stored
session layout/bounds/metrics across three neighboring panes and DPR
1.0, 1.5, 2.0, and 2.5. It also checks repeat resolution and child conversion.
No new test uses sleeps, polling, prose matching, or source-shape assertions.

`RED-<test-name>.log` and `GREEN-<test-name>.log` contain commands, numeric
output, counts, and exit codes. `red-tests.patch` records the pre-production
tests. The first cold build took 15m38s; its enclosing 1200-second tool deadline
expired before the second test executed. `RED-host-timeout-incomplete.log`
retains that incomplete attempt without claiming it as RED. The standalone
second command then executed and captured the expected failure.

Final-source validation, all exit 0:

| Command / artifact | Result |
|---|---|
| Both exact commands above | 1 passed each |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract --test native_terminal_composition_contract -- --nocapture` / `GREEN-contracts.log` | 12 Wayland + 7 composition passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests -- --nocapture` / `GREEN-host-tests.log` | 27 passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_child_surface_contract -- --nocapture` / `GREEN-child-contract.log` | 5 passed |
| Changed-file LSP, severity all / `lsp-diagnostics.log` | No diagnostics found on all six files |
| `cargo check --manifest-path src-tauri/Cargo.toml` / `cargo-check.log` | Passed |
| `cargo build --manifest-path src-tauri/Cargo.toml` / `cargo-build.log` | Passed |
| Scoped `git diff --check` / `diff-check.log` | Passed |

The library's seven existing compiler warnings remain in the logs, including
unused imports and dead-code warnings; they also occur in RED. None was
suppressed. The earlier successful validation before the final attach-error
preservation adjustment is retained in `before-attach-error-preservation/`;
the top-level GREEN/check/build logs all validate the final source.

## Numeric portable exercise and native QA still required

`numeric-geometry.log` extracts actual shared-edge test output from the final
contract run. For DPR 1.5 and 2, the first pane resolves from origin
(10.25,20.25), size 400.5x300.5 to origin (10,20), logical size 401x301, scale 2,
and buffer 802x602. Its right neighbor starts at logical x=411, physical x=822,
with buffer width 800. Its lower neighbor starts at logical y=321, physical
y=642. The first pane's stored host right/bottom edges therefore coincide with
those neighbors. This workstation derived 16x33 physical cells at scale 2 and
stored a 50x18 grid for those fixtures; the test does not pin machine font
metrics. The default policy regression preserves 601x451 physical output for
400.5x300.5 at DPR 1.5 and preserves fractional AppKit frames.

This is a portable geometry/session-layout execution on arm64 macOS, not a
native child GUI exercise. The Linux cfg branch was source-inspected but not
Linux-target compiled or linked here. No compositor, app window, desktop,
browser, remote host, or daemon was launched/controlled. Native Wayland QA from
`review.md` remains pending: two distinctly numbered panes, outgoing scale and
buffer extents, compositor diagnostics, resize through odd/even physical widths
at DPR 2, fractional DPR only if actually reported, full content/edges with no
overlap/gap and no `invalid_size`. A Mac arithmetic pass does not close that QA.

## Diff identity and owned-resource cleanup

`owned-diff.patch` contains only this worker's six paths; SHA-256:
`dea20e8e45778e646ce0a849c4d55c3094667d563e86091f8a0bc1f766abf78b`.
`provenance.log`, `owned-diff-stat.txt`, and the empty
`owned-baseline-vs-current-head.patch` retain scope/base receipts. Source remains
uncommitted for lead-owned integration; this worker never used the shared index.

All directly spawned validation commands have exited. The initial tool-deadline
attempt was terminated and no D3 test/Cargo process remained at the cleanup
inspection. No persistent D3 server/browser/desktop resources were created.
Build caches and requested evidence are intentionally retained. Language-server
services are tool-managed and were not terminated; unrelated processes and the
foreign Windows work were not touched. See `cleanup.log` for the final receipt.
