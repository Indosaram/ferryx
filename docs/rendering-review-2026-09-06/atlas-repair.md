# Atlas repair increment: D1 / D4 / D7

## Outcome and acceptance boundary

**Renderer increment delivered and verified.** Commit
`17fb10608ad31e0e60ae565e8dc686afdc4735f9` on
`fix/rendering-atlas-20260906` completes representable current frames under atlas
pressure and reports truthful tracked payload. The isolated repair worktree is
`/Users/indo/code/project/orca-lite-atlas-20260906`; its index and working tree are
clean after the commit. No main merge or push was performed.

**Native desktop acceptance remains PENDING for the lead.** These are real Metal
GPU renders/readbacks, including the public surface-view and viewport APIs on
offscreen attachments, not macOS compositor/drawable screenshots. No desktop,
daemon, or user input was used. Image-read attempts on two saved PNGs returned
`Current model does not support images`; no human-style visual-inspection verdict
is claimed. Exact numerical full-pixel comparisons did execute and pass.

Owner: task `st_01a0777e`, UTC 2026-09-06. The final execution contract and A1-A3
corrections in `atlas-repair-plan.md` were treated as authoritative. The earlier
verification delta was design readiness, not prior behavior approval.

## Provenance and scope

| Identity | Value |
| --- | --- |
| Base commit | `e2a67859400e80c480de42a6ec687cda6235c275` |
| Base tree | `eea4311e20c74805c641b0aed8920da2c59abf51` |
| Repair commit | `17fb10608ad31e0e60ae565e8dc686afdc4735f9` |
| Repair tree | `e42e470d7e1e715f043e1e4246187b93f472dfee` |
| Complete scoped repair diff SHA-256 | `eeaf16aa2ab7c88ed736ced528212a48ef5946a4a8593a5f74a94ca415616d3d` |
| Pre-production RED-test-only patch SHA-256 | `83534c0b1b00507e2ec61de46ba74dad916afe5b067350f3ad06b00167e805fc` |

The committed `4cab8ab` atlas/cache baseline was already present. Before
production edits, the two files still matched the supplied baseline hashes:
atlas `4b3e5b75c3f9d516aa37738e390d113015cd3da590bd86432fe2e9b53b083082`,
row cache `a3476565925cea82f6fb87d38699af29e02e3c97f8f8f5063fd6ae3a755273a9`.
No old dirty patch was imported and no baseline or failing RED commit was made.
The branch had no configured upstream; it remains local.

The commit changes only five approved renderer files (`atlas.rs`, `row_cache.rs`,
`renderer.rs`, `instances.rs`, `types.rs`) and two direct renderer contract files:
`offscreen_render.rs` adds the named all-tile readback case;
`theme_contract.rs` mechanically updates six existing calls for the fallible
row-builder/prepared-sequence signature. Its assertions are unchanged. The
existing `dirty_update_atlas.rs` contract is completely unchanged. No manifest,
dependency, shader, host, IPC, platform, UI or vendor source was edited.

This report is intentionally outside that commit in the requested main-tree
documentation path. Other owners' main/repair-tree work was not touched.

## Mechanism delivered

- Normal preparation retains dirty-row caching without visible-set preflight.
  All three real renderer entry paths call `prepare_frame_instances` before
  creating an encoder or submitting draws.
- Capacity is represented by `Err(NativeTerminalError::LimitExceeded)`; empty
  glyphs are `Ok(None)` and inserted/resident glyphs are `Ok(Some(entry))`.
  This uses the existing typed error rather than adding a separate Full enum.
  Row generation cannot mistake capacity failure for successful absence.
- On the first Full, shared cell filtering collects one row-major,
  first-occurrence prepared sequence, retaining actual allocation dimensions and
  Alpha/Subpixel/Color/empty raster classification. Fit and the rebuild consume
  this same sequence; empties are not independently rerasterized in attempt two.
- One shelf-placement operation serves both simulation and upload. It rejects
  either oversized extent before modifying shelves; whitespace and all-zero
  non-color rasters reserve no slot. Color rasters keep the prior color rule.
