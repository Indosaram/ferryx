# Decision-Complete Bounded Atlas Strategy for D1, D4, and D7

## Executive Summary & Problem Formulation

This specification defines the decision-complete architectural repair plan for three confirmed renderer defects in `orca-lite`:
- **D1 / HIGH:** Dense visible glyph sets cannot converge under atlas pressure (repeating visible sets exceeding capacity repeatedly drop the same glyphs).
- **D4 / MEDIUM:** History-only atlas exhaustion strands the final frame (historical entries fill shelves; when a final frame introduces a new glyph, it is omitted, presentation reports success, and the host coordinator transitions to idle with text permanently missing).
- **D7 / MEDIUM:** Atlas accounting violates its existing contract bound (`dirty_update_atlas.rs:29` panics with exit 101 because `allocated_bytes = max_capacity_bytes + entry_overhead > max_capacity_bytes` on the first insertion, and dual RGBA8 textures are accounted at 5 B/px rather than true 8 B/px).

### Baseline Comparison: Clean HEAD vs. Foreign Patched Baseline

| Dimension | Clean HEAD (`b8f82d707`) | Foreign Baseline (`4cab8ab8a` / Main) | Required Repair Target |
| :--- | :--- | :--- | :--- |
| **Atlas Dimensions** | Fixed `512 x 512` | Fixed `1024 x 1024` | Dynamic Bounded: `1024 x 1024` base, grows dynamically to fitting candidate power-of-two up to actual `device.limits().max_texture_dimension_2d`. |
| **Overflow Handling** | Mid-frame reset: `entries.clear()`, cursor at `(0,0)`, `generation++`. Overwrites textures and invalidates UVs mid-draw. | Deferred flag: `overflow_pending = true`, returns `None`. Next frame clears atlas and row cache. | Reactive Current-Frame Rebuild: Detects overflow, differentiates D4 (history) vs. D1 (capacity), selects fitting dimension via exact shelf simulation, reallocates once, and completes frame synchronously. |
| **Omission Outcome** | Mid-frame corrupt glyphs / wrong characters drawn from overwritten UVs. | Safe UVs, but omitted glyphs become background color; frames never converge if visible set > 992 slots. | Zero visible glyph omissions within bounded capacity; 100% complete frame presented on every call. Structured error on hardware cap exhaustion. |
| **Empty Glyph Filter** | Empties rasterized and packed into atlas slots. | Skips all-zero non-color rasters, but check occurs *after* shelf wrap/advance check. | Explicit `AtlasInsertResult`: skips whitespace and all-zero rasters *before* shelf reservation. |
| **Texture Formats** | Dual RGBA8 (`44f1578`) | Dual RGBA8 (`mask_texture` 4 B/px + `color_texture` 4 B/px = 8 B/px) | Dual RGBA8; accounted truthfully at 8 B/px. |
| **Accounting Basis** | `allocated_bytes = 512*512*5 + overhead`, `MAX_CAPACITY = 4 MB` (Passed). | `allocated_bytes = 1024*1024*5 + overhead`, `MAX_CAPACITY = 1024*1024*5` (FAILS exit 101). | Truthful derivation: actual texture bytes (`W*H*8`) + exact tracked metadata (`entries.len() * sizeof(pair)`), bounded by device-limit capacity. |

### Concrete Defect Signatures

1. **D1 Trigger (Dense Visible Set Convergence Failure):**
   - Config: Physical cells 16x32, scale 2.0, viewport 80x25 (1280x800 px).
   - Input: 1,000 distinct covered wide Hangul glyphs (each 32x32 px plus 1 px horizontal advance = 33 px shelf advance).
   - In 1024x1024: Shelves = $\lfloor 1024 / 32 \rfloor = 32$; Slots per shelf = $\lfloor (1024 + 1) / 33 \rfloor = 31$; Total capacity = $32 \times 31 = 992$ slots.
   - Frame 0: Inserts 992 glyphs. The remaining 8 return `None` and are omitted. `overflow_pending = true`. Incomplete frame presented.
   - Frame 1 (identical snapshot): Start-of-frame sees `overflow_pending = true`, clears atlas, clears row cache. Reruns row 0..25 in identical order. Inserts same 992 glyphs, drops same last 8 glyphs.
   - Outcome: Every repaint drops the exact same 8 glyphs. The screen never converges.
   - Scale-Up Boundary: A valid 160x50 terminal grid at 16x32 physical cells contains 8,000 cells (4,000 wide keys), requiring 4,000 slots. A fixed 2048x2048 texture holds $\lfloor 2048/32 \rfloor \times \lfloor 2049/33 \rfloor = 64 \times 62 = 3,968$ slots, which still fails to converge. The strategy must support multi-step geometric growth (e.g. 1024 -> 4096) bounded only by the actual device limit.
