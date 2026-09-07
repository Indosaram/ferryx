# Atlas and cached-frame coherence review - 2026-09-06

## Result and evidence level

Two HIGH pressure defects remain in the **current working tree**, despite the
foreign `overflow_pending` fix. One prevents convergence when the visible glyph
working set exceeds capacity; the other leaves a recoverable final frame
incomplete until an unrelated repaint. Neither is the old mid-frame UV overwrite.

**PASS - source audit only:** the branches, callers, guards, working-tree diff,
and relevant test bodies below were read. **Runtime reproduction: NOT RUN.** No
tests, app launches, desktop input, GPU experiments, or fault injection were run.
The proposed RED tests do not exist yet; their commands are execution targets
after implementation, not evidence of a failing test today. A zero-test filtered
run must not be accepted as RED or GREEN.

Baseline: HEAD `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`. The scoped diff contains
foreign changes only in `renderer/atlas.rs` (110 additions, 9 deletions) and
`renderer/row_cache.rs` (4 additions). This review changes only this report.
Paths below are repository-relative; the prefix `R/` means
`src-tauri/src/native_terminal/renderer/`, and `N/` means
`src-tauri/src/native_terminal/`.

## Current patch, not HEAD's old behavior

- `R/atlas.rs:148-158` now sets `overflow_pending` and returns `None` instead of
  clearing entries, resetting coordinates, and bumping generation during a row
  build. Existing entries stay at their original UVs for that frame.
- `R/row_cache.rs:30-36` consumes that flag **at the next invocation**, clears the
  atlas, and clears every cached row before reuse. `R/atlas.rs:284-290` resets the
  flag and increments generation. This is a real fix for stale cached UVs across
  the deferred reset, not a missing generation guard.
- Textures grow from 512 square to 1024 square. All-zero non-color rasters return
  `None` at `R/atlas.rs:172-175`; whitespace returns before packing at :126-128.
- History checked: `38f887e` introduced the row cache and its generation guard;
  `8f44243` introduced dual texture accounting; `44f1578` changed the mask texture
  from R8 to RGBA without changing that accounting basis. The current deferred
  reset and empty-mask behavior are uncommitted, not changes in those commits.

## Shared caller chain and observable output

1. `N/terminal.rs:322-323` calls `capture_render_snapshot` in
   `N/render_pass.rs:24-199`. The latter copies the whole grid, not only foreign
   dirty rows; allocation/foreign-call errors using `?` prevent a partial snapshot
   from being returned. `N/cell_extractor.rs:49-186` decodes text, width and styles.
2. `N/surface_host.rs:601-624` captures the terminal snapshot and applies focus /
   preedit state. Scheduled dispatch calls the host at :362-369; explicit bounds
   rendering also calls it at :1751-1758.
3. The host calls `render_to_surface_viewport` at
   `N/surface_host.rs:1977-1987`; its wrapper at `R/renderer.rs:258-281` reaches
   `render_to_surface_viewport_internal`. After viewport validation at :297-314,
   the real surface path calls `RowCacheManager::update_and_flatten` at :316-322.
   The offscreen and direct-view callers use the same seam at :137-143 and
   :212-218. LSP reference lookup confirmed these three production call sites.
4. `R/row_cache.rs:46-60` iterates rows in increasing order, calls
   `build_row_instances` at :54, and caches its output without a completeness
   field. `R/instances.rs:180-202` calls `GlyphAtlas::get_or_insert` at :188. On
   `None`, it emits **no glyph instance**, while the background was already added
   at :138-141. LSP found this as the sole production atlas insertion caller.
5. `R/pass.rs:108-128` clears the attachment, draws all backgrounds, then draws
   only the glyph instances it received. Missing glyphs therefore become cell
   background, not retained pixels from the previous presentation. The renderer
   submits, checks GPU errors, and returns only `(rebuilt, reused)` at
   `R/renderer.rs:401-403`. Atlas capacity exhaustion is not a GPU error.
6. The host presents and returns `presented: true` at
   `N/surface_host.rs:1988-1992`. Dispatch only requests another render for
   `!receipt.presented` at :370-373. The atlas flag is not in that receipt.

## Severity-ordered findings

### A1 - HIGH: a dense visible working set never converges after deferred overflow

**Status:** source-proven omission for the concrete capacity/input class below;
desktop manifestation and installed-font coverage remain unrun.

