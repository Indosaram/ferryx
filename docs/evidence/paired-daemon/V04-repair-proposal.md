# V04: real Ghostty VT for the no-default-features remote gateway

Status: proposal only; parent review required before source changes.
Task: st_01a093a2. Diagnosis performed read-only; this document is the sole authorized write. No builds, installs, tests, daemon operations, source edits, commits, or overlay transfers were performed. Results below are historical log evidence, not execution claims from this task.

## Recommendation

Make the existing real `libghostty-vt` engine a mandatory backend dependency, while retaining `native-terminal` as the default-enabled GPU/native-surface feature. Compile the existing Ghostty-backed `RemoteTerminalMirror` and grid socket handler in both configurations. Do not import the foreign headless ANSI emulator. Do not change the mandated command to add a feature.

This is a production feature-boundary defect, not five independent test failures and not an A03 authorization regression. An opt-in `ghostty-vt` feature alone would not fix `--no-default-features`: the engine must remain available with no optional features selected. This changes the build prerequisites of no-default builds; it does not replace the desktop terminal experience.

## Evidence and ownership

Unless explicitly marked foreign, paths and line numbers refer to the current `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean` candidate. Line numbers describe the pre-repair source, not future patch positions.

- `docs/evidence/paired-daemon/A03-clean-baseline-remote.log:1-4` records the exact locked no-default `--lib remote::` command on untouched tracked source at `cd16c90`. Lines 316-377 record the five failures and the result: 179 passed, 5 failed, 0 ignored.
- `A03-clean-runtime.log:1-3` records the clean-candidate command chain and exit 101. Lines 749-812 show the same five failures, with 196 passed, 5 failed, 0 ignored. Its A03-specific phase separately reports 18 passing tests at line 422. That does not make the full remote gate green.
- `overlay/A03-final-remote-regression.log:1,376` records a foreign-overlay no-default run with 218 passing tests. It is evidence about that different source tree, not an admissible clean-candidate receipt.
- Foreign ownership is documented in `/Users/indo/code/project/orca-lite-wt/herdr-wave0/docs/evidence/paired-daemon/foreign-manifest.json:1-7,50-86`: source `/Users/indo/code/project/orca-lite`, destination `herdr-wave0`, captured head `0af6411ba4212c0e34db35621080d3bc4d2e218d`, and pre-existing mirror/server/tests/service modifications. This is provenance, not attribution to a named author; the current overlay also contains subsequent work. None of it is owned or implemented by V04.
- Both worktrees already contained dirty files at initial inspection. The parent subsequently reported the scoped clean A03 commit as `0ce84a1`, with no product source changes since this diagnosis began, and confirmed reproduction of the exact five failures on both `cd16c90` and the candidate. This commit identity/update is parent-provided provenance, not a new execution receipt from V04. V04 remains a separate proposed repair; future edits must be narrow hunks against that candidate, preserving A03 ownership in `remote/server.rs` and `remote/tests.rs`, not replacement files from the foreign tree.

## Five symptoms, one cause

All five tests are ordinary `#[tokio::test]` tests, not native-feature tests:

| Test in `src-tauri/src/remote/tests.rs` | Contract that must remain |
| --- | --- |
| `test_grid_render_attach_sends_full_frame_with_session_dimensions`, 1860-1933 | Authenticated real PTY attachment emits JSON Text `grid`, 93 columns x 27 rows. |
| `test_grid_render_initial_frame_uses_requested_viewport_dimensions`, 1936-1967 | Requested 47 x 18 viewport is reflected in both initial full grid and real PTY size. |
| `test_grid_render_live_pty_output_emits_grid_frame_after_attach`, 1970-2004 | Post-attach PTY input produces a grid/gridDiff containing the marker. |
| `test_grid_render_resize_returns_full_frame_with_requested_geometry`, 2007-2035 | WebSocket resize produces a full `grid` at 51 x 17. |
| `test_daemon_remote_worktree_selection_then_grid_terminal_control`, 2394-2667 | Actual daemon/worktree selection, safe metadata, desktop confirmation, grid attach, binary socket input and rendered marker. |

The helpers require a successful HTTP 101 upgrade (`tests.rs:1612-1650`) and reject non-Text frames (`1652-1656`, `1846-1856`). EOF is classified as Close (`1671-1680`). Thus these logs are not JSON assertion failures after a valid grid frame.

Trace above the symptom:

