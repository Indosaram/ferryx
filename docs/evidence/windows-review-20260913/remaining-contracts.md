# Remaining Windows contract fixtures: bounded source closure

2026-09-13; task st_01a09876. **Remaining assigned fixture source coverage COMPLETE; execution and parent goal INCOMPLETE; isolation approval pending.** This is not a passing-test, native-binary, or implementation receipt.

## Boundary and method

Lead identifier qualification: the RC-01 through RC-05 identifiers in this
report are local to this contract review. In aggregate records use
`CONTRACT-RC-01` through `CONTRACT-RC-05`. They are not the separate RC-01
through RC-04 remote application defects in `remaining-remote-callers.md`.
CONTRACT-RC-04 is a macOS-only validator observation, excluded from Windows
repair requirements; no additional macOS desktop QA is authorized by it.

Read `gap-verification.md` section 4, `gap-tooling.md` in full and the native-input first-wave report. The latter supplies production-input findings, not a complete integration-fixture receipt; none of the remaining native paths is silently credited to its selective coverage. All 13 fresh paths below were read in full with `read`. Settled fifteen source files were not reread wholesale; their exact inventory receipts are reused. Read ancestor instructions, git status/diff, Cargo manifest/build entry and bounded callees to resolve cfg, execution and ownership rather than classifying source uncertainty as runtime uncertainty.

The census is `git ls-files 'src-tauri/tests/*' 'src-tauri/src/ssh/helper_setup_tests.rs'`: initially 60 tracked paths = 59 integration files plus the assigned SSH unit fixture. Native prefix includes geometry/macOS files even without a `_contract` suffix. The initial 25 native paths comprise 13 settled + 12 fresh. Adding permissions/rorca from the settled fifteen and SSH yields **28 assigned-or-receipted paths = 15 existing + 13 fresh**. The other 32 are explicitly outside this remaining-native/engine/input assignment, not certified reviewed here. During final verification two foreign image fixtures became tracked: the current census is **62 = 28 assigned/receipted + 32 other-domain + 2 foreign-owner exclusions**, with 26 native-prefix paths. This is an exhaustive ledger for that predicate and observed moving tree, not an exhaustive repository test audit.

`T/` in the two tables means `src-tauri/tests/`. `GT` means `gap-tooling.md`, section "Source coverage inventory", the row bearing the identical path and source range. Its accepted disposition and proposed commands are independently dispositioned in `gap-verification.md` sections 3 and 5. Source ranges are dirty-tree observations, not immutable hashes.

## 1. Exact assigned path -> receipt ledger