**Defective seam:** `R/atlas.rs:148-158`, `R/row_cache.rs:30-36,46-60`, and
`R/instances.rs:187-202`. All three renderer callers above publish this result;
there is no page, growth, complete-frame retry, or fallback draw after a capacity
miss. Existing hits are protected, but missing entries cannot be drawn.

**Deterministic trigger:**

1. Use a valid config with physical narrow cells 16x32, scale 2.0, and a snapshot
   of 80 columns x 25 rows. Put 1,000 distinct covered Hangul glyphs in row-major
   order, each a `Wide` cell followed by a `SpacerTail`; hide the cursor. No
   empty/invisible glyphs or style changes are required.
2. Each wide allocation is 32x32. With the one-pixel horizontal advance at
   `R/atlas.rs:277`, a shelf holds `floor((1024 + 1) / (32 + 1)) = 31` entries.
   There are `floor(1024 / 32) = 32` shelves: capacity **992**, not 1,024.
   The viewport is only 1280x800, accepted by `R/renderer.rs:493-508`.
3. Frame F0 inserts the first 992 keys. The remaining eight hit :154-158 and
   contribute no instances. The incomplete frame is submitted normally.
4. Invoke another frame with exactly the same snapshot. At entry, the foreign
   patch clears the atlas and rows. The same row-major traversal fills the same
   992 slots and drops the same last eight. Repeat any finite number of times:
   the same omissions recur and `overflow_pending` is set again.

At the 20x40 physical-cell config already used by the 2x viewport test, wide slots
are 40x40 and capacity is only `25 * 25 = 625`; an 80x24 all-distinct wide grid has
960 entries. Ordinary repetition of a few CJK glyphs does not trigger this; the
count is distinct `(text, bold, italic)` keys, not occupied cells.

**Existing tests:**

- `R/atlas.rs:353-401`,
  `atlas_overflow_never_invalidates_earlier_entries_mid_frame`, tests stable
  generation/first entry, then explicitly clears and reinserts one key. It never
  renders a row or tries to retain an over-capacity visible set across frames.
- `src-tauri/tests/native_terminal_renderer_contract/dirty_update_atlas.rs:8-55`
  repeats a small character set and checks stats, not dense-frame completeness.
- `.../offscreen_render.rs:214-264`,
  `test_retina_scale_2_renderer_viewport_and_config_update`, checks output size
  and opaque margins, not a dense CJK working set or repeated overflow.