- Candidate selection tries the current size, geometric growth and the final
  negotiated device limit. It allocates only the selected size, including a
  directly observed 1024 -> 4096 jump. History-only pressure clears at the
  current size. Generation invalidates old UV rows; growth recreates both texture
  bindings before encode.
- Every row-preparation error clears all potentially incomplete cached rows.
  Thus cap-selection errors start with invalidated rows, too. The second
  preparation result is returned directly: a second Full is an error, with no
  partial-success path and no third attempt. Test-only cap/fault/attempt counters
  exercise the actual preparation/cache/upload path, not a replacement policy.
- No host scheduling changes were made. An acquired native swapchain texture
  may still be dropped unpresented on an error; no successful partial presentation
  is promised or allowed by this renderer gate.

## Actual RED before production edits

Evidence root (all paths below relative to it):
`/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/atlas/`.

| Case | Actual committed-base RED | Matching final GREEN |
| --- | --- | --- |
| D1: `all_visible_glyphs_survive_when_dense_2x_working_set_repeats` | Real Apple M4 Max / Metal initialization and 1000 raster-ink checks succeeded. Both real frames had 992 ink origins; assertion failed, 1 test failed, exit 101. Independently audited missing/wrong tiles are exactly 992-999 in both frames. | 1000/1000 origins in F0 and F1; unchanged F1 reuses all 25 rows; every full tile matches bounded fresh references. |
| D4: `last_frame_renders_new_glyph_when_only_history_fills_atlas` | Exactly 992 successful covered wide insertions, then one final render of U+D7A3. Fresh reference has ink. Whole-frame equality failed, 1 test failed, exit 101; saved readback differs in 822 of 4096 bytes. | Exactly one final render matches the independent fresh frame in all 4096 bytes. |
| D7: `dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache` | Re-observed here: original initial bound assertion at `dirty_update_atlas.rs:29` failed, 1 test failed, exit 101. | Original initial bound, all 50 dirty updates, final entry bound and final byte bound pass, exit 0. |

Raw receipts: `red-all_visible_glyphs_survive_when_dense_2x_working_set_repeats.log`,
`red-last_frame_renders_new_glyph_when_only_history_fills_atlas.log`, `red-q7.log`;
the first two frames/reference are in `red/`. `red-tests.patch` contains only
renderer test additions. The final tests retain those pixel/origin assertions;
the prewarm call only unwraps the new fallible insertion result.

The first build attempts failed because this isolated tree has no `ui/dist`.
Those logs are retained as `red-*-setup-failure.log` and are NOT behavioral RED.
No missing field/helper, zero-match, GPU-init or font-init failure was counted.

## Final verification and executable reproduction

Runtime: Darwin arm64, Apple M4 Max / Metal, negotiated
`max_texture_dimension_2d=8192`, Rust/Cargo 1.92.0, Zig 0.16.0. The existing local
Ghostty clone is pinned to `6a508fd5e34c7e222c052a6d00bb3891ff3feace` and was reused.

Build/test environment (shell-only, no persistent configuration edits):

```sh
cd /Users/indo/code/project/orca-lite-atlas-20260906
export CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target
export CARGO_BUILD_JOBS=8
export TAURI_CONFIG='{"bundle":{"resources":[]}}'
export ATLAS_EVIDENCE_DIR=/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/atlas/reproduction
```

`TAURI_CONFIG` disables missing UI bundle-resource copying only; these tests do
not launch/package the frontend. Shared build caches were reused without cleaning
or killing lock holders.

