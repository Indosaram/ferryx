# D3 independent Wayland geometry verification

## Decision

**ACCEPT the scoped source geometry repair and portable Q4 proof. Native
Wayland acceptance is PENDING. Overall rendering acceptance is NOT APPROVED.**
Both required regressions executed and failed for the intended numeric mismatch
before production edits, subsequently passed, and passed once each in this
independent verifier run. The production path consumes the repaired geometry;
this is not a helper-only fix. No native Wayland window, buffer commit, pixels,
or compositor diagnostics were exercised by this verifier or the D3 repair
worker. Linux-target compilation/linking is also unverified.

Verifier: `st_01a07765`, session `01a07765-bb6e-769b-b2a1-774b589efb5f`.
Execution host: arm64 macOS. Review date: 2026-09-06 UTC.
Only this report was authored; no source/test edits, staging, commits, desktop
interaction, remote writes, or attribution of concurrent UI/Windows changes.

## References and source identity

- Contract: `review.md:151-182` (D3), `review.md:383-401` (Q4),
  `review.md:442-466` (surface evidence, specifically D3 at line 458), and
  `REPAIR_PHASE.md:95-125` (scope, RED ordering, host integration and native QA).
- Repair report: `wayland-repair.md`, read in full and checked against code,
  raw logs, patch identity, and the repair worker's tool journal.
- `W/` below means `/Users/indo/code/project/orca-lite-rendering-20260906/`;
  `N/` means `W/src-tauri/src/native_terminal/`; `T/` means `W/src-tauri/tests/`.
- `E/` means main-tree `.omo/evidence/ulw/rendering-review-20260906/D3/`.
- `J` means main-tree
  `.omo/senpi-task/children/st_01a07733/sessions/st_01a07733/2026-09-06T14-50-52-097Z_01a07733-6541-7c3b-a56a-f65597ffaa4e.jsonl`.
  Journal references below are one-based JSONL lines, with UTC timestamps.

Observed worktree HEAD: `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145`.
The six D3 paths have no committed difference from original task base
`b8f82d707f0cb99907e3d79c0c9cdc75053ef931`. Their current uncommitted diff
matches `E/owned-diff.patch` byte-for-byte, independently checked before and
after the verifier's test execution. SHA-256:
`dea20e8e45778e646ce0a849c4d55c3094667d563e86091f8a0bc1f766abf78b`.
All six file hashes also match `E/provenance.log:10-15`.

The scoped files are `N/child_surface.rs`, `N/composition.rs`,
`N/platform/mod.rs`, `N/platform/linux.rs`, `N/surface_host.rs`, and
`T/native_terminal_wayland_subsurface_contract.rs`. The foreign dirty
`N/platform/windows.rs` is excluded. UI work is not part of this disposition.
The repair is uncommitted and this acceptance is tied to the recorded bytes,
not to future shared-worktree changes.

## RED ordering and same-contract GREEN

**Accepted: the two numeric failures are real REDs, not zero matches,
compilation failures, or an unavailable GUI.**

| Evidence | Independently checked disposition |
| --- | --- |
| Baseline and test-only preparation | `J:6` reports only foreign UI dirt at 14:51:08Z. `J:30-35` records test-file patch attempts; `E/red-tests.patch` contains only the two new tests/imports. `J:39`, at 15:14:16Z, still lists only the Wayland test plus foreign Windows dirt, not D3 production edits. |
| First RED | `J:36-37`: run starts 14:54:03Z; by tool return 15:14:03Z, width 801, height 600, scale 2, assertion `1 != 0`, one failed test, exit 101. Raw receipt: `E/RED-buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional.log:509-532`. |
| Incomplete second attempt | The enclosing 1200-second deadline expired. `E/RED-host-timeout-incomplete.log` ends after Cargo's `Running` line, with no test count or assertion result. It proves neither RED nor GREEN and is not counted. |
| Second RED | `J:42-43`: standalone run starts 15:14:32Z and returns 15:16:03Z. Actual request layout is 600x450 versus child 800x600; one failed test, exit 101. Raw receipt: `E/RED-host_extent_matches_wayland_buffer_extent_at_fractional_scale.log:71-91`. |
| First production edit | `J:50-51`, 15:17:42Z, patches the geometry/selection implementation after both RED receipts. Host integration follows at `J:57`, 15:19:56Z. Thus RED-before-production is established by recorded tool order, not file mtimes or the repair report alone. |
| Final-source validation | Attach-error handling was adjusted at `J:95`, 15:32:59Z. `J:100-101`, 15:33:40Z-15:38:08Z, reruns the exact tests, related contracts, host tests, check and build after that adjustment. Top-level `E/GREEN-*.log` and check/build logs are the final receipts; archived earlier results are not substituted. |

The first test's assertion is unchanged. The second retains the same
400x300/DPR-1.5 input, nonzero 20x40 cell metrics, real
`NativeTerminalBoundsRequest::layout`, and host-versus-child extent assertion.
Its geometry-path change is to resolve the request using the new production
policy; added checks require scale equality and grid 40x15. This is the
post-repair path explicitly required by Q4, not a relaxed expected value or a
hand-computed replacement host.

## Independent executable result

