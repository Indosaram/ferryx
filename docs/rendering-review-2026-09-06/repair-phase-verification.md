# Repair phase independent verification - 2026-09-06

## Decision

**FAIL - the phase is not ready to proceed to renderer implementation from the
atlas plan as written.** D2's scoped code and executable IPC/lifecycle evidence
pass. The selected renderer-local growth strategy is sound in outline, but its
specified fit simulation and failed-frame cache handling do not establish its
own completeness invariants. Correct A1-A3 below before authorizing that plan.
This is a request to correct the existing selected strategy, not to select a new
design or broaden production scope.

**Aggregate native rendering acceptance is NOT approved.** Desktop access is
unavailable under the established task boundary. macOS compositor screenshots,
Wayland presentation, and Windows owner-thread destruction proofs remain pending.
Independent inspection of the browser PNG pixels is also blocked: all three
`read` calls returned `Current model does not support images`. File signatures,
dimensions, source, action logs and cleanup receipts are not visual inspection.

Verifier: task `st_01a07743`. Only this report was authored, using `apply_patch`.
No production/test edits, commits, desktop input, browser launches, broad suites,
Rust executions, or rebuilds were performed. The focused D2 regression command
below ran once. Relevant programming, debugging/partial-evidence/cleanup and
visual-QA skills and applicable AGENTS instructions were read. No delegation or
image-capable reviewer tool was exposed to this child.

## Criterion verdicts

| Criterion | Result | Evidence / remaining condition |
| --- | --- | --- |
| D2 changed-file scope and provenance | **PASS** | Actual commit `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145` changes only the pane, its lifecycle test and lifecycle module; recorded and actual patch hashes agree. |
| D2 actual RED before production fix, same named GREEN | **PASS** | Raw failure logs, matching GREEN logs, and producer tool transcript establish the execution order and right-reason assertions; not zero-match or compilation failures. |
| D2 deterministic real-module regressions | **PASS** | Ten new cases use real pane/lifecycle/provider, deferred IPC and React `act`; independent one-shot run: 10 passed, exit 0. |
| D2 departed recovery, queued execution, stale publication | **PASS** | Commit-scoped identity guards before lifecycle mutation, at queued execution, after recovery, and before receipt/error publication; departure and A -> B -> A queue cases pass. |
| D2 legitimate warm reparent/readiness/shared recovery | **PASS** | Cancellation and shared readiness remain intact; returning queued owner supplies its own geometry. Positive concurrent-input case independently passes; original reparent/readiness tests are unchanged and included in the recorded 167-pass run. |
| D2 changed-file diagnostics, related tests, build | **PASS** | Independent LSP on all three changed files returned `No diagnostics found`; raw producer receipts show 167 tests and tsc/Vite build passing. No unchanged gate was rerun. |
| D2 real browser component/IPC entry evidence and cleanup | **PASS** | Three final scenarios, real imports/input path, differing bounds, captured IPC journals and owned-resource cleanup checked. This is controlled IPC, not the complete desktop app. |
| D2 independent browser visual inspection | **FAIL - evidence unavailable** | Three actual PNG read attempts could not expose images to this model. An image-capable reviewer must inspect all three captures; no visual verdict is implied. |
| D1/D4 strategy selection and termination intent | **PASS, design only** | Reactive current-frame rebuild, current-size history eviction, device-bounded geometric growth selected once, at most two preparation attempts, typed capacity failure; no host scheduling redesign. |
| D1 exact device-bounded fitting algorithm | **FAIL** | A1: oversized glyph can incorrectly fit. A2: all-zero non-color rasters are counted in simulation but skipped in insertion. |
| D1/D4 complete-frame/error-path cache invariant | **FAIL** | A3: failed attempt leaves incomplete rows cacheable; second-attempt overflow disposition is unspecified. |
| D1/D4 texture binding and successful-rebuild UV invalidation | **PASS, design only** | Both texture views and sampler rebound before encode after growth; clear/growth invalidates every row and discards first-attempt instances. Does not cure A3's failure branch. |
| D7 accounting strategy | **PASS for explicitly tracked payload** | Two RGBA8 textures at 8 B/px plus resident key/value payload, with a derived device/cell-capacity metadata bound. Not total allocator or native GPU memory; documentation clarification below is required. |
| D1/D4/D7 exact RED/GREEN/readback seams | **FAIL as a complete acceptance packet** | Q3/Q7 are concrete and correct; Q2 needs a baseline-compatible count seam, and bounded/error/empty cases needed to close A1-A3 are absent. Existing named core commands are retained below. |
| Atlas baseline reproduction/isolation | **PASS for scoped baseline identity; status prose stale** | Actual main-tree atlas/row-cache hashes and two-file baseline patch hash match. Repair worktree is no longer clean/untouched; do not reset or import unrelated main-tree changes. |
| Native macOS / D3 Wayland / D6 Windows / aggregate rendering | **FAIL - acceptance pending** | Exact remaining actions below. This phase cannot close native criteria by source review or JS/GPU tests alone. |

