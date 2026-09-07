# Independent atlas verification: D1 / D4 / D7 and A1-A3

**Disposition: approve the verified renderer implementation scope in commit
`17fb10608ad31e0e60ae565e8dc686afdc4735f9`. Native desktop acceptance is PENDING.**
No implementation blocker was found in the scoped diff. This is not approval of
main integration, packaging, other rendering repairs, or native 1x/2x display
behavior. GPU API/readback success is not a desktop pass.

Verifier: `st_01a077a8`, 2026-09-06 UTC. Only this report was written; no
production/test changes, commits, desktop actions, or whole-suite/build reruns.
The authoritative decisions are the final "Lead adoption and current execution
contract" and "Verification corrections A1-A3" sections of
`atlas-repair-plan.md`, not its superseded pseudocode/worktree instructions.

## Source and evidence identity

Paths used below:

- `W`: `/Users/indo/code/project/orca-lite-atlas-20260906`.
- `E`: `/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/atlas`.
- Renderer source references are relative to `W/src-tauri/src/native_terminal/renderer`.
- Contract references are relative to `W/src-tauri/tests/native_terminal_renderer_contract`.

| Item | Independently checked identity |
| --- | --- |
| Branch / parent | `fix/rendering-atlas-20260906` / `e2a67859400e80c480de42a6ec687cda6235c275` |
| Parent tree | `eea4311e20c74805c641b0aed8920da2c59abf51` |
| Repair commit / tree | `17fb10608ad31e0e60ae565e8dc686afdc4735f9` / `e42e470d7e1e715f043e1e4246187b93f472dfee` |
| Actual complete parent-to-repair diff SHA-256 | `eeaf16aa2ab7c88ed736ced528212a48ef5946a4a8593a5f74a94ca415616d3d`, identical to `E/final-scoped.patch` |
| RED test-only patch SHA-256 | `83534c0b1b00507e2ec61de46ba74dad916afe5b067350f3ad06b00167e805fc` |
| Parent `atlas.rs` SHA-256 | `4b3e5b75c3f9d516aa37738e390d113015cd3da590bd86432fe2e9b53b083082` |
| Parent `row_cache.rs` SHA-256 | `a3476565925cea82f6fb87d38699af29e02e3c97f8f8f5063fd6ae3a755273a9` |

Git inspection and independently computed hashes agree with `base.log` and the
before/after-commit provenance receipts. The RED patch's old renderer blob
`3cad2fa` matches the actual parent blob
`3cad2fa1e122f2fe7a1e48680f708096abe31eac`. The parent already contains the supplied
committed atlas/cache baseline; this is not a repair against the older 512 atlas.

The actual commit changes exactly seven files: `atlas.rs`, `instances.rs`,
`renderer.rs`, `row_cache.rs`, `types.rs`, `offscreen_render.rs`, and
`theme_contract.rs`. The last file adapts six existing row-builder calls to the
new fallible/prepared signature without weakening assertions. The original D7
contract is unchanged. No manifest, dependency, HAL, vendor, shader, host,
platform, IPC, or UI source was imported or edited by this increment. The repair
worktree was clean on inspection.

## Criterion dispositions

| Criterion | Checked disposition | Actual evidence and scope |
| --- | --- | --- |
| D1: complete dense visible sets | **PASS, renderer scope** | Same named 1000-key regression changes from 992/992 ink origins to 1000/1000. Independently rerun GREEN passed. F1 reuses all 25 rows. The 4000-key test and saved full-tile audit verify direct 1024 -> 4096 growth, one allocation, all 4000 entries, and 50 reused rows on F1. Native compositor closure remains pending. |
| D4: history-only final frame | **PASS, renderer scope** | Exactly 992 successful wide insertions precede exactly one final render of U+D7A3. RED is a blank final frame despite an ink-bearing fresh reference; GREEN equals all 4096 reference bytes. Independent rerun passed. Current-size fit causes clear/rebuild, not reliance on another output/repaint. Host-idle desktop closure remains pending. |
| D7: truthful bounded payload | **PASS, tracked model** | Original contract fails its first bound assertion on the parent and passes unchanged after repair, including 50 updates and final bounds. Descriptor/type arithmetic, clear, growth, and 2x -> 1x -> 2x cases are exercised by the payload regression; values below agree with source and logs. Not a total-memory measurement. |
| A1: individual width/height guards | **PASS** | `Shelf::place` rejects either extent before wrapping/mutating the shelf; the same operation drives fitting and insertion. Real renderer test independently exceeds width (1200) and height (1200) at cap 1024, requires `LimitExceeded`, zero entries, no offscreen target, and no GPU error. Raising the controlled cap to 1500 succeeds with exact fresh pixels, covering the final non-power-of-two candidate. |
| A2: authoritative prepared classification | **PASS** | Shared cell filtering and ordered first-occurrence keys feed `PreparedGlyphs`; each retains dimensions and the actual Alpha/Subpixel/Color/empty result. Both fit and attempt two consume those retained objects. Eight explicitly checked non-whitespace, all-zero non-color rasters mixed with 992 covered keys fit at cap 1024 after historical pressure, with two attempts, zero growth, and exact complete reference pixels. |
| A3: no cacheable partial result | **PASS** | `row_cache.rs` clears all entries on every row-build error, including complete rows cached earlier in that attempt. Therefore even fitting failure via `?` occurs after cache invalidation. Attempt two's `Result` is returned directly. Independent repeated-error regression passed: three cap errors and three injected second-pass errors remain typed failures; smaller valid frames recover exactly. |

