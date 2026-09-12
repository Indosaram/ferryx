# V04 independent review

Reviewer: recovery child `st_01a093c5`, 2026-09-12. Base: `0ce84a1`.
Scope: eleven Rust/Cargo files in `V04-command-request.json`, including the new
untracked `src-tauri/src/native_terminal/viewport.rs`. UI changes are excluded.

## Verdict

**PASS for the narrowly scoped VT CPU/presentation separation and observed local
macOS regression gates, with residual gates below. Not full-plan, cross-platform,
security-completeness, resource-boundedness, or native-desktop-pixel approval.**

No new blocking defect was found in the scoped diff. The repair removes the
no-default grid socket's unconditional early close by making the existing real
Ghostty engine and handler available without GPU presentation. It does not add a
second emulator or substitute remote grid rendering for desktop native surfaces.
Existing security/resource weaknesses noted below are inherited, not repaired or
erased by the passing logs.

This review ran no builds, tests, installs, diagnostics that could trigger builds,
daemon/desktop actions, or commits. Only read-only source/evidence inspections and
static comparisons were performed; this report was created with `apply_patch`.
The suspended worker's workloads/resources were not resumed or duplicated.

## Independent source verification

All eleven current file SHA-256 values match the ownership manifest in
`V04-command-request.json`. The tracked Rust/Cargo diff contains ten files; the
eleventh is the new viewport file, which must accompany the eventual patch.
`git diff --check -- src-tauri` completed successfully.

- `Cargo.toml:49-58` retains `default = ["native-terminal"]` and the five optional
  presentation dependencies: wgpu, raw-window-handle, winit, bytemuck and pollster.
  Its sole change is a comment. Tauri, audio and target-specific libraries remain
  unconditional: this is not a GUI-dependency-free CLI/relay packaging split.
- `build.rs:35-40` unconditionally invokes the existing Ghostty build helper,
  retaining its error exit. Windows manifest logic is untouched. No-default
  builds now intentionally require the real VT toolchain and archive.
- `lib.rs` exposes `native_terminal` unconditionally. `native_terminal/mod.rs`
  gates child surfaces, composition, input, platform, renderer, scroll overlays,
  surface errors/host/snapshot and wheel modules/exports on `native-terminal`.
  CPU engine, snapshots, FFI, selection and viewport remain unconditional.
  Existing platform-specific cfgs remain behind the presentation boundary.
- Static extraction comparisons prove `ScrollViewport`, `ScrollbarState`,
  `scroll_viewport` and `query_scrollbar` were copied exactly from base `scroll.rs`
  to `viewport.rs`. The C tagged-union construction and error propagation are
  unchanged. Engine, terminal and selection changes only redirect the relevant
  imports/call. Scroll retains presentation type re-exports and its byte-identical
  test module.
- A normalized comparison proves `remote/mirror.rs` equals base after removing
  native feature guards and changing its test guard to `cfg(test)`. Real engine
  feed, geometry-aware history replay, line diff baseline, wide-cell handling,
  attributes, cursor and scrolling semantics are preserved. All nine existing
  CPU mirror tests now run in no-default builds.
- The full `handle_terminal_grid_socket` body is byte-identical to base.
  `server.rs` changes are limited to imports/helper gates and dispatch to that
  existing handler. Authentication precedes attachment/upgrade: token validation,
  revocation subscription, active desktop selection admission and the outer
  `while_device_authorized` fence remain. Raw socket logic, recovery and A03
  authority logic are not edited. `remote/auth.rs`, `remote/protocol.rs`,
  `remote/tests.rs` and `remote/machine_auth_tests.rs` independently compare
  byte-identical to base. No tests/assertions were removed, ignored or weakened.
- `src-tauri/native_terminal/build_ghostty.rs`, FFI declarations/types,
  lifecycle and platform module independently compare byte-identical to base.
  The helper requires Ghostty SHA
  `6a508fd5e34c7e222c052a6d00bb3891ff3feace` and Zig version prefix `0.16.0`, maps
  explicit macOS/Linux/Windows triples, rejects unmapped targets, and emits
  static link metadata. VT-only Zig flags and OUT_DIR-local prefix/caches are
  unchanged. No new ABI or fallback/link override is introduced. This verifies
  preservation of the pin/check mechanism, not a fresh vendor cleanliness audit;
  the version check is a prefix check, not strict version equality.

Two preliminary helper-path reads/comparisons used nonexistent paths. Inspection
of `build.rs` located the actual helper at `src-tauri/native_terminal/build_ghostty.rs`;
the corrected read and equality comparison succeeded. No failed inspection was
treated as successful validation. No git-backed Cargo.lock equality is claimed.

## Existing execution evidence inspected

Paths below are under `docs/evidence/paired-daemon/`. These are observed retained
outputs, not executions by this reviewer. Command/feature attribution follows
the command request and parent execution record; the logs do not themselves
contain a complete shell command/environment receipt or source digest.