**Cheapest faithful RED seam (new unit test, no desktop):**

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::row_cache::tests::all_visible_glyphs_survive_when_dense_2x_working_set_repeats -- --exact --nocapture
```

Given a real `GpuContext`, `GlyphAtlas`, `RowCacheManager`, and the 1,000-glyph
snapshot, render F0 through `update_and_flatten` as setup. Verify fixture glyphs
have ink, so a missing font cannot masquerade as overflow. When the unchanged F1
is built through the same method, then require a glyph instance at every one of
the 1,000 wide-cell origins. Current-source prediction: 992, with the last eight
absent. This hits the **successive-frame reset**, not merely first-frame pressure.
Do not consume `take_overflow_pending` in the test before F1: that changes the
mechanism being tested. Use actual glyph keys, not multi-character `a123` keys.
GPU initialization must fail loudly if unavailable, not silently skip.

A companion first-frame test can use the same fixture. After repair, use
`render_snapshot`/offscreen readback against independently rendered small tiles
to prove those instances sample correct glyphs, rather than merely increasing
the instance count. No wall-clock waiting is needed for the primary RED seam.

**Real-surface scenario (not executed):** in a later authorized debug app session
started by `bun tauri dev`, fill a sufficiently large 2x pane with a deterministic
range of at least 1,000 covered Hangul syllables, using absolute cursor positioning
to avoid accidental bottom-row scrolling. Hide the cursor, capture the last rows,
force explicit repaints without changing the text, and compare against the source
fixture. Repeat at 1x and 2x. Record measured cell dimensions and distinct glyph
count; compute capacity from those dimensions instead of assuming this fixture's
16x32 metrics describe the user's font.

**Repair direction for synthesis:** capacity handling must support the complete
visible working set before presenting it (for example, appropriately bounded
pages/batches or a sized allocation). A next-frame clear or automatic retry alone
cannot solve this input; an unconditional reschedule would create endless work.
Do not restore mid-frame overwrites of slots already referenced by a draw batch.

### A2 - HIGH: a recoverable final frame stays incomplete because overflow is not a repaint reason

**Status:** source-proven conditional on an exhausted historical atlas and no
further external render request. This needs no simultaneous over-capacity visible
set, failed GPU allocation, or scheduler race.

**Defective seam and guards rechecked:** `R/atlas.rs:154-158` returns `None`;
`R/row_cache.rs:30-31` only recovers on a future call; `R/renderer.rs:401-403`
does not expose incompleteness; `N/surface_host.rs:1988-1992` reports a successful
presentation; :370-373 does not schedule a follow-up for it. In
`RenderScheduleCoordinator::finish_render`, `N/surface_host.rs:245-251`, the
`RENDERING` state transitions to `(RENDER_IDLE, false)` when no new request has
arrived. The coordinator correctly preserves requests made during a frame;
there simply is no request from atlas overflow.

**Trigger sequence:**

1. Across successful historical frames, fill the atlas with 992 distinct wide
   32x32 glyph keys without attempting key 993. Old offscreen keys remain in the
   map; neither row changes nor leaving the viewport evicts them. Atlas packing
   state advances at `R/atlas.rs:277-279` and resets only on explicit clear.
2. The final output changes the visible snapshot to one new covered wide glyph.
   The current visible set would easily fit in an empty atlas. The final frame
   reaches the capacity branch and omits that glyph, while setting the flag.
3. Presentation succeeds. No more output/resize/focus/selection request arrives
   during or after this render. Dispatch sees `presented: true`, and completion
   returns the coordinator to idle. The native surface remains background-only
   at that cell for an unbounded period.
4. A later external repaint does clear/rebuild and restores the glyph. This
   explains a recovery on resize or other interaction without invoking a stale
   generation bug. It is separate from A1, which cannot recover this way.

**Existing tests:** the atlas overflow test explicitly supplies the missing clear
and reinsertion itself. `dirty_row_reuse.rs:6-41` tests one-row mutation under low
pressure, not an incomplete final frame. The coordinator tests at
`N/surface_host.rs:2376-2432` verify output-driven follow-up and clean completion;
they do not integrate atlas pressure with the successful-present receipt.

**Cheapest faithful RED seam (new renderer-internal test):**

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::last_frame_renders_new_glyph_when_only_history_fills_atlas -- --exact --nocapture
```

Given a real renderer configured for 16x32 at 2x, prewarm its private atlas with
exactly 992 distinct, nonempty wide rasters through the real `get_or_insert`
method; require every insertion to succeed. No overflow has happened yet. When
`render_snapshot` is called **once** for a 2-column, 1-row, cursor-hidden snapshot
containing covered glyph 993 plus its spacer, then its glyph area must match a
fresh renderer rendering that same final input. Derive the reference from the
fixture, not the warmed renderer's cache. Current-source prediction: warmed
output is background-only; fresh output contains ink. This avoids hundreds of
offscreen readbacks just to construct historical atlas state and does not need a
mock GPU or sleep.

This test chooses the stronger, simple contract that the final render itself is
complete. If synthesis deliberately chooses deferred recovery instead, retain a
separate regression through the **actual production completion policy**: subscribe
to frame completion before submitting the last snapshot, drain the scheduled
follow-up via exact callbacks, and require a complete frame without any new PTY
input. A test that calls `schedule_render()` itself after seeing overflow would
manufacture the missing behavior and is not a regression test. Bound callback
waits; do not poll or sleep.

**Real-surface scenario (not executed):** scroll deterministic pages of distinct
covered CJK characters through a 2x pane until historical entries reach capacity;
finish with a new, very small labeled line and hide the cursor. Stop all producer
output. Capture the final presentation before any interaction, then force one
explicit repaint and capture again. Expected regression signature is missing
glyphs before the repaint and recovery afterward. Synchronize to frame/output
completion rather than timing a screenshot after a fixed delay. Repeat after
1x/2x transitions and distinguish this case from A1's oversized current page.

**Repair direction for synthesis:** arrange a complete current-frame rebuild
before submission when historical entries are the only obstruction, or propagate
an explicit bounded recovery reason to completion scheduling. Keep this separate
from A1 so an over-capacity current page cannot spin indefinitely.

## Other checked cache paths: conditional risks, not reproduced pressure defects

