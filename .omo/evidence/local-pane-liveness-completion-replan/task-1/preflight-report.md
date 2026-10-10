# Task 1: Remote Preflight Verification Report

**Plan:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`  
**Date:** 2026-10-03  
**Lane:** Sole Remote Verifier (Task 1: Read-Only Remote Preflight)  
**Status:** COMPLETE (All Remote Preconditions Verified; Zero Builds/Tests Executed)

---

## 1. Executive Summary

This preflight independently validates the remote hosts required for executing the local pane liveness and split completion plan. In strict adherence to project boundaries and instructions:
- **No local builds or tests** were executed on this Mac.
- **No product source edits** were made.
- **No production daemons or PTYs** were touched, and **production daas was not contacted**.
- **Native GUI permissions** were requested from the user but remain **NOT granted**; zero desktop actions were initiated.
- Evidence is written exclusively under workspace `C` (`/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-1`). Main repository `.omo` was not touched.

All three target builder/verifier environments are confirmed healthy, toolchain-complete, disk-sufficient, and pinned to the exact Ghostty commit (`6a508fd5e34c7e222c052a6d00bb3891ff3feace`).

---

## 2. Windows Preflight: `maho-win`

- **Target:** `sook@100.126.171.58`
- **SSH Key:** `/Users/indo/code/project/maho-workspace/.secrets/signing/maho_win_builder_ed25519`
- **Actual Identity:**
  - User: `desktop-1lapjmp\sook`
  - Hostname: `DESKTOP-1LAPJMP`
  - OS Version: `Microsoft Windows [Version 10.0.26200.9457]`
  - Architecture: `AMD64` (native x86_64)
- **Disk Budget:**
  - Drive `C:`: 253.33 GB free (272,011,862,016 bytes), 677.18 GB used, 930.51 GB total.
  - Requirement: Meets the 20 GiB (`21474836480` bytes) threshold with substantial headroom (>253 GB).
- **Toolchains:**
  - `rustc`: 1.97.0 (2d8144b78 2026-07-07)
  - `cargo`: 1.97.0 (c980f4866 2026-06-30)
  - `cargo tauri`: tauri-cli 2.11.4
  - `git`: 2.55.0.windows.2
  - `node`: v24.19.0
  - `bun`: 1.4.0
  - `zig`: 0.16.0
  - `rustup`: `stable-x86_64-pc-windows-msvc (default)` (targets installed: `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`)
- **Native Exit Code Capture Validation:**
  - Validated via transferred PowerShell script (`powershell.exe -NoProfile -ExecutionPolicy Bypass -File`):
    - `cmd.exe /d /c exit 7` -> captured `$LASTEXITCODE = 7` (while unbound `$LASTEXIT` evaluates to empty string).
    - `cmd.exe /d /c exit 0` -> captured `$LASTEXITCODE = 0` (while unbound `$LASTEXIT` evaluates to empty string).
  - Verifier script was cleanly removed after execution; removal verified via `Test-Path -> False` receipt.
- **Ghostty Root & Commit Pin:**
  - Location: `C:\Users\sook\task2-ghostty-6a508fd5`
  - HEAD Commit: `6a508fd5e34c7e222c052a6d00bb3891ff3feace` (matches `EXPECTED_GHOSTTY_SHA` exactly).
  - Working Tree: Clean (`git status -s` returned 0 modified files).
  - Junction Command: `mklink /J <tree>\src-tauri\vendor\ghostty C:\Users\sook\task2-ghostty-6a508fd5`
- **Signing & Packaging Tools:**
  - `signtool.exe`: Present at `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe` and `10.0.28000.0\x64\signtool.exe`.
  - `makeappx.exe`: Present at `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\makeappx.exe` and `10.0.28000.0\x64\makeappx.exe`.
  - Certificates: Development signing certificates present in `Cert:\CurrentUser\My`. Per runbook, Windows Store MSIX is produced unsigned for Partner Center ingestion.
- **Remote Candidate Paths:**
  - Candidate parent: `C:\Users\sook\ferryx-pane-completion`
  - Status: Checked and confirmed **FREE** (does not exist, no collisions).
  - Layout:
    - Source: `C:\Users\sook\ferryx-pane-completion\source`
    - Binary: `C:\Users\sook\ferryx-pane-completion\source\src-tauri\target\debug\ferryx.exe`
    - Evidence: `C:\Users\sook\ferryx-pane-completion\evidence`
    - Runtime: `C:\Users\sook\ferryx-pane-completion\runtime`

---

## 3. macOS Preflight: `maho-mac`

- **Target:** `I552267@100.65.239.35`
- **Actual Identity:**
  - User: `I552267`
  - Hostname: `CQFQ4P2LXK`
  - OS: `Darwin arm64`
- **Disk Budget:**
  - Filesystem: `/System/Volumes/Data`
  - Available: 26,895,220 KiB (~27.51 GB free), 402,997,320 KiB used, 94% capacity.
  - Requirement: Meets the 20 GiB (`21474836480` bytes) threshold with ~27.5 GB free.
- **Toolchains:**
  - `rustc`: 1.92.0 (ded5c06cf 2025-12-08) at `/Users/I552267/.cargo/bin/rustc`
  - `cargo`: 1.92.0 (344c4567c 2025-10-21) at `/Users/I552267/.cargo/bin/cargo`
  - `tauri`: tauri-cli 2.10.1 at `/Users/I552267/.bun/bin/tauri`
  - `zig`: 0.16.0 at `/opt/homebrew/bin/zig`
  - `git`: 2.54.0 (Apple Git-157) at `/usr/bin/git`
  - `node`: v22.23.1 at `/Users/I552267/.local/bin/node`
  - `bun`: 1.4.2 at `/Users/I552267/.bun/bin/bun`
  - `xcodebuild`: Xcode 27.0 (Build version 27A266a) at `/usr/bin/xcodebuild`
- **Ghostty Root & Commit Pin:**
  - Location: `/Users/I552267/ferryx-ghostty`
  - HEAD Commit: `6a508fd5e34c7e222c052a6d00bb3891ff3feace` (matches `EXPECTED_GHOSTTY_SHA` exactly).
  - Working Tree: Clean (`git status -s` returned 0 modified files).
  - Symlink Command: `ln -s /Users/I552267/ferryx-ghostty <tree>/src-tauri/vendor/ghostty`
- **Signing & Notarization Prerequisites (Split Host Contract):**
  - On `maho-mac`:
    - Code signing identity: `0D2E16F5ACE28557225CFB0294CE45E4953EBDDB "Developer ID Application: Indo Yoon (5DUM8WPB4C)"` is present in Keychain.
    - Notarization profile: `xcrun notarytool` confirms `FerryxNotary` keychain profile is NOT stored on `maho-mac`.
  - On local coordinator (`macbook`):
    - Code signing identity: `0D2E16F5ACE28557225CFB0294CE45E4953EBDDB "Developer ID Application: Indo Yoon (5DUM8WPB4C)"` is present.
    - Notarization profile: `xcrun notarytool history --keychain-profile FerryxNotary` succeeded (last submission: `2026-10-03T11:42:51.920Z`).
  - **Contract Split:** Remote compilation runs on `maho-mac`. The resulting unsigned (or locally signed) bundle is retrieved to the coordinator Mac, where `finalizeMacosBundle` performs submission to Apple notary, stapling, and `spctl` gate validation without compilation.
- **Remote Candidate Paths:**
  - Candidate parent: `/Users/I552267/ferryx-pane-completion`
  - Status: Checked and confirmed **FREE** (does not exist, no collisions).
  - Layout:
    - Source: `/Users/I552267/ferryx-pane-completion/source`
    - Binary: `/Users/I552267/ferryx-pane-completion/source/src-tauri/target/debug/ferryx`
    - Evidence: `/Users/I552267/ferryx-pane-completion/evidence`
    - Runtime: `/Users/I552267/ferryx-pane-completion/runtime`

---

## 4. Linux Preflight: `omaki` (Authorized Release Builder)

- **Release Configuration Provenance:**
  - `scripts/lib/release-contract.mjs`: `VALID_HOSTS = ["macbook", "omaki", "maho-win"]` with permitted kinds `appimage`, `deb`, `cli-linux-amd64`.
  - `scripts/release-hosts.example.json`: `"omaki": { "ssh": "indo@100.91.254.71", "platform": "linux", "root": "/home/indo/ferryx-releases", "minFreeBytes": 21474836480 }`.
  - **Isolation Boundary:** `omaki` is the sole authorized Linux build host. It is completely distinct from `daas` (headless production daemon host connected solely via outbound relay tunnel; daas was NOT touched).
- **Target:** `indo@100.91.254.71`
- **Actual Identity:**
  - User: `indo`
  - Hostname: `indo`
  - OS: `Linux x86_64`
- **Disk Budget:**
  - Filesystem: `/dev/mapper/root` mounted on `/home`
  - Available: 113,179,444 KiB (~115.89 GB free), 845,212,716 KiB used, 89% capacity.
  - Requirement: Meets the 20 GiB (`21474836480` bytes) threshold with substantial headroom (>115 GB).
- **Toolchains:**
  - `rustc`: 1.98.0 (88d9e12ae 2026-08-18) at `/home/indo/.cargo/bin/rustc`
  - `cargo`: 1.98.0 (797e8a9bc 2026-08-05) at `/home/indo/.cargo/bin/cargo`
  - `zig`: 0.16.0 at `/usr/local/bin/zig`
  - `git`: 2.55.0 at `/usr/bin/git`
  - `node`: v26.8.1 at `/usr/local/bin/node`
  - `bun`: 1.4.0 at `/home/indo/.bun/bin/bun`
- **Ghostty Root & Commit Pin:**
  - Location: `/home/indo/ghostty.bundle`
  - Verification: Verified via temporary bare clone; HEAD commit is `6a508fd5e34c7e222c052a6d00bb3891ff3feace` (exact match with `EXPECTED_GHOSTTY_SHA`).
  - Temporary verification directory was cleanly deleted (`CLEANUP_OK`).
- **Remote Candidate Paths:**
  - Candidate parent: `/home/indo/ferryx-pane-completion`
  - Status: Checked and confirmed **FREE** (does not exist, no collisions).
  - Layout:
    - Source: `/home/indo/ferryx-pane-completion/source`
    - Binary: `/home/indo/ferryx-pane-completion/source/src-tauri/target/debug/ferryx`
    - Evidence: `/home/indo/ferryx-pane-completion/evidence`
    - Runtime: `/home/indo/ferryx-pane-completion/runtime`

---

## 5. Artifacts Generated

1. `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-1/hosts.json`
2. `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation/.omo/evidence/local-pane-liveness-completion-replan/task-1/preflight-report.md` (this document)

---

## 6. Next Steps for Root Coordinator

- Task 1 preflight requirements are completely fulfilled.
- Ready to receive root's frozen source manifest before any staging or builds begin.
- Verifier lane will pause here until source candidate is ready.
