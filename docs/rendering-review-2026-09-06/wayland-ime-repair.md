# D3 receipt / IME effective-scale repair

Initial task `st_01a0780d`; test-first recovery `st_01a078bc`, review date
2026-09-06. Implemented in
`/Users/indo/code/project/orca-lite-rendering-20260906`, on `c349e3a` with
`bce59b4` immediately below it. No staging or commit occurred.

The final fix has now been reapplied after a fresh, baseline-compatible wire and
component RED. The recovery chronology and fresh verification are recorded below
under **Pristine-production RED recovery**. The earlier scaffolded attempt and
its artifacts remain historical; they are not relabeled as pristine RED.

## Delivered delta

Native receipts carry optional `effective_scale_factor` from stored presentation
bounds. The manual IPC converter preserves it as `effectiveScaleFactor`; absent
geometry omits the wire field. Deserialization accepts older receipts without
that field. Only float-containing receipt types lose `Eq`; `PartialEq` remains.

`NativeTerminalPane.updateImeAnchor` uses that value only for receipt pixel-to-CSS
conversion. Missing/null scale falls back to raw WebView DPR. Attach and bounds
requests still carry raw DPR. No metric rounding, mouse routing, host geometry
policy, or D2 owner guard changed.

Raw DPR 1.5, stored scale 2, cells 16x32, cursor (2,3) now gives the actual focus
sink CSS left 16, top 48, width 8, height 16.

Evidence root (`E`):
`/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/D3/receipt-delta`.
`E/delta.patch` is the minimal four-file delta relative to entry, not HEAD (which
also includes original D3 geometry work). `E/baseline/` preserves entry sources
and diff. `E/scope-check.log` confirms other D3 file diffs are byte-identical and
records Windows/D2 commits. The scoped patch preserves prior D3 host geometry.

The final recovery delta is `E/redo/final-delta.patch`: exactly the saved fix plus
one baseline-compatible IPC wire test. `E/delta.patch` remains the original
attempt's patch. All new recovery artifacts are separate under `E/redo/`.

## Receipt paths / locks

| Path | Effective scale source |
| --- | --- |
| Set bounds -> render -> render_snapshot | Host stored resolved logical bounds, also used for renderer density |
| Presented and dropped frame | Both pass `self.logical_bounds` to the shared constructor |
| Direct receipt (input/paste) | Session stored bounds under the existing sessions guard |
| Focus with host | Existing focus path copies session bounds to host, then render_snapshot |
| Focus without host | Already-loaded logical bounds passed to fallback constructor |
| Preedit | Existing get_receipt path after render scheduling |
| Scroll | Existing render/get_receipt branches and the manual converter |
| Mouse, including selection-driven render | Existing render and nested IPC receipt; no coordinate/routing changes |

The constructor extracts only `bounds.scale_factor`. No lock acquisition was
added. IPC command dispatch/error handling is unchanged.

## Original attempt: executed RED / GREEN (historical)

- `RED-rust.log`: one test executed and failed `left: None`, `right: Some(2.0)`.
  It resolves DPR 1.5 using the existing Wayland policy, stores geometry through
  prepare_session_layout, then calls real get_receipt with a Tauri MockRuntime
  window (no native GUI). The stored-scale assertion passed first.
- `RED-ui.log`: one actual component test executed and failed on focus-sink style:
  expected `16px`, received `21.333333333333332px`.
- `GREEN-rust.log` and `GREEN-ui.log`: those same tests passed after the fix.

Sequencing qualification: to make Rust RED executable, the optional internal
field, its necessary Eq removal, and one explicit test literal were scaffolded
first, with the constructor hardcoded to None. This was not a pristine-production
RED; no scale propagation, wire converter change, or UI correction existed yet.
The failure was a runtime assertion, not a compiler error. The resolved prior
scope blocker is archived at `prior-scope-blocker.md`. Entry inspection found no
completed delta or owned live verification process to duplicate.