2. **D4 Trigger (History-Only Final Frame Strand):**
   - Accumulate 992 historical glyphs across earlier frames without clearing.
   - Final snapshot: 2 columns, 1 row, containing 1 new covered wide glyph (glyph 993) plus spacer tail. Visible requirement is only 1 glyph.
   - Frame render: `get_or_insert` exceeds shelf bounds, sets `overflow_pending = true`, returns `None`.
   - Host presents background-only frame and receives `presented: true` (`surface_host.rs:1988-1992`).
   - Host coordinator sees `presented: true`, has no pending requests, and transitions to `RENDER_IDLE` (`surface_host.rs:245-251`).
   - Outcome: Producer is idle; no subsequent render occurs. Glyph 993 remains missing indefinitely.
3. **D7 Trigger (Accounting Contract Violation):**
   - Test: `tests/native_terminal_renderer_contract/dirty_update_atlas.rs:29:5`.
   - Assertion: `assert!(initial_stats.allocated_bytes <= initial_stats.max_capacity_bytes)`.
   - Code: `MAX_CAPACITY_BYTES = (1024 * 1024 * 5) = 5,242,880`. `allocated_bytes = 5,242,880 + entries.len() * sizeof(entry)`.
   - Outcome: On the very first inserted glyph, `allocated_bytes > MAX_CAPACITY_BYTES`. Immediate panic with Cargo exit 101.

---

## Architectural Survey & Component Ownership

```
[Daemon / PTY Stream] 
         │
         ▼
[SurfaceHost (surface_host.rs)] ── owns coordinator, layout, and target presentation
         │  calls synchronously: render_snapshot / render_to_surface_viewport
         ▼
[NativeTerminalRenderer (renderer.rs)] ── OWNS: GpuContext, RenderPipelines, 
         │                                       GlyphAtlas, atlas_bind_group, RowCacheManager
         ├── coordinates complete-frame instance preparation (prepare_frame_instances)
         │
         ├──► [RowCacheManager (row_cache.rs)] ── per-row dirty tracking & instance cache
         │           │
         │           ▼
         │    [instances.rs] ── builds RectInstances and GlyphInstances
         │           │
         │           ▼
         ├──► [GlyphAtlas (atlas.rs)] ── shelf packing, WGPU textures, UV mapping, capacity stats
         │           │
         │           ▼
         └──► [pass.rs] ── encodes WGPU render passes (clear, background rects, glyph quads)
```

### Module Boundaries and Blast Radius

- `GlyphAtlas` (`renderer/atlas.rs`):
  - Owns WGPU textures (`mask_texture`, `color_texture`), views, sampler, entries HashMap, coordinates, generation counter.
  - Sizing authority: manages dynamic texture dimensions (`width`, `height`, `max_dimension`).
  - Allocation authority: executes shelf packing, texture writes via `queue.write_texture`, and texture reallocation on growth.
  - Blast Radius: Internal to `renderer/`.
- `RowCacheManager` & `instances.rs` (`renderer/row_cache.rs`, `renderer/instances.rs`):
  - Tracks row hashes and cached instance buffers (`RowCacheEntry`).
  - Generates quad instances from glyph atlas entries.
  - Blast Radius: Internal to `renderer/`.
- `NativeTerminalRenderer` (`renderer/renderer.rs`):
  - Central orchestrator owning device, pipelines, uniform buffers, atlas, and `atlas_bind_group`.
  - Boundary Guard: Encapsulates all recovery. When an overflow occurs during instance generation, the renderer determines whether to evict history or grow the atlas, rebuilds instances within the current frame, recreates `atlas_bind_group` if textures grew, and presents a 100% complete frame.
  - Blast Radius: Confined to `renderer/`.
- `SurfaceHost` (`surface_host.rs`):
  - Direct host caller. Manages swapchain frame presentation and dispatch scheduling.
  - **Independence Guarantee:** Because the renderer completes the frame synchronously, `SurfaceHost` continues to receive `presented: true` with zero missing glyphs. The host scheduling coordinator and D5 dropped-frame recovery remain completely decoupled from atlas recovery.

---

## Evaluation of Candidate Designs

### Candidate 1: Multi-Page / Array Texture Atlas (Paged Batches)

- **Architecture:** Maintain a `Vec<AtlasPage>` of fixed 1024x1024 textures. In shaders, use `texture_2d_array<f32>` or split `encode_terminal_passes` into multiple draw calls per page bind group.
- **Coupling & Blast Radius:** Extreme. Modifies WGSL shader sources (`GLYPH_SHADER_SRC`), `RenderPipelines` bind group layouts, `GlyphInstance` vertex attributes (must add `layer_index`), instance sorting, and render pass encoding loops in `pass.rs`.
- **Memory Footprint:** High per-pane baseline. Pre-allocating multiple array layers or pages multiplies texture memory (each 1024x1024 page = 8 MiB).
- **Failure Modes:** Risk of driver-level texture array limitations or downlevel WebGPU incompatibility; complexity in cache invalidation across multiple pages.
- **Verdict:** **REJECTED.** Violates the mandate for the smallest correct renderer-local repair.

### Candidate 2: Whole-Frame Visible Preflight with Proactive Clearing & Growth