## D2 source and evidence audit

Paths in this section are relative to
`/Users/indo/code/project/orca-lite-rendering-20260906` unless stated otherwise.
`D2/` denotes main-repository
`.omo/evidence/ulw/rendering-review-20260906/D2/`.

The actual commit is parented on `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.
Its scope is 3 files, 265 insertions, 9 deletions:

- `ui/src/components/NativeTerminalPane.tsx`
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`
- `ui/src/lib/nativeTerminalLifecycle.ts`

`git diff b8f82d707 HEAD -- <these three paths>` SHA-256 is
`cc043653a09cfb65039b07362583743dc096e822af279414f4b58618751f7bea`, equal to both
`D2/ui-increment.patch` and `D2/staged-ui-increment.patch`. Current three-file
hashes also match `provenance-before-commit.log`; there is no current UI diff.
Foreign Wayland contract changes existed at entry; a foreign `platform/windows.rs`
diff appeared during review. Neither is in the D2 commit or was touched here.

The production path was traced through active-tab-only rendering in
`TerminalSplitView.tsx:716-718,793-817`, `TerminalPane.tsx:133-140`, then the real
pane and lifecycle. The committed layout effect at pane `:537-543` replaces the
owner object on target/visibility transitions and nulls it on unmount. Equality
of session strings alone cannot revive a departed continuation. `performAttach`
at `:687-715` guards before calling forced reattach (which cancels held detach)
and again inside the queued callback. `sendInput` at `:739-818` guards execution,
successful receipt publication, recovery creation, recovery completion/focus,
and stale recovery/retry errors.

Lifecycle `:217-249` replaces only a not-yet-started attachment operation when a
real returning owner reuses readiness. The operation is removed from the queued
map before it starts. Started IPC stays serialized and shared; legitimate pending
and held detachment cancellation remains unchanged. Tests exercise A's initial
attach held open, recovery queued behind it, departure/unmount, and A -> B -> A
return with new bounds. They require zero obsolete input retry, exactly the live
owner's recovery attachment when returning, and no A detach on that return.
No daemon PTY-close policy was changed.

### RED/GREEN provenance, not just producer prose

The producer transcript is:
`.omo/senpi-task/children/st_01a0772e/sessions/st_01a0772e/2026-09-06T14-45-20-491Z_01a0772e-55eb-796b-8dc6-ebc500e78f41.jsonl`
in the main repository. Its tool results at lines 59-61 record the three REDs
at `14:49:59Z`. Additional RED runs at `14:51:17Z`. The pre-production diff-stat
receipt at line 72 (`14:52:14Z`) contains only the lifecycle test file. The first
production patch attempt then failed syntactically; the corrected production
patch ran at `14:52:48Z`, before matching GREEN commands at `14:53:13Z`.
The test helper's initial definite-assignment locals were changed to initialized
locals before the additional RED; the three required scenario bodies/assertions
were not weakened for GREEN.

