# Windows audit: packaging domain

Status: COMPLETE (source review); lane packaging; task st_01a0984a; 2026-09-13.
Baseline: HEAD b7ad4516; inspected current working tree (read-only audit; zero source edits made).
Scope: src-tauri/tauri*.conf.json, capabilities, build.rs, Cargo/features, windows/msix, scripts, updater/CLI/install, Windows startup/config.
Evidence base: current source contracts, release-hardening receipts, and prior audit docs. Installed binary (2026-08-28) is stale.
Methodology: re-opened all cited source spans and callers; tested compilation/contracts with cargo test and bun test.

## Domain coverage and disposition

| Path / boundary reviewed | Disposition |
| --- | --- |
| src-tauri/tauri*.conf.json (base, windows, linux, macos) | PKG-04 (overlay chrome), PKG-07 (msi target in base), PKG-09 (macOSPrivateApi in base); opacity contract verified |
| src-tauri/capabilities/ (default.json, capabilities.json) | Capabilities sound; updater:default, process:default, and notifications granted to webview 'main' |
| src-tauri/build.rs & native_terminal/build_ghostty.rs | PKG-10 (unconditional MSVC manifest linker flags on GNU Windows); Zig 0.16.0 requirement intentional |
| src-tauri/Cargo.toml (build features, target dependencies) | PKG-09 (global macos-private-api feature); notify-rust and windows-sys target dependencies sound |
| src-tauri/windows/msix/ (AppxManifest.xml, priconfig.xml) | Identity (ProjectMaho.Ferryx / CN=68073D7F) verified against Store contract; refutes L6-PACKAGING-18 |
| scripts/build-msix.ps1 & test-build-msix.ps1 | PKG-03 (omits ui/dist and helpers from staging); signing refactored (L6-PACKAGING-9 fixed) |
| scripts/release-* & build-latest-json.mjs | release-contract.mjs enforces valid host/artifact matrix; L6-PACKAGING-4/6/12/19 refuted by contract |
| ui/src/lib/windowsStoreMigration.ts & updater.ts | PKG-01 (BLOCKER: cmd_ prefix missing in IPC calls broke Store updater & migration toast) |
| src-tauri/src/ipc/updater.rs & lib.rs updater wiring | Backend commands cmd_updater_managed_externally and cmd_distribution_channel registered soundly |
| src-tauri/src/ipc/cli_install.rs | PKG-05 (HOME env lookup and Unix-only symlink logic leaves CLI unsupported on Windows) |
| src-tauri/src/daemon/launchd.rs & startup persistence | PKG-06 (launchd-only autostart; no Windows Task Scheduler or Run key implementation) |
| src-tauri/src/daemon/server.rs, agent_extension.rs | PKG-12 (agent state listener socket and extension install disabled on Windows); runtime dirs sound |
| src-tauri/src/notification/notify_rust_adapter.rs | PKG-11 (MSIX AUMID mismatch when setting explicit app_id on packaged notifications) |
| src-tauri/tests/windows_edge_probe_contract.rs | PKG-02 (BLOCKER: points to non-existent .omo ephemeral path, breaking cargo test compilation) |
| .github/workflows/build-test.yml | PKG-08 (Cargo Test step gated to Linux only, leaving Windows tests unexecuted in CI) |

## Confirmed findings, severity ordered

### PKG-01 - P1 (BLOCKER): IPC command name mismatch disables Store updater guard and migration toast
- Evidence: ui/src/lib/windowsStoreMigration.ts:35 calls invoke('distribution_channel'); ui/src/lib/updater.ts:59 calls invoke('updater_managed_externally'). src-tauri/src/ipc/updater.rs:22,42 defines cmd_updater_managed_externally and cmd_distribution_channel; registered in src-tauri/src/lib.rs:1069-1070 via tauri::generate_handler![...].
- Reachability: App.tsx:264-265 calls maybeShowWindowsStoreMigrationNotice() and startUpdatePolling() -> checkForUpdate() -> updatesManagedExternally().
- Mechanism: Tauri v2 registers exact function identifiers without stripping 'cmd_'. Invoking 'distribution_channel' or 'updater_managed_externally' fails with 'command not found'. In windowsStoreMigration.ts, catch block silently swallows error and exits (toast never appears on NSIS installs). In updater.ts, catch block sets managedExternallyCache = false. On Store (MSIX) installs, tauri_plugin_updater is intentionally not registered in Rust (lib.rs:1059); frontend proceeds to call check() on unregistered plugin, throwing an unhandled plugin error.
- Failing-first: Vitest unit test in ui/ asserting invoked command name matches Rust command identifier; or real Tauri mock invoke asserting 'distribution_channel' throws while 'cmd_distribution_channel' succeeds.
- Binary observable: On packaged Windows Store build, opening app triggers console error: plugin updater not found. On NSIS build, migration notice toast is never displayed.
- Smallest scope: Update command invocations in ui/src/lib/windowsStoreMigration.ts:35 and ui/src/lib/updater.ts:59 to use 'cmd_distribution_channel' and 'cmd_updater_managed_externally'.