Exact RED/GREEN commands from the isolated worktree:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::receipt_reports_effective_presentation_scale -- --exact --nocapture
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'uses effective native scale for IME anchor at fractional webview density'
```

## Original attempt: focused verification (historical)

| Evidence | Executed result |
| --- | --- |
| GREEN-ui-focused.log | 171 passed: NativeTerminalPane.test.tsx 139, lifecycle 25, nativeTerminalLifecycle 7 |
| GREEN-host.log | All 28 surface_host tests passed, including existing ownership/geometry guards |
| GREEN-ipc.log | All 14 IPC tests passed; serde/converter covers absent, fractional/integer scale and both presentation statuses |
| GREEN-geometry.log | Child-surface 5, composition 7, Wayland 12 passed |
| ui-build.log | bun run --cwd ui build: TypeScript and Vite passed |
| cargo-check.log, cargo-build.log | Default-feature Mac check/build passed, jobs=8 |
| final-lib-test-compile.log | cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run passed after test-only formatting |
| diagnostics.log | Initial post-fix LSP clean on all four files; final Rust refresh limitations recorded |

New UI tests cover effective fractional density, ordinary equal scale, missing
scale and null scale. They use a pre-armed bounds promise inside React act with
test-runner timeout; no sleeps/polling were added. Existing lifecycle tests and
D2 guards were retained rather than refactored. git diff --check passed.

Verification limitations are not hidden:

- After test-only formatting, Rust LSP refresh cancelled host and timed out IPC
  after 3000ms. Earlier diagnostics were clean; final affected library/test
  compilation passed.
- Extra `cargo check --tests` failed in unchanged
  `src-tauri/tests/windows_edge_probe_contract.rs:1`: `couldn't read
  tests/../../.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1:
  No such file or directory (os error 2)`. See final-cargo-check-tests.log.
  The tracked test is unchanged and the fixture absent from this worktree.
  No test was skipped, suppressed, deleted, or edited to bypass this failure.
- Full logs retain existing unrelated unused/dead-code warnings in input,
  font_manager, notifications, terminal/session, worktree/manager and scoped
  integration-test helper modules.

## Original attempt: browser evidence / cleanup (historical)

`browser-qa.mjs` bundles the actual worktree component using existing Bun/React
caches, serves ephemeral localhost, and opens isolated Bun WebViews.
`browser-entry.jsx` injects DPR at the JS boundary and supplies controlled Tauri
IPC receipts. This is component/IPC proof, not native display proof.

The browser clicks the real viewport and types x, awaits its pre-armed exact
input signal, settles React, and captures actual DOM geometry plus attach/bounds/
focus/input actions. Readiness/input have bounded failure timeouts, no sleeps or
polling. browser-run.log records three passed scenarios:

- browser-fractional.json/.png: raw 1.5/native 2 -> (16,48), 8x16. Actual viewport
  origin (25,90), sink origin (41,138), sink size 8x16. Requests retained DPR 1.5.
- browser-ordinary.json/.png: raw/native 2 -> (16,48), 8x16.
- browser-legacy.json/.png: raw 1.5/no scale -> (21.333333,64),
  10.666667x21.333333, preserving fallback.

Screenshots expose the actual normally-invisible focus sink in red using harness
CSS only. Files were captured, but image inspection was unavailable to this
worker; acceptance uses asserted browser DOM values, not a claimed visual review.

browser-cleanup.json records three closed views, zero retained event callbacks
per unmounted component, and owned server stopped. No global browser-close or
foreign-process termination occurred. Prepared UI/Ghostty/cargo caches and prior
evidence were reused non-destructively and retained. No new dependencies,
desktop controls, native application launch or daemon occurred.

Native Wayland screenshots, compositor presentation, and interactive IME
candidate-window acceptance remain pending on an actual Wayland desktop.

## Pristine-production RED recovery

Task `st_01a078bc` followed `RECEIPT_RED_REDO.md`, without a policy exception or
history rewrite. All paths in this section are relative to `E/redo/`.

1. Saved all four owned files under `saved/` and verified their entry-to-fix diff
   was byte-identical to the original recorded passing `E/delta.patch`.
   `entry-identity.json` records owned and foreign SHA-256 identities and HEAD.
2. Applied `restore-owned.apply_patch`, reversing only that owned delta, not a
   HEAD restore. All four files were then byte-identical to `E/baseline/`, which
   already contains original D3 geometry and D2 guards. Windows was untouched.
3. Added only `wire_receipt_reports_effective_presentation_scale` in the IPC test
   module and restored only the four UI test cases. `RED-test-only.patch`,
   `red-source/` and `RED-source-hashes.json` preserve the executed baseline.
   Host and UI production were byte-identical to entry; the IPC production
   prefix, including all original receipt fields, derives and converter, was
   byte-identical too. No optional-field scaffold existed in this RED.
4. Executed both exact commands below before any production reapplication.
   `RED-wire.log`: compiled successfully, **0 passed / 1 failed**, exit 101,
   assertion `left: Null`, `right: 2.0`. `RED-ui.log`: **1 failed / 24 filtered**,
   exit 1, expected `16px`, received `21.333333333333332px`.
5. Applied `reapply-fix.apply_patch` forward only after both failures. Verified
   that the final four files equal the saved fix exactly, except for the new
   wire test. The original host getter regression, serde assertions, all UI
   scale cases and all D2 assertions remain intact.
6. Executed the identical wire/UI commands GREEN, one selected test passed in
   each. `GREEN-source-hashes.json`, `scope-check.log` and `chronology.log` tie
   the final sources to subsequent tests/builds/browser runs. No source changed
   after reapplication. HEAD remained `c349e3a6ffa1ff36cef30ac4d66dd78250e3f428`.

The new wire regression resolves raw DPR 1.5 through the existing Wayland policy,
stores real bounds and 16x32 metrics through `prepare_session_layout`, verifies
stored scale 2 through `session_logical_bounds`, calls real `get_receipt` using
Tauri MockRuntime, then passes its result through the unchanged-at-RED private
`into_ipc_receipt` and `serde_json::to_value`. It checks physical metrics and the
machine-consumed JSON field, not source text or a synthetic receipt literal.
State teardown precedes the wire assertions, including the expected RED failure.

Exact RED and GREEN commands, from the isolated repair worktree:

```sh
export CARGO_BUILD_JOBS=8
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal::tests::wire_receipt_reports_effective_presentation_scale -- --exact --nocapture
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'uses effective native scale for IME anchor at fractional webview density'
```

### Fresh final verification

| Recovery artifact | Result on final sources |
| --- | --- |
| `GREEN-wire.log`, `GREEN-ui.log` | Same two baseline RED commands now pass |
| `GREEN-host-regression.log` | Original requested host getter regression: 1 passed |
| `GREEN-ipc.log` | 15 passed, including real stored-state wire test and optional-field serde cases |
| `GREEN-host.log` | 28 passed; original geometry and owner guards retained |
| `GREEN-geometry.log` | Child 5, composition 7, host contract 18, Wayland 12: all 42 passed |
| `GREEN-ui-focused.log` | D2 three-file target: 139 + 25 + 7 = 171 passed in one run |
| `diagnostics.log` | All four final files: no LSP diagnostics, before builds |
| `cargo-check.log`, `cargo-build.log` | Default-feature macOS check and build passed, jobs=8 |
| `ui-build.log` | TypeScript and Vite build passed |
| `scope-check.log` | Owned source identities stable; captured foreign D3/Windows identities unchanged; diff check passed |

Every command is retained at the start of its log and collected in
`commands.md`. Existing compiler unused/dead-code warnings are retained, not
suppressed. The historical broad `cargo check --tests` failure on the absent
Windows edge-probe fixture remains recorded above; that broad command was not
rerun or claimed GREEN in this recovery. Focused library and all four relevant
integration targets compiled and executed successfully.

### Fresh browser proof and cleanup

Copied the existing two browser harness files into `redo/` without changing them
or overwriting earlier evidence; ran them anew after the fresh UI build. The
harness bundles the actual final component with controlled IPC and JS-injected
DPR, clicks its viewport and types x only inside isolated Bun WebViews. It uses
pre-armed input/readiness signals with bounded timeouts, not sleeps or polling.

`browser-run.log` passed all three scenarios with assertions on actual DOM sink
styles and unmodified raw DPR in attach/bounds requests:

- `browser-fractional.json/.png`: raw 1.5/native 2 -> local (16,48), size 8x16;
  viewport origin (25,90), DOM sink origin (41,138), DOM sink size 8x16.
- `browser-ordinary.json/.png`: raw/native 2 -> local (16,48), size 8x16.
- `browser-legacy.json/.png`: raw 1.5/missing scale -> local (21.333333,64),
  size 10.666667x21.333333, retaining the fallback.

`browser-input-hashes.json` records harness/UI/CSS inputs. Screenshots were
captured anew, but the image-read tool reported this model cannot view images;
there is no claimed visual inspection. Acceptance here is the asserted DOM
geometry and recorded IPC actions, not native display proof.

`browser-cleanup.json` confirms all three components unmounted with zero retained
callbacks, all three views closed and server stopped. `cleanup-check.log` records
no remaining listener on the owned ephemeral port. No desktop launch/input,
foreign process termination, dependency/manifest/vendor change, staging or
commit occurred.

**Native Wayland GUI acceptance is still pending**: compositor presentation,
native screenshots and interactive IME candidate-window placement require an
actual Wayland desktop. The fresh browser proof is component/controlled-IPC
evidence only.