| Required exact test name | RED output | Matching GREEN |
| --- | --- | --- |
| does not reclaim the outgoing surface when input fails after tab replacement | `red-replacement.log`: expected attach count 1, got 2; 1 failed, exit 1 | `green-replacement.log`: 1 passed, exit 0 |
| does not recover input when its owner becomes hidden | `red-hidden.log`: unwanted A attach after A detach; 1 failed, exit 1 | `green-hidden.log`: 1 passed, exit 0 |
| does not retry input after its owner leaves during recovery | `red-recovery.log`: expected input count 1, got 2; 1 failed, exit 1 | `green-recovery.log`: 1 passed, exit 0 |

`red-additional.log` contains six assertion failures and one positive shared-
recovery pass. The failures distinguish stale input/retry IME position, stale
recovery/retry alerts, and obsolete queued-owner input. Their final test bodies
retain those assertions. No lifecycle mock is installed. New cases contain no
sleep, polling or `waitFor`; subscribers/deferred gates precede actions, and
React `act` settles reactions with Vitest's timeout as a failure bound. Older,
unchanged lifecycle component tests still use `waitFor`; those are not presented
as new deterministic coverage or silently refactored in this report-only task.

### Independent execution receipt

Executed once from the repair worktree, at local `00:10:52` on September 7
(`2026-09-06T15:10:52Z`), with the repository's Vitest script:

```sh
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not reclaim the outgoing surface when input fails after tab replacement|does not recover input when its owner becomes hidden|does not retry input after its owner leaves during recovery|guards queued recovery attachment while preserving a returning owner|does not publish a stale|shares one recovery for concurrent inputs from a live owner'
```

```text
RUN v3.2.7 /Users/indo/code/project/orca-lite-rendering-20260906/ui
NativeTerminalPane.lifecycle.test.tsx (21 tests | 11 skipped) 68ms
Test Files 1 passed (1)
Tests 10 passed | 11 skipped (21)
Duration 978ms
INDEPENDENT_D2_EXIT=0
```

The 11 exclusions are existing tests outside this independent filter, not failed
tests skipped to obtain GREEN. The synchronous tool invocation completed; no
background test worker or server was deliberately started. All three independent
changed-file LSP calls returned `No diagnostics found`.
`D2/focused-ui.log` separately shows the producer's single 167-pass/three-file
run, including unchanged lifecycle readiness/reparent tests. Its expected injected
`mount attach failed` stderr remains visible. `D2/build.log` shows tsc plus Vite,
1863 modules, exit 0. These unchanged broader gates were inspected, not rerun.

### Browser artifacts and cleanup

Read both `browser-entry.jsx` and `browser-qa.mjs`, all three scenario JSON files,
the final run log, all eight failed-attempt logs and paired cleanup JSON receipts.
The bundle imports the actual worktree component/provider (and thereby lifecycle)
and real Tauri JS wrappers; only `__TAURI_INTERNALS__` IPC is controlled. It clicks
the real viewport and presses `x`, then uses harness replacement/visibility
controls and subscribed signals with bounded waits. This is not an App-tab E2E.

The final journals contain A bounds `(25,173,798,278)` and B bounds
`(105,173,498,278)`, scale 1. Replacement: A attach/input/detach counts 1/1/1,
B detach 0. Hidden: A counts 1/1/1 and no reattach. Recovery: A counts 2/1/1,
B detach 0, with recovery started while A still owned the pane. The final script
asserts absence of alerts. Logs and source support these IPC claims, not visible
native content or compositor z-order. Unlike the focused recovery unit test,
the browser recovery script finishes recovery before releasing B presentation;
the remaining native scenario must also cover completion after B presents.

All three PNGs have actual PNG signatures, 1280x900 dimensions and RGBA encoding
(`file` independently checked). Their pixel contents could not be reviewed by
this model despite `read` attempts on each actual file. Do not label them visually
approved, fully composited, or proof of terminal ink. The harness source explicitly
labels its terminal region as not native terminal pixels.