### PKG-02 - P1 (BLOCKER): Ephemeral scratch path in windows_edge_probe_contract.rs breaks cargo test
- Evidence: src-tauri/tests/windows_edge_probe_contract.rs:1-3: const EDGE_WRAPPER: &str = include_str!('../../.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1');
- Reachability: cargo test --manifest-path src-tauri/Cargo.toml --test windows_edge_probe_contract (or whole-workspace cargo test --tests).
- Mechanism: The file in .omo/ulw-loop/... was a temporary agent run artifact not committed to git. Compiling this test fails immediately with os error 2 (No such file or directory). Masked in CI only because build-test.yml enumerates test binaries individually.
- Failing-first: cargo test --manifest-path src-tauri/Cargo.toml --test windows_edge_probe_contract --no-run fails with exit 101.
- Binary observable: rustc compile error: couldn't read tests/../../.omo/...: No such file or directory.
- Smallest scope: Delete src-tauri/tests/windows_edge_probe_contract.rs or check in test fixture under scripts/fixtures/.

### PKG-03 - P1 (BLOCKER): MSIX packaging script omits ui/dist and helpers from layout
- Evidence: scripts/build-msix.ps1:268 copies resolvedExePath, Assets, and AppxManifest.xml to stagingDir; omits ui/dist and resources/helpers declared in src-tauri/tauri.conf.json:48-49.
- Reachability: scripts/lib/release-platforms.mjs:658 -> build-msix.ps1 -> MakeAppx.exe -> Ferryx_x64.msix. Runtime callers: src-tauri/src/remote/server.rs:1964 (resolve_dist_dir_from) and src-tauri/src/ssh/helper_assets.rs:107 (resolve_helper_asset).
- Mechanism: MakeAppx packages only files in stagingDir. The resulting MSIX archive has no ui/dist or helpers/. When running from WindowsApps, embedded remote server cannot locate web assets and fails to serve remote web clients; SSH helper cannot resolve bundled ferryx-remote-helper.exe.
- Failing-first: Contract test in scripts/build-msix.test.mjs inspecting packaged .msix zip entries; assert ui/dist/index.html and helpers/manifest.json exist.
- Binary observable: Launching packaged MSIX and enabling remote server returns 404 / 'ui/dist not found'.
- Smallest scope: In scripts/build-msix.ps1, copy ui/dist to $stagingDir/ui/dist and src-tauri/resources/helpers to $stagingDir/helpers.

### PKG-04 - P1 (BLOCKER): Windows window chrome uses titleBarStyle Overlay without caption control margin
- Evidence: src-tauri/tauri.windows.conf.json:14 specifies titleBarStyle: 'Overlay' and hiddenTitle: true. ui/src/components/WorkspaceHeader.tsx:30 uses pr-2 (8px); ui/src/components/TabBar.tsx:372 uses pr-1.
- Reachability: Application launch on Windows -> webview spans titlebar -> native caption controls (Minimize, Maximize, Close) drawn in top-right ~140px.
- Mechanism: Unlike macOS where traffic lights are at top-left (with dedicated 72px pad in WorkspaceHeader.tsx:35), Windows native caption buttons occupy top-right. Webview header and tab bar place agent chips and tab controls directly in this region without padding, causing native controls to overlay and intercept pointer events.
- Failing-first: Visual/layout test asserting interactive DOM controls on Windows reside outside [width - 140px, width] in the top 36px bar.
- Binary observable: Clicking agent chip or trailing tab action in top-right triggers Windows window minimize/close instead of web action.
- Smallest scope: Update src-tauri/tauri.windows.conf.json to titleBarStyle: 'Visible' and hiddenTitle: false matching tauri.linux.conf.json, OR add Windows-specific pr-[140px] to WorkspaceHeader and TabBar.