1. `remote/server.rs:1116-1194` authenticates the socket, obtains the device revocation fence, recognizes `render=grid`, requires declared active selection, optionally resizes the control client's PTY, and obtains a sequenced attachment before upgrade. The five tests establish selection; the daemon test explicitly confirms it at `tests.rs:2569-2579`.
2. The backend is already independent of native rendering: `remote/backend.rs:38-76,78-170` defines and implements real session describe/attach/input/resize operations. `terminal/service.rs:166-177` delegates to the output hub. `terminal/output_hub.rs:555-589` subscribes first and snapshots segmented history under the same lock. No alternate shell or fake backend is needed.
3. After upgrade, `handle_terminal_socket` branches on `render_grid` (`server.rs:1219-1239`). With native-terminal enabled it calls the grid handler. Without it, the branch merely consumes `(socket, session_id, attachment, device, state)` and returns. The requested connection is dropped without its required initial grid frame. This branch deterministically explains the first-frame failures.
4. The real handler and helpers are gated at `server.rs:1445,1451,1467`; the entire mirror implementation and imports are gated throughout `remote/mirror.rs:1-255`. The umbrella engine module is gated at `src/lib.rs:10-11`, and Ghostty compilation/linking is gated by `CARGO_FEATURE_NATIVE_TERMINAL` at `build.rs:36-41`.
5. `Cargo.toml:49-57` conflates that umbrella feature with optional WGPU, raw-window-handle, winit, bytemuck and pollster. Disabling GPU features therefore also removes backend VT interpretation, although no GPU is required to interpret PTY bytes.

There is no reason to relax active-selection authorization, change machine scopes, or rewrite tests to accept a closed socket.

## Real headless VT is separable, with one important internal seam

The existing engine is already a CPU-side VT adapter, despite its name:

- `native_terminal/terminal.rs:110-148` owns a Ghostty terminal handle, callback context and selection gesture; its Send invariant requires serialized access, not a window or GPU. Preserve its Mutex ownership model.
- `native_terminal/lifecycle.rs:16-111` allocates the C terminal and registers callbacks/scrollback limits, without creating a platform surface. It uses the already-existing terminal preferences/default scrollback values.
- `terminal.rs:323-390` implements dimensions, resize and feed through Ghostty C calls. `native_terminal/render_pass.rs:24-180` captures Ghostty render state into owned cursor/cell data, not GPU draw calls.
- `remote/mirror.rs:18-159` already implements real feed, recorded-geometry replay, full-frame baseline and line diffs. Lines 184-255 map wide cells, colors, attributes and cursor style. Its fixed 8 x 16 cell metrics (`13-16`) are backend VT metrics, not a request to create a native window.

However, simply ungating `native_terminal` will not compile without GPU dependencies. `native_terminal/mod.rs:8-37,40-61` declares and re-exports both engine and surface modules. Also `native_terminal/scroll.rs:6-8` imports `composition::PhysicalBounds` and `renderer::RectInstance`; its pure VT types and C calls (`16-31,228-278`) share a file with GPU overlay geometry (`33-226`). `composition.rs:3` in turn imports `child_surface::WaylandSubsurfaceGeometry`. Leaving this chain unconditional would pull the presentation side back into the engine.

### Minimum proposed source boundaries

1. **`src-tauri/build.rs`**: invoke the existing Ghostty build/link helper irrespective of `native-terminal`; preserve fail-fast errors, pinned source/toolchain and platform manifest handling. Do not add test-only linking or download fallback.
2. **`src-tauri/src/lib.rs` and `native_terminal/mod.rs`**: expose the engine module unconditionally. Keep presentation modules and presentation-only re-exports behind the existing `native-terminal` feature: `child_surface`, `composition`, `input`, `platform`, `renderer`, `surface_host`, `surface_snapshot`, `surface_error`, `wheel`, and corresponding exports. Keep their existing default-feature tests runnable; do not alter their assertions or introduce test suppression.
3. **VT dependency closure retained unconditionally**: `bell`, `cell_extractor`, `color`, `cursor`, `engine`, `error`, `guards`, `key`, `key_encoder`, `lifecycle`, `mouse`, `mouse_encoder`, `paste`, `queries`, `render_pass`, `search`, `selection`, `snapshot`, `sys::{constants,ffi,types}`, `terminal`, and viewport/scrollbar VT operations. `terminal.rs:7-48` and `engine.rs:3-9` enumerate the engine's direct dependencies. Key/mouse/selection support is part of the existing safe engine; deleting it to manufacture a smaller terminal is unnecessary.
4. **Break the mixed scroll seam without inventing a renderer**: move only `ScrollViewport`, `ScrollbarState`, `scroll_viewport` and `query_scrollbar` into a small unconditional `native_terminal/viewport.rs`. Re-export them through `scroll` for existing presentation callers and through the umbrella module for the mirror. Update direct core imports in `engine.rs:8`, `terminal.rs:28`, and the core selection call at `selection.rs:308`. Leave original scroll overlay geometry and its existing tests on the existing presentation side. This preserves all previously runnable tests; it does not hide any failure behind a new test cfg. No replacement RectInstance or no-op GPU types.
5. **`remote/mirror.rs`**: remove the native-feature guards on the real implementation and change its existing test module from `cfg(all(test, feature = "native-terminal"))` to `cfg(test)`. This expands real-engine test coverage under no-default; it does not substitute the foreign tests or parser.
6. **`remote/server.rs`**: ungate mirror/grid imports, Duration/mpsc, `grid_text_message`, `enqueue_grid_operation`, and `handle_terminal_grid_socket`. Replace the two cfg branches at 1219-1239 with the existing real-handler call and return. Keep its typed `NativeTerminalError`, actual backend routing, revision/sequence recovery, active-selection watcher, device cancellation, input admission and single outbound writer unchanged.
7. **`Cargo.toml`**: preserve `default = ["native-terminal"]` and all five optional presentation dependencies. Document the now-mandatory VT / optional presentation distinction; no new Rust dependency or lockfile update is expected for this approach. A separate crate can be considered later but is not necessary for these failures.