Final `browser-cleanup.json`: server 49872 stopped, three WebViews closed, all
three component-unmount callback counts zero. The script closes views in `finally`,
stops its server and calls `Bun.WebView.closeAll`. Failed-attempt receipts likewise
record stopped servers and closed views; failed scenarios do not all claim zero
callbacks, but their entire views were closed. Independent `lsof` found no listener
on 49872 or any of the eight earlier recorded ports. Failures are retained as
harness setup/action synchronization failures, not concealed product passes.

## Atlas source/plan review and required corrections

This is **QA-by-read of a source-only plan**, not renderer implementation or
runtime proof of a new renderer. Read actual main-tree `atlas.rs`, `row_cache.rs`,
`instances.rs`, `renderer.rs`, `gpu_context.rs`, `types.rs`, `rasterizer.rs`, the
accounting test and the relevant host acquisition/presentation/coordinator path.

Keep the selected reactive strategy: no preflight on ordinary cached frames;
on pressure, choose a fitting current-or-grown dimension, allocate at most once,
invalidate UV-bearing rows, and complete synchronously before encode. All three
real preparation callers (`render_snapshot`, `render_to_surface_view`, viewport
internal) must use the same completion gate. The offscreen-viewport wrapper
already reaches the viewport internal path, so this need not modify host policy.

### A1 - D1: the specified fit predicate accepts an oversized glyph (blocker)

Plan `simulate_shelf_fit` at lines 199-228 checks wrap and vertical extent but
never rejects `width > candidate_dimension`. For a 1024-square candidate and one
wide glyph with `cell_width_px=600`, `cell_height_px=32`, width is 1200: wrapping
resets x to 0, y stays 0, the height test passes, x becomes 1201, and it returns
true. Those cell dimensions pass `RendererConfig::validate`; a two-column
snapshot is also accepted. The selected dimension cannot hold that upload.
The current inserter has the same missing horizontal-size rejection.

**Required correction:** state and implement rejection of individual glyph
extents larger than a candidate before shelf mutation, in both fit/insertion
semantics. Select a larger fitting candidate or return `LimitExceeded` at the
effective device cap, never perform an out-of-bounds write. Specify finite
candidate enumeration including the actual final cap (without overshooting it).
Include an executable oversized-entry boundary regression in the implementation
packet. This checks the algorithm's claimed domain, not a newly asserted incident.

### A2 - D1/D4: simulation and insertion disagree about empty rasters (blocker)

The plan promises all-zero non-color rasters are discarded before reservation,
but `collect_unique_visible_glyphs` only rejects whitespace and cell flags, and
`simulate_shelf_fit` reserves a slot for every remaining key. No raster/ink
classification is supplied. The real rasterizer distinguishes alpha/subpixel
from color buffers; current atlas rejection at `atlas.rs:170-175` can discard
non-whitespace uncovered glyphs. Counting those as packed entries can turn
history-only pressure into unnecessary growth, or return a hardware-cap error
even though all actually drawable visible glyphs fit. Thus "exact simulation"
and "smallest fitting dimension" are not established by the supplied algorithm.

**Required correction:** define one consistent ordered reservation set shared
by fit and rebuild, including the same raster-empty rule and dimensions. Specify
how that classification remains consistent during the recovery attempt; do not
just remove the empty test. Preserve the existing rule that color entries are
not rejected solely for zero bytes. Add a pressure case mixing covered ink and
uncovered non-whitespace empties, asserting no false growth/cap failure and
complete covered-glyph pixels. This remains within atlas ownership.

### A3 - D1/D4: errors can leave a successfully reusable partial frame (blocker)

