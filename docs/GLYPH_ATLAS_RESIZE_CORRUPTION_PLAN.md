# Ferryx Native Terminal: Window Resize Glyph Atlas Corruption Investigation & Implementation Plan

**Date:** 2026-09-04  
**Status:** Reviewed & Approved by Oracle  
**Target Subsystems:** `src-tauri/src/native_terminal/renderer/atlas.rs`, `src-tauri/src/native_terminal/renderer/row_cache.rs`

---

## 1. Problem Statement & Symptom Analysis

### 1.1 Observed Symptoms (User Screenshot)
- **Visual corruption on window resize / maximize**:
  - Across multiple terminal panes (split views and single pane), the upper rows display scattered, incorrect glyphs while character positioning, column/row alignment, cursor position, and whitespace remain intact.
  - Examples of 1:1 character substitution observed in the screenshot:
    - `Native Surface` -> `N•t-vr Su..f·r` (`e` rendered as `r`)
    - `단축키` -> `다축키` (`단` rendered as `다`)
    - `룩앤필` -> `록앤필` (`룩` rendered as `록`)
    - `현재 구조의 꼬인 상태는` -> `현간 tt조의 꼬ㄹ른시 내도`
  - In all panes, the **bottom rows render completely normally and cleanly**, whereas the **top rows exhibit glyph substitution**.

---

## 2. Root Cause Analysis

### 2.1 Under-dimensioned Glyph Atlas (512x512 px)
- In `src-tauri/src/native_terminal/renderer/atlas.rs:8-9`:
  ```rust
  const ATLAS_WIDTH: u32 = 512;
  const ATLAS_HEIGHT: u32 = 512;
  ```
- In `get_or_insert()` (line 133): wide CJK cells allocate `width = config.cell_width_px * 2` and advance `cursor_x += width + 1`.
- On modern macOS Retina / HiDPI displays (device scale factor 2.0), standard terminal fonts (13–14pt) produce cell pixel dimensions of approximately 16x32px for ASCII and 32x32px for CJK (wide) characters.
- Packing capacity of a 512x512 texture:
  - CJK cells (32x32 + 1px padding): $\lfloor 512 / (32 + 1) \rfloor = 15$ glyphs per shelf.
  - Vertical shelves: $\lfloor 512 / 32 \rfloor = 16$ shelves.
  - Hard physical ceiling is **only 240 wide CJK glyphs**.
  - ASCII cells (16x32 + 1px padding): $\lfloor 512 / 17 \rfloor \times 16 \approx 480$ glyphs.
- A single terminal pane with Korean text (e.g. AI agent conversation, git logs, code reviews) easily exceeds 300–600 unique characters across a typical 40–60 row scrollback view.

### 2.2 Destructive Mid-Frame Atlas Purge & Texture Overwrite
- In `src-tauri/src/native_terminal/renderer/atlas.rs:139-146`:
  ```rust
  if self.cursor_y + height > ATLAS_HEIGHT {
      // Capacity boundary reached: clear and reset
      self.entries.clear();
      self.cursor_x = 0;
      self.cursor_y = 0;
      self.row_height = 0;
      self.generation = self.generation.wrapping_add(1);
  }
  ```
- When `update_and_flatten()` processes a terminal snapshot row-by-row (from row 0 downwards), the atlas reaches capacity mid-frame (e.g. at row 25).
- At that point, `entries.clear()` wipes the glyph lookup cache, resets the packing cursor to `(0, 0)`, and increments `generation`.
- When subsequent rows (rows 26..rows-1) insert new glyphs, `queue.write_texture()` writes those glyphs into the GPU texture starting at `(0, 0)`, directly overwriting the texture regions previously occupied by rows 0..25.