### Coherence and termination trace

1. `renderer.rs:573-640` first calls the existing dirty-row path without a
   visible-set preflight. A successful ordinary frame returns immediately.
   Recovery starts only on `LimitExceeded`.
2. `instances.rs` uses the same `visible_glyph` predicate for row generation and
   `prepare_visible_glyphs`: row-major, clipped to snapshot rows/columns, excluding
   spacer heads/tails, invisible cells, and empty text. `PreparedGlyphs::add`
   preserves first occurrence and style-bearing key identity. Whitespace remains
   a retained empty classification; attempt two does not rerasterize empties.
3. `atlas.rs` simulates each candidate using the same `Shelf::place` as upload,
   from the current dimension through geometric growth to the negotiated cap.
   It selects first and replaces textures once, rather than allocating every
   intermediate candidate. The 4000-key fixture exceeds 2048's 3968 wide slots.
4. `reset_for_rebuild` clears entries/shelves and increments generation whether
   retaining the current textures or growing. Row-cache generation comparison
   invalidates old UVs. Growth replaces both texture views and their sampler in
   the atlas bind group before returning prepared instances. Upload routes Alpha
   and Subpixel to the mask texture and Color to the color texture; all-zero
   Color still reserves a slot, preserving the original color rule.
5. `render_snapshot` (`renderer.rs:681`), `render_to_surface_view` (`:746`), and
   `render_to_surface_viewport_internal` (`:822`) all propagate preparation
   errors before creating their draw encoder/submitting a pass. Public viewport
   rendering delegates to the internal gate. The surface API regression actually
   forces growth separately through view and translated viewport paths; its
   attachments are offscreen, not acquired native drawables.
6. Existing host code at `surface_host.rs:1977-1992` propagates the renderer
   result before `frame.present()` and before returning `presented: true`.
   Renderer failure can drop an already acquired texture unpresented, as the
   final plan explicitly permits. No scheduling changes are needed for a
   representable frame completed synchronously.

`Result<Option<AtlasEntry>, NativeTerminalError>` is a valid typed implementation
of Inserted/Empty/Full: `Ok(Some)` / `Ok(None)` / `Err(LimitExceeded)`. No failure
is converted to successful absence. The recovery function has no third call or
retry loop. The fault seam injects Full after 40 successful rebuild insertions,
so one whole row can already be cached; it does not replace the preparation,
packing, cache, or upload mechanism under test. The cap-only path records one
attempt, the injected second-pass path two, on every repeated failing call.

## Actual RED/GREEN and chronology

All three behavioral RED logs were read, not inferred from a build failure:

| Receipt under `E` | Executed result and failed requirement |
| --- | --- |
| `red-all_visible_glyphs_survive_when_dense_2x_working_set_repeats.log` | 1 executed, 0 passed / 1 failed, exit 101. Metal initialization and all 1000 fixture-ink assertions precede the failing expected-origin-vector assertion; both frames contain only origins 0-991. |
| `red-last_frame_renders_new_glyph_when_only_history_fills_atlas.log` | 1 executed, 0 passed / 1 failed, exit 101. Successful 992-key prewarm and positive fresh-reference ink precede whole-frame inequality. |
| `red-q7.log` | 1 executed, 0 passed / 1 failed, exit 101, at unchanged `dirty_update_atlas.rs:29`: `atlas allocated bytes must not exceed max capacity`. |

The saved RED patch contains only the two baseline-compatible real-renderer
tests and their fixture/readback helpers. Their assertions remain in final
source; the D4 insertion additionally unwraps the new outer `Result`. D1's
full-cell oracle is supplied separately by the final integration regression and
artifact audit, not falsely attributed to its original origin-count assertion.