- **Architecture:** Before generating row instances, iterate all visible cells in `snapshot.grid`, count distinct glyph keys, estimate required shelf area, and clear or grow the atlas *before* any row is processed.
- **Coupling & Blast Radius:** Low-to-medium. Confined to `renderer/`, but requires maintaining a duplicate layout estimation model.
- **Overhead & Flaws:** Requires scanning 2,000–12,000 cells on *every single frame* (even when 100% of rows are clean and cached). Furthermore, if preflight shelf estimation differs by even 1 pixel from actual shelf wrapping, runtime overflow can still occur, necessitating an emergency fallback path anyway.
- **Verdict:** **REJECTED.** Incurs continuous per-frame CPU allocation overhead to handle an exceptional boundary condition.

### Candidate 3 (RECOMMENDED): Reactive Current-Frame Rebuild with Fast Shelf Simulation & Multi-Step Geometric Growth

- **Architecture:**
  1. *Fast Path (Normal Frames):* Row cache iterates rows. Clean rows are reused directly from cache; dirty rows call `get_or_insert`. Zero preflight scan overhead.
  2. *Overflow Detection:* When `get_or_insert` cannot fit a glyph on the current shelves, it returns `AtlasInsertResult::Full`. Row processing records overflow and marks the frame incomplete.
  3. *Deterministic Shelf Simulation on Visible Set:* Before encoding or submitting passes, `NativeTerminalRenderer` intervenes:
     - Extracts the unique visible glyph keys from `snapshot.grid` in exact row-major order, applying the identical filtering and advance rules as `build_row_instances`.
     - Tests candidate dimensions $C \in [\text{current\_dim}, 2048, 4096, \dots, \text{max\_device\_dim}]$ via `simulate_shelf_fit`.
     - **Branch A (D4 - History Pressure):** If visible glyphs fit within current dimensions ($C = \text{current\_dim}$), historical entries are the sole cause. `atlas.clear()` evicts history, resets coordinates to `(0,0)`, and bumps generation. Row cache is cleared. Rows are rebuilt fresh in Attempt 2. All visible glyphs fit.
     - **Branch B (D1 - Capacity Pressure):** If visible glyphs exceed current dimensions, find the smallest candidate dimension $C \le \text{device.limits().max_texture_dimension_2d}$ where `simulate_shelf_fit` returns `true`. `atlas.grow_to(C, &gpu.device)` allocates the fitting size **once** (supporting multi-step jumps such as 1024 -> 4096). `atlas_bind_group` is recreated, row cache is cleared, and rows are rebuilt fresh in Attempt 2. All visible glyphs fit.
     - **Branch C (Hardware Limit Exceeded):** If no candidate dimension up to the device limit can fit the visible set, **never** report a successful partial frame. Immediately return a structured error: `Err(NativeTerminalError::LimitExceeded)`.
  4. *Strict Termination Invariant:* At most **one** rebuild pass is permitted per frame render. Unbounded retry is strictly forbidden.
- **Trade-Off Analysis:**
  - *Coupling:* Smallest possible blast radius. Zero changes to shaders, pipeline vertex layouts, `instances.rs` structures, or host callers.
  - *Performance:* Zero CPU overhead on clean frames. The shelf simulation runs only when an atlas actually fills. Allocates GPU textures at most once.
  - *Memory:* Default pane memory remains at 8 MiB. Only panes displaying massive distinct character sets grow to larger textures.
- **Verdict:** **SELECTED.**

---

## Detailed Design Specification & Invariants

### 1. Invariants

- **INV-1 (Current-Frame Completeness):** No terminal frame shall be presented with omitted glyphs if the unique visible glyph set can physically fit within the hardware-bounded atlas capacity ($\le \text{device.limits().max_texture_dimension_2d}$).
- **INV-2 (Zero Stale UV References):** No `GlyphInstance` submitted to a render pass shall reference UV coordinates from an evicted atlas generation or an overwritten shelf. Atlas clearing or growth invalidates all cached rows and forces a complete row rebuild.
- **INV-3 (Strict Termination Bound):** A call to `render_snapshot`, `render_to_surface_view`, or `render_to_surface_viewport` shall perform at most **one** recovery rebuild. The execution loop terminates unconditionally after iteration 2.
- **INV-4 (Single Allocation on Multi-Step Growth):** When growth is triggered, the simulation selects the exact fitting candidate dimension up to the hardware limit and allocates textures exactly once. Iterative single-step allocations are prohibited.
- **INV-5 (Structured Failure at Hardware Cap):** When visible glyph demand exceeds the hardware limit (`device.limits().max_texture_dimension_2d`), the renderer shall return `Err(NativeTerminalError::LimitExceeded)`. Presenting partial frames or logging log-only success at the hardware cap is strictly forbidden.
- **INV-6 (Texture & Bind Group Coherence):** Whenever atlas dimensions change, new GPU textures are allocated, previous textures are dropped, and `atlas_bind_group` is recreated before any render pass is encoded.
- **INV-7 (Truthful Accounting):** At all times:
  $$\text{allocated\_bytes} = (\text{width} \times \text{height} \times 8) + (\text{entries.len()} \times \text{std::mem::size\_of::< (GlyphKey, AtlasEntry) >()})$$
  $$\text{max\_capacity\_bytes} = (\text{max\_dimension} \times \text{max\_dimension} \times 8) + (\text{max\_entries} \times \text{std::mem::size\_of::< (GlyphKey, AtlasEntry) >()})$$
  $$\text{allocated\_bytes} \le \text{max\_capacity\_bytes} \quad \text{(invariantly true)}$$