| Path (T/ prefix) | Receipt and Windows/fixture disposition |
|---|---|
| `native_terminal_renderer_contract.rs` | GT :1-18; entry for nine modules, no Windows exclusion. |
| `native_terminal_renderer_contract/config_validation.rs` | GT :1-62; typed invalid-config cases. |
| `native_terminal_renderer_contract/dirty_row_reuse.rs` | GT :1-41; GPU row-reuse counters, not HWND. |
| `native_terminal_renderer_contract/dirty_update_atlas.rs` | GT :1-55; GPU mutation/atlas bounds. |
| `native_terminal_renderer_contract/font_rasterization.rs` | GT :1-253; RF-08 format assumptions, installed fonts. |
| `native_terminal_renderer_contract/offscreen_render.rs` | GT :1-613; GT-07 false green + RF-08, optional owned ATLAS_EVIDENCE_DIR. Foreign moving image fields, read-only. |
| `native_terminal_renderer_contract/snapshot_builder.rs` | GT :1-215; deterministic DTO helper, foreign image field preserved. |
| `native_terminal_renderer_contract/standalone_poc.rs` | GT :1-96; GT-05 actual example executable resolution and unbounded child. |
| `native_terminal_renderer_contract/theme_contract.rs` | GT :1-383; explicit theme + GPU cases, foreign image field preserved. |
| `native_terminal_renderer_contract/visual_artifact.rs` | GT :1-181; owned TempDir PNG + offscreen gestures, no native delivery. |
| `native_terminal_surface_host_contract.rs` | GT :1-1506; in-memory attachment/pump, no live daemon; GT-08 close-event timeout. |
| `native_terminal_child_surface_contract.rs` | GT :1-113; geometry/visibility model, no HWND lifetime proof. |
| `native_terminal_glyph_raster_contract.rs` | GT :1-58; variant-neutral, Menlo fallback not selected-face proof. |
| `permissions_contract.rs` | GT :1-48; GT-06 prose expectation mismatch, macOS launcher side effect. |
| `rorca_native_contract.rs` | GT :1-735; whole-file cfg(unix), zero Windows tests; Unix git/daemon/profile effects. |
| `native_terminal_capability_contract.rs` | Fresh :1-243, F1 below; real VT scrollback, selection, paste, search, palette and tracking. |
| `native_terminal_composition_contract.rs` | Fresh :1-419, F2; logical/physical validation, grid, containment and AppKit-coordinate arithmetic. |
| `native_terminal_drag_drop_coordinates.rs` | Fresh :1-39, F3; whole-file macOS+feature gate; no Windows execution. |
| `native_terminal_engine_contract.rs` | Fresh :1-10, F1; ungated entry imports all five following modules. |
| `native_terminal_engine_contract/feed_and_snapshot.rs` | Fresh :1-172, F1; ANSI palette, CJK/emoji/combining-cell content, NUL feed and independent copied snapshots. |
| `native_terminal_engine_contract/key_encoding.rs` | Fresh :1-146, F1; exact ordinary/DECCKM/function/Ctrl-L/Kitty bytes, typed C0/DEL/PUA rejection. Not Ctrl-punctuation coverage (existing native-input-05). |
| `native_terminal_engine_contract/lifecycle_and_control.rs` | Fresh :1-177, F1; creation/resize/errors/title/BEL and 50 sequential create/feed/snapshot/drop cycles, not a timing test. |
| `native_terminal_engine_contract/mouse_encoding.rs` | Fresh :1-209, F1; typed invalid geometry, disabled tracking and SGR envelope. RC-03 weak coordinate assertion below. |
| `native_terminal_engine_contract/screen_and_cursor.rs` | Fresh :1-90, F1; exact CUP/relative cursor, alternate-screen restore, retained output and reflow. |
| `native_terminal_input_boundary_contract.rs` | Fresh :1-790, F4; actual attached Rust adapters + real VT/channels, no live IPC client. RC-02 ambient preferences below. |
| `native_terminal_macos_background.rs` | Fresh :1-51, F5; harness=false, real hidden macOS window; non-Mac main prints not-applicable and returns success, not Windows assertion success. |
| `native_terminal_wayland_subsurface_contract.rs` | Fresh :1-270, F6; ungated shared geometry and stored layout; Windows executes Wayland arithmetic, not Wayland compositor. RC-02 ambient preference/font dependency. |

Separate assigned unit path: **`src-tauri/src/ssh/helper_setup_tests.rs:1-373` -> fresh F7**. This is not an integration test target; `lib.rs:17` -> `ssh/mod.rs:8` -> `helper_setup.rs:307-309` cfg(test) child (the declaration is at file end). No ancestor Unix gate; only the final symlink test at :356-373 is cfg(unix).

### F1 - VT-only ownership and determinism

All capability/engine files are Windows-reachable with the default `native-terminal` feature; no child/file OS exclusions. They instantiate owned `NativeTerminal` values or `Box<dyn TerminalEngine>`, feed fixed bytes and inspect synchronous results. No shell, SSH, daemon endpoint, clipboard, GPU adapter or desktop construction occurs in these fixture bodies. Terminal creation does not import Ghostty CLI preferences: `terminal.rs:136-152` -> `lifecycle.rs:20-145` allocates FFI/context, registers callbacks and sets built-in scrollback. `terminal.rs:127-132` frees gesture before teardown; `lifecycle.rs:149-169` unregisters callbacks then frees the terminal. Snapshot strings/grids are owned, and the mutation test asserts that ownership.