### Font changes with unchanged metrics can retain old atlas contents

**Source observation, MEDIUM conditional cache-coherence risk:**
`R/renderer.rs:98-105` invalidates only when `RendererConfig` changes. Its fields
at `R/types.rs:193-203` contain dimensions, scale and theme, but no font identity
or font revision. `R/font_manager.rs:77-89` can replace the font manager when a
family/fallback list changes. Keeping the primary family and size while changing
only configured fallback families preserves metrics by the primary-face path in
`R/coretext_font.rs:378-429`.

The real preference path was checked: `src-tauri/src/ipc/preferences.rs:132-145`
updates overrides and calls `rerender_native_sessions`; :52-90 reuses the native
hosts through `state.render`, rather than recreating them. `N/surface_host.rs:812-824`
reapplies terminal colors only. The occupied-host path at :1730-1747 and the
per-present configuration at :1908-1922 pass the same config when metrics/theme
are unchanged. `R/atlas.rs:131-138` then returns an existing key without consulting
the new rasterizer. A row hash change alone cannot repair an already cached glyph.

Trigger: warm a fallback-dependent glyph, change only the fallback to a face with
a different raster/coverage, and rerender in the same pane with identical physical
metrics and theme. Existing keys retain the old raster; new keys use the new font.
No installed-font pair was exercised, so this is **not established as the user's
intermittent symptom** and is not a typography recommendation.

The 2x config test at `.../offscreen_render.rs:214-264` updates config before its
first render; it does not exercise a warmed cache or a font-only change. Proposed
isolated integration target, with one test/process to avoid preference-global
interference:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_font_cache_contract cached_glyph_uses_new_fallback_when_primary_metrics_are_unchanged -- --exact --nocapture
```

Given two available fallback fixtures with demonstrably different output for the
chosen glyph and identical primary metrics, warm under A. When applying B through
the preference/config path and rendering the same cell, then compare to a fresh
renderer under B. Restore prior overrides with scoped cleanup. The reference must
differ from A before accepting the fixture. Real surface: change only a fallback
via the normal preferences command and compare an already displayed glyph with
the same glyph in a fresh pane. No such test or scenario was run; synthesis may
defer this conditional configuration path independently of A1/A2.

### Transient native raster allocation failure is indistinguishable from intentional empty ink

**Conditional failure-path risk, not a guaranteed normal-operation trigger:**
`R/coretext_raster.rs:209-223` returns `None` on a null `CGBitmapContextCreate`;
:311-317 maps that to `false`. For a normal covered base character,
`R/font_manager.rs:154-156,218-229,299-302` leaves the zero buffers and returns
`Alpha` without an error result. `R/atlas.rs:172-175` now skips it, and
`R/instances.rs:187-202` omits the quad. `R/row_cache.rs:50-60` caches the row's
normal hash and nonempty background list anyway. A later unchanged frame reuses
the omission without calling the rasterizer, even if the temporary resource
failure has ended. A cursor/selection change on another row does not invalidate it.

The existing empty-PUA test `R/atlas.rs:404-435` proves only an intentional empty
result on a fresh atlas; it does not inject a transient failure or retry a cached
row. This behavior also has an antecedent in HEAD, where a zero raster could be
inserted as a blank atlas entry. The foreign patch saves the atlas slot but does
not add row-level failure recovery. No GPU OOM or process allocator failure was
observed; Rust `Vec` allocation failure must not be described as this recoverable
native-context branch.

Proposed test seam, only if synthesis takes up this conditional path:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::row_cache::tests::unchanged_row_recovers_when_native_raster_allocation_recovers -- --exact --nocapture
```

Given a **thread-scoped one-shot** fault at the native bitmap-context creation
boundary, build a row containing a known covered glyph and record consumption of
that exact fault. Release the fault as fixture setup. When rebuilding the unchanged
snapshot, then require the glyph to recover or an explicit retryable failure to be
surfaced according to the selected repair contract. Keep the real font resolver,
atlas, row cache and GPU; do not mock `build_row_instances` or inject arbitrary
glyph vectors. Do not implement a rasterizer redesign to obtain this evidence.
Real surface would need that same isolated one-shot fault on an authorized debug
build, followed by an explicit repaint without row changes, plus cleanup. Ordinary
desktop memory-pressure stress is not a deterministic substitute. This remains
unproven without controlled fault evidence.