Actual `row_cache.rs:48-60` caches a dirty row after insertion, including a row
whose glyph was omitted. The proposed change returns a transient overflow bool
but retains that caching and removes next-frame `take_overflow_pending` handling.
In the plan's no-fitting-candidate branch, the renderer returns `LimitExceeded`
without clearing/invalidation of those first-attempt cached rows. A subsequent
identical snapshot can then reuse them all, see no new `Full`, and return success
with missing glyphs. The one-call error invariant is insufficient across calls.
The plan also labels attempt 2 "proven to fit" without specifying the disposition
of a second `Full` or preventing publication of its incomplete cached output.

**Required correction:** explicitly make incomplete rows non-reusable on every
failure exit (invalidate/discard them or retain completeness that forces recovery).
Check the second attempt's completeness and return a structured error without
encode/submit if it still fails; never add an unbounded retry. Require repeated
identical over-cap calls to remain errors, then a smaller fitting snapshot to
recover correctly, as well as a bounded unexpected-second-overflow case. Verify
these through the real preparation/cache integration, not a standalone policy
mock. No host scheduling extension is needed.

### Accounting, GPU semantics and bounded scope clarifications

- D7's texture arithmetic is correct for the actual descriptors: 1024 squared
  times 8 = 8,388,608 texture-payload bytes. `entries.len() * sizeof(pair)` is
  **resident entry payload**, not actual HashMap bucket allocation, retained
  capacity, String heap allocation, driver overhead or transient old textures.
  The plan deliberately defines tracked payload, which is acceptable, but carry
  those exclusions into the `GlyphAtlasStats` field documentation in `types.rs`
  and approve that narrow documentation scope. Do not call this total native
  memory or exact allocator accounting. The configured-capacity description
  must reflect growth to the effective device limit rather than a fixed budget.
- With uniform positive cell dimensions and width at least `cell_width_px`, the
  proposed maximum entry formula is a conservative packing bound. Derive it
  from the current configuration (recompute after config changes) and perform
  byte arithmetic without intermediate overflow. Do not merely inflate the
  former fixed maximum or weaken `dirty_update_atlas` assertions.
- Bind group replacement before encode and row invalidation on clear/growth are
  coherent with the existing texture-view bindings and UV layout. WGPU may
  retain old resources while queued/in-flight work references them; logical
  replacement is not proof of immediate physical deallocation.
- Correct the claim "no unpresented drops" at plan line 268: the real host
  acquires a swapchain texture before calling the renderer (`surface_host.rs:
  1942-1988`). A typed renderer error prevents `frame.present()` but the acquired
  frame is then dropped unpresented. That is not a successfully presented partial
  frame and does not require changing host ownership.
- Device limits are the effective requested device limits, not necessarily the
  adapter's maximum hardware capability. The current default-limit request can
  fail on an adapter that cannot meet it; this plan must not claim automatic
  downlevel negotiation it does not implement.

### Exact evidence seams for implementation

Retain these literal commands from the corrected plan. Q2/Q3/readback tests do
not exist as new repaired tests yet; no execution or GREEN is claimed here.

```sh
# D1/Q2: repeated dense working set, actual frame preparation and glyph origins
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::all_visible_glyphs_survive_when_dense_2x_working_set_repeats -- --exact --nocapture
# D4/Q3: exactly one final render after history-only prewarm, against fresh ink
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::last_frame_renders_new_glyph_when_only_history_fills_atlas -- --exact --nocapture
# D1: every dense cell's actual readback versus independently rendered small tile
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract offscreen_render::test_dense_scale_2_working_set_pixel_readback_integrity -- --exact --nocapture
# D7/Q7: existing unchanged initial and 50-update accounting contract
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
```

- **Q2 correction:** `OffscreenFrame` exposes pixels and rebuilt/reused row counts,
  not a glyph-instance count. Specify the actual baseline-compatible internal
  count/origin observation in the renderer test, and couple it to the real render
  entry. A call to a not-yet-created helper must not be the RED compilation error.
  Prove nonzero fixture ink, use the 1000 distinct wide-glyph 80x25 snapshot at
  16x32 physical cells and scale 2, require all origins in F0 and unchanged F1,
  and require F1 row reuse. Keep assertions the same for RED/GREEN.