### PKG-05 - P2 (HIGH): CLI launcher installation fails on Windows due to POSIX HOME and symlink logic
- Evidence: src-tauri/src/ipc/cli_install.rs:56 queries HOME; lines 77-87 return is_supported: false when !is_unix; line 205 wraps symlink creation in #[cfg(unix)].
- Reachability: Frontend Settings -> CLI Install -> cmd_cli_launcher_status / cmd_cli_launcher_install.
- Mechanism: On Windows, HOME is undefined (standard is USERPROFILE or LOCALAPPDATA), returning HomeDirNotFound. Symlink creation requires Developer Mode or elevation on Windows; no batch shim (ferryx.cmd) is written.
- Failing-first: Integration test calling cmd_cli_launcher_status on Windows expecting is_supported: true and valid launcher path.
- Binary observable: Settings UI shows CLI launcher as 'unsupported' or fails with 'User home directory could not be determined'.
- Smallest scope: In cli_install.rs, query USERPROFILE/LOCALAPPDATA on Windows, write a ferryx.cmd shim to %LOCALAPPDATA%\\Microsoft\\WindowsApps\\ferryx.cmd, and set is_supported = true.

### PKG-06 - P2 (HIGH): Daemon autostart persistence missing on Windows
- Evidence: src-tauri/src/daemon/launchd.rs:17 returns None on non-macOS; lines 87-88 return Err('Cannot determine HOME directory').
- Reachability: Daemon persistence / autostart settings.
- Mechanism: Autostart is implemented exclusively for macOS launchd (.plist in ~/Library/LaunchAgents). No Task Scheduler (schtasks.exe) or registry Run key (HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run) exists for Windows.
- Failing-first: Unit test querying autostart status on Windows asserting a valid provider exists.
- Binary observable: Daemon does not survive reboot on Windows; background processes and remote access disconnect permanently.
- Smallest scope: Implement Windows autostart provider creating a scheduled task or HKCU Run entry pointing to ferryx.exe --daemon.

### PKG-07 - P2 (HIGH): Base tauri.conf.json specifies WiX 'msi' target breaking local Windows builds
- Evidence: src-tauri/tauri.conf.json:54 includes 'msi' in bundle.targets.
- Reachability: Local developer runs bun tauri build on Windows without passing --bundles nsis.
- Mechanism: Tauri invokes candle.exe / light.exe; if WiX Toolset is not installed in PATH, build halts with error. MSI is retired from release contract (scripts/lib/release-contract.mjs).
- Failing-first: Run bun tauri build on Windows environment without WiX; asserts candle.exe not found failure.
- Binary observable: Build failure: error running candle.exe: No such file or directory.
- Smallest scope: Remove 'msi' from bundle.targets in src-tauri/tauri.conf.json (retaining 'nsis').

### PKG-08 - P2 (HIGH): Windows excluded from CI Cargo Test matrix
- Evidence: .github/workflows/build-test.yml:127 gates Cargo Test step behind if: matrix.os_name == 'linux'.
- Reachability: CI pull request validation.
- Mechanism: Windows runners only execute cargo check and cargo build (to catch Ghostty link errors), but never run cargo test. Windows-specific regressions merge undetected.
- Failing-first: Run workflow policy validator or CI matrix check asserting cargo test executes for windows-latest.
- Binary observable: Breaking tests on Windows (e.g. PKG-02) pass CI unnoticed.
- Smallest scope: Add cargo test step for Windows runner in build-test.yml with target-appropriate exclusions.

### PKG-09 - P3 (MEDIUM): macOS private API globally enabled in base config and Cargo features
- Evidence: src-tauri/tauri.conf.json:3 sets macOSPrivateApi: true; src-tauri/Cargo.toml:66 activates macos-private-api feature globally.
- Reachability: Built Windows binaries carry macOS private API flag in Tauri runtime configuration.
- Mechanism: Shared base configuration leaks platform-specific configuration into Windows and Linux artifacts.
- Failing-first: tauri.conf.json linting test asserting platform-specific flags reside in tauri.<os>.conf.json.
- Binary observable: Windows Tauri runtime metadata reports macOSPrivateApi = true.
- Smallest scope: Move macOSPrivateApi: true to src-tauri/tauri.macos.conf.json; gate Cargo feature in Cargo.toml.