Each of these eight exact internal commands ran successfully with the prefix:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::<name> -- --exact --nocapture
```

| `<name>` | Observed proof |
| --- | --- |
| `all_visible_glyphs_survive_when_dense_2x_working_set_repeats` | Both 1000-key 80x25 frames complete; F1 row reuse. |
| `last_frame_renders_new_glyph_when_only_history_fills_atlas` | One final output after 992-key history, exact fresh-reference equality. |
| `dense_working_set_grows_across_multiple_atlas_sizes` | 4000 keys, 160x50, one 1024 -> 4096 allocation, all 50 rows reused on F1; all 4000 full tiles equal independently fresh batches of at most 640 entries. |
| `oversized_glyph_is_rejected_before_shelf_placement` | Width 1200 and height 1200 separately rejected at controlled cap 1024 before target/upload errors; final non-power-of-two candidate 1500 succeeds and matches a fresh reference. |
| `empty_rasters_do_not_force_atlas_growth_under_pressure` | Full history, 1000 non-whitespace keys with eight verified empty rasters, one new covered key: two preparations succeed at cap 1024, zero growth, 992 resident ink entries and complete fresh-reference pixel equality. |
| `failed_frame_cannot_be_reused_as_partial_success` | Three identical cap failures and three injected second-pass Full failures remain typed errors; attempt counts bounded to 1/2; no offscreen target created; both smaller-frame recoveries match fresh ink. Fault triggers after 40 successful second-pass insertions, allowing a complete row to have been cached. |
| `surface_entry_paths_complete_dense_frames_before_submission` | Fresh renderers independently force growth through public surface-view and surface-viewport APIs; full attachment readback equals the offscreen result, including translated viewport and every outside background pixel. |
| `tracked_payload_matches_textures_entries_clear_and_config_changes` | Descriptor/key-value arithmetic verified at base, growth, clear, 2x -> 1x -> 2x config changes, and repopulation; scale-change frames match fresh references. |

The two exact integration commands also passed (each ran one test):

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract offscreen_render::test_dense_scale_2_working_set_pixel_readback_integrity -- --exact --nocapture
```

The readback integration uses three fresh reference renderers, batches of
400/400/200, not 1000 GPU-device initializations. Every reference tile must have
ink, and all 1024 pixels / 4096 RGBA bytes of each tile in both dense frames must
match exactly. There are no sleeps, polling delays or tolerance-based comparisons
in the added tests.

Broader and runnable-entry gates:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --example native_terminal_renderer_poc
export CARGO_BIN_EXE_native_terminal_renderer_poc="$CARGO_TARGET_DIR/debug/examples/native_terminal_renderer_poc"
"$CARGO_BIN_EXE_native_terminal_renderer_poc" --headless --output /Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/atlas/poc-headless.png
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml
```

- Example build and real headless execution: **exit 0**, 50 frames, 800x480 PNG,
  final dirty rows 1 rebuilt / 23 reused. No window mode was invoked.
- Full contract: **26 passed, 0 failed, 0 ignored**, exit 0.
- Native-terminal library filter: **119 passed, 0 failed, 0 ignored** (489 other
  tests filtered by the requested domain), exit 0.
- All seven changed-file LSP requests: **No diagnostics found**, before final
  `cargo check`; check **exit 0**. Seven existing warnings outside changed files
  remain visible in the Cargo logs; none was suppressed or fixed out of scope.
- Formatting and scoped `git diff --check` passed. No new dependencies.

Intermediate failures are preserved, not hidden: the initial integration build
exposed six existing `theme_contract` call sites requiring the signature update;
`green-test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache.log`
contains that compilation failure. The first full contract ran 24 passing tests
and failed the two existing standalone CLI cases because the executable was
absent at their default path (`full-contract.log`). Building the actual existing
example and using their existing `CARGO_BIN_EXE` seam resolved the prerequisite,
without editing or weakening those contracts. The corrected full run is
`full-contract-final.log`. `final-*.log` contains all ten final exact passes;
`full-native-terminal.log`, `build-example.log`, `poc-headless.log`, and
`cargo-check.log` contain the other complete receipts.

## Full saved-pixel audit

`green-final/` contains PNGs, unpadded raw RGBA8 and dimension/row metadata for
the named exact tests. Broader-run captures are separately namespaced.
`readback-audit.py` independently reopens those files and reference batches;
`readback-audit.json` / `.log` record **exit 0** and:

- RED dense F0/F1: exactly eight different/missing tiles, indices 992-999;
  the other 992 full tiles already match the independent references.
- GREEN dense and integration F0/F1: zero different/missing tiles, all 1000.
- GREEN 4000-key F0/F1: zero different/missing tiles, all 4000.
- History final: 822 differing bytes RED, zero GREEN.
- Empty-pressure frame: all 4,096,000 bytes equal the independent reference.
- Both smaller error recoveries: all 4096 bytes equal the fresh reference.
- Surface view and translated viewport: all 4,096,000 terminal bytes match;
  the viewport case additionally verifies all 223,232 outside pixels are opaque
  black. These use actual GPU copies/readbacks, not glyph counts alone.

Dense GREEN RGBA SHA-256:
`9ee3000f2570969a9132e82b22ca4be21b77eeb1f4e72093c16fda96bdd5193d`.
4000-key GREEN RGBA SHA-256:
`f5b3bc1c56bda685d6c1b0cd6e6c70ae08e530c5ee10078e2fad4cba9e13c97a`.
Each is identical between F0/F1. Native compositor pixels remain outside this
audit; unsupported image input also prevents a visual-review claim.

## D7 accounting model and observed values

Current tracked payload is exactly two RGBA8 descriptor payloads, `width * height
* 8`, plus `resident_entries * size_of::<(GlyphKey, AtlasEntry)>()`. The observed
pair size is 64 bytes, measured by `size_of`, not a fabricated heap estimate.
The maximum uses the actual negotiated extent and current configuration:

```text
max_entries = floor(max_dimension / cell_height)
            * floor((max_dimension + 1) / (cell_width + 1))