Executed from `W/` once each, 2026-09-06 **15:59:36Z-16:00:04Z**:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
```

| Test | Actual verifier output | Result |
| --- | --- | --- |
| `buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional` | Position (0,0), buffer 802x600, scale 2; width and height divisibility assertions pass. | 1 passed, 0 failed, 11 filtered out; exit 0 |
| `host_extent_matches_wayland_buffer_extent_at_fractional_scale` | Host physical origin (20,40), extent 800x600, grid 40x15; child logical origin (10,20), extent 800x600, scale 2. | 1 passed, 0 failed, 11 filtered out; exit 0 |

Raw verifier command/output is retained in this task's tool journal under
`.omo/senpi-task/children/st_01a07765/sessions/st_01a07765/2026-09-06T15-45-50-958Z_01a07765-bb6e-769b-b2a1-774b589efb5f.jsonl`.
No separate evidence file was written because this task authorizes only the
report. No monitor tool or `monitor` executable was available; the bounded
foreground Bash call completed normally in approximately 28 seconds. There
were no retries, sleeps, or polling loops.

## Code and coverage criteria

| Criterion | Disposition and exact mechanism |
| --- | --- |
| Integer-scale divisibility, including original odd-width trigger | **ACCEPT.** `N/child_surface.rs:57-96` validates finite inputs, selects the existing nearest-integer scale (minimum 1), snaps logical left/top and right/bottom edges, then multiplies their differences by scale. The 400.5-width/2x case becomes 802 rather than 801. Collapsed extents, physical overflow and scale above `i32::MAX` are rejected. |
| One geometry for host buffer and grid | **ACCEPT at source and portable seam.** `N/composition.rs:13-33` returns canonical integral bounds and selected integer scale. `N/surface_host.rs:1725-1779` gets the actual host/target before resolving the request, derives font metrics from that resolved scale, and calls real `prepare_session_layout`. `:118-132` reaches unchanged `SurfaceCompositionLayout::compute`; `:866-976` creates/resizes the actual terminal, stores bounds/metrics/layout and notifies PTY resize. |
| Real caller reaches resolver, not an unused helper | **ACCEPT at source.** `W/src-tauri/src/ipc/native_terminal.rs:505-545` forwards bounds/DPR into main-thread `state.render`. That render calls the resolver at `N/surface_host.rs:1745` before layout. The regression executes that resolver and the same bounds-request layout code, not a duplicate formula. It does not create a native target or run the GUI caller; those are pending below. |
| Renderer/swapchain receives same extent and density | **ACCEPT at source.** `N/surface_host.rs:1920-1957` updates viewport and renderer density from stored canonical bounds, then configures from layout width/height. `N/renderer/renderer.rs:122-129` forwards those dimensions unchanged to `gpu_context.rs:115-139`, which supplies actual `SurfaceConfiguration` width/height. Cells come from `font_manager.rs:325-332` at the resolved scale. Rendering uses local viewport (0,0), and frame presentation occurs at host `:2009`. |
| Initial host setup cannot count as a presented raw-DPR frame | **ACCEPT at source.** `N/surface_host.rs:1884-1918` creates a provisional renderer and 1x1 surface configuration but does not acquire/present a frame. Direct render replaces density/layout before its first acquisition. Cold attach has no host and is provisional; scheduled render and focus do not insert a missing host (`:357-362`, `:1802-1819`). |
| Child placement/scale uses same canonical bounds | **ACCEPT at source; protocol execution PENDING.** `N/platform/linux.rs:382-392` reconverts canonical bounds to `WaylandSubsurfaceGeometry` and passes logical position plus integer scale to unchanged `wayland_child.rs:372-393`, which issues `SET_POSITION` and `SET_BUFFER_SCALE`. The second conversion is idempotent, covered by the shared-edge test. The helper's physical fields remain unnecessary at this protocol call because the host now configures the matching buffer. |
| Scheduled, focus and warm-attach layouts stay coherent | **ACCEPT at source.** Scheduled render reads stored layout/bounds (`N/surface_host.rs:337-362`); focus does likewise (`:1782-1819`). Both attach paths consult an existing target policy before deriving/storing metrics (`:979-1018`, `:1024-1132`). Invalid bounds stay inside existing logged fallback branches rather than aborting attachment early. Related host receipts retain invalid-attach, warm-return, detached-ownership and PTY-size tests. No native warm-Wayland render was executed. |
| Shared pane edges, not independently rounded sizes | **ACCEPT for valid nonnegative pane geometry.** `T/native_terminal_wayland_subsurface_contract.rs:66-130` exercises three touching panes through the resolver, real font metrics and real `prepare_session_layout` at DPR 1, 1.5, 2 and 2.5. It checks child shared edges, repeated resolution, host extents, stored bounds/metrics/layout, and grid division. `E/GREEN-contracts.log:100-111` contains the actual numeric executions. |
| Non-Wayland geometry preserved | **ACCEPT at source and portable contracts.** `N/platform/mod.rs:93-100` selects Wayland only under Linux and only when `linux.rs:352-354` reports an actually created Wayland child. X11/other targets use identity resolution. Existing `ChildSurfaceGeometry`, AppKit conversion and `SurfaceCompositionLayout::compute` are unchanged. Test `T/native_terminal_wayland_subsurface_contract.rs:133-162` preserves fractional AppKit bounds and 601x451 physical layout at DPR 1.5. This does not claim native Windows/X11/macOS GUI verification. |
| Appropriate regression integration and determinism | **ACCEPT.** The five new tests use real production geometry/layout/session code and machine values. No mock host, AST/source-string pinning, sleeps, polling, or prose assertions. Fixed metrics in Q4 make its exact grid deterministic; the broader test obtains real font metrics and asserts relationships instead of machine-specific font numbers. |

For example, at DPR 1.5 and 2, shared-edge pane 0 snaps from
(10.25,20.25,400.5,300.5) to (10,20,401,301), scale 2, buffer 802x602.
Its right neighbor starts at logical x=411/physical x=822; its lower neighbor
starts at logical y=321/physical y=642. The host's physical right/bottom edges
are exactly those values. The recorded Mac font metrics were 16x33, giving
grid 50x18, but these font-specific numbers are not pinned by the test.
Existing negative-origin clamping is retained; the shared-edge guarantee here
concerns normal nonnegative pane coordinates, not arbitrary off-parent inputs.

## Related validation evidence inspected

The following are **repair-worker executions independently inspected in full**,
not extra verifier reruns. All top-level logs end in exit 0 and follow the final
attach-error-preservation edit (`J:95,100-101`).

| Evidence | Actual result |
| --- | --- |
| `E/GREEN-contracts.log` | 7 composition + 12 Wayland tests passed, no failed/ignored/filtered tests. |
| `E/GREEN-host-tests.log` | 27 host tests passed; 571 unrelated tests filtered. Includes PTY resize, warm attach, invalid initial/reattach bounds, detached ownership, and render coordination contracts. |
| `E/GREEN-child-contract.log` | 5 child geometry/visibility tests passed. |
| Both `E/GREEN-<exact-test-name>.log` files | One selected test passed each; numeric results match verifier execution. |
| `E/cargo-check.log` | `cargo check --manifest-path src-tauri/Cargo.toml` finished successfully in 20.35 seconds. |
| `E/cargo-build.log` | `cargo build --manifest-path src-tauri/Cargo.toml` finished successfully in 1m35s. This is a Mac build, not a Linux build. |
| `E/lsp-diagnostics.log`, `J:68-74,97-98` | All six changed-file checks returned no diagnostics; host was checked again after its final edit. Linux cfg code is not validated as Linux by Mac diagnostics. |
| Scoped `git diff --check` | Passed in the repair evidence and independently in this verifier session. |

Seven library compiler warnings remain visible in RED, GREEN, check/build and
the verifier runs: unused `Manager`, unnecessary `mut` in font manager, and
existing dead-code items. The lib-test log instead includes unused `super_key`
among its seven warnings. Nothing was suppressed. These are not new D3 errors.

## Blocked native acceptance and exact remaining proof

**PENDING / not satisfied:** `review.md:458` and
`REPAIR_PHASE.md:121-125` require actual Wayland presentation, not arithmetic.
The source path at `N/platform/mod.rs:93-98`, `N/platform/linux.rs:300-307,382-392`
and `N/platform/wayland_child.rs:372-393` is Linux-only. This workstation and
the retained D3 evidence supply neither a Linux build/link result nor native
execution of that path. No screenshot or `invalid_size` absence is inferred.

Acceptance requires an authorized Linux Wayland runner to:

1. Build the identified repair bytes for Linux, launch the authorized debug
   surface (`bun tauri dev` per `review.md:444-452`), and establish that an actual
   Wayland child was created, not X11 or disabled-subsurface fallback.
2. Identify two distinct numbered panes, frontend/backend pane IDs, OS/backend,
   build/diff identity, raw bounds/DPR, canonical bounds, cell metrics/grid,
   outgoing `set_buffer_scale`, actual attached buffer extents, and frame
   completion. Record compositor diagnostics alongside those values.
3. Resize at actual DPR 2 across widths that would produce odd and even physical
   sizes before quantization, including the 400.5-logical-width trigger. Require
   both actual buffer dimensions divisible by scale, buffer/scale equal to the
   intended canonical logical extent, and shared pane edges coincident.
4. Exercise DPR 1.5 only when genuinely reported by the WebView/output; record
   actual scale 2 and matching host buffer/grid/placement. Otherwise label that
   native fractional-DPR case unexercised, not passed through a forced formula.
5. Capture completed frames with full numbered content and all pane edges,
   no gap/overlap or clipping into adjacent panes/chrome, and no `invalid_size`.
   Include resize and warm reattach/scheduled output to confirm stored geometry
   survives the real host paths. Register completion signals before actions;
   do not use sleeps or successful polling as proof.
6. Retain resource/PTY identity and owned-resource cleanup receipts. Shared
   ordinary split/move/resize/tab/search/overlay QA remains a separate aggregate
   requirement (`review.md:463-466`).

This report closes the independent **D3 source/portable-evidence review only**.
Linux compilation and the native checks above remain blocked on an authorized
native runner; no source-only result closes them or approves overall rendering.

## Cleanup and write boundary

`E/cleanup.log` records repair-worker cleanup at 2026-09-06T15:42:40Z,
retention of the incomplete attempt, no persistent D3 GUI/server resources,
and intentionally retained build cache/evidence. Its process filter alone is
not proof that all system Cargo work stopped. This verifier's two exact Cargo
commands returned exit 0, and a subsequent process inspection found no live
exact Wayland regression executable or matching Cargo test command. Unrelated
Cargo/compiler activity was observed and left untouched.

No persistent process was spawned by this verifier, and no desktop, browser,
daemon, remote host, PTY, or shared index was controlled. Only this uncommitted
verification report was added; build cache updates from the requested test
execution are retained. The six source/test files remain byte-identical to the
repair evidence. Report LSP diagnostics were requested but are unavailable
because no Markdown server is configured; report whitespace was checked with
`git diff --no-index --check /dev/null` against this new file.

## Receipt/IME delta verdict - st_01a077ae, 2026-09-06

**Original D3 geometry/source acceptance is retained. Receipt/IME delta
acceptance is BLOCKED: no delta implementation or behavioral proof was
delivered in the inspected snapshot. Native Wayland acceptance remains PENDING;
overall rendering is NOT APPROVED.** This addendum preserves the original
report above. UI source is authorized review scope for this delta; its absence
from the original worker's scope is not a fault assigned to that worker.

Verifier session: `01a077ae-1b2f-7fa9-92ca-6a40d20ef4f9`, arm64 macOS.
The observed repair HEAD is still `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145`.
The full six-path geometry diff was read and its SHA-256 remains
`dea20e8e45778e646ce0a849c4d55c3094667d563e86091f8a0bc1f766abf78b`.
There is no diff from `bce59b4` in the IPC receipt file, React component, or
component lifecycle test. Foreign Windows work was neither modified nor
attributed. All source references below are in `W/`, with `N/` as defined above.

### Original geometry evidence recheck

Both exact commands in the original report were executed **once each** from
`W/`, **17:05:54Z-17:05:55Z**. Each ran one test, passed, filtered 11, and exited
0. The first produced buffer 802x600 at scale 2. The second produced host and
child 800x600, host origin (20,40), child logical origin (10,20), grid 40x15.
The seven existing library warnings remained visible; none was suppressed.
No monitor tool/executable is available here; the bounded foreground command
completed in about two seconds, without polling, sleeps, or retries.

The original RED logs were rechecked: one executed failure each, remainder
`1 != 0` and host `(600,450) != (800,600)`, both exit 101. Journal tool calls
`J:36-43,50-51` reconfirm RED-before-production at 15:17:42Z; the interrupted
second attempt is still excluded. Full top-level related contract/host/child,
check/build and cleanup logs were read again: 12+7, 27, and 5 tests pass;
Mac check/build exit 0 after the final adjustment at `J:95,100-101`.
These are retained producer receipts, not newly executed full suites.

The actual render still resolves the target's policy before metrics/layout
(`N/surface_host.rs:1725-1779`), stores canonical geometry, and uses it for
renderer density, buffer extent and child viewport (`:1928-1957`). The full
unchanged diff retains shared-edge session-layout coverage, idempotence and
identity policy for non-Wayland targets. These passes do not test the missing
receipt scale or actual Wayland protocol execution.

### Delta criteria and exact dispositions

`D/` below means `docs/rendering-review-2026-09-06/WAYLAND_RECEIPT_DELTA.md`.
`I/` means `W/src-tauri/src/ipc/native_terminal.rs`; `U/` means
`W/ui/src/components/NativeTerminalPane.tsx`.

| Criterion | Disposition and evidence |
| --- | --- |
| Effective scale comes from actual stored presentation bounds (`D/:41-46,72-74`) | **BLOCKED / absent.** `N/surface_host.rs:134-166` has neither an effective-scale field nor a scale argument in `from_snapshot`. `get_receipt` at `:1703-1722` reads stored layout/metrics but does not return stored bounds' scale. Focus fallback at `:1814-1820`, dropped-frame receipt at `:1987-1993`, and presented receipt at `:2012-2021` likewise omit it. Geometry being correct does not satisfy this receipt contract. |
| Scale reaches the serialized wire and every receipt path (`D/:72-74`) | **BLOCKED / absent.** `I/:26-39` has no scale field; the manual converter `:1422-1438` cannot transmit it. Set-bounds (`:530-532`) and focus (`:563-564`) use render/focus; input (`:817-818`) and paste (`:1081-1082`) use `get_receipt`; preedit (`:597-598`, host `:1860`) delegates to `get_receipt`; scroll (`:857-871`) uses render or direct receipt; mouse's optional receipt (`:1263-1294`) renders stored bounds then uses the same converter. All lose effective scale at the shared receipt boundary. Serialization test `I/:1446-1459` checks `presented`, not scale. |
| Actual focus-sink anchor at DPR 1.5/native 2 (`D/:48-61`) | **BLOCKED / source-proven defect remains.** `U/:591-607` still divides pixel metrics by raw `scaleFactorRef.current`, populated by raw DPR measurement (`:661-684,692,1598`). Actual textarea style consumes that state at `:1947-1959`. Cursor (2,3), cells 16x32 therefore computes CSS left/top (21.333...,64), size 10.666...x21.333..., rather than (16,48), 8x16. This is source arithmetic, not a newly executed component RED or native pixel observation. |
| Preserve raw DPR for geometry requests; no mouse rewrite (`D/:17-20,56-61`) | **ACCEPT unchanged source.** Attach forwards measured raw scale at `U/:705-712`; bounds requests use it at `:1606-1609`. `sendMouse` at `:980-1022` sends CSS-local client coordinates and only consumes the nested receipt for the non-motion anchor. No mouse-coordinate regression is inferred from the missing receipt field. |
| Ordinary equal-scale and absent-scale compatibility (`D/:57-61,65-67`) | **UNCHANGED source behavior; delta proof BLOCKED.** Current component always divides by raw DPR, including legacy receipts. Equal-scale arithmetic therefore retains its existing behavior. There is no effective-scale contract or new ordinary/missing-scale regression to verify, so these are not delta GREEN claims. |
| Preserve D2 owner guards (`D/:35-37,65-67`) | **ACCEPT unchanged source identity, not a fresh suite pass.** Component/lifecycle test are byte-identical to `bce59b4`. Owner invalidation at `U/:537-543`, queued attach check at `:704`, input checks before execution/publication/recovery at `:749-774`, and after recovery at `:788-793` remain. No new delta edits can be credited with a D2 test run. |
| No additional host-lock acquisition order (`D/:72-73`) | **ACCEPT unchanged source; future delta still requires review.** No receipt production changes exist. The already-held sessions state in direct receipt and available bounds in focus/render are the correct source for an additive scale field, rather than another hosts-lock lookup. |
| Both named delta REDs precede production, then same tests GREEN (`D/:39-61`) | **BLOCKED / not executed.** Neither `receipt_reports_effective_presentation_scale` nor `uses effective native scale for IME anchor at fractional webview density` exists in its specified test file. The delta worker journal records reads and its blocker-report write, no test execution or production edit. Running a nonexistent exact filter would produce no behavioral evidence; no zero-match run is represented as RED/GREEN. Original geometry REDs cannot substitute. |
| Related delta tests, diagnostics and affected UI/Rust checks (`D/:65-67`) | **BLOCKED / no delta receipts.** `wayland-ime-repair.md` explicitly claims none. Original geometry build/contracts remain accepted only for their unchanged scope; no receipt/IME or post-delta D2 validation is inferred. No new formula-only or source-pinning test was introduced by this verifier. |
| Actual component browser DOM/action log, screenshot and cleanup (`D/:68-70`) | **BLOCKED / missing.** No `E/receipt-delta` artifact directory exists in the inspected evidence tree, and the repair report claims no browser execution. Existing D2 artifacts are not fractional-DPR/native-scale delta proof. No browser was launched by this verifier. |

### Blocker ownership and release criteria

`wayland-ime-repair.md` is accurately a **scope-blocker report**, not a repair
completion report. Its proposed minimal IPC-path extension is technically
necessary: changing only the internal host receipt and React consumer cannot
cross the manually copied serialized boundary. The producer's explicit
allowlist in `D/:28-33` omits IPC production code. Its journal records the
blocker report write at 17:04:45Z, before this verification. Authorizing UI
review scope alone does not add the missing IPC production write permission.
The lead must authorize the additive receipt field/converter/serialization
test changes in `src-tauri/src/ipc/native_terminal.rs` for the producer; this
verifier has no source-write authorization and did not implement a workaround.

Delta acceptance requires the two named behavioral tests to execute and fail
for the missing stored scale/wrong actual anchor **before delta production**,
then pass using stored scale across every construction/conversion path above.
Require ordinary and absent-scale cases, unchanged raw-DPR requests and D2
guards, affected checks, and actual-component browser DOM geometry plus a
screenshot at DPR 1.5/native 2. Subscribe to exact async completion before
triggering actions, with bounded timeouts; no timing-luck or formula-only proof.
Even successful mocked-IPC component/browser evidence closes only that boundary.
Native acceptance still requires the earlier Linux Wayland build, actual child,
buffer/scale/edge/compositor and completed-frame receipts, plus real IME
candidate placement at the cursor when the fractional-DPR case is available.

### This verifier's evidence and cleanup

Raw commands/results are in
`.omo/senpi-task/children/st_01a077ae/sessions/st_01a077ae/2026-09-06T17-04-54-063Z_01a077ae-1b2f-7fa9-92ca-6a40d20ef4f9.jsonl`.
The delta producer journal is
`.omo/senpi-task/children/st_01a077ac/sessions/st_01a077ac/2026-09-06T17-03-04-884Z_01a077ac-70b4-75c7-8c55-906e7a50a2be.jsonl`.
Both verifier Cargo commands exited; a subsequent exact process inspection
found no matching Wayland regression executable or Cargo invocation. No
persistent resources, desktop interaction, remote writes, staging or commits
were created. Only this report was appended, using `apply_patch`; test build
caches are intentionally retained. Worktree `git diff --check` passed.

## Delivered receipt/IME delta verification - st_01a07818, 2026-09-06

**RETAIN original D3 geometry/source acceptance. ACCEPT the delivered delta's
source behavior and portable/component proof, with the RED sequencing
qualification below. Native Wayland acceptance remains PENDING; overall
rendering is NOT APPROVED.** No remaining delta code regression was established.
The earlier missing-implementation/IPC-scope blockers above are historical and
are superseded by this addendum, not deleted. Literal RED-before-any-production-
edit compliance is **NOT MET** for Rust: a non-propagating receipt field scaffold
preceded RED. This is an evidence-process exception, not a failing final test.

Verifier session: `01a07818-4d24-7c52-8044-dab6e6f71f69`, arm64 macOS.
`W/`, `N/`, `T/` and `E/` retain their original meanings. Here `H` is
`N/surface_host.rs`, `I` is `W/src-tauri/src/ipc/native_terminal.rs`, `U` is
`W/ui/src/components/NativeTerminalPane.tsx`, `L` is its neighboring
`NativeTerminalPane.lifecycle.test.tsx`, and `D` is
`docs/rendering-review-2026-09-06/WAYLAND_RECEIPT_DELTA.md` (current revision).
`ED/` is `E/receipt-delta/`. Line references below refer to inspected final bytes.
`JD` is the producer journal:
`.omo/senpi-task/children/st_01a0780d/sessions/st_01a0780d/2026-09-06T18-49-16-500Z_01a0780d-a9d4-7dcc-81e7-cc0507907298.jsonl`.

### Scope and original geometry preservation

Observed HEAD: `c349e3a6ffa1ff36cef30ac4d66dd78250e3f428`. Its committed
difference from `bce59b4` is confined to the foreign Windows implementation;
that work was not modified, reviewed for acceptance, or attributed to D3.
The current authorization explicitly includes the narrow IPC field/converter
change (`D:31-33`) and UI source (`D:35-36`).

Independently reconstructed the four-file delta from `ED/baseline/` to current
files and compared it byte-for-byte with `ED/delta.patch`. SHA-256:
`a82909c24ae7d6bbdc2fc8abb34cf6872d078e4aebae43b827365bf4dd9356ea`.
All six original geometry diff blocks match the producer's entry baseline.
Five remain byte-identical in the current diff; the host has only the reviewed
additive receipt/test delta. Thus the original patch identity and acceptance
remain valid for geometry, not as a hash of the now-extended host file.

Rechecked the actual call chain: bounds IPC (`I:507-547`) dispatches real host
render; `H:1730-1783` resolves the actual target policy before font metrics and
`prepare_session_layout`; `H:969-976` stores layout, canonical bounds and metrics
together. Scheduled render (`H:338-371`), focus (`H:1786-1827`) and direct render
carry those bounds to the host. `H:1933-1963` uses them for child viewport and
renderer density and uses layout dimensions for the configured buffer.
Linux `platform/linux.rs:300-305,352-354,382-392` selects only a real Wayland
child and reconverts canonical geometry for the unchanged protocol calls at
`platform/wayland_child.rs:372-393`. The original shared-edge/idempotence and
non-Wayland identity tests remain unchanged; no global quantization was added.

Both original REDs were rechecked against raw assertion output and original
journal `J:36-43,50-51`: one executed numeric failure each, `1 != 0` for width
801/scale 2 and `(600,450) != (800,600)`, exit 101, before production at
15:17:42Z. The incomplete timeout is still excluded. Full original related
contract, host, child, Mac check/build and cleanup logs were inspected again:
7+12, 27 and 5 tests passed; check/build ended exit 0. Original acceptance is
not reopened merely because a separately scoped delta was subsequently added.

### Delta code and behavioral criteria

| Criterion | Evidence-backed disposition |
| --- | --- |
| Effective scale comes from stored presentation geometry (`D:44-49,75-77`) | **ACCEPT.** `H:148-168` extracts `logical_bounds.map(|bounds| bounds.scale_factor)`, not raw window DPR or another formula. `H:1707-1727` takes the session's stored bounds under its existing guard. The regression at `H:2405-2437` resolves raw 1.5 to Wayland 2, stores it through actual `prepare_session_layout`, calls actual `get_receipt`, and asserts stored 2, receipt `Some(2.0)` and unchanged physical cells 16x32. MockRuntime supplies only the unused-window API boundary; terminal/session geometry and receipt code are real. |
| Every host construction path retains scale | **ACCEPT at source.** Direct receipt passes `session.logical_bounds` (`H:1725`); no-host focus passes the already-loaded bounds (`H:1824`); dropped and presented frames both pass `self.logical_bounds` (`H:1998,2026`). Focus/scheduled paths first copy session geometry onto the host. A search of all production receipt constructors found no unpatched constructor. Native acquisition/presentation branches were source-traced, not dynamically exercised by the receipt unit test. |
| Scale reaches every wire receipt (`D:31-33,75-77`) | **ACCEPT at source and converter contract.** Optional field/serde defaults at `I:26-41` and the manual copy at `I:1424-1441` retain it. Set-bounds/focus/preedit (`I:533-534,566-567,600-601`), input (`I:820-821`), paste (`I:1084-1085`), both scroll branches (`I:859-875`), and optional nested mouse receipt (`I:1266-1301`) reach the same converter. Existing cases yielding no mouse receipt still yield none. `I:1449-1480` tests absent, 1.5 and 2 scales, both presentation statuses, omission of absent field and deserialization round-trip. Only float-containing types lose `Eq`; routing/errors are unchanged. |
| No new lock acquisition order (`D:75-77`) | **ACCEPT.** The delta supplies geometry already available at each constructor; it introduces no lock call. Original geometry lock behavior is unchanged. |
| Actual focus-sink anchor at raw DPR 1.5/native 2 (`D:51-64`) | **ACCEPT at component boundary.** `U:592-611` divides receipt pixel metrics by effective scale, and `U:1950-1963` applies the resulting state to the actual textarea. `L:191-223` observes its real style after an event-gated bounds receipt: cursor (2,3), cells 16x32 gives local CSS (16,48), size 8x16. This assertion executed in RED and GREEN; it is not a detached formula or source-string test. |
| Raw geometry DPR and physical metrics remain unchanged (`D:59-64`) | **ACCEPT.** `U:664-687,693-717,1594-1613` retains measured DPR in attach/bounds requests; only receipt-to-CSS conversion changes. The component tests assert both requests carry raw DPR. `cellSizeRef` still stores unrounded physical metrics. Mouse continues sending CSS-local coordinates (`U:983-1025`); no mouse defect or rewrite is inferred. |
| Ordinary/missing-scale compatibility (`D:61-64,68-70`) | **ACCEPT.** Component cases cover raw/native 2, raw 1.5 with undefined scale and with null scale. Ordinary produces (16,48), 8x16; legacy/null retain (21.333...,64), 10.666...x21.333.... Host constructor test supplies `None`; wire test confirms absent-field omission and legacy decode. Browser separately exercises ordinary and omitted scale, not null. |
| D2 ownership guards retained (`D:39-40,68-70`) | **ACCEPT.** The UI production delta touches only receipt typing and conversion. Commit-scoped invalidation (`U:538-545`), queued attach guard (`U:707`), input publication/recovery checks (`U:752-796`) remain. No existing lifecycle test was removed or rewritten. The full focused UI suite independently passed 171 tests, including departure/recovery/stale-result/returning-owner behavior. |
| Deterministic, integrated new coverage (`D:57-64,68-73`) | **ACCEPT for the new tests.** Bounds promises are armed before component render and settled under React `act`, with the test-runner failure timeout. No new polling, sleep, prose pinning or AST/source matching. The Rust test uses actual storage/receipt APIs. Browser uses the actual component plus controlled IPC, not a mocked component or native Wayland backend. |

### RED ordering: qualified, not silently relabeled

`JD:28-29` records the successful scaffold at **18:51:36Z**: internal optional
field, internal `Eq` removal, constructor hardcoded to `None`, required IPC
test literal, and behavioral tests. The earlier ambiguous patch attempt at
`JD:24-25` failed and is not counted as successful preparation.
`JD:30-32` runs both tests at **18:51:50Z**, returning by **18:52:08Z**:

- `ED/RED-rust.log:71-88`: one test executed, stored-scale assertion passed,
  receipt assertion failed `None != Some(2.0)`; journal confirms exit 101.
- `ED/RED-ui.log:5-56`: one actual component test failed because left was
  `21.333333333333332px`, expected `16px`; exit 1. Other tests were filtered by
  the requested name, not deleted or skipped to make a suite pass.

Scale propagation, the wire field/converter, and UI correction follow at
**18:52:54Z** (`JD:41-42`). Same-test GREEN follows at **18:53:10Z-18:53:28Z**
(`JD:43,48`, `ED/GREEN-rust.log`, `ED/GREEN-ui.log`), one pass each. The
fractional test inputs and assertions were not weakened; null compatibility
was added. Subsequent Rust changes at `JD:61` are test formatting only.

**Disposition:** right-reason behavioral RED before propagation/UI correction
is established. The stronger literal criterion in `D:42,59` (before production
edits) is **NOT MET** for Rust because of the scaffold. `wayland-ime-repair.md`
discloses this accurately. Accepting that narrow compile scaffold as the RED
baseline requires an explicit lead evidence-policy exception; a later rerun
cannot retroactively establish the original ordering. No source fix is
recommended for this process exception, and it does not invalidate the actual
GREEN behavior or original geometry REDs.

### Independent executable verification and related receipts

Executed from `W/` once each, **19:01:57Z-19:02:10Z**, with
`CARGO_BUILD_JOBS=8`:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::receipt_reports_effective_presentation_scale -- --exact --nocapture
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx src/components/NativeTerminalPane.test.tsx src/lib/nativeTerminalLifecycle.test.ts
```