### 2. Precise Row-Major Shelf Simulation Algorithm

To ensure the simulation mathematically matches the rebuild pass, `simulate_shelf_fit` applies the exact row-major first-occurrence ordering, cell filtering, and advance rules as `build_row_instances` (`instances.rs:180-184`) and `GlyphAtlas::get_or_insert` (`atlas.rs:126-175`):

```rust
pub struct UniqueVisibleGlyph {
    pub key: GlyphKey,
    pub is_wide: bool,
}

pub fn collect_unique_visible_glyphs(
    snapshot: &RenderSnapshot,
) -> Vec<UniqueVisibleGlyph> {
    let mut unique = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Exact row-major traversal matching build_row_instances
    for row in 0..snapshot.rows {
        if let Some(cells) = snapshot.grid.get(row as usize) {
            for col in 0..snapshot.cols {
                if let Some(c) = cells.get(col as usize) {
                    // Exact exclusion rules from instances.rs:180-184
                    if c.wide == CellWide::SpacerTail
                        || c.wide == CellWide::SpacerHead
                        || c.text.is_empty()
                        || c.invisible
                    {
                        continue;
                    }

                    // Exact whitespace exclusion from atlas.rs:126-128
                    if c.text.chars().all(char::is_whitespace) {
                        continue;
                    }

                    let key = GlyphKey {
                        text: c.text.clone(),
                        bold: c.bold,
                        italic: c.italic,
                    };

                    if seen.insert(key.clone()) {
                        unique.push(UniqueVisibleGlyph {
                            key,
                            is_wide: c.wide == CellWide::Wide,
                        });
                    }
                }
            }
        }
    }

    unique
}

pub fn simulate_shelf_fit(
    unique_glyphs: &[UniqueVisibleGlyph],
    candidate_dimension: u32,
    cell_width_px: u32,
    cell_height_px: u32,
) -> bool {
    let mut cursor_x = 0u32;
    let mut cursor_y = 0u32;
    let mut row_height = 0u32;

    for g in unique_glyphs {
        let width = if g.is_wide { cell_width_px * 2 } else { cell_width_px };
        let height = cell_height_px;

        if cursor_x + width > candidate_dimension {
            cursor_x = 0;
            cursor_y += row_height;
            row_height = 0;
        }

        if cursor_y + height > candidate_dimension {
            return false; // Exceeds candidate_dimension x candidate_dimension
        }

        cursor_x += width + 1;
        row_height = row_height.max(height);
    }

    true
}
```

### 3. Execution Flow in `NativeTerminalRenderer`

Inside `prepare_frame_instances(snapshot, selection)`:

```
[Start prepare_frame_instances]
              │
              ▼
[Attempt 1: update_and_flatten]
              │
     Overflow encountered?
     ├── NO ──► [Return instances directly (Fast path, 0 overhead)]
     └── YES
          │
          ▼
[collect_unique_visible_glyphs(snapshot)] (Row-major, identical filters)
          │
          ▼
[simulate_shelf_fit(unique, current_dimension, cw, ch)]
     ├── TRUE (D4: History pressure)
     │     │
     │     ├──► atlas.clear()  (resets cursors to 0,0; generation++)
     │     └──► RowCacheManager.clear()
     │
     └── FALSE (D1: Visible set exceeds current dimension)
           │
           ▼
[Search candidate sizes: next_power_of_two ..= device_limit]
           │
     Fitting candidate size C found?
     ├── YES ──► atlas.grow_to(C, &device)  (Allocates ONCE)
     │          recreate_atlas_bind_group()
     │          RowCacheManager.clear()
     │
     └── NO (Exceeds hardware cap)
          │
          └──► RETURN Err(NativeTerminalError::LimitExceeded)
               (No partial frame presented, no unpresented drops)
          │
          ▼
[Attempt 2: update_and_flatten (Proven to fit completely)]
          │
          ▼
[Return complete instances]
```

### 4. GPU Resource & Hardware Limit Semantics

