# macOS Ferryx.app Release Replacement Report

Date: 2026-09-10
Author: OMO / Senpi
Scope: Local macOS release app build, Developer ID signing, inode-preserving app replacement, and live daemon preservation.

## 1. Build & Signing

- **Commit:** `6a96a31` (`fix(terminal,ssh): remediate abnormal exit attach error and ssh detect timeout fallback`)
- **Version:** `2026.908.1`
- **Build Command:**
  ```sh
  APPLE_SIGNING_IDENTITY="Developer ID Application: Indo Yoon (5DUM8WPB4C)" \
  TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/ferryx-updater.key)" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(head -1 ~/.tauri/ferryx-updater.key.password)" \
  bun tauri build --bundles app
  ```
- **Signing Identity:** `Developer ID Application: Indo Yoon (5DUM8WPB4C)`
- **Team ID:** `5DUM8WPB4C`
- **Bundle ID:** `com.ferryx.app`
- **Signature Verification:** `codesign --verify --deep --strict` passed cleanly (exit code 0).
- **Executable Binary SHA-256:** `4c1048db180c91824d7f3df150b1ae883cd228b7cd0771e4aa5eb5f7954baff2`

## 2. Inode-Preserving App Swap

- **Installed Path:** `/Applications/Ferryx.app`
- **Previous GUI PID:** `22435` (gracefully stopped via `osascript` / `SIGTERM`)
- **New GUI PID:** `97432`

## 3. Daemon & Session Preservation

- **Daemons Preserved:**
  - PID `827`: legacy peer daemon preserved
  - PID `26697`: previous active daemon preserved
  - PID `65513`: canonical daemon active
- **Total Sessions Preserved:** 24 active sessions completely intact.
- **PTY Disruption:** Zero PTY master file descriptors terminated.