These test content/protocol, not glyph ink or PTY execution. The ANSI palette is built-in rather than ambient theme injection. No fixed sleeps, polling or wall-clock comparison in these files. No new platform fixture failure established except the weaker SGR oracle recorded below. Foreign PNG decoder registration/image-limit additions in lifecycle and image_cache in terminal are read-only integration context; the image subsystem is not certified by these preexisting text fixtures.

### F2 - Composition

No file gate; numeric validation and explicit cell metrics run on Windows without actual AppKit calls. `to_appkit_frame` is arithmetic. Only descriptor assertions in :338-347 are cfg(target_os="macos"); Windows still constructs its descriptor but **does not assert its pointer transparency**. Thus containment success cannot refute native-input-01. All values local, deterministic and allocation-only; no process or cleanup action, GPU or native window.

### F3/F5 - Deliberately macOS-only observations

Drag/drop's :6 ancestor gate excludes both cases on Windows. The orphan-view case :12-14 returns successfully when `MainThreadMarker::new()` is None, a confirmed conditional false-green prerequisite (RC-04). Its second test uses a local division helper rather than production event conversion; it proves only arithmetic. Retained view ownership, no external PID/files.

Background is explicitly `[[test]] harness=false, required-features=["native-terminal"]` in Cargo.toml:40-43. macOS main requires the actual process main thread, creates NSApplication and a retained NSWindow, never orders it on screen, disables release-on-close, creates/removes owned child, asserts opacity/RGB/invisibility across two backgrounds. No sleeps or child processes. It still initializes AppKit and is not authorized for execution here. Non-macOS :48-51 is an intentional no-op main; no Windows background proof, not a newly broken Windows test.

### F4 - Real Rust boundary, synthetic transport

No OS gate. Fixture :16-38 creates a per-test channel and pending Tokio task, empty history/epoch1, no native app handle; `attach_daemon_attachment::<tauri::Wry>(..., None)` supplies no daemon client. Production chain `ipc/native_terminal.rs:689-757` checks surface ownership -> `surface_host.rs:1278-1478` creates terminal/session and pump. No network endpoint or PTY is started; using Wry as a type is not constructing a Tauri app. Selection/copy reads strings, not the OS clipboard. Ctrl-V test asserts exactly 0x16 and release-empty at the adapter, not actual Windows clipboard/input policy.

Positive output tests subscribe before sending at :369-385 and :459-476; two-second bounds fail on missing update, not success-on-silence. Pump Output feeds before `update_sender.send_replace` (`surface_host.rs:1554-1587`). Fresh sessions have no pending detector work before their only output, so these are meaningful state signals, not sleeps. Double-click :767-788 injects timestamps; no real delay. The source-registration test :568-603 scans machine-consumed command identifiers in generate_handler, not prose; it is a lexical smoke check, not command dispatch or per-cfg reachability proof.

Detach deliberately retains session/pump (`surface_host.rs:1915-1935`); it is not cleanup. These per-test Tokio runtimes own pending stream/pump tasks and cancel them on runtime teardown. Fixtures do not close/await each task explicitly and therefore do not prove production graceful teardown. No borrowed daemon/process is at risk; future completion assertions need owned cancellation/join signals, not GT-08 silence. Preference override :633-651 is global in this test executable, restored to defaults rather than prior state and not guarded against panic. Only this file's one case writes overrides, while sibling tests can read them. Present keys in sibling cases do not establish a competing Alt assertion failure, so no fabricated race reproduction; require scoped restoration/controlled fixture settings rather than retries.

### F6 - Geometry with real preference/font lookup

Wayland name does not imply Linux cfg: all 12 cases compile for Windows. Most are pure logical position/buffer divisibility/scale/error assertions; default-presentation :133-165 explicitly checks Windows/X11 physical fractional geometry. :64-130 calls global `derived_cell_metrics_for_scale` and `prepare_session_layout`, then compares layout using those returned metrics. Chain: `font_manager.rs:325-331` -> global :77-88 -> preferences; `surface_host.rs:985-1045` owns new real VT sessions and applies imported theme/scrollback. No Wayland display, HWND or GPU renderer is created. State has no attachment tasks here and drops owned terminals. Font size/family are ambient; extreme imported metrics can exceed pane width/height and produce invalid grid, so this is not an isolated constant-font fixture. RC-02 owns the execution/isolation issue, not a claim Wayland geometry is broken on Windows.

