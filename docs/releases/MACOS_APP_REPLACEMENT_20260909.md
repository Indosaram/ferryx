# macOS Ferryx.app Release Build and Replacement Report

Date: 2026-09-09
Author: OMO / Senpi
Scope: Local macOS release app build, Developer ID signing, inode-preserving app replacement, and daemon preservation.

## 1. Build & Signing

- **Commit:** `4cf749d` (incorporating `4378850 fix(sidebar): reveal primary worktree when expanding empty workspaces`)
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
- **Executable Binary SHA-256:** `21659cae8f34447ce07f596a201dc99c3535b10ce5b887680a0629d5231de8f6`

## 2. Inode-Preserving App Swap

- **Backup Path:** `/Applications/Ferryx.app.bak-202609082123`
- **Installed Path:** `/Applications/Ferryx.app`
- **Previous GUI PID:** `35996` (gracefully stopped via `osascript` / `SIGTERM`)
- **New GUI PID:** `79006`

## 3. Daemon & Session Preservation

- **Surviving Daemons:**
  - PID `845`: legacy daemon (epoch `1788875291502`), 19 active sessions preserved
  - PID `57449`: legacy daemon (epoch `1788877224715`), 9 active sessions preserved
  - PID `36119`: canonical daemon (epoch `1788899080133`), 18 active sessions preserved
- **Total Sessions Preserved:** 46 active sessions intact.
- **PTY Disruption:** Zero PTY master file descriptors were terminated.
