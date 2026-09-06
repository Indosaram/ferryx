# Native terminal glyph corruption after display switch (2026-09-06, corrected verdict)

> v2 — the first draft blamed a missing density-resend in an assumed old build.
> The user corrected the premise: the running app IS the fixed build
> (verified: `/Applications/Ferryx.app` = 2026.905.1, contains `076e429`), and
> the corruption still recurs. This version is the corrected analysis.
> Verification rule learned: identify the actual running process build first
> (`ps -axo pid,lstart,comm` + `Info.plist`) before attributing an incident to
> unshipped/unfixed code.

## Symptom

After moving the window between displays (38-inch 1x external -> MacBook
built-in 2x Retina), the native terminal renders wrong letters in otherwise
correct positions: intact glyph shapes, wrong characters (e.g. "7" where other
letters belong, Korean jamo-syllable mismatches). Corruption recurs later, then
disappears. Shrinking the logical display area (~80%, fewer cells) makes it
stop.

The "wrong letter at the right cell" shape rules out any layering/stretching
mechanism and pins it to a glyph-texture (atlas) problem.

## Root cause: glyph atlas overflow at Retina scale

All facts code-verified:

- Atlas textures are a fixed **512x512 px** (`renderer/atlas.rs:8-9`).
  Entries persist across frames; only an overflow or a config change clears them.
- Cell metrics are 8x16 px at 1x (13pt MesloLGS NF; pinned by
  `renderer/font_manager.rs::test_meslo_lgs_nf_13pt_matches_ghostty_cell_geometry`)
  and 16x32 px at 2x. A glyph slot therefore quadruples in area at Retina scale.
- Slot capacity per atlas fill:
  - 1x: narrow 512/9=56 per row x 32 rows = **~1,800**; wide CJK 16x16 =
    31 per row x 32 rows = **~1,000**.
  - 2x: narrow 16x32 => 512/17=30 per row x 16 rows = **~480**; wide CJK
    32x32 => 512/33=15 per row x 16 rows = **~240**.
- Dense agent frames (Korean prose + bold duplicates + icons) carry
  hundreds-thousands of unique glyph keys (bold/regular are separate keys;
  whitespace and empty PUA glyphs also consume slots). At 1x the working set
  fits; at 2x it does not.

### Why overflow corrupts the frame (atlas.rs:137-150)

`GlyphAtlas::get_or_insert` handles capacity exhaustion **mid-frame** by:

```rust
self.entries.clear();            // map only!
self.cursor_x = 0; self.cursor_y = 0;   // rewrite texture from origin
self.generation += 1;
```

It does NOT clear the textures and does NOT tell the frame builder. Rows
already built in this frame hold glyph instances with UVs captured from the
now-evicted entries; subsequent inserts rewrite exactly those texture regions
from (0,0) with different glyphs. The rows present with stale UVs ->
**wrong glyphs at correct positions**, matching the screenshots exactly.
(`RowCacheManager` only sees the generation bump at the START of the NEXT
`update_and_flatten`, row_cache.rs:30-34.)

### Why display-switch-specific and recurring

- At 1x (38") capacity is ~1,000-1,800 slots: realistic frames fit, the
  overflow path basically never runs.
- On the first 2x render after the switch, `076e429`'s resend correctly applies
  scale_factor=2 through `render()` -> `update_config` -> `atlas.clear()`
  (surface_host.rs:1707-1760, renderer.rs:98-104). Every subsequent frame
  rasterizes at 2x slots with ~480/240 capacity: frames whose unique-glyph
  working set floats around a few hundred trigger the mid-frame clear whenever
  accumulation crosses the threshold -> corruption **recurs intermittently**,
  following content pressure (streaming Korean text adds uniques).
- Reducing the logical area (~80%) cuts cells and unique glyphs per frame below
  the budget -> frames stay clean. The display-density resend fix itself works
  as designed; it merely exposes the overflow promptly.

(The earlier "leaked WgpuObserverLayers composite stale frames" hypothesis is
weakened by the vendored-wgpu-hal findings: leaked layers are removed from the
layer tree but not object-freed - a memory leak, not a compositing one. The
937-layer leak and this corruption are separate defects sharing one process.)

## Fix IMPLEMENTED (2026-09-06, uncommitted working tree)

Files: `renderer/atlas.rs`, `renderer/row_cache.rs` (114 insertions, 9 deletions).

1. Frame-atomic overflow: `get_or_insert` now sets `overflow_pending` and
   returns `None` past capacity (blank glyph, bg still drawn); the clear is
   deferred to the start of the next `update_and_flatten`
   (`take_overflow_pending()` -> `atlas.clear()`), whose generation bump drops
   the row cache in lockstep. No mid-frame invalidation => no stale UVs =>
   wrong glyphs are impossible even at genuine capacity exhaustion.
2. Capacity: `ATLAS_WIDTH/HEIGHT 512 -> 1024`. 2x slot budget (16x32 cells):
   narrow 480 -> **1920** slots; wide CJK (32x32) 240 -> **992** slots — the
   proven 1x-era budget restored at Retina scale. `MAX_CAPACITY_BYTES` now
   derives from the atlas size so `GlyphAtlasStats` is truthful
   (5,242,880 vs stale 4 MiB).
3. All-zero non-color rasters (whitespace, uncovered PUA like U+10FFFD) are
   returned as `None` without allocating a slot — slot pressure reduced.

Tests (RED proven pre-fix, GREEN after):
- `atlas_overflow_never_invalidates_earlier_entries_mid_frame` — pre-fix failed
  with "3000 unique 16x32 slots must exceed any atlas budget" (old code never
  returned None because it cleared+wrapped silently); post-fix passes.
- `atlas_skips_all_zero_raster_without_allocating_slot` — pre-fix failed
  (a zero-ink PUA consumed a real slot); post-fix passes.

Verification run by two independent executors (delegate + supervising agent):
- `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal`:
  **111 passed / 0 failed** (both runs).
- `cargo check`: exit 0, zero warnings attributable to the changed files.
- Pre-existing unrelated breakage: `cargo test ... native_terminal` (no
  `--lib`) fails to COMPILE `tests/windows_edge_probe_contract.rs` — an
  `include_str!` into a missing `.omo/ulw-loop/.../run-edge-probes.ps1`
  (tracked, unmodified by this work; another session's puzzle).

## Remaining idea (not implemented)

- Expose `GlyphAtlasStats` (entry_count, generation, overflow count) in
  `switchDebug("terminal.surface.presented")` so future incidents carry the
  smoking gun in the trace.

## Workarounds on current builds

- Corruption is per-frame and content-driven: any reflow/repaint that lowers
  the unique-glyph count (smaller logical display area, smaller pane, fewer
  columns) clears it; an app restart resets everything.
- The defect is cosmetic; the terminal grid/state is never corrupted.

## Cross-references

- Frontend density resend (works as designed): commit `076e429`,
  `ui/src/components/NativeTerminalPane.tsx` (matchMedia resolution listener).
- Memory-leak defect (separate): `docs/FERRYX_MEMORY_BASELINE_2026-09-05.md`,
  vendored `src-tauri/vendor/wgpu-hal` `[super dealloc]` patch (uncommitted).