### F7 - SSH helper setup fixture

Eighteen Windows-reachable tests cover pure serde/location/hash collision/readiness/typed mapping plus three async preflight cases. POSIX home/remote paths are protocol data validated by explicit RemotePlatform (`runtime.rs:88-114`), not Windows local directories. Windows-drive cases exercise native-independent path generation and exit-1 sentinels; no PowerShell executable is needed for these pure mapping cases. No .exe fixture failure simply because a remote POSIX helper string lacks a suffix.

`install` validates remote strings, then awaits spawn_blocking metadata/read/empty check (`helper_setup.rs:69-107`), only afterward constructs script and ssh_plan/bounded_output_with_stdin (:109-158). NamedTempFile empty test owns its open file; ordinary second-handle read is compatible, no rename/chmod of the open local file. Invalid ensure_started executable is rejected by remote-path validation before ssh_plan (:278 onward). The missing-binary test uses an unowned absolute/root-relative local path and is conditional RC-01, not unconditionally safe. Parser handles exact ready/protocol1 JSON; map tests assert codes/stages/causes and sentinels. Existing assertion :259 pins "not installed" prose unnecessarily (RC-05); message equality in transport test instead verifies preservation of supplied data, not wording.

Final Unix symlink test uses TempDir/sentinel and calls process::start. Reopened `ferryx_scope/ssh/process.rs:26-30,247-268`: symlink_metadata -> validate_private rejects symlink before canonicalize/is_live/current_exe spawn/log. Thus its current source does not start a helper. TempDir owns and removes link and sentinel. Windows reparse/ACL path is not exercised because this one case is Unix-gated; retain it and allocate an owned Windows-specific equivalent separately if required, not a blanket cfg removal.

## 2. New bounded validator findings and exact future gates

All commands below are **proposals, not executed or currently authorized**. Register same-assertion RED/GREEN with nonzero test count, native target/source/executable hash and exact binary under test. Missing toolchain, zero tests or skipped prerequisite is INCOMPLETE. Preserve all existing tests and behavior coverage, no blanket cfg exclusions, no prose-pinning. No source ownership acquired; coordinate helper/input/native owners before implementation.