### 2.3 Broken Generation-Change Synchronization in `RowCacheManager`
- In `src-tauri/src/native_terminal/renderer/row_cache.rs:24-52`:
  ```rust
  pub fn update_and_flatten(...) -> ... {
      if atlas.generation != self.last_atlas_generation {
          self.entries.clear();
          self.last_atlas_generation = atlas.generation;
      }
      ...
      for row in 0..snapshot.rows {
          // builds row 0..k (referencing generation N UVs at (0, 0))
          // row k triggers overflow: atlas.generation becomes N+1, texture (0,0) overwritten!
          // builds row k..rows (referencing generation N+1 UVs at (0, 0))
      }
      ...
      // combines all rows: rows 0..k point to overwritten glyphs!
  ```
- `RowCacheManager` only checks `atlas.generation` **once at the start of the frame**.
- If an overflow happens mid-frame:
  1. Rows 0..k keep UV coordinates pointing to atlas generation N.
  2. Rows k..end overwrite `(0, 0)` in generation N+1.
  3. The rendered frame draws rows 0..k with whatever new glyphs were placed at `(0, 0)` by rows k..end!
  4. On the subsequent frame, `atlas.generation != last_atlas_generation` triggers a cache wipe. However, because the total unique glyphs on screen still exceed 240, the rebuild pass overflows AGAIN mid-frame at row ~25, causing a permanent mid-frame thrashing loop!

### 2.4 Why Window Resize Specifically Triggers This
1. **Row Count Expansion**: Resizing or maximizing the window increases `snapshot.rows` from ~20 to ~50-70. More text is visible at once, causing unique glyph count to jump past the 240 threshold.
2. **Row Cache Invalidation on Resize**:
   ```rust
   if self.entries.len() != snapshot.rows as usize {
       self.entries = vec![RowCacheEntry::default(); snapshot.rows as usize];
   }
   ```
   Every frame during interactive window drag that changes the row count wipes all cached rows. This forces an immediate, synchronous rebuild of all rows in a single frame from row 0 to the bottom, ensuring immediate atlas exhaustion.

---

## 3. Implementation Specifications (Oracle Reviewed)

### Step 1: Enlarge Atlas Texture & Defensive Checks (`src-tauri/src/native_terminal/renderer/atlas.rs`)
1. **Atlas Dimensions**:
   - `const ATLAS_WIDTH: u32 = 2048;`
   - `const ATLAS_HEIGHT: u32 = 2048;`
2. **Capacity Contract Safety**:
   - In `tests/native_terminal_renderer_contract/dirty_update_atlas.rs`, the contract asserts:
     `allocated_bytes <= max_capacity_bytes`.
   - Texture memory alone: $2048 \times 2048 \times 8\,\text{bytes} = 33,554,432\,\text{bytes} \equiv 32\,\text{MB}$.
   - Add headroom for HashMap table allocations and metadata:
     `const MAX_CAPACITY_BYTES: usize = 36 * 1024 * 1024; // 36 MB`
3. **Memory Metrics in `stats()`**:
   - Update to reflect RGBA8 mask + RGBA8 color (8 bytes/pixel):
     ```rust
     let allocated_bytes = (ATLAS_WIDTH * ATLAS_HEIGHT * 8) as usize + entry_overhead;
     ```
4. **Defensive Dimension Guard in `get_or_insert()`**:
   - Prevent out-of-bounds writes or WGPU device panics on extreme zoom/font sizes:
     ```rust
     if width > ATLAS_WIDTH || height > ATLAS_HEIGHT {
         tracing::warn!(width, height, ATLAS_WIDTH, ATLAS_HEIGHT, "Glyph dimensions exceed atlas limits");
         return None;
     }
     ```
- **Capacity Impact**:
  - CJK cells (32x32 + 1px padding): $\lfloor 2048 / 33 \rfloor \times \lfloor 2048 / 32 \rfloor = 62 \times 64 = 3,968$ glyphs (~16.5x increase).
  - ASCII cells (16x32 + 1px padding): $\lfloor 2048 / 17 \rfloor \times 64 = 120 \times 64 = 7,680$ glyphs (~16x increase).
  - The entire KS X 1001 common Korean character set (2,350 characters) plus full ASCII and box-drawing characters fits comfortably in a single atlas simultaneously.

