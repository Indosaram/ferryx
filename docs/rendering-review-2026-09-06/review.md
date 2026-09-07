# Verified rendering source-review synthesis - 2026-09-06

## Decision and evidence boundary

Accept **five source-proven rendering defects, one Windows teardown API
violation, and one runtime-reproduced atlas accounting contract defect**, with
the qualifications below. Only accounting has a captured RED, supplied by the
baseline executor and checked here against its report/raw log. No native pixel
failure has been reproduced in this synthesis. These are not seven established
causes of the reported macOS incident. Font-only invalidation and native raster
allocation failure remain explicitly unproven conditional investigations.

This report independently checks `atlas.md`, `surface.md`, `frontend.md`, and
`PLAN.md` against current source, nearest guards/callers, existing test bodies,
working-tree diff, and touched-path history. Only this report was authored. No
production/test edits, tests, builds, app launches, desktop input, fault hooks,
commits, or persistent processes were made by this synthesis child.

Baseline inspected: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`, plus foreign
uncommitted work. In particular:

- `renderer/atlas.rs`: 110 additions / 9 deletions; `renderer/row_cache.rs`: 4
  additions. The deferred clear, 1024-square atlas, and empty-mask rejection are
  **present in the reviewed tree, absent from HEAD**. Do not reproduce HEAD's old
  mid-frame UV overwrite and label it a remaining deferred-clear defect.
- The reviewed surface/platform/IPC and pane/lifecycle production paths have no
  working diff. `App.tsx` has foreign onboarding changes, not a replacement of
  the checked tab-selection/terminal ownership path.
- History includes `38f887e` (row cache), `44f1578` (RGBA mask), `3fa25a1` (native
  Windows/Linux children), `e978064` (snapping/Metal layer configuration),
  `2446f6d` (invalid initial bounds), `faad966` (host ownership and scheduled
  completion guards), `8ed71b3` (shared attach readiness), `076e429` (density
  updates), and `2aef606` (masking). Existing fixes are not new findings.
- The lead's repair worktree `/Users/indo/code/project/orca-lite-rendering-20260906`
  was independently observed clean at the same HEAD. **It is not the reviewed
  atlas baseline.** Before atlas RED/repair, the lead must explicitly arrange
  the approved foreign baseline in an isolated repair tree and record its diff
  identity. Do not silently copy, revert, or claim ownership of foreign work.

`baseline.md` records 111 Rust `--lib native_terminal` tests and 157 focused UI
tests passing, plus Cargo check and UI build passing, with retained warnings.
Those are another executor's receipts, not reruns here. `--lib` does **not** run
`--test native_terminal_renderer_contract`. The subsequently delivered
`atlas-contract-baseline.md` and raw `baseline/atlas-contract.log:739-756` record
the exact accounting integration test executing **one test, one failure**, exit
**101**, at `dirty_update_atlas.rs:29`. That is D7 below, not contradicted by the
111-test lib pass. None of these baseline commands observes native pixels.

Reference shorthand below: `N/` = `src-tauri/src/native_terminal/`, `R/` =
`N/renderer/`, `I/` = `src-tauri/src/ipc/`, `U/` = `ui/src/`, and `T/` =
`src-tauri/tests/`. Line references are working-tree, one-based. AST searches
confirmed all three production row-cache calls in `R/renderer.rs` and both
`performAttach` calls in the pane. LSP document symbols worked; reference queries
returned no Rust usages / only the TS declaration, so those incomplete results
were not treated as absence of callers. Direct reads and AST searches supplied
the caller evidence. Applicable AGENTS files and programming, debugging, and
ast-grep skills were read. No delegation tool was exposed to this child.

## Confirmed defects, severity ordered (source versus runtime marked per item)

### D1 / HIGH - Dense visible glyph sets cannot converge (atlas A1)

**Confirmed at source; raster coverage and desktop reproduction not measured.**
For a visible working set larger than the atlas, every repaint omits the same
glyphs. This is missing text, not overwritten UVs or a font sharpness complaint.

`R/atlas.rs:126-158` checks existing keys first, wraps shelves, then sets
`overflow_pending` and returns `None` on exhaustion. `R/instances.rs:138-141,180-202`
has already emitted the cell background and emits no glyph for `None`.
`R/row_cache.rs:30-60` clears the atlas/cache on the next call, traverses rows in
the same order, and caches incomplete rows without a completeness field.
`R/pass.rs:104-128` clears the attachment and draws only the supplied instances.
There is no later fallback draw. The production host reaches this seam through
`N/surface_host.rs:1977-1988` -> `R/renderer.rs:258-322,401-403` and presents the
incomplete result successfully.

**Concrete accepted input:** physical cells 16x32, scale 2, 80x25 snapshot,
cursor hidden; 1,000 distinct, nonempty, covered wide Hangul cells, each followed
by a spacer tail. All styles can be identical. Each allocation is 32x32, with
horizontal advance 33. Capacity is `floor(1025/33) * floor(1024/32) = 992`.
The 1280x800 terminal is within `R/renderer.rs:493-508` validation. F0 omits eight
origins; unchanged F1 clears and repeats the same allocation order, again
omitting eight. This argument is conditional on verified ink for the fixture,
not on a particular font being installed. Repeated use of a few CJK keys does
not cause this; distinct `(text,bold,italic)` keys matter.

**Guard/test falsification:** generation invalidation is correct and does not
solve capacity. The foreign atlas unit test (`R/atlas.rs:353-401`) checks stable
earlier entries, manually clears, and reinserts one key. It does not test a
complete over-capacity frame. `T/native_terminal_renderer_contract/dirty_update_atlas.rs:8-55`
uses a small repeating set; `offscreen_render.rs:214-264` changes config before
its first render and checks dimensions/opaque gutters, not glyph completeness.

**Repair contract:** support a complete bounded visible working set, including
this input, without reusing UV storage still referenced by the frame. Prefer a
renderer-owned complete-frame capacity/rebuild solution; if growth is selected,
bound it by device limits and update texture bindings coherently. A fixed larger
texture merely moves the threshold; an unconditional next-frame retry spins
forever on this input. Keep D1 and D4 as separate criteria under one atlas owner.

### D2 / HIGH - Obsolete input recovery cancels legitimate surface teardown (frontend F1)

**Confirmed asynchronous ownership violation; native z-order manifestation not
reproduced.** A first input rejection after a tab replacement can retain a native
host whose React owner has departed.

`U/components/NativeTerminalPane.tsx:728-796` checks visibility/target only before
the initial invoke. Its rejection continuation force-attaches captured A at
`:766`, then retries after `await recovery`, without a live-owner check.
`performAttach` at `:680-708` measures the current viewport ref but uses the old
target ID. `U/lib/nativeTerminalLifecycle.ts:254-259` deletes the attached marker;
`:196-214` cancels pending/held A detachment and `:227-233` establishes a new
generation. These cancellation guards correctly protect a genuinely returning
owner; the obsolete recovery caller violates their precondition.

**Concrete ordering:** A presented -> send A input with response pending ->
select B -> B attaches, first bounds receipt pending -> reject old A input ->
A's forced recovery succeeds -> B reports `presented:true`. A's outgoing detach
was parked under B, but recovery removed it. B's presentation has no A waiter to
release. A remains attached without a current pane owner.

The real caller is `U/App.tsx:1242-1263` ->
`U/state/workspaceStore.ts:1008` -> `U/state/layout.ts:194-210` -> active-tab-only
`U/components/TerminalSplitView.tsx:716-718,793-817` ->
`TerminalPane.tsx:133-140`. Old input can genuinely reject asynchronously:
`I/native_terminal.rs:791-830` awaits daemon write and main-thread receipt.
`N/surface_host.rs:1011-1109` warm reattach accepts live stream/pump, restores
attached state and layout. Scheduled output uses an existing retained host at
`:337-362`. The native detached-host guard at `:826-845` cannot reject this:
the legitimate detach never ran. Ordinary effect cleanup/completion checks at
pane `:1704-1742,1782-1824` do not protect `sendInput`'s separate continuation.

**Hidden-owner variant:** hide A with Settings/search/provider, await actual
detach, then reject pending input. Unauthorized attach/retry is also reachable.
But attach alone does not reconstruct a removed GPU host; the scheduled path
uses `hosts.get_mut`, not host insertion. Therefore reject the stronger claim
that this ordering alone proves an overlay pixel leak.

**Tests checked:** immediate self-heal/retry-limit tests at pane test
`:1388-1469`, lifecycle test `:130-183` for dropped bounds, `:244-423` for masking,
and lifecycle unit test `:109-128` for a genuinely returning owner. None settles
old input after ownership changes. Keep these positive contracts.

**Repair contract:** invalidate a pane-lifetime token on target, visibility, and
unmount transitions; check it before recovery ownership mutation, before any
queued attachment operation actually executes, after recovery before retry,
and before publishing stale results. A check only at the initial send is the
current bug; a check only before awaiting recovery leaves a second race.
Do not remove legitimate lifecycle cancellation or close the daemon PTY.

### D3 / HIGH - Wayland child scale and rendered extent disagree (surface 1)

**Confirmed geometry/protocol contract mismatch; Linux GUI untested.**
`N/child_surface.rs:69-87` rounds DPR to an integer buffer scale, but rounds the
product of fractional logical size and that scale. `N/platform/linux.rs:378-387`
forwards only position/scale to `wayland_child.rs:372-393`; the helper's physical
extent is unused. Actual swapchain dimensions come independently from original
DPR in `N/composition.rs:151-155` -> `N/surface_host.rs:1924-1932` ->
`R/gpu_context.rs:128-139`. The real-child descriptor guard does not reconcile
those dimensions. Native Wayland creation is selected at Linux `:300-307` when
Wayland handles are present and `FERRYX_DISABLE_WAYLAND_SUBSURFACE` is unset.

**Concrete triggers:** (a) DPR 2, pane width 400.5: helper and host both produce
801 pixels, scale 2, not divisible by scale. (b) If the WebView reports DPR 1.5,
400x300 logical produces 600x450 host extent versus helper 800x600 and scale 2;
the supplied buffer describes 300x225 logical. Frontend edge snapping at
pane `:407-419` permits half-logical-pixel sizes at DPR 2; its measurement and
bounds IPC (`:654-678,1589-1593`, `I/native_terminal.rs:505-534`) forward raw DPR.
The first trigger does not require fractional DPR.

Local Wayland protocol source (`wayland-client-0.31.11/wayland.xml:1464-1470`)
explicitly requires buffer dimensions divisible by buffer scale at commit and
specifies `invalid_size` otherwise. No viewport destination is established by
the inspected child implementation. Do not claim a compositor disconnect was
observed, or that X11 exercises this branch.

**Tests checked:** all cases in `T/native_terminal_wayland_subsurface_contract.rs`.
Its scale-1.5 case only checks helper output with integral logical dimensions;
no test reconciles the host extent or uses odd physical width at 2x.

**Repair contract:** one Wayland-specific presentation geometry must feed both
child placement and buffer/grid configuration. Quantize shared pane edges and
derive buffer/cell scale consistently, not just the helper that currently has
unused extents. Preserve macOS/Windows/X11 geometry. Also retain neighboring-pane
edge agreement; independently rounding sizes can introduce overlaps or gaps.

### D4 / MEDIUM - History-only atlas exhaustion strands the final frame (atlas A2)

**Confirmed missing recovery trigger; no idle desktop frame captured.** This is
distinct from D1: the current visible set fits easily after a clear.

Warm 992 covered 32x32-wide keys without requesting a 993rd, then render a final
two-column, one-row snapshot containing only new covered key 993 and spacer.
The atlas retains historical keys; no row departure evicts them. That one new
glyph is omitted and the pending flag is set. `R/row_cache.rs:30-31` only repairs
on a later call. Renderer completion exposes row counts, not completeness
(`R/renderer.rs:401-403`); host reports `presented:true` at
`N/surface_host.rs:1988-1992`. Scheduled completion requeues only `!presented`
at `:369-373`; absent another request, coordinator `:245-251` returns to idle.
The pump's trailing-idle detection at `:1271-1297` is not a periodic render.
A later explicit repaint restores the cell, but is not guaranteed to occur.

**Guard/test falsification:** the coordinator preserves requests arriving during
a render; there is simply no atlas-originated request here. Tests at host
`:2376-2432` exercise that existing guard. Dirty-row reuse test
`T/native_terminal_renderer_contract/dirty_row_reuse.rs:6-41` is low-pressure;
the atlas unit test manually supplies the missing recovery. Baseline passes do
not close this gap. Severity is lower than D1 because an external repaint heals
it; a stopped producer can nevertheless leave text absent indefinitely.

**Repair contract:** complete the current frame by rebuilding from its visible
set when history is the obstruction. This keeps the repair renderer-local and
provides a simple offscreen contract. If deferred recovery is chosen instead,
explicitly propagate and bound that reason through real production completion
policy, with a D1 termination test. Do not fabricate a successful test by having
the test itself request the missing repaint.

### D5 / MEDIUM - Direct one-shot presentation drops have no retry (surface 2)

**Confirmed missing retry conditional on acquisition Timeout; no injected or
natural timeout observed.** `N/surface_host.rs:1942-1973` retries Lost/Outdated
once, then classifies Timeout as a dropped/non-presented frame. `render` returns
that receipt directly at `:1751-1758`; direct `set_focus` does likewise at
`:1783-1794`. They do not reach scheduled completion's retry logic.

The strongest concrete caller is macOS scroll: `src-tauri/src/lib.rs:541-577`
changes VT scroll offset, calls direct render, ignores receipt, and consumes the
native event. `I/native_terminal.rs:686-693` only updates VT; it does not schedule.
Portable scroll IPC `:835-877`, UI scroll `:1027-1035,1915-1923`, focus `:629-636`,
and overlay/attention IPC `:921-931,971-981` also do not rearm a dropped frame.
Given an idle attached host and stopped PTY output, a last scroll followed by
Timeout leaves the old image while VT offset has changed. Overlay timers or
later output may heal it but are not required in the native-scroll ordering.

An additional backend check did not refute this conditional branch: vendored
Metal `surface.rs:187-198` returns `Ok(None)` for absent next drawable;
`wgpu-core-24.0.5/src/present.rs:217` maps that to Timeout and
`wgpu-24.0.5/src/api/surface.rs:104` exposes `SurfaceError::Timeout`. Metal
configuration disables ordinary drawable timeout when supported, so this is
**not evidence that routine macOS scrolling frequently times out**.

**Tests/guards checked:** `N/surface_error.rs:26-30` only checks classification.
Coordinator tests only check scheduled work. Pane bounds retry at
`:1596-1602` and lifecycle tests `:130-183` protect initial/bounds presentation;
reject the report-wide generalization that all dropped initial frames stick.

**Repair contract:** centralize dropped-frame rearming for actual direct and
scheduled host renders; dispatch only after releasing host locks, preserve
ownership checks, and avoid Wry inline recursion. Preserve fatal error handling;
do not retry OOM or a permanently failed host indefinitely.

### D6 / MEDIUM - Windows child teardown violates owner-thread API (surface 3)

**Confirmed API/lifetime defect, not confirmed stale visible pixels or deadlock.**
Creation is main-thread set-bounds dispatch (`I/native_terminal.rs:528-534`) ->
host creation -> `N/platform/windows.rs:233-258` `CreateWindowExW`. Async detach
and close IPC at `:484-501` directly remove/drop hosts at
`N/surface_host.rs:1498-1531`. Windows target `Drop` at `:328-336` calls
`ShowWindow(SW_HIDE)` and `DestroyWindow` inline, ignoring the latter result.

Locked dependency source independently confirms async command dispatch:
`tauri-macros-2.6.3/src/command/wrapper.rs:378-392` uses
`respond_async_serialized`; `tauri-2.11.5/src/ipc/mod.rs:371-388` spawns it on the
async runtime. Microsoft's [DestroyWindow contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-destroywindow),
retrieved in this session, says a thread cannot destroy another thread's window.
Attach then unmount/overlay detach therefore reaches an invalid cross-thread
destruction attempt. The preceding hide defeats an inference that the child
necessarily stays visible. No synchronous-cross-thread deadlock is established.

**Tests checked:** pure child visibility contract `:93-113`, mirror-struct drop
order test at host `:2180-2203`, and host-isolation contract `:717-778`. None
creates a real HWND. Host fields at `:1853-1855` correctly drop surface before
target; that invariant does not supply the missing thread dispatch.

**Repair contract:** transfer teardown to the owning UI thread while preserving
surface-before-target order, without a synchronous wait holding `hosts`. Cover
detach and close at one boundary; account for teardown/drain callers as well.
Windows-native evidence is required before declaring this fixed. Keep this
platform-specific contract task separate from the macOS symptom claim.

### D7 / MEDIUM - Atlas accounting violates its existing bound (atlas accounting)

**Runtime-reproduced contract defect; not a native pixel or GPU-OOM reproduction.**
The exact existing integration test rendered its initial snapshot, passed width
and positive-entry checks, then failed at
`T/native_terminal_renderer_contract/dirty_update_atlas.rs:29:5`:

```text
running 1 test
atlas allocated bytes must not exceed max capacity
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 24 filtered out
```

Executor receipt: `atlas-contract-baseline.md`, Cargo exit 101, run once between
14:28:19Z and 14:29:43Z. Raw output independently checked here:
`.omo/ulw-loop/rendering-review-20260906/baseline/atlas-contract.log:739-756`.
The 50-update loop was not reached. Actual byte values were not printed.

The source explains the asserted mismatch: `R/atlas.rs:293-300` adds positive
entry overhead to the same 5-byte-per-pixel base used as `MAX_CAPACITY_BYTES`
at `:13`. Any populated atlas exceeds that reported maximum. Separately,
the two RGBA8 textures at `:47-75` have 8 MiB of texel payload, not the reported
5 MiB basis; that numeric resource explanation is source arithmetic, not a
measured GPU allocation trace. The foreign patch changes the budget to equal
the base and exposes the existing integration failure; the RGBA accounting
basis predates it. Do not describe all accounting errors as introduced today.

**Repair contract:** give `allocated_bytes` and `max_capacity_bytes` consistent,
truthful semantics for both textures and bounded entry metadata. Keep the real
initial/repeated-update contract; do not relax/delete the assertion or merely
inflate the maximum to silence it. The lead has registered accounting as a
separate subgoal and will preserve the foreign main-tree atlas patch. Its
repair overlaps atlas ownership and must be serialized with D1/D4.

## Exact minimal RED queue (Q1-Q6 proposed; Q7 already captured)

Q1-Q6 commands run from the chosen repair repository root **after the named
tests are added by a repair owner**. Those tests are not runnable reproductions
today and were not executed here. Q7 is an existing captured RED. Require one
selected test to run and an assertion
failure for the mechanism specified, not zero matches, compilation failure,
missing fonts/device, or an unavailable GUI. UI uses the repository's Vitest
script, not substitution with `bun test`. No sleeps, polling, or wait-for-time
success conditions: register exact signals before actions, settle deferred IPC
under React `act`, and use timeouts only as failure bounds.

### Q1 - D2: cheapest independent repair first (UI; HEAD baseline is sufficient)

Keep the real component and lifecycle module. Fake only Tauri IPC and layout
reads. Reset lifecycle/pane singleton state; A/B IDs and rectangles must differ.
Register `inputStarted`, B `boundsStarted`, and detach completion signals before
actions. A is first presented, then its input promise held. Rerender to B and
await B bounds start; reject A input and drain its reactions before allowing B's
valid receipt. Assert A has no recovery attach/retry and does detach exactly
once; B stays attached. Current-source prediction: second A attach, cancelled A
detach. No fake lifecycle implementation may hide that cancellation.

```sh
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not reclaim the outgoing surface when input fails after tab replacement'
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not recover input when its owner becomes hidden'
```

The second test awaits detach after the real visibility provider turns false,
then rejects old input and asserts no attach/retry. During repair add a companion
case for departure while recovery itself is pending; the test should require no
post-departure retry/result publication even when recovery later succeeds.

### Q2 - D1: repeated visible pressure (GPU, no desktop)

Add a row-cache internal unit test using real GPU, atlas, row cache, and the
1,000-glyph fixture described in D1. Preflight every glyph's nonzero ink before
packing; use actual single glyph keys, not `a123` strings. Build F0 as setup.
Action: build unchanged F1 through the real `update_and_flatten`. Require glyph
instances at all 1,000 wide origins. Prediction on reviewed foreign baseline:
992 with last eight absent. Do not consume the overflow flag between F0 and F1.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::row_cache::tests::all_visible_glyphs_survive_when_dense_2x_working_set_repeats -- --exact --nocapture
```