Recorded UTC chronology: base receipt 16:17:12; RED test-only patch mtime
16:18:28; completed behavioral RED logs 16:27:48, 16:28:22, and 16:28:56;
production row-cache file mtime 16:35:29; final exact GREEN logs 16:46-16:47;
repair commit 16:54:43. This is consistent with the author's recorded test-first
sequence, the matching base blob, and the defect-specific old accounting/992-slot
runtime output. **Chronology is an audit of retained receipts and filesystem
timestamps, not an independently witnessed edit timeline or a new baseline
replay.** No incompatible new helper, zero-test match, or initialization failure
is accepted as RED. The three `*-setup-failure.log` files are explicitly excluded.

## Independent execution in this verification

Executed once each, in `W`, with the author's documented shell-only prerequisite:

```sh
export CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target
export CARGO_BUILD_JOBS=8
export TAURI_CONFIG='{"bundle":{"resources":[]}}'
unset ATLAS_EVIDENCE_DIR
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::all_visible_glyphs_survive_when_dense_2x_working_set_repeats -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::last_frame_renders_new_glyph_when_only_history_fills_atlas -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::failed_frame_cannot_be_reused_as_partial_success -- --exact --nocapture
```

**3 executed, 3 passed, 0 failed, 0 ignored; each exit 0 and 607 filtered out.**
Tool output in this verifier session is the primary receipt; no separate log or
readback files were written. Dense runtime output is Apple M4 Max / Metal,
1000/1000 origins, 33,618,432 current bytes / 544,751,616 maximum. Error recovery
reports 1 resident key at cap 1024 and 41 after the injected-fault recovery; the
latter correctly retains 40 harmless resident historical keys rather than
claiming that error handling necessarily frees the atlas.

The initial package-cache lock wait resolved normally. No retry, fixed sleep,
polling delay, or test skip was used. The added atlas tests are synchronous GPU
render/readback assertions, not timing-based convergence checks.

## Full-cell evidence audit

Read both Rust reference-generation implementations and `E/readback-audit.py`.
Also executed a separate, read-only Python comparison in this verifier session
against the raw RGBA files and dimension metadata; it ended
`INDEPENDENT_READ_ONLY_AUDIT_PASS`, exit 0. The existing audit script was not run
directly because it overwrites its JSON output.

- All 1000 integration cells compare **every one of 4096 RGBA bytes** in both
  F0 and F1. References are fresh 400/400/200-key batches, not another pressured
  1000-key render. Every expected tile must have ink.
- All 4000 cells compare full tiles against fresh batches of at most 640 keys.
  The test explicitly checks each reference remains at base dimension 1024;
  F0/F1 whole-frame equality and the independent audit cover both frames.
- Re-audited 14,000 full tile comparisons: 12,000 GREEN tiles all equal and
  2000 RED tiles with exactly indices 992-999 different in each RED frame.
- Re-audited history: 822/4096 bytes differ in RED, zero in GREEN. Empty-pressure
  frame: all 4,096,000 bytes equal. Both smaller error recoveries: all 4096 bytes
  equal their fresh reference.
- Both surface API attachments equal the already independently validated dense
  pixels in all 4,096,000 terminal bytes. The translated viewport additionally
  checks all 223,232 outside pixels are opaque black. Thus the surface test's
  pressured offscreen oracle is cross-validated, not used as sole correctness
  proof.

Verified GREEN SHA-256 values (identical F0/F1): dense 1000
`9ee3000f2570969a9132e82b22ca4be21b77eeb1f4e72093c16fda96bdd5193d`;
dense 4000 `f5b3bc1c56bda685d6c1b0cd6e6c70ae08e530c5ee10078e2fad4cba9e13c97a`.
This is numerical raw-readback verification, not visual inspection of PNGs or
screenshots. The saved PNG audit checks dimensions/signature, not decoded
PNG-to-RGBA equality. The required pixel proof rests on raw GPU readbacks.

The fixture does not pass on a zero-font backend: initialization/ink assertions
are mandatory. On this macOS execution `rasterizer.rs` delegates to
`FontManager::global`, which uses real CoreText and effective terminal
preferences. Exact resolved font-file/version hashes were not captured; these
are same-host reference comparisons, not portable golden-font certification.
Existing color/mask coexistence, emoji, rasterization, and configuration
contracts passed in the reviewed broader logs. A synthetic all-zero Color
recovery fixture is not present; preservation of that special rule is verified
directly in classification/upload source, not claimed as new runtime coverage.

## Broader gates and resource accounting

Read complete `full-contract-final.log`, `full-native-terminal.log`,
`build-example.log`, `cargo-check.log`, and `poc-headless.log`:

| Gate | Actual retained result |
| --- | --- |
| Ten `final-*.log` exact regressions | 10 executions, 10 passed; each exit 0 |
| Full renderer contract | 26 passed, 0 failed/ignored/filtered, exit 0 |
| Native-terminal library filter | 119 passed, 0 failed/ignored, 489 filtered, exit 0 |
| Existing POC example build | exit 0 |
| POC `--headless` execution | exit 0; 50 frames, 800x480, 1 rebuilt / 23 reused rows |
| Cargo check | exit 0 |