- **Q3:** prewarm exactly 992 covered wide entries without requesting overflow,
  then perform one two-column final render. Hide the cursor and explicitly prove
  the fresh reference has ink before whole-frame equality. Never supply the
  otherwise-missing second repaint as the GREEN action.
- **Readback:** compare every wide origin, not merely nonblank counts. Use
  matching cursor/style/scale state and unambiguous coordinates: terminal column
  times 16, or wide-origin ordinal times 32. Avoid creating 1000 simultaneous
  GPU devices/targets merely to obtain independent small-tile references.
- **D1 boundary packet missing:** give exact test names/commands and assertions
  for the specified 4000-key case requiring a 1024 -> 4096 jump (2048 holds only
  3968), single growth allocation, repeated hard-cap failure/recovery, oversized
  glyph and empty-raster pressure cases A1-A3. They must fail for the named
  behavior, not no-device/no-font/compile/zero-match conditions. Verify the shared
  viewport preparation route too; offscreen viewport readback reaches it without
  a desktop. These are behavior tests for implementation, not prose tests now.
- **Q7 already RED:** independently read
  `.omo/ulw-loop/rendering-review-20260906/baseline/atlas-contract.log:739-756`
  and `atlas-contract-baseline.md`: one test executed, initial bound failed at
  `dirty_update_atlas.rs:29`, exit 101, before the 50-update loop. This is not a
  pixel/OOM failure. Keep the same contract for GREEN and separately assert the
  corrected payload arithmetic through growth, clear and config changes.

### Baseline and integration provenance

Independently computed main-tree SHA-256 values match the proposed foreign
baseline `4cab8ab8ac671499b7a20c8f66baac338eea9c15`:

```text
atlas.rs     4b3e5b75c3f9d516aa37738e390d113015cd3da590bd86432fe2e9b53b083082
row_cache.rs a3476565925cea82f6fb87d38699af29e02e3c97f8f8f5063fd6ae3a755273a9
two-file b8f82d707..4cab8ab8 patch:
67e0ab444b147b9d865b533c08fb74b6f5f559aba87191cb1945f68231b0603f
```

Only these renderer baseline paths may be arranged by the lead in the isolated
tree before atlas RED. No main manifest, vendor HAL, App/onboarding or unrelated
platform work may be copied. The plan's "untouched, clean git tree" statement is
stale: D2 is now committed and independent platform owners have uncommitted work.
Preserve it; refresh the actual source/diff identity, explicit baseline approval
and owner/index coordination before implementation. Main HEAD observed during
verification is `e2a67859400e80c480de42a6ec687cda6235c275`, not the historical
baseline; scoped file hashes, not a stale whole-tree-clean assertion, are the
relevant comparison. No imported failing baseline commit is authorized here.

Repair-worktree diff checking passed. Main-tree `git diff --check` reported the
foreign `docs/FERRYX_METAL_OWNERSHIP_INVESTIGATION_2026-09-05.md:203: new blank line
at EOF`; it was not changed or treated as a D2/atlas blocker.

## Exact remaining native and visual QA

These are pending acceptance actions, not executed commands. An authorized native
QA owner must identify the exact debug build/diff, OS/backend, frontend/backend
pane IDs, PTY identities, measured cell metrics/DPR/bounds, event-gated fault
consumption and completed presentation; retain screenshots and owned cleanup.
No existing user app or daemon is authorized for worker restart or incidental input.

1. **D2 macOS replacement:** present distinct A/B text at unequal rectangles.
   Hold A input response and B's first presentation, switch through actual tabs,
   reject A input, release B. Capture A host detach and B presentation; screenshot
   must show only B with no old surface over pane/chrome. Resize B and emit
   identifiable background A output to expose any retained visible host.
2. **D2 masking and recovery:** repeat with real Settings/search/provider masking,
   await native detach before rejection, and prove no A recovery/retry while
   covered. Start live A recovery, leave while attach is pending, finish after B
   presents; require no retry, focus theft, stale IME position or banner. Include
   queued departure and A -> B -> A returning-owner order with changed bounds.
   Positive controls: shared same-owner recovery retries each input once; warm
   same-session reparent preserves surface/content and does not close the PTY.
