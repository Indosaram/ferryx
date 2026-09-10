# macOS Ferryx.app Release Replacement Report (Shortcuts & Child Webview Fixes)

Date: 2026-09-09
Author: OMO / Senpi
Scope: Local macOS release app replacement, Developer ID signing verification, inode-preserving backup swap, and live PTY daemon preservation.

## 1. Build & Signing

- **Commit:** `57abc83` (`fix(shortcuts): restore cross-platform shortcuts and child webview focus handling`)
- **Version:** `2026.908.1`
- **Signing Identity:** `Developer ID Application: Indo Yoon (5DUM8WPB4C)`
- **Team ID:** `5DUM8WPB4C`
- **Bundle ID:** `com.ferryx.app`
- **Signature Verification:** `codesign --verify --deep --strict` passed cleanly (exit code 0).
- **Executable Binary SHA-256:** `ea2708eb90152308f687ad95b9c267d2fc6d308fa3c4d86073fbcb8303130de3`

## 2. Inode-Preserving App Swap

- **Backup Path:** `/Applications/Ferryx.app.bak-20260909225357` (created via `mv` to preserve existing process inodes)
- **Installed Path:** `/Applications/Ferryx.app`
- **Previous GUI PID:** `26679` (gracefully stopped via `osascript` / `SIGTERM`)
- **New GUI PID:** `65493`

## 3. Daemon & Session Preservation

- **Daemons Preserved:**
  - PID `827`: legacy peer daemon preserved
  - PID `26697`: previous active daemon preserved
  - PID `65513`: canonical daemon active
- **Total Sessions Preserved:** 24 active sessions completely intact.
- **PTY Disruption:** Zero PTY master file descriptors terminated.