## Empty-glyph, allocation and test-coverage caveats

- Empty text / Unicode whitespace returns before cursor mutation at
  `R/atlas.rs:126-128`; spacer heads/tails and invisible cells are filtered in
  `R/instances.rs:180-184`. These are not accidental glyph losses.
- The all-zero check is **after** shelf wrap and capacity testing
  (`R/atlas.rs:148-175`). An unsupported non-whitespace glyph requested when the
  atlas is full can set overflow before it is rasterized; a skipped raster after
  wrap does not roll back packing coordinates. Thus the patch's fresh-empty test
  cannot prove empties are packing-neutral under pressure. This can cause extra
  clears; it is not independently established here as visible glyph corruption.
- `GlyphAtlas::new` allocates two 1024-square RGBA8 textures
  (`R/atlas.rs:47-75`): 8 MiB of texel payload, before metadata. Stats at :293-300
  still use 5 bytes/pixel. Furthermore, `max_capacity_bytes` at :13 is exactly
  that 5-byte base, while `allocated_bytes` adds positive per-entry overhead.
  Consequently the existing bound assertion in
  `.../dirty_update_atlas.rs:24-32` cannot hold after a nonempty atlas populates
  under this patch. This is a **source-proven verification mismatch, not a test
  failure observed in this audit**, and does not itself cause glyph omission.
  Cheapest existing command to check it later:

  ```sh
  cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
  ```

- GPU resource errors are captured in `R/gpu_context.rs:89-92` and drained by
  :142-149; render callers check after submission. Allocation-failure recovery of
  the GPU device/atlas was not exercised. Do not conflate a GPU error with the
  ordinary atlas-capacity `None`, which never enters that error channel.

## Disproven hypotheses and limits

**Disproven by the checked current source:**

- "Overflow still clears in the middle of building a frame": removed by the
  foreign patch. The old incorrect-glyph-by-overwritten-UV mechanism is not A1/A2.
- "Deferred clear reuses cached rows containing old-generation UVs": generation
  is checked immediately after clear, before row reuse (`R/row_cache.rs:30-36`).
- "Scale missing from the row hash means normal scale updates reuse old glyphs":
  the hash omits it, but `RendererConfig` equality includes scale, and the real
  host calls `update_config`; changed config clears the atlas and invalidates all
  row instances through generation. Font identity is a different dependency.
- "Partial snapshots from foreign allocation failure poison the row cache":
  checked fallible snapshot construction returns `Err` before publishing a grid;
  scheduled host capture handles that failure before entering the renderer.
- "Any CJK-heavy screen must overflow": repeated keys hit the map before packing
  (`R/atlas.rs:137-138`); only a sufficiently large distinct raster working set or
  retained history causes these capacity misses.
- "The coordinator loses requests made during a render": the checked
  `schedule_render` / `finish_render` transitions preserve a coalesced follow-up.
  A2 is a missing atlas-originated request, not that previously repaired race.

**Limits:**

- No runtime PASS is claimed. Counts above are arithmetic and control-flow
  consequences, not measured frames or screenshots. Font availability, actual
  device dimensions, and whether these sequences explain the reported desktop
  incident require the specified runtime evidence.
- All 19 renderer module files were read, along with the named snapshot path,
  direct host/config/preference callers and relevant test bodies. LSP symbols and
  references were used for atlas insertion, overflow consumption, row flattening,
  and config updates; an AST search found the clear call sites in three renderer
  files. Source line references were checked against the working tree, not the
  AST helper's zero-based display offsets.
- Width is not part of `GlyphKey`; preedit width classification differs from
  general Unicode width handling (`N/surface_host.rs:547-598`). A full
  preedit-to-committed width-transition reproduction was not established in this
  bounded audit, so no additional incorrect-width finding is promoted here.
- Extreme accepted cell sizes larger than the atlas and native GPU OOM/device
  recovery were not tied to ordinary desktop callers. They are not asserted as
  causes of this incident. No vendor/backend redesign or typography review was
  attempted.
- Foreign `Cargo.toml`, atlas/cache, vendored wgpu-hal, dev-runner and onboarding
  changes were left untouched. No debug artifacts, source/test edits, or commits
  were created. This report is uncommitted and describes the observed shared-tree
  baseline; synthesis must preserve/recheck foreign fixes when choosing repairs.