This is the cheapest omission RED. GREEN acceptance additionally needs real
offscreen readback against independently rendered small tiles, including glyphs
across allocation boundaries, so incorrect UVs cannot pass by instance count.

### Q3 - D4: history-only final output (GPU, no desktop; same atlas owner)

New renderer-internal unit test: prewarm its actual private atlas with exactly
992 successful covered wide insertions without overflow. Action: **one** call to
`render_snapshot` for new glyph 993 plus spacer, cursor hidden. Compare output to
a fresh renderer's same final input; first prove that reference has ink. Current
prediction: warmed frame is background-only, fresh frame contains glyph ink.
No hundreds of readbacks are needed to construct historical packing state.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests::last_frame_renders_new_glyph_when_only_history_fills_atlas -- --exact --nocapture
```

This deliberately chooses current-frame completeness as the repair contract.
Deferred recovery instead requires an additional actual production completion
test proving automatic bounded follow-up without new output; a manually invoked
second render is not sufficient proof of that policy.

### Q4 - D3: two small real-geometry REDs before Linux surface QA

The first new test calls real `WaylandSubsurfaceGeometry::from_logical_bounds`
with width 400.5, height 300, DPR 2 and asserts extent divisibility by returned
scale. Prediction: 801 modulo 2 is 1. The second compares the actual composition
layout used by the host to Wayland geometry for 400x300 at DPR 1.5. Use real
`SurfaceCompositionLayout::compute` / bounds request layout and nonzero metrics,
not a duplicate formula in a mocked host. Prediction: 600x450 versus 800x600.
After repair the assertion must use the host's new platform geometry path.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
```