### PKG-10 - P3 (MEDIUM): Windows build script emits MSVC linker flags unconditionally on GNU toolchain
- Evidence: src-tauri/build.rs:20,39 emits /MANIFEST:EMBED and /MANIFESTINPUT whenever CARGO_CFG_TARGET_OS == Ok('windows').
- Reachability: cargo build --target x86_64-pc-windows-gnu.
- Mechanism: Flags are MSVC link.exe syntax; GNU ld rejects /MANIFEST with unrecognized option error.
- Failing-first: cargo build --target x86_64-pc-windows-gnu fails at link phase.
- Binary observable: Linker error: ld: unrecognised option: /MANIFEST:EMBED.
- Smallest scope: Gate MSVC linker args with CARGO_CFG_TARGET_ENV == Ok('msvc') in src-tauri/build.rs.

### PKG-11 - P3 (MEDIUM): Toast notification app_id conflict in packaged MSIX
- Evidence: src-tauri/src/notification/notify_rust_adapter.rs:36-43 passes builder.app_id(_app_id) where _app_id is 'com.ferryx.app' (src-tauri/src/ipc/notifications.rs:53).
- Reachability: Dispatched notification with click target in packaged MSIX install.
- Mechanism: In MSIX, AppUserModelID is managed by package identity (ProjectMaho.Ferryx_<hash>!Ferryx). Explicitly setting 'com.ferryx.app' causes Windows Action Center to drop or misattribute toasts.
- Failing-first: Windows toast test checking toast notification display from packaged MSIX identity.
- Binary observable: Toast notifications fail to display or click routing fails under MSIX installation.
- Smallest scope: Check updater_managed_externally() in notify_rust_adapter.rs; omit app_id under Store package.

### PKG-12 - P3 (MEDIUM): Agent state listener socket and extension install disabled on Windows
- Evidence: src-tauri/src/daemon/agent_extension.rs:22 queries HOME; src-tauri/src/daemon/server.rs:1344,1497 gates spawn_agent_state_listener behind #[cfg(unix)].
- Reachability: AI agent state reporting from local agents (.omo, .pi).
- Mechanism: Windows lacks Unix domain socket listener for agent state reports and extension installation fails to locate %USERPROFILE%.
- Failing-first: Daemon test on Windows verifying agent state report channel readiness.
- Binary observable: Active agent state chips in UI remain stuck in idle on Windows.
- Smallest scope: Add Windows named pipe or loopback port for agent state listener and USERPROFILE directory probe.

## Prior audit refutations and fixed dispositions

- L6-PACKAGING-4 (Debian and MSI packages produce no updater manifest artifacts): Refuted / Not applicable. Retired by release-contract.mjs. MSI is discontinued; Debian is package-managed (isUpdater: false); NSIS and macOS self-update.
- L6-PACKAGING-6 (Updater architecture mapping drops ARM64 for Windows and Linux): Refuted / Not applicable. release-contract.mjs maps targets via host receipts; no Windows ARM64 build host is configured in VALID_HOSTS.
- L6-PACKAGING-9 (MSIX packaging signs with self-signed untrusted certificate): Fixed. Hardened in st_01a07fca: defaults to -SkipSigning for Store ingestion, requires explicit -CertThumbprint for sideloading.
- L6-PACKAGING-12 (CI workflow omits Windows Authenticode code signing): Refuted / Not applicable. Hosted CI release producer workflows retired by release-workflow-policy.mjs; release-local.mjs drives trusted local release hosts.
- L6-PACKAGING-13 (Vendored Ghostty requires nightly Zig 0.16.0): Refuted / Intentional. Submodule 6a508fd5 requires Zig 0.16.0 language features; CI installs Zig 0.16.0.
- L6-PACKAGING-18 (MSIX manifest identity conflicts with com.ferryx.app): Refuted / Not applicable. ProjectMaho.Ferryx and CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36 are the registered Microsoft Partner Center Store identity.
- L6-PACKAGING-19 (Updater archive layout validator requires macOS structure): Refuted / Not applicable. build-latest-json.mjs:368 restricts assert-updater-archive-layout.mjs strictly to if (artifact.kind === 'macos-updater').

## Unknowns and boundaries

- Microsoft Store certification ingestion: automated validation passes MakeAppx /nv, but live Partner Center ingestion requires live publisher submission.
- Windows ARM64 target support: no hardware or cross-compilation pipeline currently verified.
- Direct caption control hit-testing on Windows 11 snap layouts: behavior depends on OS window manager compositing and whether transparent: false is active.
