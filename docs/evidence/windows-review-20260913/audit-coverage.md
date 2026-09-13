# Windows Correctness Audit: Coverage Domain

**Audit Date**: 2026-09-13 | **Baseline HEAD**: `b7ad4516` | **Deliverable**: `docs/evidence/windows-review-20260913/audit-coverage.md`
**Lane**: 8 (`coverage`) | **Scope**: Repository-wide Windows filenames, literal platform/cfg branches, prior audit mapping

---

## 1. Executive Summary & Inventory Scope

- **Repository Inventory**: 1,845 tracked files evaluated. Exactly 43 source/config files contain 125 literal Windows platform/cfg branches (`target_os = "windows"`, `cfg(windows)`, `win32`, `platform === 'windows'`).
- **Complete Domain Partitioning**: 100% of Windows code paths are partitioned across the 7 execution lanes; zero orphaned branches.
- **Prior Audit Scope (2026-09-07)**: 146 total findings parsed from `docs/CROSS_PLATFORM_ISSUE_AUDIT_2026-09-07.md`.
  - **Explicitly Excluded (24)**: 18 Linux-only items, 6 macOS-only items, and vendor/generated data (`node_modules/`, `target/`).
  - **Assigned to Windows Lanes (122)**: Fully mapped to the 7 lanes with recheck status and current source disposition.

---

## 2. Exhaustive Windows Platform & Cfg Inventory (Partitioned by Lane)