Both Wayland tests ran one, passed, filtered 11, exit 0: 802x600/scale 2 and
matching host/child 800x600 with grid 40x15. The receipt test ran one, passed,
filtered 598, exit 0. UI ran in parallel and passed **171/171** (139+25+7),
including the named fractional regression and all compatibility/D2 cases;
no separate duplicate exact UI run was necessary. Seven existing Rust warnings
and the expected attach-rejection test's stderr remained visible.

The producer's following logs were read in full, not represented as independent
reruns: `ED/GREEN-host.log` 28 passed; `GREEN-ipc.log` 14 passed;
`GREEN-geometry.log` 5+7+12 passed; `GREEN-ui-focused.log` 171 passed;
`ui-build.log` TypeScript/Vite passed; `cargo-check.log` and `cargo-build.log`
Mac default-feature checks/build passed (7.59s/18.50s); and
`final-lib-test-compile.log` passed after test formatting. `JD:51-54` confirms
the related executions. All four current delta files also returned **No
diagnostics found** in this verifier's fresh parallel LSP checks, closing the
producer's final Rust diagnostic-refresh cancellation/timeout limitation.

The extra producer `cargo check --tests` remains **FAILED**, not a delta
regression: `ED/final-cargo-check-tests.log:97-107` reports
`couldn't read tests/../../.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1:
No such file or directory (os error 2)`, at
`W/src-tauri/tests/windows_edge_probe_contract.rs:1-3`.
Independently confirmed that tracked test has no working diff and the fixture
is absent. No failed test was edited, suppressed, or skipped. This prevents an
all-tests-compilation GREEN claim, not the scoped receipt/geometry acceptance.