3. **D1 macOS dense ink:** after renderer repair, display at least 1000 verified
   covered distinct Hangul glyphs in a sufficiently large 2x pane, at fixed
   row/column positions with cursor hidden and no bottom-row scroll. Compute
   capacity from measured metrics. Capture allocation-boundary/last rows and
   unchanged repaints against the fixture; repeat at 1x and 1x -> 2x -> 1x.
   Every glyph must be the correct glyph, not merely nonblank. Record NSView,
   backing-layer/Metal drawable dimensions alongside actual pixels.
4. **D4 macOS idle final frame:** fill historical keys with deterministic pages
   while each current page fits; finish with a new tiny labeled line and stop
   producer output. Capture final presentation before interaction and after one
   explicit repaint. Acceptance is already-correct first final frame, without
   unrelated input or retry spin. D7 additionally needs its arithmetic/contract
   GREEN; screenshots cannot prove byte accounting.
5. **D3 native Wayland:** separately owned repair/verification remains outside
   this report. Record actual buffer scale/extent and compositor diagnostics
   through odd/even physical widths at 2x in two numbered neighboring panes;
   exercise actual DPR 1.5 if supplied. Require divisibility, correct full content
   and shared edges, no gaps/overlaps or `invalid_size`. macOS geometry tests or
   X11 presentation cannot close this criterion.
6. **D6 Windows:** run the real child owner-thread destruction contract and real
   detach/close/overlay/tab-switch flow on Windows. Subscribe to `WM_NCDESTROY`
   before worker teardown, correlate each HWND with its creating thread, require
   owner-thread destruction and no accumulated hidden children or event-loop
   stall. Keep WGPU surface-before-target drop order. No Windows pass is inferred
   from source on this Darwin host.
7. **D5 direct final frame:** on the actual host surface, consume one injected
   acquisition Timeout during a final native scroll with stopped output; require
   automatic subsequent successful presentation without another action. Include
   Lost -> Timeout -> success and retained ownership, with dispatch after host
   locks release. This independent criterion is not solved by atlas recovery.
8. **Shared regression / browser image review:** actual split, pane-to-tab move,
   resize, tab switch, search and overlay close must preserve content, bounds and
   PTYs. An image-capable independent reviewer must open all three existing D2
   PNGs (or recapture if UI inputs change) and record visual findings. Even a
   browser visual PASS will not substitute for items 1-7.

## Handoff boundary

D2 requires no production correction identified by this audit. Its code/IPC
acceptance is separate from unfinished visual/native acceptance. The atlas owner
must correct A1-A3 and complete the corresponding executable acceptance packet;
the lead must refresh provenance/approved baseline integration and serialize
shared platform/index work. Until then, the decision-complete renderer-plan gate
is **FAIL**, with the exact blockers above rather than a blanket rejection of
the selected growth strategy. This report is uncommitted in the shared main tree.

Report validation: the Markdown LSP request returned no configured `.md` server,
not a clean diagnostic result. `git diff --no-index --check /dev/null <report>`
emitted no whitespace diagnostics (exit 1 denotes the added-file difference).
Final status also showed concurrent Wayland production and main-tree permissions
work beyond the earlier snapshot. Those foreign edits remain untouched; they do
not change the independently tested repair-worktree UI inputs or authorize any
claim that either whole working tree is clean.

## Delta verification - corrected atlas design readiness

Verifier: task `st_01a07774`, 2026-09-06. **READY for renderer implementation
against A1-A3.** This delta supersedes only the earlier atlas design-readiness
FAIL and its request for missing decisions. It does not certify implemented
behavior, executed atlas tests, renderer acceptance, or native/visual acceptance.
The prior report and its pending QA requirements remain intact.

### Evidence and blocker disposition