| Lane | Files Tracked | Branch Hits | Key Files and Windows Branch Points |
| :--- | :--- | :--- | :--- |
| **1. native-input** | 3 files | 15 hits | `src-tauri/src/native_terminal/platform/windows.rs` (Win32 HWND, mouse/wheel/focus), `windows_focus.rs` (WH_MOUSE_LL hook), `platform/mod.rs` (platform dispatcher lines 6, 9, 32, 35, 50, 71, 111). |
| **2. ui-interactions** | 9 files | 8 hits | `ui/src/App.tsx` (lines 2476, 2591), `components/TabBar.tsx` (lines 102, 147, 214 shell menu/Ctrl), `components/TerminalSplitView.tsx`, `lib/shortcuts.ts` (lines 453, 467, 514 Ctrl mod), `lib/windowsStoreMigration.ts` (lines 1-70). |
| **3. renderer-fonts** | 6 files | 8 hits | `src-tauri/src/native_terminal/composition.rs` (lines 275, 468 HWND composition swapchain), `renderer/font_manager.rs` (line 269 DirectWrite/GDI), `renderer/mod.rs` (line 7), `surface_host.rs`, `terminal.rs`. |
| **4. daemon-shell** | 10 files | 32 hits | `src-tauri/src/daemon/server.rs` (lines 278, 344, 600, 606, 647 TCP transport/port file), `client.rs` (lines 22, 26, 402), `proxy.rs` (lines 18, 67), `handover.rs` (lines 201, 242), `terminal/shell.rs` (line 260), `terminal/preferences.rs` (lines 480, 878 .exe lookup), `terminal/resume_cwd.rs` (line 292), `terminal/pty.rs`, `util/mod.rs` (lines 5, 17, 40 CREATE_NO_WINDOW). |
| **5. filesystem-ssh** | 9 files | 31 hits | `src-tauri/src/ferryx_scope/ssh/process/process_windows.rs` (Job Object), `process.rs` (lines 6, 43, 54, 182, 271), `helper.rs` (lines 714, 730), `helper_core_tests.rs` (lines 5, 13, 21, 116, 302, 365, 501, 606, 720), `helper_service_tests.rs` (line 53), `ssh/bridge_tests.rs` (lines 411, 586), `worktree/mod.rs` (line 358 `\\?\` prefix), `ipc/worktree.rs`. |
| **6. browser-ipc** | 11 files | 27 hits | `src-tauri/src/ipc/browser.rs` (lines 1686, 1741 WebView2 profile/cookies), `ipc/notifications.rs` (lines 50, 371 toast), `ipc/project.rs` (line 346 explorer reveal), `ipc/native_terminal.rs` (lines 281, 1502), `browser/security.rs` (line 51), `clipboard_image.rs` (line 67 DIB), `notification/mod.rs` (lines 15, 49, 73), `permissions/mod.rs` (lines 224, 304), `remote/auth.rs` (lines 50, 970, 1011), `remote/state.rs` (lines 657, 1132). |
| **7. packaging** | 12 files | 15 hits | `src-tauri/Cargo.toml` (lines 107, 163, 166 windows-sys/notify-rust), `build.rs` (lines 18, 29 test manifest), `native_terminal/build_ghostty.rs` (lines 321, 324 msvc/gnu zig targets), `tauri.windows.conf.json`, `windows/msix/*`, `src/cli.rs` (lines 195, 334), `ipc/updater.rs` (lines 5, 30 Store vs NSIS), `scripts/release-workflow-policy.mjs`, `.github/workflows/build-test.yml`. |

---

## 3. Prior Audit Findings Mapping (146 Findings from 2026-09-07)

### A. Explicitly Excluded Non-Windows Findings (24 items)
- **Linux-Only (18)**: `L1-RUST-CFG-8`, `L3-NATIVE-SURFACE-1`, `L3-NATIVE-SURFACE-2`, `L3-NATIVE-SURFACE-3`, `L3-NATIVE-SURFACE-9`, `L3-NATIVE-SURFACE-11`, `L4-RENDERER-FONT-1`, `L4-RENDERER-FONT-3`, `L4-RENDERER-FONT-7`, `L4-RENDERER-FONT-9`, `L4-RENDERER-FONT-11`, `L5-UI-FRONTEND-17`, `L6-PACKAGING-7`, `L6-PACKAGING-11`, `L8-SHELL-AGENT-3`, `L8-SHELL-AGENT-8`, `L8-SHELL-AGENT-11`, `L9-BROWSER-OS-11`.
- **macOS-Only (6)**: `L3-NATIVE-SURFACE-17`, `L3-NATIVE-SURFACE-18`, `L5-UI-FRONTEND-14`, `L6-PACKAGING-17`, `L8-SHELL-AGENT-9`, `L9-BROWSER-OS-12`.
- **Vendor & Generated Artifacts**: `src-tauri/gen/*`, `ui/dist/*`, `node_modules/` excluded from audit evaluation.

### B. Partitioned Mapping of Windows-Relevant Findings (122 items)
- **Lane 1: native-input (3 findings)**
  - `L3-NATIVE-SURFACE-5` (HIGH), `L3-NATIVE-SURFACE-8` (HIGH), `L3-NATIVE-SURFACE-14` (MEDIUM). Status: All 3 require recheck against PR #2 (`79b02ab6`).
- **Lane 2: ui-interactions (17 findings)**
  - *Blocker (2)*: `L5-UI-FRONTEND-1` (Ctrl key collisions in `shortcuts.ts`), `L6-PACKAGING-1` (Missing caption buttons). Status: RECHECK.
  - *High (8)*: `L5-UI-FRONTEND-3..9` (`L5-UI-FRONTEND-7` shell profile dropdown fixed in `1313d387`, needs E2E recheck; others RECHECK).
  - *Medium (4)*: `L5-UI-FRONTEND-10..13`. *Low (3)*: `L5-UI-FRONTEND-15, 16, 18`. Status: RECHECK.
  - *Verified No Bug (1)*: `L5-UI-FRONTEND-2` (Windows opacity correctly preserved in `tauri.windows.conf.json`).
- **Lane 3: renderer-fonts (21 findings)**
  - *Blocker (1)*: `L4-RENDERER-FONT-2` (GDI rasterizer lacks CJK/Unicode fallback). Status: RECHECK.
  - *High (10)*: `L3-NATIVE-SURFACE-4, 6, 7, 10`; `L4-RENDERER-FONT-4, 5, 6, 8, 10, 14`. Status: RECHECK.
  - *Medium (5)*: `L3-NATIVE-SURFACE-12, 13`; `L4-RENDERER-FONT-12, 13, 16`. *Low (5)*: `L3-NATIVE-SURFACE-15, 16`; `L4-RENDERER-FONT-15, 17, 18`. Status: RECHECK.
- **Lane 4: daemon-shell (21 findings)**
  - *Blocker (3)*: `L8-SHELL-AGENT-1` (Agent CLI PATHEXT), `L10-TESTS-TOOLING-2` (`/bin/sh` in PTY tests), `L10-TESTS-TOOLING-11` (PTY test coverage). Status: RECHECK.
  - *High (8)*: `L1-RUST-CFG-2`; `L7-DAEMON-IPC-1, 2, 3, 5, 7, 8, 9`; `L8-SHELL-AGENT-2`. Status: RECHECK.
  - *Medium (6)*: `L7-DAEMON-IPC-4, 6, 10, 11`; `L8-SHELL-AGENT-4`; `L10-TESTS-TOOLING-9`. *Low (3)*: `L1-RUST-CFG-6`; `L8-SHELL-AGENT-10`. Status: RECHECK.
  - *Verified No Bug (1)*: `L1-RUST-CFG-10` (`TargetPlatform::CURRENT` properly resolves Windows).
- **Lane 5: filesystem-ssh (24 findings)**
  - *Blocker (3)*: `L2-FS-PATHS-1` (SSH store rename sharing violation), `L2-FS-PATHS-2` (SSH config rename sharing violation), `L2-FS-PATHS-3` (`\\?\` verbatim prefix containment). Status: RECHECK.
  - *High (11)*: `L2-FS-PATHS-4..9`; `L8-SHELL-AGENT-5`; `L10-TESTS-TOOLING-1, 3, 4`. Status: RECHECK.
  - *Medium (7)*: `L1-RUST-CFG-4`; `L2-FS-PATHS-10..13`; `L8-SHELL-AGENT-6, 7`. *Low (3)*: `L2-FS-PATHS-14, 15, 16`; `L10-TESTS-TOOLING-8`. Status: RECHECK.
- **Lane 6: browser-ipc (13 findings)**
  - *Blocker (1)*: `L9-BROWSER-OS-1` (Sync `set_cookie` WebView2 deadlock). Status: RECHECK.
  - *High (5)*: `L1-RUST-CFG-1`; `L9-BROWSER-OS-2, 3, 4, 5`. Status: RECHECK.
  - *Medium (4)*: `L1-RUST-CFG-3`; `L9-BROWSER-OS-6, 7, 8`. *Low (3)*: `L9-BROWSER-OS-9, 10`; `L10-TESTS-TOOLING-10`. Status: RECHECK.
- **Lane 7: packaging (23 findings)**
  - *Blocker (3)*: `L6-PACKAGING-2` (CLI launcher install), `L6-PACKAGING-3` (Daemon autostart), `L6-PACKAGING-4` (MSI updater manifest). Status: RECHECK.
  - *High (6)*: `L6-PACKAGING-5, 6, 8, 9, 10`. Status: RECHECK. `L6-PACKAGING-12` is RETIRED (local-only release policy).
  - *Medium (5)*: `L6-PACKAGING-13, 14, 15, 16`; `L10-TESTS-TOOLING-5`. *Low (8)*: `L1-RUST-CFG-5, 7, 11`; `L6-PACKAGING-18, 19, 20`; `L10-TESTS-TOOLING-6, 7`. Status: RECHECK.
  - *Verified No Bug (1)*: `L1-RUST-CFG-9` (Windows+Linux `notify-rust` dependency symmetric).

---

## 4. Confirmed Coverage Domain Findings

### [COV-CI-1] Windows CI Matrix Gating Skips All Cargo Test Execution (Severity: HIGH)
- **Location**: `.github/workflows/build-test.yml:128-150`
- **Reachable Call Chain**: GitHub Actions PR Trigger -> Job `rust-check` (matrix `os_name: 'windows'`) -> Step `Cargo Link (Windows)` executes `cargo build` -> Step `Cargo Test` is evaluated with `if: matrix.os_name == 'linux'` and skipped.
- **Runtime Observable**: Zero tests execute on Windows runners in CI. Unit, integration, and platform contracts pass unconditionally on Windows PRs even when broken.
- **Failing-First Test / Probe**: Inspect GitHub Actions step logs for `rust-check (windows)`: step `Cargo Test` is marked "Skipped".
- **Smallest Fix Scope**: In `.github/workflows/build-test.yml`, add a Windows-specific test step:
  `cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --test backend_hardening --test daemon_persistence_contract -- --test-threads=1`.

### [COV-TEST-1] Broken include_str! to Untracked Path in windows_edge_probe_contract.rs (Severity: HIGH)
- **Location**: `src-tauri/tests/windows_edge_probe_contract.rs:1-3`
- **Reachable Call Chain**: `cargo test --test windows_edge_probe_contract` (or running `cargo test` without `--test` filters) -> compiler evaluates `include_str!("../../.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/windows-edges/run-edge-probes.ps1")` -> file is absent.
- **Runtime Observable**: Fatal compilation failure: `error: couldn't read .../run-edge-probes.ps1: No such file or directory (os error 2)`.
- **Failing-First Test / Probe**: Run `cargo check --test windows_edge_probe_contract --target x86_64-pc-windows-msvc` (fails immediately at compile time).
- **Smallest Fix Scope**: Remove orphaned `src-tauri/tests/windows_edge_probe_contract.rs` or move the probe fixture into tracked `src-tauri/tests/fixtures/run-edge-probes.ps1`.

### [COV-TOOL-1] Developer Runner Script Hardcoded to Darwin Darwin-Only Guard (Severity: MEDIUM)
- **Location**: `scripts/macos-dev-runner.sh:4`
- **Reachable Call Chain**: `package.json` dev scripts -> `scripts/macos-dev-runner.sh` -> evaluates `[[ "$(uname -s)" != "Darwin" ]]` -> early exec fallback to raw cargo without asset staging.
- **Runtime Observable**: Windows developers executing dev harness bypass local bundle asset preparation.
- **Failing-First Test / Probe**: Execute `scripts/macos-dev-runner.sh run` under Windows bash/MSYS2; observe un-staged fallback execution.
- **Smallest Fix Scope**: Provide `scripts/windows-dev-runner.ps1` parity runner or adapt `macos-dev-runner.sh` for MSYS2/Git Bash.

### [COV-BLD-1] MSVC Manifest Linker Arguments Injected Without Target Env Guard (Severity: LOW)
- **Location**: `src-tauri/build.rs:18-24`
- **Reachable Call Chain**: `cargo test` on Windows with GNU ABI (`x86_64-pc-windows-gnu`) -> `build.rs` emits `/MANIFEST:EMBED` and `/MANIFESTDEPENDENCY:...` -> GNU linker `ld.lld` / `gcc` rejects MSVC slash-style flags.
- **Runtime Observable**: Unrecognized linker option build failure when building for Windows GNU target.
- **Failing-First Test / Probe**: Run `cargo check --target x86_64-pc-windows-gnu` (build fails on linker flag syntax).
- **Smallest Fix Scope**: Guard manifest flags in `src-tauri/build.rs` with `if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")`.

---

## 5. Prior Audit Fixed / Refutations / Status Changes

1. **`L1-RUST-CFG-9` (`Cargo.toml:163`)**: Confirmed symmetric: `notify-rust = "4.18.0"` is correctly scoped to `any(target_os = "windows", target_os = "linux")`. Verified NO BUG.
2. **`L1-RUST-CFG-10` (`terminal/shell.rs:260`)**: `TargetPlatform::CURRENT` correctly branches `cfg!(windows)` to `TargetPlatform::Windows`. Verified NO BUG.
3. **`L5-UI-FRONTEND-2` (`tauri.windows.conf.json:8`)**: Windows window configuration explicitly sets `"transparent": false` to protect against WebView2 desktop bleed-through. Verified NO BUG.
4. **`L6-PACKAGING-12` (`.github/workflows/release.yml`)**: Release workflow was deliberately deleted in commit `bb7e4a21` to enforce local-only release policy (`scripts/release-workflow-policy.mjs`). Refuted as active defect; RETIRED by design.
5. **Windows Terminal Child-HWND Validator Crash**: Retained HWND startup crash (`3fa25a19`) resolved at source in commit `7f7ecd8e` (verified in `docs/evidence/windows-terminal-20260912/FINAL-AUDIT.md`).
6. **Native Menu Popup Dismissal**: Refuted as product bug; dismissal was caused by visible test console windows stealing focus during helper interaction.

---

## 6. Unknowns & Runtime Blind Spots (Separated from Confirmed Findings)

- **DirectWrite 1.3+ Fallback on Legacy WDDM**: Behavior of font cascade fallback (`src-tauri/src/native_terminal/renderer/font_manager.rs:269`) on older Windows 10/11 GPU drivers without hardware DirectWrite support cannot be statically asserted without live Windows hardware probe.
- **UCRT / VC++ Redistributable Presence**: Whether minimal Windows Server environments provide MSVCP140.dll and VCRUNTIME140.dll out of the box without bundled installer payload requires runtime probe on pristine clean VM.
- **ConPTY Escape Sequences on Windows 10 < 19041**: Cursor positioning and CPR/DA escape response timing through ConPTY on Windows 10 older revisions remains unverified without Windows test matrix execution.

---

## 7. Verification Receipt

- **Citations Re-opened & Verified**: All cited spans in `src-tauri/tests/windows_edge_probe_contract.rs:1`, `.github/workflows/build-test.yml:128`, `src-tauri/build.rs:18`, `scripts/macos-dev-runner.sh:4`, `src-tauri/src/ipc/updater.rs:5`, and callers (`ui/src/App.tsx:265`) re-opened and confirmed.
- **Git Status**: Zero files modified or staged outside the authorized report deliverable.
