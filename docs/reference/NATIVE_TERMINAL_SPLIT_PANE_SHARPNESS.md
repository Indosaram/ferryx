# Why split panes look less sharp than the first pane

Investigation date: 2026-09-04. Repo state: `199761a` (main). Diagnosed and fixed.

> **Regression note (2026-09-04, second report).** The user saw this symptom come back.
> The cause was not a code change: the fix below had only ever existed in an uncommitted
> working tree, and the tree was later reset. `git log` showed no sharpness commit after
> `199761a`, `git stash list` was empty, `measureGeometry` was sending raw
> `getBoundingClientRect()` values again, and `configure_terminal_layers` did not exist --
> `update_viewport` was back to setting gravity and filters on the backing layer. The fix
> has been re-applied and committed. Do not leave this change uncommitted again.

## Symptom

After splitting, the first pane stays crisp while every other pane's text looks slightly
soft/hazy. Font, theme, and content are identical in all panes.

## Verdict

The terminal pixels are rasterized identically for every pane. The softness is introduced
**after** rendering, by Core Animation, because each pane's native layer is placed and sized
from **unrounded CSS geometry** while its Metal drawable is sized in **whole pixels**. Any
sub-pixel difference makes CoreAnimation resample the entire pane with bilinear filtering.

The first pane escapes this because its origin is the (integral) left edge of the terminal
container; every later pane's origin is `container_x + ratio * W + 1px divider`, which is
fractional in the general case.

## Evidence chain

1. **Per-pane rendering is identical.**
   `surface_host.rs` keys hosts by session (`HashMap<String, NativeTerminalSurfaceHost>`), and
   each host renders its grid at surface-local origin `(0,0)` (`local_viewport = {x:0,y:0,…}`),
   with cell metrics from `font_manager::derived_cell_metrics_for_scale(scale)` and an atlas
   that is cleared whenever the config changes (`renderer.rs::update_config` → `atlas.clear()`).
   The glyph atlas sampler is `FilterMode::Nearest` (`atlas.rs`). Nothing in the render path
   depends on which pane it is, and the renderer has no focused/unfocused branch.
   → Any per-pane difference must come from surface placement, not rasterization.

2. **Bounds are sent unrounded.**
   `ui/src/components/NativeTerminalPane.tsx::measureGeometry` sends
   `{x: rect.x, y: rect.y, width: rect.width, height: rect.height}` straight from
   `getBoundingClientRect()`, plus `scaleFactor = devicePixelRatio`. No snapping.

3. **The AppKit frame keeps those fractions, the drawable does not.**
   `platform/macos.rs::update_viewport` → `LogicalBounds::to_appkit_frame` → `view.setFrame(...)`
   with the raw fractional rect. Meanwhile `composition.rs::SurfaceCompositionLayout::compute`
   **rounds**: `physical_w = (width * scale).round()`, and `surface_host.rs::render_snapshot`
   configures the wgpu surface with exactly that integer size.
   → layer box (points × contentsScale) ≠ drawableSize (integer pixels).

4. **The properties meant to prevent resampling are set on the wrong layer.**
   `MacosCompositorTarget` applies `setContentsGravity: topLeft`, `setMagnificationFilter:
   nearest`, `setMinificationFilter: nearest` to `msg_send![view, layer]`. For a
   `wantsLayer` NSView that object is an **`NSViewBackingLayer`**, not the Metal layer — proven
   by commit `199761a` ("remove invalid setDrawableSize on NSViewBackingLayer to prevent crash",
   after the `SIGABRT` crash reports of 2026-09-04 08:46/08:49/08:52).
   wgpu-hal 24.0.4 (`src/metal/layer_observer.rs`) creates the real `CAMetalLayer` as a
   **sublayer** and only KVO-syncs `contentsScale` and `bounds`; its `setContentsGravity:
   kCAGravityTopLeft` line is commented out ("Uncomment when debugging resize issues").
   → The layer that actually holds terminal pixels keeps `kCAGravityResize` +
   `kCAFilterLinear`: the drawable is *stretched* into the layer box and any fractional
   position is resampled.

5. **This display gives no headroom.** `3840 x 1600`, "UI Looks like: 3840 x 1600" →
   backing scale factor **1.0**. One CSS px = one device px, so a 0.5 px offset is a full
   50/50 bilinear blend of neighbouring pixels — maximum smear, uniform over the pane.

6. **The user's actual layout produces exactly that.** From live state
   (`~/Library/Application Support/com.ferryx.app/session_state.json`, sidebar width `319`
   from `ferryx.sidebar.width`), the active tab is a 4-pane layout with ratios
   `0.5 / 0.4592 / 0.5 / 0.4592`. With a maximized window the terminal container is
   `3840 - 319 = 3521` px (odd), and the horizontal panes land at:

   - pane A (first): `x = 319.0000` (offset **0.0000**), `w = 1616.8432`, drawable `1617`
   - pane B: `x = 1936.8432` (offset **-0.1568**), `w = 951.5784`, drawable `952`
   - pane C: `x = 2889.4216` (offset **+0.4216**), `w = 950.5784`, drawable `951`

   Only the first pane is pixel-aligned. Panes B and C are composited at a fractional
   offset — every glyph in them is resampled.

## Aggravating factors

- `PaneResizeDivider` stores dragged ratios as `Number(ratio.toFixed(4))` (e.g. `0.4592`),
  so after any divider drag the split almost never lands on a whole pixel.
- `flexBasis: ${ratio * 100}%` resolves in fractional layout units; a 50/50 split of an
  **odd** container width also yields `.5` geometry (harmless at 2x, blurry at 1x).
- The 1 px `w-px` divider shifts every subsequent pane by one more pixel, which keeps the
  first pane aligned and pushes the fraction onto its siblings.

## Fix

1. `ui/src/components/NativeTerminalPane.tsx` — new `snapBoundsToDevicePixels()`, applied in
   `measureGeometry()`, the single chokepoint feeding both `cmd_native_terminal_attach` and
   `cmd_native_terminal_set_bounds`. It snaps **edges**, not sizes: `x`, `y`, `x + width` and
   `y + height` are each rounded to whole device pixels and the size is derived from the
   difference. Because `round(aR + 1) == round(aR) + 1`, the 1 px divider gap between two
   panes is preserved exactly — no overlap, no double gap — while the AppKit frame, the layer
   box and `drawableSize` now agree, so CoreAnimation has nothing left to resample.
2. `src-tauri/src/native_terminal/platform/macos.rs` — `configure_terminal_layers()` keeps
   `contentsScale` on the `NSViewBackingLayer` (wgpu's KVO observer forwards it) and now walks
   `layer.sublayers` to apply `contentsGravity = topLeft` plus nearest min/mag filters to the
   **CAMetalLayer** itself, identified by `respondsToSelector: drawableSize`. Previously those
   three properties were set on the backing layer, where they are inert. Any transient
   mismatch during a resize now crops one edge instead of stretching the whole terminal.
   `setDrawableSize:` is still never sent to the backing layer — that is the `199761a` crash.

Tests: `snapBoundsToDevicePixels` unit cases (identity at 1x/2x, the fractional split-pane
rect above, the gapless-divider invariant, invalid scale factor) plus a geometry-contract case
asserting the snapped attach payload, in `ui/src/components/NativeTerminalPane.test.tsx`.
The AppKit call itself is not unit-tested.

Still unfixed by design: `PaneResizeDivider` keeps storing `toFixed(4)` ratios. That no longer
matters, because the snap happens after layout.