### Browser artifacts and remaining native proof

**ACCEPT controlled-IPC browser DOM/action evidence (`D:71-73`).** Read
`ED/browser-entry.jsx`, `browser-qa.mjs`, all three JSON action/geometry logs,
`browser-run.log` and `browser-cleanup.json`. The harness imports the actual
worktree component, injects JS DPR, clicks its viewport and types `x`. Its
input promise is armed before the action, then awaited with a bounded failure
timeout and React settling; readiness is also event-bounded. It never creates
a native Wayland target or calls the Rust receipt producer.

Fractional and ordinary logs record viewport origin (25,90), textarea origin
(41,138), size 8x16: actual DOM-local (16,48). Both retain their respective raw
DPR in attach and bounds. Legacy retains fractional CSS style values; its
actual DOM rect is browser-quantized (46.328125,154), 10.65625x21.328125, not
an exact infinite-precision formula. Each log includes the real input action.
The three PNGs exist with valid PNG chunks/CRCs and dimensions 1280x1100.
Image read was attempted but this model's tool response omitted the image as
unsupported: **no independent visual inspection is claimed**. Screenshot
artifact presence is accepted; their pixels are not proof of native IME.

**PENDING native acceptance (`review.md:458`, `D:80`).** The earlier native
checklist remains in force: authorized Linux build/link and actual Wayland
child (not X11/fallback), two numbered panes, actual scale/buffer extents and
compositor diagnostics, odd/even-width resize at real DPR 2, fractional DPR
only if genuinely reported, shared edges/full content, no `invalid_size`,
completed-frame and cleanup receipts. Add real interactive IME composition and
candidate-window placement at the rendered cursor, including raw 1.5/native 2
when available, with stored scale/wire receipt/textarea geometry correlated to
that same native frame. MockRuntime, jsdom and injected browser DPR do not close
any native-display criterion. The report-only/macOS boundary cannot supply
this proof; an authorized native Wayland runner is required.