Pure geometry runs on this macOS workstation; real Wayland presentation does
not. Fixing only the first test's helper cannot close the second defect.

### Q5 - D5: deterministic drop through direct host render

Introduce the smallest test-only acquisition/dispatch seam integrated with the
real direct host path. Attached existing host, idle coordinator; arm one Timeout
at acquisition, subscribe to dispatched-frame/completion before direct render.
Require one retry dispatch and eventual successful final presentation without
new input. Do not invoke `schedule_render` from the test or replace the direct
host path with a standalone policy function. Keep Lost -> Timeout -> success as
a follow-on case. The current inspected code has no ready-made injection seam;
building that seam is prerequisite work, not a claimed available harness.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::one_shot_render_requeues_when_frame_is_dropped -- --exact --nocapture
```

### Q6 - D6: Windows-native teardown RED (not executable on this workstation)

New target creates the actual child on a pumping owner thread, observes its HWND
and owner, then drops it from a worker as production detach does. Subscribe to
`WM_NCDESTROY` before drop; require destruction on owner thread. A bounded event
timeout fails the test; it is not a successful polling heuristic. Cleanup must
destroy any surviving child on its owner even when the RED assertion fails.
Exercise real detach/close boundary in subsequent integration acceptance, not
only a generic `DestroyWindow` demonstration.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_windows_teardown_contract child_is_destroyed_on_owner_thread_when_detached_by_worker -- --exact --nocapture
```