### Step 2: Mid-Frame Atlas Purge Resilience in `RowCacheManager` (`src-tauri/src/native_terminal/renderer/row_cache.rs`)
Implement a structured 2-pass loop with early break on generation changes:

```rust
pub fn update_and_flatten(
    &mut self,
    snapshot: &RenderSnapshot,
    selection: Option<&SelectionSnapshot>,
    config: &RendererConfig,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
) -> (Vec<RectInstance>, Vec<GlyphInstance>, u16, u16) {
    for pass in 0..2 {
        let start_generation = atlas.generation;
        if start_generation != self.last_atlas_generation {
            self.entries.clear();
            self.last_atlas_generation = start_generation;
        }

        if self.entries.len() != snapshot.rows as usize {
            self.entries = vec![RowCacheEntry::default(); snapshot.rows as usize];
        }

        let mut rebuilt: u16 = 0;
        let mut reused: u16 = 0;
        let mut generation_diverged = false;

        for row in 0..snapshot.rows {
            let row_hash = compute_row_hash(row, snapshot, selection, config);
            let cached = &self.entries[row as usize];

            if cached.hash == row_hash && !cached.bg_instances.is_empty() {
                reused += 1;
            } else {
                let (bg, glyph) =
                    build_row_instances(row, snapshot, selection, config, atlas, queue);
                self.entries[row as usize] = RowCacheEntry {
                    hash: row_hash,
                    bg_instances: bg,
                    glyph_instances: glyph,
                };
                rebuilt += 1;
            }

            // Early short-circuit if an atlas overflow occurred during this row
            if atlas.generation != start_generation {
                generation_diverged = true;
                break;
            }
        }

        // Mid-frame overflow handling: retry pass with clean atlas
        if generation_diverged {
            if pass == 0 {
                tracing::warn!(
                    from_gen = start_generation,
                    to_gen = atlas.generation,
                    "Glyph atlas overflowed mid-frame; restarting row cache pass"
                );
                atlas.clear();
                self.entries.clear();
                self.last_atlas_generation = atlas.generation;
                continue;
            }
            tracing::warn!("Visible frame content exceeds entire atlas capacity; rendering partial atlas");
        }

        // Build final flattened instances
        let total_cells = (snapshot.cols as usize) * (snapshot.rows as usize);
        let mut all_bg = Vec::with_capacity(total_cells + 16);
        let mut all_glyph = Vec::new();
        for entry in &self.entries {
            all_bg.extend_from_slice(&entry.bg_instances);
            all_glyph.extend_from_slice(&entry.glyph_instances);
        }

        return (all_bg, all_glyph, rebuilt, reused);
    }

    unreachable!("Structured retry loop must return within 2 passes");
}
```

---

## 4. Verification & Testing Strategy

### 4.1 Unit Tests
1. **Atlas Dimension & Capacity Test (`src-tauri/src/native_terminal/renderer/atlas.rs`)**:
   - Verify `ATLAS_WIDTH == 2048`, `ATLAS_HEIGHT == 2048`.
   - Verify `allocated_bytes <= max_capacity_bytes` under heavy population.
   - Verify defensive guard rejects oversized requests (`width > 2048 || height > 2048`) without panic.
2. **RowCacheManager Mid-Frame Recovery Test (`src-tauri/src/native_terminal/renderer/row_cache.rs`)**:
   - Create a test fixture that artificially trips `atlas.clear()` / generation bump halfway through a multi-row snapshot.
   - Verify `update_and_flatten()` retries and returns instances with consistent UV coordinates from the new generation only.
3. **Resize Row Count Test**:
   - Verify rapid changing of row counts preserves cache consistency.

### 4.2 Regression Tests
- Run `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal`.
- Run `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract`.
- Execute `bun tauri dev` desktop QA for window resize and split pane transitions.