### Verifier cleanup and report boundary

`ED/browser-cleanup.json` records three closed views, zero retained event
callbacks for each unmounted component and stopped server. Harness `finally`
blocks close only owned views/server; no global close is present. At 19:04:14Z,
independent process inspection found no matching exact test/Cargo/browser-QA
command, and `lsof` found no listener on the recorded producer port 50025.
The original cleanup log remains accepted with its earlier stated limits.

No monitor tool/executable was available. Requested executions used bounded
foreground calls (about 13 seconds Rust, 4.49 seconds UI), without polling,
sleeps, retries or persistent processes. Raw verifier commands/results reside
in `.omo/senpi-task/children/st_01a07818/sessions/st_01a07818/2026-09-06T19-00-53-669Z_01a07818-4d24-7c52-8044-dab6e6f71f69.jsonl`.
Only this report was appended via `apply_patch`. No source/test edits, staging,
commits, desktop/browser control, native app launch, daemon or remote writes
were performed. Requested test build caches are retained.

## Final receipt RED recovery verdict - st_01a078d6, 2026-09-06

**ACCEPT the new pristine-production wire/component RED recovery and final
receipt/IME delta. RETAIN original D3 source/portable geometry acceptance.
Native Wayland acceptance remains PENDING; overall rendering is NOT APPROVED.**
No delta code regression or remaining recovery-process blocker was established.
This is a new accepted attempt under `RECEIPT_RED_REDO.md`, not a waiver or a
retroactive correction of the earlier scaffolded attempt. The historical
qualification and all earlier report text above remain intact. The earlier
request for an evidence-policy exception is not needed for this new attempt.