### Q7 - D7: existing accounting RED; no new test needed for the initial failure

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_renderer_contract dirty_update_atlas::test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache -- --exact --nocapture
```

Already executed once by the baseline executor, failed for the intended bound
at line 29, exit 101. Use this same test for GREEN after the accounting repair
on the explicitly captured atlas baseline. Also verify truthful texture/metadata
semantics against the selected capacity implementation, rather than treating
any two internally consistent but false numbers as adequate accounting.

## Surface QA actions and evidence still required

Not executed here. Use only the authorized debug launch `bun tauri dev`; no
release app, foreign-process restart, or incidental input in user terminals.
Each action log must identify build/diff, OS/backend, frontend/backend pane IDs,
measured cell dimensions, DPR/bounds, and actual frame completion. Subscribe to
the relevant event before the action, capture screenshots of completed frames,
retain resource/PTY identity and cleanup receipts. An ordinary visual pass
without the named trigger does not close a fault-ordering criterion.

| Defect | Exact real-surface action and acceptance |
| --- | --- |
| D1 | In a sufficiently large 2x pane, place at least 1,000 covered distinct Hangul glyphs at absolute row/column positions; hide cursor and avoid bottom-row scroll. Compute capacity from measured metrics. Capture last rows, force unchanged repaints, and compare against fixture. Repeat at 1x and after 1x -> 2x -> 1x moves. Every glyph stays correct, not merely nonblank. |
| D4 | Feed deterministic distinct-glyph pages into a stable pane until historical atlas keys reach the measured threshold; finish with a new tiny labeled line and no more producer output. Capture final presentation before interaction, then one explicit repaint. Pre-fix signature: omitted glyph restored only by repaint. Acceptance: correct final line without unrelated input, no retry spin. Keep current page below capacity to distinguish D1. |
| D2 | Journal an isolated event-gated fault hook: hold A input response and B's first bounds receipt, select B through real tabs, reject A, then release B. Observe attached host IDs and screenshots, resize B, and send identifiable output to background A. Only B may occupy its pane. Repeat split-to-tab with unequal rectangles and Settings during pending input; no replay to departed owner, PTYs survive. |
| D5 | With stopped numbered scrollback, arm one acquisition Timeout for the next native macOS scroll render, subscribe to present, then scroll once. Require the new rows with no second user action. Repeat Lost -> Timeout -> success; use portable scrollbar IPC on other OS runners. Record injected fault consumption so a normal successful acquisition cannot masquerade as GREEN. |
| D3 | On native Wayland, record outgoing buffer scale/extent and compositor diagnostics. Two distinct numbered panes; resize through odd/even physical widths at 2x. Exercise 1.5 only if actual DPR reports it. Require full pane edges/content, no overlap/gap and no `invalid_size`. X11 or headless arithmetic success is insufficient. |
| D6 | Windows real tab switches and masking overlays: correlate every child create with owner-thread `WM_NCDESTROY`; require one live child per attached pane, responsive event loop, and no old pane above replacement/chrome. Repeated attach/detach must not accumulate hidden children. |
| D7 | No standalone screenshot can prove byte accounting. Its acceptance is the actual contract GREEN and truthful resource semantics; use D1/D4 surface scenarios to ensure any coupled capacity change preserves rendering. Do not count accounting RED as a pixel reproduction. |

Shared acceptance also includes ordinary split, move, resize, tab switch, search,
and overlay transitions without injected faults. For macOS density transitions,
inspect child NSView frame, backing layer bounds/contentsScale, Metal sublayer
frame/drawableSize, and rendered content together. Source layer configuration
alone is not a display observation. `PLAN.md` records lead native QA access as
blocked (`runtime_unavailable`); this child neither retried nor resolved it.

## Remaining input findings: checked disposition

Every subsidiary hypothesis from the three reports is accounted for here.
"Rejected" means the cited proposed mechanism is contradicted by the checked
guards, not that all rendering is proven correct. "Unproven" means do not add
it to the active repair queue without the missing trigger evidence.

| Input finding/hypothesis | Disposition and independently checked reason |
| --- | --- |
| Atlas still clears mid-frame; deferred clear reuses stale cached UVs | **Rejected / foreign fix present.** `R/atlas.rs:154-158` returns without eviction; `R/row_cache.rs:30-36` clears and checks generation before reuse. D1/D4 are the remaining omission mechanisms. |
| Scale missing from row hash invalidates density rendering | **Rejected for normal scale updates.** Config includes scale (`R/types.rs:193-203`), host calls `update_config`, which clears atlas on inequality (`R/renderer.rs:98-105`). Hash omission alone is not a bug. |
| Any CJK-heavy screen overflows | **Rejected.** Atlas key hit precedes packing (`:131-138`); only distinct key pressure/historical accumulation applies. |
| Partial snapshots on foreign allocation failure poison cache | **Rejected for checked fallible path.** `N/render_pass.rs:24-199` propagates allocation/extraction errors before returning snapshot; terminal `:322-323` and host capture `:601-624,343-352` use that result before rendering. Cell extraction validates widths/styles and propagates errors. No claim about an untested foreign library returning logically inconsistent success. |
| Font/fallback changes with identical metrics retain old ink | **Conditional cache-coherence risk, deferred.** `R/font_manager.rs:77-89` rebuilds for changed comma-separated family list; `R/coretext_font.rs:327-369,378-429` uses primary metrics; config has no font identity. Preference IPC (`I/preferences.rs:52-106,132-145`) reuses hosts and existing keys can hit old atlas entries. No differing covered fallback fixture was exercised. Existing Retina test updates before warm render. This is not a confirmed cause of intermittent incident or typography repair request. |
| Transient native raster allocation failure becomes permanent blank | **Conditional failure-path risk, deferred.** `R/coretext_raster.rs:209-223,311-317` returns no ink for null bitmap context; font manager `:154-156,218-229,299-302` produces zero buffer, atlas skips, row cache records normal hash/background. Recovery needs controlled thread-scoped one-shot native-context fault, not Rust allocator OOM stress. Empty-PUA test proves intentional empty behavior only. HEAD also cached blank rasters: do not attribute the whole issue to the foreign patch. |
| Empty glyphs are always packing-neutral | **Rejected as a universal claim, not a separate visible corruption finding.** Whitespace returns before mutation, but non-whitespace zero-ink rejection follows wrap/capacity; at full atlas it can set pending before rasterization (`R/atlas.rs:126-175`). Fresh empty-PUA test does not cover pressure. Extra resets alone do not establish a new visible defect. |
| Atlas stats/budget are truthful and bounded | **Runtime-reproduced contract defect D7, distinct from pixel symptoms.** Exact integration test ran one and failed line 29 with exit 101; report and raw log checked. Two RGBA8 textures versus 5-byte basis, plus entry overhead exceeding the equal-base maximum, are the independently checked source explanation. The 111-test lib baseline did not run this target. |
| Width absent from GlyphKey / preedit width differs | **Unproven incident hypothesis.** Atlas key lacks width; `N/surface_host.rs:547-598` uses range-based preedit width and extractor reads committed Ghostty width at `N/cell_extractor.rs:117-137`. No exercised same-key preedit -> committed width transition was provided. Do not promote merely from different algorithms. |
| Huge accepted cell sizes / GPU OOM / device-loss host recovery | **Unproven normal product trigger.** Config permits dimensions up to 4096 (`R/types.rs:225-239`), but no ordinary caller/incident requires such an atlas allocation. `R/gpu_context.rs:89-92,142-149` reports GPU errors separately; capacity `None` does not enter that channel. No observed device loss/OOM or broader redesign justified. |
| Lost/Outdated always recovers, or all acquisition drops stick | **Both overgeneralizations rejected.** Host retries Lost/Outdated once, reports second non-Timeout error, and retries scheduled/bounds Timeout through existing policy. Only direct final drop is D5. No full-device rebuild proven. |
| Detached render resurrects geometry; missing ownership recheck; output during frame loses follow-up | **Rejected / committed guards present.** `N/surface_host.rs:826-845,1498-1514,1707-1728` shares hosts lock through ownership/present; `:192-259` preserves follow-up. Tests at `:2065-2108,2376-2432` cover these seams. D2 cancels legitimate detach before this guard applies; D4 lacks an originating request. |
| Cold invalid bounds necessarily leave blank; initial drop releases outgoing pane | **Rejected / committed fixes present.** Attach tolerates invalid initial layout, ordinary pane completion awaits attach, and non-presented bounds retries without releasing waiters. Checked host attach/reattach path and pane lifecycle tests `:130-183`. No blanket automatic retry claim for generic errors. |
| Warm return replays history / always loses PTY size | **Rejected / committed fix present.** Live-task guard and retained VT path at host `:1011-1109` reassert PTY dimensions without history replay; host unit `:2024-2061` and integration `:781-853` test retained output. Detach keeps pump; close destroys VT. `N/terminal.rs:120-124`/`lifecycle.rs:85-107` tear down callbacks/handle only on actual terminal drop. |
| macOS uses root WebviewWindow, inert backing layer, double pane offset, or reachable async late-unhide | **Rejected for checked production path.** Actual child is created, Metal observer sublayer is configured, local viewport origin is zero, only post-present reveal is host guarded/main-thread. `N/platform/macos.rs:283-348,515-548`, host `:1935-1940,1988`, vendored Metal `surface.rs:63-107,149-171` and observer `:141-164` were read. Off-thread raw reveal code alone does not establish a reachable race. Actual KVO/display timing remains unproven. |
| Windows/Linux `update_viewport(None)` no-op proves detach failure; unsupported children silently use root | **Rejected.** Production detach drops hosts, not just viewport None. Real-child descriptor guard rejects no-child/unsupported cases. Windows's actual destructor thread is D6. macOS separately dispatches off-thread view removal; do not transfer D6 to macOS by analogy. |
| Foreign HAL fix is a new rendering geometry repair | **Rejected attribution.** Current observer contains superclass `dealloc`; it is foreign baseline work. Checked observer scale/frame and configure paths do not make that cleanup change a new glyph/geometry finding. |
| Shared attach resolves before readiness; same-turn reparent tears down returning sibling | **Rejected / committed guards present.** Lifecycle `:196-250` cancels legitimate return and shares attachment promise. Read all seven lifecycle unit tests, including readiness/failure and returning-held-detach cases. D2 is the caller lifetime violation, not an instruction to remove these guards. |
| Fixed bounds prevent DPR update; newest pending resize discarded | **Rejected / committed guards present.** Pane resolution query/window listener `:1769-1780`, pending geometry `:1645-1694`; lifecycle density test `:425-452` and component coalescing test `:705-789` exercise them. |
| Zero-area/failed bounds permanently deduplicated; stale normal completion publishes | **Rejected for stated mechanism.** Measure gate `:671-675`, failure cache clearing `:1620-1633`, `isSubscribed` checks and cleanup `:1595,1647,1796`; component tests `:843-978`. Already-issued IPC is not cancelled, and generic errors need another measurement/banner retry. |
| Wrong frontend/backend ID used | **Rejected for ordinary pane binding.** Pane `:536` requires backend ID when session object exists; `TerminalPane.tsx:133-140` supplies that object. D2 instead retains an obsolete but valid backend ID. |
| Normal modal masking/opt-out remains broken | **Rejected for checked callers.** Visibility hook `:20-25,54-68` scans actual mounted dialogs/search, checks closest opt-out; TerminalPane conditionally mounts search and App mounts Settings. Existing lifecycle masking tests cover newly dispatched input, not D2 pending continuations. |
| Pane-to-tab respawns rather than moves PTY; every terminal should mask for drag | **Rejected.** `U/state/layout.ts:400-477` moves existing session mapping; `TerminalSplitView.tsx:1020` provider masks drop-feedback owner. No rendering defect justified by a different drag-visibility preference. Mocked pane-tree tests are not native QA. |
| Position-only bounds drift at fixed size | **Unproven.** Pane observes size/window/DPR, not arbitrary ancestor translation; checked tab/split/reparent paths alter owner/effect or size. No concrete unsignalled incumbent translation trigger supplied. Do not add polling. |
| Attribute-only visibility changes / nested provider conjunction | **Unproven product caller.** Hook observes child-list mutations and provider replaces context. Production provider references identify TerminalSplitView, not nested owners; no dynamic opt-out caller established. API possibilities alone are not defects. |
| Outgoing host retained forever when replacement never presents | **Unproven error-path hypothesis.** Lifecycle attach rejection releases waiters (`:234-240`); bounds generic errors/zero area differ from successful attach then dropped bounds. No concrete permanent product ordering established beyond D2. Do not collapse these failure modes. |

Deferred font/allocation probes, only if later incident evidence justifies them:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_font_cache_contract cached_glyph_uses_new_fallback_when_primary_metrics_are_unchanged -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::row_cache::tests::unchanged_row_recovers_when_native_raster_allocation_recovers -- --exact --nocapture
```