| ID | Source-proved condition; exact future regression command; binary condition |
|---|---|
| RC-01 | `helper_setup_tests.rs:128-144` assumes `/nonexistent/binary/path/ferryx-helper` absent outside an owned root. If present and nonempty, install proceeds to real SSH 127.0.0.1:22; an SSH error can satisfy the allowed IoError and falsely pass. On Windows leading slash is current-drive rooted, not a guarantee of absence. Allocate owned missing child + transport recorder seam before repair; **never create the global path to reproduce**. `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::helper_setup::tests::ssh_helper_setup_install_rejects_missing_local_binary -- --exact --nocapture`. RED intended preflight oracle permits transport entry; GREEN records metadata failure on owned absent child and zero SSH spawn, also under injected colliding ambient-path result. Binary: exact native Windows test executable returns typed local failure without network/process action and removes only owned temp root. This is a harness defect, not a production SSH exploit. |
| RC-02 | Input attach and Wayland global metrics import ambient preferences: preferences.rs:602-612 executes PATH `ghostty +show-config` with blocking `output()` and no deadline; :638-695 reads HOME/XDG config; :776-823 caches imports. Thus these "model" fixtures can execute an unowned CLI/hang before their Tokio timeout, and use uncontrolled font/theme/scrollback. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract -- --nocapture` and `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract -- --nocapture`. Allocate explicit imported-preference fixture seam and observable process-spawn recorder (no ambient CLI or env mutation racing tests). RED records import attempt/contaminated fixture settings; GREEN zero ambient spawn/read plus known valid metrics and preserved real adapter/layout assertions. Binary: both exact native Windows test executables complete with deterministic owned configuration and no user profile/CLI contact; this does not prove HWND behavior. Restore prior cache/overrides on all exits, not just successful assertions. |
| RC-03 | mouse_encoding.rs:151-209 describes exact cell (3,3) but only tests nonempty/ESC[</final M. Wrong button/coordinates such as ESC[<0;1;1M satisfy it. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract mouse_encoding::test_mouse_encode_disabled_and_sgr_enabled_tracking -- --exact --nocapture`. Fault the real encoding boundary to wrong coordinate/button: RED validator accepts; strengthened same regression rejects, correct production bytes GREEN exactly `\x1b[<0;3;3M`. No string prose oracle. Binary: captured bytes from native Windows engine executable equal button0, column3,row3; real DOM->PTY routing remains native-input-02/-04, not fixed by this assertion. |
| RC-04 | drag_drop_coordinates.rs:12-14 turns missing main-thread prerequisite into success. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_drag_drop_coordinates backing_scale_factor_falls_back_to_one_without_window -- --exact --nocapture` on authorized macOS after allocating main-thread execution seam. Controlled worker-thread invocation must fail the validator rather than return success; proper main-thread production view executes the scale assertion. Binary: owned retained orphan view reports1 on actual main thread, assertion count nonzero, no visible window. Windows remains explicitly excluded; do not count zero Windows tests as GREEN. Related validator family GT-07, distinct path/prerequisite. |
| RC-05 | helper_setup_tests.rs:259 pins human wording although same case already checks CliExecutableNotFound/helper_missing. `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::helper_setup::tests::ssh_helper_setup_ensure_started_windows_exit_1_with_sentinel_maps_to_missing_helper -- --exact --nocapture`. Controlled message-only variation with identical structured fields currently fails; GREEN checks code/stage/executable/root/cause and machine sentinel, retaining transport/contradiction cases. Binary: Windows exit1+missing sentinel maps to expected typed fields regardless of diagnostic wording; no real SSH prerequisite. Preserve test, replace only prose oracle. |

RC-01/02/04 are conditional source-proved safety/false-green paths, not observed execution. RC-03 is an oracle weakness, not a newly proven wrong encoder. RC-05 is validator brittleness, not product behavior failure. These local IDs are proposals for parent registration/deduplication, not additions already merged into global defect totals.

## 3. Compilation prerequisites versus executable prerequisites

`lib.rs:10-11` gates native module by feature, Cargo.toml:48-56 enables it by default. Ungated integration imports do not become valid no-default-features targets; no-feature mode is not a Windows exclusion workaround. Cargo auto-discovers the ordinary test roots; engine modules are not separate test binaries. SSH unit module is cfg(test) under the library and reaches Windows independent of its one Unix case.

Compiling these default-feature test executables still builds/links the desktop library even for pure math. `build.rs:30-65` installs Windows Common-Controls v6 manifest linker arguments and invokes Ghostty build when feature set. `native_terminal/build_ghostty.rs:6-7,26-116,195-292` checks Zig 0.16.0 prefix, pinned Ghostty revision, runs git/Zig and builds the target static library in Cargo OUT_DIR. Future native Windows build requires approved matching Rust/MSVC SDK/linker, available dependency/native libraries, pinned vendor contents and compatible Zig; absence is infrastructure INCOMPLETE. No toolchain or artifact qualification occurred here.

After loading a matching executable, F1/F2 do not need an installed shell/helper/GPU/window; F4/F6 currently have hidden CLI/profile prerequisites RC-02. A controlled native font/metrics fixture remains necessary for F6. F7 needs local TempDir permission, not SSH, except the unsafe RC-01 branch. Existing fifteen retain GT prerequisites: renderer GPU/fonts/readback and owned evidence roots, verified example artifact and bounded child, permissions no-desktop seam, rorca Unix-only live effects. Those receipts are not broadened into native runtime-green. Actual Windows physical input/IME/clipboard/drop/focus, DPI, HWND presentation/lifetime and ConPTY survival remain genuine runtime gates outside this fixture-source closure.

## 4. Exact exclusion ledger for other tracked integration files

Every entry below is `T/` plus the displayed path; exclusion is from **this assignment**, not from Windows test execution. Contents were not reread and no uninspected cfg/safety claim is made. In particular Ghostty build/ABI tests belong to toolchain/packaging, not the remaining engine/input fixtures. Existing packaging source findings do not grant permission to run them.

| Path | Exclusion domain |
|---|---|
| `app_menu_contract.rs` | Menu |
| `backend_hardening.rs` | Backend hardening |
| `clean_dev_resource_contract.rs` | Development tooling/resources |
| `daemon_handover_contract.rs` | Daemon |
| `daemon_persistence_contract.rs` | Daemon; existing unsafe harness remains excluded from execution |
| `e2e_agent_workflow.rs` | Agent workflow |
| `ghostty_build_contract.rs` | Toolchain/build fixture |
| `ghostty_build_info_ffi.rs` | Toolchain/ABI fixture |
| `ipc_hardening_contract.rs` | General IPC |
| `macos_dev_bundle_contract.rs` | Packaging |
| `platform_menu_contract.rs` | Menu |
| `relay_pairing_generation_regression.rs` | Relay |
| `remote_project_public_contract.rs` | Remote project |
| `scoped_chat.rs` | Scoped API |
| `scoped_control.rs` | Scoped API |
| `scoped_design.rs` | Scoped design; gap-verification section4 already separates unintegrated capability |
| `scoped_history.rs` | Scoped API |
| `scoped_push.rs` | Scoped API |
| `scoped_ssh.rs` | Scoped SSH, distinct from assigned helper setup module |
| `session_persistence_integration.rs` | Persistence |
| `ssh_browse_live.rs` | Live SSH |
| `ssh_posix_live.rs` | Live SSH |
| `ssh_project_identity_live.rs` | Live SSH |
| `ssh_windows_live.rs` | Live SSH |
| `updater_config_contract.rs` | Packaging |
| `updater_endpoint_contract.rs` | Packaging |
| `windows_edge_probe_contract.rs` | Packaging/tooling; COV-TEST-1 receipt in gap-tooling lead correction |
| `windows_window_opacity_contract.rs` | Packaging opacity receipt, coverage.md lexical row; not remaining native tree |
| `worktree_safety.rs` | Worktree |
| `zero_config_final_audit.rs` | Zero-config audit |
| `zero_config_gen4_audit.rs` | Zero-config audit |
| `zero_config_gen5_regression.rs` | Zero-config audit |

The following two files were untracked initially and became tracked during final verification. They are now included in the census with explicit foreign-owner exclusions, not acquired as new review scope:

| Path (T/ prefix) | Exclusion domain |
|---|---|
| `native_terminal_images.rs` | Foreign moving image fixtures; owner review/freeze required, not reviewed here |
| `terminal_image_environment.rs` | Foreign moving image environment fixture; owner review/freeze required, not reviewed here |

No contents/behavior certification for these two files. Their image production integration plus rescan/App/close work stays read-only; settled renderer image DTO diffs were observed, not amended. Freeze/recheck by owner remains required and is not recast as a runtime-only unknown.

## 5. Verification and completion

Initial/prewrite status and diff showed 30 foreign tracked modifications, 391 insertions/40 deletions. Only this previously absent report was created with apply_patch. All inspected file contents were obtained with read; Bun was used for inventory arithmetic, no Python. No source edits, tests, builds, desktop/daemon/remote operations, refs, branches or commits. Applicable ancestor instruction search found no extra report/test/SSH instructions.

The first final check correctly failed on the two newly tracked foreign image paths; no test failed or ran. After adding their owner-exclusion rows, the ledger is checked by expanding T/ rows and adding the explicit unit path against git ls-files: 62 unique paths, 62 accounted, no omissions/extras/duplicates; native26, receipts15, fresh13, other-domain exclusions32, foreign exclusions2. Foreign tracked diff meanwhile changed to 9 files, 73 insertions/4 deletions, so the original 30-file status is historical, not a freeze claim. Report whitespace and saved content checked separately. This closes only the named remaining contract fixture source gap in gap-verification section4. Other remaining source tasks, historical/runtime probes, repairs, owner freeze and isolation approval remain open. Report is uncommitted in a shared dirty tree, vulnerable to concurrent changes; no parent goal or PR completion claim.