Verifier session: `01a078d6-798c-7abd-ae78-34896121ae7b`, arm64 macOS.
`W/`, `N/`, `E/`, `ED/`, `H`, `I`, `U`, `L` and `D` retain the preceding
addendum's definitions. `ER/` means `ED/redo/`. Recovery journal `JR` is:
`.omo/senpi-task/children/st_01a078bc/sessions/st_01a078bc/2026-09-06T22-00-22-309Z_01a078bc-9e25-7907-b8bc-b21670773833.jsonl`.
Journal line numbers below are one-based; timestamps are UTC. Source references
refer to the final inspected repair bytes at HEAD
`c349e3a6ffa1ff36cef30ac4d66dd78250e3f428`, not main-tree source.

### Recovery criteria and independently checked chronology

Read the updated `wayland-ime-repair.md`, both delta contracts, the full scoped
working diff, recorded test-only diff, source snapshots, raw RED/GREEN logs and
the recovery's actual patch/execution tool calls. The following dispositions
implement `RECEIPT_RED_REDO.md` items 1-6 without relaxing their ordering.

| Criterion | Disposition and evidence |
| --- | --- |
| Save the existing passing owned delta (item 1) | **ACCEPT.** `JR:25-26`, 22:01:38Z-22:01:39Z, saves the four files and checks the baseline-to-saved diff against the earlier `ED/delta.patch`. Independently verified every saved file against `ER/entry-identity.json`. HEAD and original D3/Windows hashes are recorded separately. |
| Restore only the owned delta, preserving geometry/D2/Windows (item 2) | **ACCEPT.** The same tool call applies `ER/restore-owned.apply_patch`, a reverse of the owned delta rather than a HEAD checkout, then verifies all four files against `ED/baseline/`. Independently reconstructed the original geometry host from `bce59b4` plus `E/owned-diff.patch`; it equals that receipt-entry baseline. All six original geometry patch blocks match `ED/baseline/worktree.patch`; the other five still match the current working diff exactly. |
| Baseline-compatible wire test; no production scaffold (items 3-4) | **ACCEPT.** `JR:32-33`, 22:02:25Z-22:02:26Z, adds only the wire test and restores the four UI cases. Independently reconstructed `ER/RED-test-only.patch` from baseline and `ER/red-source/`: only IPC test-module and lifecycle-test additions exist. Host and UI production are byte-identical to entry; IPC production through the converter is also byte-identical. Old receipt structs retain `Eq` and have no effective-scale field. The old code compiles in the actual RED run. |
| Wire RED really executes and fails at the intended JSON boundary (items 3-4) | **ACCEPT.** `JR:34-35`, starts 22:02:43Z and returns by 22:03:04Z; `ER/RED-wire.log:71-95` records successful compilation, one executed test, **0 passed / 1 failed**, exit 101. The stored-scale and physical-cell assertions succeed before `json["effectiveScaleFactor"]` fails **Null != 2.0** at RED `I:1483`. This is neither a compiler failure nor a zero-match filter. |
| Fractional actual-component RED precedes reapplication (item 4) | **ACCEPT.** `JR:34,36`, same pre-reapplication wave; `ER/RED-ui.log` records **1 failed / 24 name-filtered**, exit 1. Actual textarea left is **21.333333333333332px**, expected **16px**, at `L:213`. Production still divides by raw DPR; `JR:37-40` reads that old conversion and the old Rust structs after both REDs. |
| Forward reapplication only after both REDs; retain assertions (item 5) | **ACCEPT.** `JR:41-42`, 22:03:47Z-22:03:48Z, verifies unchanged RED hashes, applies `ER/reapply-fix.apply_patch`, then checks final files equal the saved fix with only the new wire test added. Independently repeated that byte comparison. The exact wire-test body is identical in RED and GREEN; original host getter, serde, UI compatibility and D2 tests are retained. |
| Identical commands GREEN and final-source validation (item 6) | **ACCEPT.** `JR:43-49`, 22:04:11Z-22:04:35Z: same wire and fractional UI commands pass one selected test each, exit 0 (`ER/GREEN-wire.log`, `GREEN-ui.log`). `JR:58-61`, 22:05:59Z-22:07:53Z, executes related suites, normal checks/builds and fresh browser proof after reapplication. Final hashes match the current files and all inspected final receipts; no stale source is substituted. |