- **Actual Hardware Limit Query:** `gpu_context.rs:60` specifies `required_limits: wgpu::Limits::default()`. In WGPU, default limits set `max_texture_dimension_2d` to 8,192, but mobile or downlevel adapters may cap at 2,048 or 4,096. The renderer queries `self.gpu.device.limits().max_texture_dimension_2d` dynamically at runtime. No hardcoded or unmeasured hardware capability is assumed.
- **Queued Texture Writes:** `queue.write_texture` stages writes into the WGPU queue. If an atlas is cleared, subsequent writes in Attempt 2 overwrite existing texel coordinates before command encoder submission. If an atlas grows, new `wgpu::Texture` instances are created; writes target the new texture, while the unreferenced old texture is deallocated by WGPU reference counting.
- **Bind Group Lifecycle:** `atlas_bind_group` holds views for `mask_texture` (binding 0) and `color_texture` (binding 1). Recreating the bind group updates the shader bindings immediately prior to command encoder pass recording.
- **Memory Footprint per Pane:**
  - Base: $1024 \times 1024 \times 8 \text{ B} = 8.0 \text{ MiB}$ texture payload.
  - 2048: $2048 \times 2048 \times 8 \text{ B} = 32.0 \text{ MiB}$ texture payload.
  - 4096: $4096 \times 4096 \times 8 \text{ B} = 128.0 \text{ MiB}$ texture payload.
  - Panes allocate only the size proved necessary by simulation. Normal panes remain at 8 MiB.

---

## Truthful Accounting Semantics (D7)

### Derivation from Actual Descriptors and Tracked Types

1. **Texture Descriptors:**
   In `GlyphAtlas::new` and `grow_to`:
   - `mask_texture`: format `wgpu::TextureFormat::Rgba8Unorm` -> 4 bytes per texel.
   - `color_texture`: format `wgpu::TextureFormat::Rgba8Unorm` -> 4 bytes per texel.
   Combined texture allocation for current dimensions:
   $$\text{texture\_bytes} = (\text{self.width} \times \text{self.height} \times (4 + 4)) \text{ bytes} = \text{self.width} \times \text{self.height} \times 8$$
2. **Tracked Metadata:**
   The atlas tracks glyph entries in `HashMap<GlyphKey, AtlasEntry>`. The tracked metadata overhead is defined strictly by the size of the tracked key-value pair without fabricated heap estimates:
   $$\text{ENTRY\_SIZE: usize} = \text{std::mem::size\_of::< (GlyphKey, AtlasEntry) >();}$$
   $$\text{metadata\_bytes} = \text{self.entries.len()} \times \text{ENTRY\_SIZE}$$
3. **Allocated Bytes:**
   $$\text{allocated\_bytes} = \text{texture\_bytes} + \text{metadata\_bytes}$$
4. **Real Bound (`max_capacity_bytes`):**
   Derived from the maximum possible allocation permitted by the hardware device limit `max_dim = device.limits().max_texture_dimension_2d`:
   $$\text{max\_texture\_bytes} = \text{max\_dim} \times \text{max\_dim} \times 8$$
   The maximum number of entries that can physically be packed is bounded by the minimum valid cell dimensions ($1 \times 1$ under `RendererConfig::validate()`), or the maximum shelf capacity:
   $$\text{max\_entries} = (\text{max\_dim} / \text{cell\_height\_px}) \times ((\text{max\_dim} + 1) / (\text{cell\_width\_px} + 1))$$
   $$\text{max\_metadata\_bytes} = \text{max\_entries} \times \text{ENTRY\_SIZE}$$
   $$\text{max\_capacity\_bytes} = \text{max\_texture\_bytes} + \text{max\_metadata\_bytes}$$

### Comparison of Accounting Across Baselines