| Evidence | Observed result |
| --- | --- |
| `V04-headless-remote.log:128-372` | 210 passed, 0 failed, 0 ignored. All nine mirror tests at 162-170; the four grid attach/viewport/live-output/resize regressions at 337-347 and daemon/worktree selection/control at 367 explicitly pass. |
| `V04-core-contracts.log:139-214` | Build contracts 19, real build-info FFI 1, capability contracts 7, engine contracts 22: 49 total passed, no failures/ignores. |
| `V04-cpu-native.log:128-158` | 23 passed, 0 failed, 0 ignored. This is the `native_terminal::` filter inventory, not a claim that all 23 are exclusively CPU engine tests. |
| `V04-headless-binaries.log:138` | Successful dev build completion for parent-reported CLI/relay command. |
| `V04-a03-cli-regression.log:129-170` | 18 passed; nested fixture's 1 pass is included, not an extra nineteenth test. Real CLI PIDs exit 0 and are reaped. Mirror/machine authority receipts show anonymous=401 and revoked=401; fixture runtime/socket/private-root cleanup is recorded. |
| `V04-default-remote.log:317-587` | 216 passed, 0 failed, 0 ignored. |
| `V04-default-native.log:138-327` | 165 passed, 0 failed, 0 ignored. Line 236 identifies Apple M4 Max, Metal. Renderer/surface contracts run; this is not evidence of pixels in a launched desktop window. |
| `V04-default-desktop-link.log:143` | Successful dev build completion; parent reports binary link exit 0. No launch/render receipt. |

Headless security regression output explicitly includes revocation before raw
upgrade, during attachment, cancellation of pending raw input, and device-local
socket closure (`V04-headless-remote.log:277-299`). The logs contain compiler
warnings, not a warning-clean result: unused imports/variables, unnecessary unsafe
blocks and dead code. Much is in untouched code; some CPU code becomes newly
compiled under no-default, so not every warning is asserted to predate that
configuration. No warning was suppressed by this diff. No failing test result
appears in the listed final gate outputs.

## Findings by severity

### High - inherited authorization gap, newly reachable in no-default grid mode

`remote/server.rs`'s grid `ClientControlMessage::Resize` branch calls backend
resize after geometry validation without checking `can_control`. Binary input and
interrupt signals do check it; initial query-geometry resize also checks Control.
Thus a View device admitted to the active grid socket can reach the existing PTY
resize call. This handler is byte-identical to base, so V04 does not weaken its
code, but extends its reachability to no-default builds. Do not interpret
"authentication unchanged" or A03 green as proof that every grid operation
enforces View/Control. This is a source finding, not an executed exploit. Track
separate authorization remediation and a real View-device grid regression.

### Medium - inherited unbounded slow-consumer queue

`handle_terminal_grid_socket` creates `mpsc::unbounded_channel::<Message>()` and
queues serialized grid frames while the sole writer awaits network sends. The
33 ms feed cadence bounds frequency, not retained bytes; scroll/resize can also
enqueue frames. There is no explicit queue memory bound/backpressure in this
handler. Per-connection mirror access remains mutex-serialized, and pinned local
futures are dropped when the enclosing select/auth fence ends; no detached task
is added. Ghostty lifecycle retains line-governed scrollback with its byte limit
unset. These mechanisms do not establish a hard per-client memory ceiling.
This risk existed with default features and is now shared by headless grid mode;
bounded-load/slow-reader acceptance remains unproven.

### Medium - inherited timing-sensitive test fixtures

`remote/tests.rs:1935-2003` uses `sleep 30` to hold PTYs alive, and the live-output
test requires the next grid frame to contain the entire marker. PTY chunking and
the production 33 ms batching can expose a partial marker. These are existing
test reliability defects, not weakened assertions or evidence of a failing V04
run. One green run proves the recorded run, not determinism. Separate fixture
repair should use explicit lifetime/EOF rendezvous and bounded semantic marker
completion rather than sleep extensions, polling delays or retries.

### Low - evidence completeness and build-resource caveats

No feature-tree receipt or pre-build LSP diagnostic receipt is present in the
reviewed V04 logs. Static Cargo/gate preservation is proven; a fully resolved
no-default dependency graph is not. The helper isolates caches in OUT_DIR but
does not pass a Zig job cap; Cargo's requested four-job cap alone is not proof of
a four-thread Zig subprocess or the requested 13 GiB resource ceiling. Logs do
not establish peak memory or global scheduling history. A03 cleanup evidence is
specific and positive, not proof that every workload cleaned every resource.

## Exact residual gates

1. **Linux and Windows:** retain target-native build/link and applicable real VT,
   remote grid, ABI/engine and default-presentation results on each supported
   release target before claiming platform support. Verify Windows static archive
   system-symbol resolution and preserve manifest behavior. macOS mapping tests
   are not Windows/Linux linker/runtime evidence. Existing Unix shell fixtures
   require honest target-specific reporting, not cfg-hiding or skipped failures.
2. **Actual native desktop pixels:** on an isolated default-feature desktop,
   verify real native terminal attachment (including paired daemon), visible
   output, input, resize, scroll and selection through its actual window surface.
   Keep platform-native presentation; offscreen Metal tests and debug linking
   cannot discharge this gate. No UI diff is approved by this report.
3. **Evidence closure:** retain the resolved no-default feature graph confirming
   this repair does not enable optional presentation dependencies; record
   diagnostic availability/results rather than implying an LSP-clean result.
   Bind execution receipts to the eleven-file manifest and command/environment
   where stronger provenance is required. Include untracked `viewport.rs` in
   delivery. Existing outputs need not be rerun merely for this review.
4. **Broader security/resource/reliability claims:** close the inherited View
   resize admission, slow-consumer memory bound, and timing-dependent fixture
   findings with separately scoped fixes/evidence before making those claims.
   They do not invalidate the narrow source-preservation verdict; they prevent
   treating it as comprehensive production hardening approval.

The local source and retained green results support the intended repair. Missing
platform execution, desktop pixels and broader guarantees remain explicitly open.