Independently reconstructed `ER/final-delta.patch` byte-for-byte from recorded
entry to current source. SHA-256:
`75c1e5167f2802a35ad4713ab062f61403d0e1a1ae83bf29751a6c1f3d028734`.
All four current file hashes match `ER/GREEN-source-hashes.json`; all captured
foreign/original geometry hashes match `ER/entry-identity.json`. The only
committed change from `bce59b4` to current HEAD is the Windows implementation.
Its hash is unchanged; Windows implementation acceptance is not attributed to
this verifier or the receipt worker.

### Final source behavior and preservation

| Criterion | Evidence-backed disposition |
| --- | --- |
| Real host geometry drives buffer/grid and placement (`review.md` D3/Q4) | **RETAIN ACCEPT; rechecked.** `H:1730-1783` gets the actual target, resolves its policy before deriving cells, and stores canonical bounds/layout/metrics through `prepare_session_layout` (`H:870-978`). Scheduled and focus paths consume those stored values (`H:338-371,1786-1827`); `render_snapshot` uses the same scale for density and layout extents for the configured buffer (`H:1933-1963`). Linux selects the policy only for a created Wayland child (`platform/linux.rs:300-305,352-354`), and canonical bounds reach actual position/scale protocol calls (`:382-392`, `platform/wayland_child.rs:372-393`). These Linux calls are source-traced, not executed here. |
| Shared edges, warm attach and non-Wayland behavior | **RETAIN ACCEPT.** The receipt delta does not alter the original edge-snapping policy, attach paths, layout or platform source. Shared-edge/idempotence/session-grid and default-policy tests are unchanged (`T/native_terminal_wayland_subsurface_contract.rs:66-181`) and pass in the fresh 42-test geometry run. Default identity still preserves 601x451 for 400.5x300.5 at DPR 1.5; no Windows/macOS/X11 quantization was added. |
| Effective native scale originates in stored geometry (`D`, receipt-path criterion) | **ACCEPT.** Shared constructor `H:148-168` extracts only `logical_bounds.scale_factor`. Direct getter passes session bounds under the existing sessions guard (`H:1706-1727`); no-host focus passes the already-loaded bounds (`H:1818-1825`); dropped and presented frames both pass host bounds (`H:1992-1999,2020-2027`). Scheduled/focus renders first copy session geometry to the host. No receipt-delta lock acquisition or raw-DPR recomputation is introduced. |
| Every serialized receipt retains scale | **ACCEPT at source plus real getter-to-wire execution.** `I:26-41` adds the optional/defaulted/omitted-when-absent field, and `I:1424-1441` copies it. Bounds, focus, preedit (`I:532-534,565-567,599-601`), input/paste (`:819-821,1083-1085`), both scroll branches (`:860-875`) and nested mouse receipt (`:1266-1301`) reach that converter. Existing branches returning no mouse receipt remain unchanged. Only float-containing receipt types lose `Eq`. Native presented/dropped/focus acquisition paths were traced, not separately exercised by MockRuntime. |
| No disconnected formula or source-pinning proof | **ACCEPT.** New wire test `I:1448-1487` resolves DPR 1.5 via the production Wayland policy, stores real terminal/session layout with cells 16x32, checks stored 2, calls real `get_receipt`, then the private converter and serde. MockRuntime supplies the unused window boundary, not a fabricated receipt. Teardown precedes the expected RED assertion. The test checks machine JSON and actual state; no AST/string source pinning is used. |
| Actual component anchor, ordinary and missing-scale behavior (`D`, GREEN criteria) | **ACCEPT at component boundary.** `U:592-611` uses `receipt.effectiveScaleFactor ?? scaleFactorRef.current`; `U:1950-1963` sets the real textarea style. `L:191-223` checks local CSS (16,48), size 8x16 at raw 1.5/native 2, equal scale 2, and undefined/null fallback. Legacy/null preserve (21.333...,64), size 10.666...x21.333.... Promises are armed before triggering the bounds request and settled under React `act`; no added sleep/polling or prose pinning. |
| D2 ownership, raw request DPR, physical cells and mouse behavior | **ACCEPT preserved.** UI production diff is limited to receipt typing and pixel-to-CSS conversion. Owner invalidation (`U:538-545`), queued-attach check (`:707`), and input publication/recovery/retry guards (`:752-796`) are retained. `measureGeometry` and attach/bounds dispatch (`:664-717,1594-1613`) still send raw DPR; tests and browser actions assert it. Cell metrics stay physical and unrounded. CSS-local mouse routing is unchanged; no mouse regression is inferred from the former IME gap. |