Reviewed the complete upstream report and the final authoritative sections of
`atlas-repair-plan.md`: `Lead adoption and current execution contract`
(lines 509-551) and `Verification corrections A1-A3` (lines 553-597).
Their concrete decisions close the identified design gaps:

- **A1 CLOSED, design only:** correction 1 rejects either individual allocation
  extent exceeding a candidate before shelf placement. This rules out the
  reviewed 1200-wide entry falsely fitting a 1024-square candidate by wrapping
  x and checking only y. The guard is an insertion/preparation requirement, not
  permission to leave the actual upload path unsafe. Retain bounded candidate
  selection through the effective final device cap from the selected strategy.
- **A2 CLOSED, design only:** correction 2 chooses one ordered unique prepared
  sequence with actual raster classifications and allocation dimensions, shared
  by fit and rebuild. Whitespace and all-zero non-color entries reserve nothing;
  color rules are preserved. Retaining the classifications through rebuild
  removes the independent-rasterization disagreement. This specific decision
  resolves the earlier lead-section allowance for conservative counting: it is
  not an alternative implementation that may falsely reject at the cap.
- **A3 CLOSED, design only:** correction 3 applies incomplete-row invalidation
  to every failed preparation, not only capacity selection failure. Generation
  invalidation is acceptable only when the next cache call necessarily observes
  it. Attempt 2 has an explicit completeness check and typed non-success before
  encode/submission; no third attempt is allowed. Repeated identical over-cap
  calls must remain errors, followed by a smaller valid frame matching an
  independent reference. This closes both cross-call partial-cache reuse and
  the formerly unspecified second-attempt failure disposition.

The three added exact renderer-internal commands name
`oversized_glyph_is_rejected_before_shelf_placement`,
`empty_rasters_do_not_force_atlas_growth_under_pressure`, and
`failed_frame_cannot_be_reused_as_partial_success`. Their required assertions
exercise real preparation/cache behavior, including covered/empty pressure and
error recovery; a controlled limit/fault seam may not bypass that integration.
The last case must exercise the explicit unexpected-second-attempt failure as
well as repeated cap errors. These are implementation acceptance obligations,
not remaining design choices and not tests this verifier claims have run.

### Execution boundary and retained acceptance gates

The final contract explicitly requires actual behavioral RED before production
edits, the existing D7 RED on the new base, and unchanged meaningful assertions
for GREEN. Missing helpers/fields, GPU/font setup failure and zero matches are
not accepted REDs. Full glyph-cell reference pixels, fresh bounded reference
batches, repeated dense frames and history-only final output remain required;
the existing report's shared preparation-route and bounded-growth obligations
are not waived by this delta.

The approved narrow `types.rs` documentation scope now covers payload exclusions.
The contract correctly uses successfully negotiated device limits and permits
dropping an acquired texture unpresented on renderer error, without presenting
a partial frame or claiming immediate physical reclamation of replaced textures.
No new source contradiction arose from these corrections, so no renderer source
re-review was needed or performed.

The adopted implementation worktree is
`/Users/indo/code/project/orca-lite-atlas-20260906`; independently executing
`git rev-parse HEAD` there returned
`e2a67859400e80c480de42a6ec687cda6235c275`, matching the final contract.
The locked-worktree, committed-baseline and no-foreign-import decisions are
accepted as the supplied execution boundary, not independently re-audited here.
This verdict does not authorize a commit or main integration.

D2 code/tests/build are outside this delta and were not rerun. Main's reported
three exact D2 cases passing (exit 0) are upstream context, not this verifier's
execution receipt. **Visual/native acceptance remains PENDING**: main's image
reads were unsupported and its vision endpoint connection failed, as supplied
in the handoff. Neither those failures nor atlas design readiness changes the
prior report's exact native/browser QA requirements.

Only this report was appended, using `apply_patch`; no production/test changes,
commits, test/build executions, browser launches or image work were performed.
The report remains uncommitted in the shared tree; foreign changes are untouched.