No production change is needed in `TerminalService`, daemon auth/protocol, relay, frontend routing or UI rendering to repair this cause. No default desktop build should lose WGPU, native child surfaces, keyboard/IME, selection, font shaping or compositor behavior. This proposal repairs the existing remote grid transport; it is not permission to use the mobile/grid UI as the desktop terminal.

## Why the foreign green overlay is not the repair

Read-only diff inspection found these relevant foreign mechanisms:

- Foreign `remote/server.rs:1387` invokes the grid handler unconditionally; `1609-1629` ungates frame helpers and makes `enqueue_grid_operation` generic over its error type.
- Foreign `remote/mirror.rs:257-1187` adds its own `headless` terminal emulator, and `1189-1190` aliases that implementation as `RemoteTerminalMirror` when native-terminal is disabled. It handles escape sequences, scrollback, cells, width and colors itself. For example, `305-316` computes the 256-color cube with multiples of 51, while the existing real-engine palette uses the normal nonzero 55 + 40*n levels (`clean native_terminal/terminal.rs:81-97`). Lines 319 onward implement a hand-maintained Unicode width table. These are separate terminal semantics, not headless use of Ghostty.
- Foreign server code also changes no-selection attachment admission (`1308-1313`), auto-spawns shells (`890`), and foreign tests change the no-active-selection response from 403 to 101. Foreign `terminal/service.rs` adds `spawn_shell`. Those changes are unrelated to the five first-frame failures and must not be swept into this repair.

The useful reference is only that the grid handler must be reachable without the presentation feature. Reuse the clean candidate's real implementation, not the foreign parser, generic error accommodation, scope relaxation, shell creation, or edited authorization expectations.

## Build/platform implications and risks

- **New prerequisite for no-default**: `native_terminal/build_ghostty.rs:6-7,26-115` requires Zig version prefix `0.16.0` and exact Ghostty SHA `6a508fd5e34c7e222c052a6d00bb3891ff3feace`. The clean candidate's `src-tauri/vendor/ghostty` directory was empty on inspection. The foreign tree has that pinned HEAD, but its source is reference-only; do not silently copy or initialize anything. Parent-approved provisioning of the clean tree is required before validation.
- The helper builds VT with executable/docs/xcframework emission disabled, ReleaseFast, and OUT_DIR-local caches (`build_ghostty.rs:214-274`). It emits static link metadata and rerun triggers (`361-391`). Preserve build-script ownership of linking (`tests/ghostty_build_contract.rs:139-155`); do not add FFI `#[link]` attributes or rely on a stale dylib from another worktree.
- Supported mappings are explicit at `build_ghostty.rs:119-143`: macOS x86_64/arm64, Linux x86_64/arm64 GNU/musl, and Windows x86_64/arm64 MSVC/GNU variants. This change extends those restrictions to no-default builds; unmapped targets must fail clearly, not use a fake VT.
- Windows link/runtime validation is mandatory, not implied by `cargo check`. The pinned foreign checkout's `vendor/ghostty/src/build/GhosttyLibVt.zig:227-267` uses the VT C module and Windows `ntdll`/`kernel32` link dependencies. The Rust helper currently emits the Ghostty static archive only. Whether transitive native imports already satisfy these symbols must be established by actual target linking; if not, narrowly add the required Windows system libraries in the build helper. Do not claim a proven link failure from source inspection alone. Preserve existing Windows manifest behavior in `build.rs:23-65`.
- This does not make the entire Ferryx crate GUI-dependency-free. Tauri, platform libraries and audio remain unconditional (`Cargo.toml:60,97-170`). The specific promise is no WGPU/native-surface requirement for VT/grid operation, not a standalone minimal relay package. A standalone relay dependency split is outside scope.
- Additional engine unit tests become discoverable once the umbrella module is unconditional. Their failures, if any, must be reported rather than hidden. Default-feature tests must also run to catch broken re-exports or presentation wiring.
- The current five tests contain `sleep 30` keepalive commands (`tests.rs:1870,1939,1973,2010,2508`). These are pre-existing fixture weaknesses, not the cause of an immediate socket drop. The daemon test creates such a CommandBuilder but does not pass it to `spawn_terminal` (`2506-2529`); it uses the actual daemon shell. Live-output tests also assume the next frame contains a complete marker, which can depend on PTY chunking and the production 33 ms coalescing interval (`server.rs:1541-1569`). A reliability patch, if needed with parent approval, must replace fixture lifetime timing with an explicit input/EOF rendezvous, subscribe before triggering output, await complete semantic marker state with a bounded timeout, and retain all original wire/geometry/marker assertions. Never lengthen sleeps or retry until green. Keep this separate from the production feature-boundary hunks.