Neither target/test exists yet. Font probe must first prove A/B fallback rasters
differ with identical primary metrics and restore preference overrides with
scoped cleanup in an isolated process. Allocation probe must record consumption
of exactly one native bitmap-context fault with real resolver/atlas/cache/GPU,
then test unchanged-row recovery. Do not mock row instances or change fonts as
a substitute for the failing native allocation boundary.

## Candidate disjoint repair ownership and ordering

| Owner/scope | Candidate paths and boundary | Dependency/conflict rule |
| --- | --- | --- |
| Frontend lifetime (D2) | `U/components/NativeTerminalPane.tsx`, its lifecycle test file; only extend lifecycle API if token execution requires it | Can begin on clean HEAD, independent of Rust and foreign onboarding. Do not edit App/onboarding or remove returning-owner guards. |
| Atlas completeness (D1 + D4), accounting (D7 separate criterion) | `R/atlas.rs`, `R/row_cache.rs`, `R/renderer.rs`, and allocation/bind-group/instance code only if required by chosen capacity strategy; named GPU regressions and existing accounting contract | One atlas path owner, or serialized atomic repairs, not concurrent atlas writes. Requires explicit approved foreign baseline first. D7 is a reproduced defect with its own registered subgoal, not optional cleanup; preserve the foreign main-tree patch. |
| Direct-drop scheduling (D5) | `N/surface_host.rs`, its narrow acquisition seam/tests; preserve IPC surface | Independent of frontend/renderer only if atlas recovery stays renderer-local. If D4 propagates receipts, serialize host integration with this owner. |
| Wayland geometry (D3) | `N/child_surface.rs`, Linux/platform geometry forwarding, Wayland contracts; minimal host/layout consumption seam | Pure helper tests can be authored independently, but host changes overlap D5. Freeze the geometry interface and integrate serially rather than calling these path-disjoint. No global macOS/Windows quantization. |
| Windows lifetime (D6) | `N/platform/windows.rs`, owner-thread cleanup boundary, Windows-native contract | If teardown transfer lives in host/IPC, serialize with D5/D3 integration. No Windows GUI verification available on this workstation. Do not alter vendored HAL to hide a target ownership violation. |

Recommended execution order: Q1 first while the atlas baseline is settled;
then Q7 accounting and Q2/Q3 under one atlas owner, with Q4 pure geometry in an independent scope; capture
Q5 through its real host seam; perform Windows Q6 on a native runner. Register
separate defect criteria even when one owner repairs two causes. Production
repairs require captured right-reason RED, same-test GREEN, affected diagnostics,
related tests/build and the named real-surface acceptance. This report closes
the source-disposition task only; it does not close aggregate rendering QA.

The report is uncommitted in a shared tree. Foreign dirty files, vendor code,
runner, manifests, and onboarding remain read-only and unchanged by this task.