Original numeric RED evidence remains valid. Rechecked raw original failures
and `J:36-43,50-51`: width 801/scale 2 fails remainder **1 != 0**, and host
**600x450 != child 800x600**, one executed failure/exit 101 each, before the
first production patch at **15:17:42Z**. The incomplete timeout attempt remains
excluded. Original source/evidence acceptance is retained, not replaced by the
receipt tests.

### Independent execution and full related evidence

Executed from `W/`, once per command, with `CARGO_BUILD_JOBS=8`:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract buffer_extent_is_divisible_by_scale_when_logical_extent_is_fractional -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract host_extent_matches_wayland_buffer_extent_at_fractional_scale -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal::tests::wire_receipt_reports_effective_presentation_scale -- --exact --nocapture
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx src/components/NativeTerminalPane.test.tsx src/lib/nativeTerminalLifecycle.test.ts
```

Rust ran **22:30:17Z-22:30:35Z**: each command executed one test and passed,
exit 0. Geometry outputs are **802x600/scale 2**, then host and child
**800x600**, grid **40x15**; each filters 11. Exact wire filters 599.
UI ran in parallel **22:30:17Z-22:30:42Z**, passing **171/171** (139+25+7),
exit 0, including fractional, equal-scale, undefined/null and D2 cases in one
run. No retry or duplicate exact UI run was needed.

Read these fresh producer logs **in full**; they are inspected producer
executions, not additional verifier reruns:

| `ER/` evidence | Result on identified final source |
| --- | --- |
| `GREEN-wire.log`, `GREEN-ui.log`, `GREEN-host-regression.log` | One selected pass each; unchanged exact RED wire/UI commands. |
| `GREEN-ipc.log`, `GREEN-host.log` | 15 IPC and 28 host tests passed. |
| `GREEN-geometry.log` | Child 5 + composition 7 + host contract 18 + Wayland 12 = **42 passed**. |
| `GREEN-ui-focused.log` | **171 passed** in one invocation. |
| `diagnostics.log`, `JR:43-47` | All four final delta files return no diagnostics before builds. |
| `cargo-check.log`, `cargo-build.log` | Normal macOS default-feature check/build pass, 6.39s/12.82s. Not Linux-target builds. |
| `ui-build.log` | TypeScript and Vite pass; 1863 modules transformed. |
| `scope-check.log` plus verifier reconstruction | Source identities and foreign hashes match; worktree `git diff --check` passes. |

The seven existing Rust unused/dead-code warnings remain visible in RED,
GREEN and this verifier's runs; expected attach-rejection test stderr also
remains visible. No warning/test failure was suppressed. The earlier broad
`cargo check --tests` failure is still **not GREEN**: read
`ED/final-cargo-check-tests.log` in full and confirmed unchanged
`T/windows_edge_probe_contract.rs:1-3` still references missing
`.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1`.
The recorded error is **No such file or directory (os error 2)**; it is a
pre-existing fixture limitation, not a receipt delta regression. This broad
command was not rerun or waived into an all-tests-compilation pass.

### Fresh browser artifacts, native blocker and cleanup

**ACCEPT fresh actual-component/controlled-IPC DOM evidence, not native GUI.**
Read both `ER/browser-*.mjs`/`.jsx` harness files, the three JSON action/geometry
logs, `browser-run.log` and cleanup. The harness imports the real component,
injects JS DPR, supplies controlled wire-shaped receipts, clicks the viewport
and types `x`. Input/readiness signals are pre-armed and failure-bounded; there
are no sleep/polling gates. `JR:58,61` records fresh execution after the UI
build. Independently checked **all 320** recorded harness/UI/CSS input hashes
against current files; the two harness files equal their earlier versions.

- Fractional raw 1.5/native 2 and ordinary 2/2 both record local anchor (16,48),
  size 8x16, viewport origin (25,90) and actual textarea origin (41,138).
- Missing-scale legacy records style (21.333333,64), size 10.666667x21.333333;
  actual DOM layout quantizes its rect to (46.328125,154), 10.65625x21.328125.
- All three retain raw DPR in attach/bounds actions and include real input `x`.
  PNG artifacts are valid 1280x1100 files with checked chunk CRCs. Fractional
  image read returned unsupported-model omission: no visual inspection is
  claimed. The harness makes the normally invisible sink red with test CSS.

**Native acceptance remains PENDING**, exactly at `review.md` D3 and native
surface evidence requirement (`:458`), `D`'s native screenshots/interactive
acceptance criterion, and `RECEIPT_RED_REDO.md`'s final native-GUI boundary.
The earlier six-item native checklist remains required: identified Linux
build/link, actual Wayland child rather than X11/fallback, two numbered panes,
correlated raw/canonical bounds, real scale/buffer/grid/frame completion and
compositor diagnostics, odd/even-width resize at real DPR 2, shared edges/full
content without gaps/overlap or `invalid_size`, warm reattach/scheduled output,
and cleanup. Fractional DPR 1.5 must be genuinely reported or marked
unexercised. Add interactive IME composition/candidate-window placement at the
rendered cursor, correlated with the same frame's stored scale, serialized
receipt and textarea geometry. MockRuntime, jsdom and injected-DPR WebViews
prove none of those native-display criteria. An authorized Linux Wayland
runner is the remaining prerequisite; no source-only approval closes it.

`ER/browser-cleanup.json` records three closed views, zero retained callbacks
for each unmounted component and stopped server on port 64271. Independent
`lsof` inspection at **22:33:51Z** found no listener there; exact process
inspection found no matching Wayland/wire Cargo test or redo browser harness.
All verifier commands exited. No monitor tool/executable was available, so
the short checks used bounded foreground calls, with no polling or sleeps.
No persistent resource, desktop/browser interaction, source/test edit, index
operation, commit or remote write was performed by this verifier. Only this
report was appended via `apply_patch`; requested test caches are retained.
Raw verifier evidence is in
`.omo/senpi-task/children/st_01a078d6/sessions/st_01a078d6/2026-09-06T22-28-36-877Z_01a078d6-798c-7abd-ae78-34896121ae7b.jsonl`.

Report diagnostics were requested but unavailable: no Markdown LSP server is
configured. The final report has no trailing whitespace and ends with a
newline; the worktree diff whitespace check passes. No new prose-pinning test
was added for this report-only change.