max_payload = max_dimension^2 * 8 + max_entries * pair_size
```

Byte multiplication is performed after widening to `usize`; theoretical maximum
arithmetic saturates at `usize::MAX` if not addressable. On this 64-bit device all
reported figures are exact, unsaturated. The narrow-cell slot count is a valid
upper bound for mixed narrow/wide entries; no arbitrary key-count/byte ceiling is
introduced. Config updates clear entries and recompute the bound from new cell
dimensions; clear does not falsely claim to release retained HashMap allocation.

| State | Texture payload | Entries | Tracked current bytes | Tracked maximum bytes |
| --- | ---: | ---: | ---: | ---: |
| Base, 16x32 | 8,388,608 | 0 | 8,388,608 | 544,751,616 |
| Dense 1000, 2048 atlas | 33,554,432 | 1000 | 33,618,432 | 544,751,616 |
| Dense 4000, 4096 atlas | 134,217,728 | 4000 | 134,473,728 | 544,751,616 |
| Cleared 2048 atlas | 33,554,432 | 0 | 33,554,432 | 544,751,616 |
| Config 8x16, one key | 33,554,432 | 1 | 33,554,496 | 566,689,792 |
| Config restored 16x32, one key | 33,554,432 | 1 | 33,554,496 | 544,751,616 |

**Exclusions:** String heap contents, HashMap spare buckets and allocator
overhead, prepared-raster temporaries, staging buffers, driver overhead and old
textures retained for in-flight GPU work. This is tracked resident key/value plus
current texture payload, **not total heap/driver memory**. Logical texture
replacement is not proof of immediate physical reclamation.

## Cleanup and handoff

All owned test/build/example invocations returned. Per-test renderers, reference
devices, textures and mapped buffers were scoped to those completed processes;
existing temporary PNG tests use `tempfile` RAII. No owned server, listener,
window, desktop app or daemon remains. `cleanup.log` records clean repair/vendor
status and no matching owned runtime processes. An unrelated later Cargo process
was observed and left untouched. No foreign lock holder was killed.

The temporary atlas-production staging file was removed. Requested RED/GREEN
logs, patches, PNG/RGBA evidence, metadata and the independent audit are retained.
Existing shared Cargo/Ghostty caches are deliberately retained, not cleaned.
Environment overrides existed only in tool shells. `provenance-before-commit.log`
and `provenance-after-commit.log` establish staged/committed diff identity and
file hashes; `commit.log` records the single successful commit.

The lead still owns native macOS dense/idle-final-frame compositor acceptance,
display-scale transition QA, and aggregate integration approval. Offscreen/API
success does not close those native criteria or the separately owned UI,
Wayland, Windows or host-acquisition repairs.