## Approval-gated command and validation plan (not executed)

Prerequisites: parent reviews the source boundary above; provision pinned Ghostty and Zig explicitly; use isolated build output and fixture roots; confirm UI dist prerequisites. Do not stop existing daemons, run destructive release scripts, or reuse foreign artifacts. Run language-server diagnostics on every changed Rust/build file before build commands. Inspect failures and stop on missing prerequisites rather than silently install or mutate git state.

On this macOS arm64 workstation, after approval and provisioning:

```sh
cd /Users/indo/code/project/orca-lite-wt/herdr-wave0-clean
export CARGO_TARGET_DIR="$PWD/src-tauri/target-v04"
export CARGO_BUILD_JOBS=4
unset A03_PRIVATE_ROOT A03_CLI_BINARY FERRYX_MACHINE_TOKEN FERRYX_RELAY_URL

# Mandatory full regression gate: no added feature and no test omission.
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture

# Real FFI and CPU engine contracts; keep all assertions.
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test ghostty_build_contract --test ghostty_build_info_ffi --test native_terminal_engine_contract --test native_terminal_capability_contract

# Same remote path with normal desktop features, then presentation/engine unit coverage.
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib remote:: -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib native_terminal:: -- --nocapture

# A03 regression gate and actual production binary linking.
cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib a03_ -- --nocapture
cargo build --locked --manifest-path src-tauri/Cargo.toml --bin ferryx

# Inspect feature graph: presentation dependencies must not be enabled by this repair.
cargo tree --locked --manifest-path src-tauri/Cargo.toml --no-default-features -e features
```

Run each applicable gate once against the reviewed candidate and retain its complete result. The expected no-default remote inventory grows because real mirror unit tests become enabled; do not pin acceptance to the historical count of 201. The five named tests must all run, with no ignored/filtered substitutes for them. In addition to existing mirror tests (`mirror.rs:257-475`: ASCII, SGR, CJK, line diffs, resize, cursor, segmented replay and scroll sign), add focused real-Ghostty cases for split UTF-8/escape input, alternate-screen restoration and recovery full-frame baseline only if coverage is missing. No prose-pinning tests.

The build contracts have pre-existing fixed temporary-path fixtures (`ghostty_build_contract.rs:203-322`), so do not run competing copies against shared temp paths. Any repair to those fixtures must retain assertions and use unique TempDirs; do not skip them. The existing workflow already provisions recursive submodules and Zig (`.github/workflows/build-test.yml:68-76`) and demonstrates a real Rust-to-TypeScript grid seam command (`151-156`), but that is not evidence that this candidate has passed it.

After successful compilation, exercise an isolated real owner/CLI/gateway through its public authenticated socket surface: declare selection, request grid attach, validate initial geometry, send input, validate rendered marker, resize, revoke and observe socket close; keep another device/session unaffected. Repeat raw non-grid attach to preserve binary framing. Launch the normal default-feature desktop build and visually verify its actual native terminal surface, input, resize, scrolling and selection, including paired-daemon attachment. A headless grid assertion cannot prove native desktop pixels. No xterm/mobile substitution is acceptable.

Repeat target-specific build/link and applicable CPU VT/grid gates on actual Linux and Windows runners, retaining logs. Existing `/bin/sh` and Unix daemon fixture assumptions must be reported on Windows rather than cfg-disabling the five tests; target-neutral fixture work is separate approval scope if required. On supported GPU-equipped platforms run existing native renderer/composition/input/surface contracts and manually verify native surfaces. No cross-platform or GPU pass is claimed by this proposal.

## Decision requested of the parent

Approve the mandatory real-VT/optional-presentation boundary and explicit clean-tree build provisioning. Keep A03 authorization ownership separate. The smallest credible repair is a handful of feature-boundary changes plus the mixed viewport dependency extraction, not a new terminal emulator or a weakened test suite. The causal diagnosis is source-backed; implementation size, successful linking, test reliability and platform behavior remain to be verified after approval.