| Metric | Clean HEAD | Foreign Baseline (`4cab8ab8a`) | Truthful Repaired Value |
| :--- | :--- | :--- | :--- |
| **Initial Texture Bytes** | $512 \times 512 \times 5 = 1,310,720$ | $1024 \times 1024 \times 5 = 5,242,880$ (Wrong) | $1024 \times 1024 \times 8 = 8,388,608$ (Truthful RGBA8) |
| **Max Capacity Calculation**| Fixed $4,194,304$ (4 MiB) | $1024 \times 1024 \times 5 = 5,242,880$ (Equals base!) | $\text{max\_dim}^2 \times 8 + \text{max\_entries} \times \text{sizeof(pair)}$ |
| **1st Entry `allocated_bytes`**| $1,310,720 + 64 \le 4,194,304$ (**PASS**) | $5,242,880 + 64 > 5,242,880$ (**FAIL 101**) | $8,388,608 + 64 \le \text{max\_capacity\_bytes}$ (**PASS**) |
| **50th Update Check** | $\le 4,194,304$ (**PASS**) | Unreachable (Panicked on #1) | $\le \text{max\_capacity\_bytes}$ (**PASS**) |

---

## Exact Write Scopes & Dependency Order

```
Step 1: renderer/atlas.rs ──► Define AtlasInsertResult, dynamic width/height, 
                              grow_to(), simulate_shelf_fit(), truthful stats()
         │
Step 2: renderer/instances.rs ──► Update build_row_instances to match AtlasInsertResult::Full
         │
Step 3: renderer/row_cache.rs ──► Return completeness/overflow status from update_and_flatten
         │
Step 4: renderer/renderer.rs ──► Implement prepare_frame_instances recovery loop, 
                                 recreate_atlas_bind_group(), and unit test
         │
Step 5: tests/ ──► Wire regression assertions for Q2, Q3, Q-Readback, and verify Q7
```

### Detailed File Changes

1. `src-tauri/src/native_terminal/renderer/atlas.rs`:
   - Replace hardcoded constants with fields: `width: u32`, `height: u32`, `max_dimension: u32`.
   - Introduce:
     ```rust
     #[derive(Debug, Clone, Copy, PartialEq, Eq)]
     pub enum AtlasInsertResult {
         Inserted(AtlasEntry),
         Empty,
         Full,
     }
     ```
   - Update `get_or_insert` to return `AtlasInsertResult`.
   - Implement `grow_to(&mut self, target_dimension: u32, device: &wgpu::Device) -> Result<(), NativeTerminalError>`.
   - Implement `collect_unique_visible_glyphs` and `simulate_shelf_fit`.
   - Fix `stats()` using the truthful formula derived above.
2. `src-tauri/src/native_terminal/renderer/instances.rs`:
   - In `build_row_instances`, match `AtlasInsertResult`:
     - `AtlasInsertResult::Inserted(entry)`: push `GlyphInstance`.
     - `AtlasInsertResult::Empty`: do nothing (normal empty/whitespace).
     - `AtlasInsertResult::Full`: record `has_overflow = true`.
   - Return `(bg_instances, glyph_instances, has_overflow)`.
3. `src-tauri/src/native_terminal/renderer/row_cache.rs`:
   - In `update_and_flatten`, propagate `has_overflow`:
     ```rust
     pub fn update_and_flatten(...) -> (Vec<RectInstance>, Vec<GlyphInstance>, u16, u16, bool)
     ```
   - Remove obsolete start-of-frame `atlas.take_overflow_pending()`.
4. `src-tauri/src/native_terminal/renderer/renderer.rs`:
   - Add `recreate_atlas_bind_group(&mut self)`.
   - Add private helper `prepare_frame_instances(&mut self, snapshot: &RenderSnapshot, selection: Option<&SelectionSnapshot>) -> Result<(Vec<RectInstance>, Vec<GlyphInstance>, u16, u16), NativeTerminalError>` implementing the multi-step recovery flow.
   - Refactor `render_snapshot`, `render_to_surface_view`, and `render_to_surface_viewport_internal` to use `prepare_frame_instances`.
   - Add the Q2 and Q3 unit tests to `renderer::tests`.

---

## Named RED/GREEN Test Regressions

### Q7 / D7 - Accounting Bound Contract Test

- **Test Command:**
  ```sh
  cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
  ```
- **Current Baseline State (RED):**
  Fails at line 29 with exit 101:
  `thread 'dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache' panicked at tests/native_terminal_renderer_contract/dirty_update_atlas.rs:29:5: atlas allocated bytes must not exceed max capacity`.
- **Repaired State (GREEN):**
  Passes initial frame render, passes initial stats assertion ($8,388,608 + \text{metadata} \le \text{max\_capacity}$), passes all 50 dirty update cycles, passes final boundedness assertion ($\le \text{initial} + 15$), and passes final max capacity assertion. Exit 0.

### Q2 / D1 - Dense Visible Working Set Convergence Test

- **Test Location:**
  `src-tauri/src/native_terminal/renderer/renderer.rs` (in `tests` module, exercising the real repaired entry path).
- **Test Name:**
  `all_visible_glyphs_survive_when_dense_2x_working_set_repeats`
- **Test Command:**
  ```sh
  cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::all_visible_glyphs_survive_when_dense_2x_working_set_repeats -- --exact --nocapture
  ```
- **Harness & Fixture:**
  - Real `NativeTerminalRenderer` initialized with config: 16x32 cells, scale 2.0, viewport 80x25.
  - Snapshot: 1,000 distinct covered Hangul syllables (`가`..), each `CellWide::Wide` followed by `CellWide::SpacerTail`.
  - First verify fixture glyphs rasterize with non-zero ink.
- **RED Behavior (on Baseline `4cab8ab8a`):**
  - Calling `render_snapshot` for Frame 0 yields 992 glyph instances (8 dropped).
  - Calling `render_snapshot` for Frame 1 (identical snapshot) yields 992 glyph instances (same 8 dropped).
  - Assertion that all 1,000 glyphs are rendered FAILS.
- **GREEN Behavior (with Repair):**
  - Frame 0 triggers overflow in Attempt 1, simulation selects candidate size 2048, atlas grows to 2048, bind group is recreated, Attempt 2 completes with all 1,000 glyph instances.
  - Frame 1 reuses cached rows and completes with all 1,000 glyph instances (`frame.reused_row_count == 25`, `frame.rebuilt_row_count == 0`).
  - Assertion PASSES.

### Q3 / D4 - History-Only Final Frame Omission Test

- **Test Location:**
  `src-tauri/src/native_terminal/renderer/renderer.rs` (in `tests` module).
- **Test Name:**
  `last_frame_renders_new_glyph_when_only_history_fills_atlas`
- **Test Command:**
  ```sh
  cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::last_frame_renders_new_glyph_when_only_history_fills_atlas -- --exact --nocapture
  ```
- **Harness & Fixture:**
  - Configure `NativeTerminalRenderer` for 16x32 cells at scale 2.0.
  - Prewarm its private atlas with exactly 992 distinct wide Hangul glyph insertions via `get_or_insert` until atlas shelf capacity is exhausted, without triggering an overflow.
  - Final snapshot: 2 columns, 1 row, containing 1 new covered wide glyph (`\u{D7A3}`) plus spacer tail.
  - Action: Execute exactly **one** call to `render_snapshot(&snapshot, None)`.
  - Baseline comparison: Render that same 1-glyph snapshot using a pristine, fresh `NativeTerminalRenderer`.
- **RED Behavior (on Baseline `4cab8ab8a`):**
  - New glyph hits overflow, returns `None`.
  - Warmed `OffscreenFrame` contains only background clear color; whole glyph cell region differs from fresh renderer.
  - Assertion `assert_eq!(warmed_frame.pixels, fresh_frame.pixels)` FAILS.
- **GREEN Behavior (with Repair):**
  - `prepare_frame_instances` detects overflow. Unique visible glyph count = 1.
  - Detects visible set fits in 1024x1024. Clears historical entries. Rebuilds frame.
  - `assert_eq!(warmed_frame.pixels, fresh_frame.pixels)` PASSES with 100% pixel identity across the entire frame.

### Q-Readback - Offscreen Pixel Readback Integrity Across Growth

- **Test Location:**
  `src-tauri/tests/native_terminal_renderer_contract/offscreen_render.rs`.
- **Test Name:**
  `test_dense_scale_2_working_set_pixel_readback_integrity`
- **Test Command:**
  ```sh
  cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract offscreen_render::test_dense_scale_2_working_set_pixel_readback_integrity -- --exact --nocapture
  ```
- **Rigorous Verification Contract:**
  - Renders the 1,000-glyph dense snapshot at 2x through `render_snapshot`. Reads back `OffscreenFrame.pixels`.
  - For **all 1,000** glyph cells:
    - Obtains the independent reference pixel tile by rendering that single glyph cell in isolation with a fresh reference renderer.
    - Extracts the $32 \times 32$ pixel block from `OffscreenFrame.pixels` at $(col \times 32, row \times 32)$.
    - Asserts `assert_eq!(frame_cell_pixels, reference_tile_pixels)` for every one of the 1,000 cells.
  - This guarantees that every glyph samples from its exact UV coordinates in the grown atlas, proving that no wrong-glyph UVs or coordinate distortions exist.

---

## Baseline Provenance & Worktree Isolation Strategy

### Worktree Scope & Isolation Rules

- **Isolated Repair Worktree:**
  `/Users/indo/code/project/orca-lite-rendering-20260906`
  Base: Clean HEAD commit `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.
  Status: Untouched, clean git tree.
- **Main Working Tree:**
  `/Users/indo/code/project/orca-lite`
  Status: Read-only for this assignment; zero edits or commits.
- **Strict Scope of Baseline:**
  The foreign baseline is strictly confined to the two renderer source files:
  1. `src-tauri/src/native_terminal/renderer/atlas.rs`
  2. `src-tauri/src/native_terminal/renderer/row_cache.rs`
  No modifications to `src-tauri/Cargo.toml`, `src-tauri/vendor/`, scripts, or UI files shall be imported.

### File Hashes & Scoped Diff Identity

The exact file hashes of the foreign baseline files (matching commit `4cab8ab8ac671499b7a20c8f66baac338eea9c15` on `origin/main`) are:
- `src-tauri/src/native_terminal/renderer/atlas.rs`:
  SHA-256: `4b3e5b75c3f9d516aa37738e390d113015cd3da590bd86432fe2e9b53b083082`
- `src-tauri/src/native_terminal/renderer/row_cache.rs`:
  SHA-256: `a3476565925cea82f6fb87d38699af29e02e3c97f8f8f5063fd6ae3a755273a9`

The exact scoped diff hash between clean base `b8f82d707` and baseline commit `4cab8ab8a` for only these two files:
```sh
git diff b8f82d707f0cb99907e3d79c0c9cdc75053ef931 4cab8ab8ac671499b7a20c8f66baac338eea9c15 -- \
    src-tauri/src/native_terminal/renderer/atlas.rs \
    src-tauri/src/native_terminal/renderer/row_cache.rs | shasum -a 256
```
Verified Scoped Patch Diff SHA-256:
`67e0ab444b147b9d865b533c08fb74b6f5f559aba87191cb1945f68231b0603f`

*(Note: The previous hash `785c7b5e...` represented the entire untracked working diff across the whole repository and is discarded as non-scoped).*

### Commit Hygiene & Integration Workflow

1. **No Broken Commits Rule:**
   No commit in the repair worktree shall contain a known failing baseline or failing RED tests. Every committed state in git history must compile cleanly and pass all tests (`cargo test` exit 0).
2. **RED Artifact Capture:**
   RED verification receipts and failure logs are captured outside git tracking in `.omo/evidence/ulw/rendering-review-20260906/` before fixes are applied.
3. **Atomic Green Repair Commit:**
   In the isolated worktree `/Users/indo/code/project/orca-lite-rendering-20260906`, the repair increment lands as a single verified atomic commit:
   - Message: `fix(native-terminal): bounded dynamic atlas growth and truthful accounting (D1, D4, D7)`
   - Body: Explicitly cites the imported baseline provenance (files `atlas.rs` and `row_cache.rs`, scoped diff SHA-256 `67e0ab44...`) and records the verified GREEN test passes.
4. **Lead Sign-Off:**
   Merging into `origin/main` is conducted only after this single green commit is reviewed and approved by the lead.

## Lead adoption and current execution contract

This section supersedes stale worktree/provenance and execution-order details
above. Main's atlas/cache baseline is now committed as `4cab8ab`; subsequent
main commits include unrelated runner and onboarding work. Atlas implementation
uses its own locked worktree:
`/Users/indo/code/project/orca-lite-atlas-20260906`, branch
`fix/rendering-atlas-20260906`, base
`e2a67859400e80c480de42a6ec687cda6235c275`.
Both renderer baseline files were read and matched main. No foreign uncommitted
Cargo, HAL or documentation change is imported. The older repair worktree now
contains other workers' live changes and is not clean or available to this owner.

Execution is test-first: add D1/D4 tests and capture their actual RED, and rerun
the existing D7 RED on this new base, before any production change. The numbered
file-dependency list above is not permission to implement before tests.
Keep each verified renderer increment green; no baseline/RED-only failing commit.
Main integration still requires the user's recorded approval, not merely lead
sign-off.

Implementation clarifications:

- Query the successfully negotiated device limit. `GpuContext::new` requests
  `wgpu::Limits::default()`; a request is not silently reduced to an unsupported
  adapter limit. No new universal hardware guarantee is implied.
- Check individual glyph width and height against every candidate before shelf
  packing. Match row-major first occurrence and insertion filtering. If zero-ink
  glyphs are included conservatively, do not claim the estimate is exact; ensure
  the true-cap path cannot falsely reject a representable visible set.
- On any unexpected second-pass capacity miss, return non-success before draw
  submission. Never emit a successful incomplete frame or spin.
- Metadata accounting must state its model honestly: tracked key/value payload
  is not total allocator, hash-table spare capacity, String heap or driver
  residency. Derive bounds from the actual descriptor/configuration and model;
  do not invent a byte ceiling or claim unmeasured total memory precision.
- Pixel proof compares every full glyph-cell tile against independently rendered
  references. Fresh reference renderers may render bounded batches that each fit
  the base atlas, avoiding 1,000 needless GPU-device initializations. Each batch
  must start without accumulated atlas pressure.
- Cover both 1,000 and 4,000 distinct visible glyph cases, repeated frames,
  history-only final output, empty glyphs and a controlled capacity-error path.
  Native desktop screenshots remain aggregate evidence, not supplied by these
  offscreen tests.

## Verification corrections A1-A3

The implementation must satisfy these decisions in addition to the selected
strategy. They resolve the concrete plan blockers in
`repair-phase-verification.md`.

1. **Oversized single glyph (A1).** A shelf candidate fails immediately if an
   individual glyph's allocation width or height exceeds that candidate. Do not
   reset x and then consider only y. Test through the real preparation path.
2. **One authoritative prepared sequence (A2).** On recovery, prepare the ordered
   unique glyphs with the actual raster classification and allocation dimensions.
   Fit calculation and atlas rebuilding consume that same prepared sequence.
   Whitespace and all-zero non-color rasters consume no slot; color raster rules
   remain unchanged. Preserve the empty classifications for that rebuild so an
   independent second rasterization cannot disagree with the fit decision.
   Include covered glyphs mixed with uncovered non-whitespace empties under
   pressure; require complete covered pixels and no false growth/cap error.
3. **No cacheable partial result (A3).** Every failed preparation invalidates all
   incomplete cached rows before another call can reuse them. Clearing the atlas
   and bumping its generation is sufficient if the next row-cache call necessarily
   observes it. Check the second attempt's completeness explicitly; any residual
   capacity failure returns a typed non-success before encoding/submission.
   Repeated identical over-cap inputs remain errors, then a smaller valid input
   must recover to the independent reference frame. There is no third attempt.

Additional exact renderer-internal tests:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::oversized_glyph_is_rejected_before_shelf_placement -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::empty_rasters_do_not_force_atlas_growth_under_pressure -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::failed_frame_cannot_be_reused_as_partial_success -- --exact --nocapture
```

Use baseline-compatible real-renderer/pixel assertions for RED; an absent new
helper or non-existent frame field is not a failing behavioral proof. A narrow
controlled limit/fault seam is permitted for the error path, provided the real
preparation and row cache consume it. Native GPU initialization/font coverage
must succeed before accepting an assertion as the required RED.

The `GlyphAtlasStats` field documentation in `renderer/types.rs` is an approved
narrow scope addition to explain tracked payload and its exclusions. An already
acquired swapchain texture may be dropped unpresented on a renderer error; the
promise is no successfully presented partial frame, not zero unpresented drops.
WGPU can retain replaced textures for in-flight work; logical replacement is not
evidence of immediate physical memory release.