That is **155 final author-side successful test executions**, representing
**145 distinct tests** (the ten exact regressions also appear in the broader
runs), plus this verifier's three executions. These are not a repository-wide
test claim. The seven-file diagnostics text is an author receipt pointing to its
tool transcript; it is not an independently reproduced LSP result here. Actual
compiler validation and warnings were inspected. Seven existing warnings outside
the changed files remain visible, including font-manager unused mutability,
notification code, input test code, and terminal/worktree dead code.

Intermediate failures are accounted for: the first full contract had 24 passes
and two failures because the existing POC binary was absent at its fallback
path. The unchanged `standalone_poc.rs` permits the compile-time
`CARGO_BIN_EXE_native_terminal_renderer_poc` path; building the actual example
and supplying it resolves that prerequisite without skipping tests. The earlier
theme call-site compilation failure was corrected by the scoped signature
adaptations, not suppressed. Missing worktree `ui/dist` setup failures are not
renderer RED; `TAURI_CONFIG` disables bundle-resource copying only, so neither
these tests nor the build receipt establish packaged UI resource correctness.

Both actual atlas descriptors are square RGBA8Unorm, one layer, one mip, one
sample, mask plus color: current texture payload is `dimension^2 * 8`. Resident
metadata is `entry_count * size_of::<(GlyphKey, AtlasEntry)>()`, observed 64 bytes
per pair. The maximum derives from the negotiated device extent (8192 here) and
current validated cell dimensions:

```text
max_entries = floor(M / cell_height) * floor((M + 1) / (cell_width + 1))
max_payload = M * M * 8 + max_entries * pair_size
```

The minimum/narrow cell gives an upper bound for mixed narrow/wide allocations;
empty entries consume no metadata slot. Cast-before-multiply and saturating
theoretical-bound arithmetic are explicit. Observed values agree with the model:

| State | Entries | Current bytes | Maximum bytes |
| --- | ---: | ---: | ---: |
| Base 16x32 | 0 | 8,388,608 | 544,751,616 |
| Dense 2048 | 1000 | 33,618,432 | 544,751,616 |
| Dense 4096 | 4000 | 134,473,728 | 544,751,616 |
| Cleared 2048 | 0 | 33,554,432 | 544,751,616 |
| 8x16 configuration, repopulated | 1 | 33,554,496 | 566,689,792 |
| Restored 16x32, repopulated | 1 | 33,554,496 | 544,751,616 |

`types.rs` correctly excludes String heaps, HashMap spare buckets/allocator
overhead, prepared rasters, staging buffers, driver overhead, and replaced
textures still in flight. Clear retains texture allocation; replacement is
logical ownership transfer, not evidence of immediate physical reclamation.
The source requests default device limits and then reads successfully negotiated
limits; no claim is made that all adapters support the request.

## Provenance, cleanup, and remaining native QA

Actual tool versions match the base receipt: Rust/Cargo 1.92.0, Zig 0.16.0,
Darwin arm64, Apple M4 Max / Metal. The existing Ghostty checkout is clean and at
`6a508fd5e34c7e222c052a6d00bb3891ff3feace`; its pinned source-lock/build receipts
agree. The repair manifest uses wgpu 24, with no added local HAL patch. Shared
Cargo target is `/Users/indo/code/project/orca-lite/src-tauri/target`, not an
unidentified installed Ferryx binary. The POC executable path is that target's
`debug/examples/native_terminal_renderer_poc`.

All three independently owned test processes returned and a subsequent process
search found no matching test executable or POC process. Author `cleanup.log`
records no owned runtime matches at 16:55:27 UTC; `journal.txt` records removal
of the temporary staging file and retention of evidence/shared caches. No
desktop/window/server/daemon was launched by this verification. Environment
overrides were shell-local; no cache cleanup or foreign process termination was
performed. Original artifacts were read only.

Report validation: read back the report and ran whitespace/diff validation with
no findings. Markdown LSP diagnostics were unavailable because no `.md` server
is configured; no successful Markdown diagnostics result is claimed.

**Remaining acceptance blocker is missing native evidence, not a demonstrated
renderer failure.** Its source is the final plan's native acceptance boundary
and `atlas-repair.md`'s explicit pending handoff. The lead still needs real
application-window/compositor captures at native 1x and 2x, dense/repeated output
and history-only final output after the producer becomes idle, plus display-scale
transition checks. Record the exact running repaired source/binary, display
scale, fixture, and capture provenance. Confirm complete/correct text and no
stale UVs through the actual presentation surface; API calls on offscreen
attachments and the successful 2x -> 1x -> 2x config unit case do not satisfy
that requirement. No native pass, main merge approval, or unrelated criterion
closure is granted by this report.
