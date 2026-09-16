# macOS Ferryx.app Release Build & Replacement Report

Date: 2026-09-16
Author: OMO / Senpi
Scope: Local release build from `main`, Developer ID signing + Apple notarization, inode-preserving app replacement with live daemon preservation. No release tag or GitHub publish (explicit user direction).

## 1. Source & Build

- **Commit:** `13eaf3fc` (`main`, tracked tree clean; untracked foreign test file `ui/src/components/NativeTerminalPane.suspendedWake.test.tsx` left untouched for its authoring session)
- **Version:** `2026.916.1` (already committed in `tauri.conf.json`/`Cargo.toml`). Kept equal to the running daemon's version so no `UpgradeBinary` / rolling handover occurs.
- **Build command:**
  ```sh
  APPLE_SIGNING_IDENTITY="Developer ID Application: Indo Yoon (5DUM8WPB4C)" \
  TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/ferryx-updater.key)" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(head -1 ~/.tauri/ferryx-updater.key.password)" \
  bun tauri build --bundles app
  ```
- **Result:** exit 0, 347 s (warm `src-tauri/target`)
- **Artifacts:** `src-tauri/target/release/bundle/macos/Ferryx.app` plus updater `Ferryx.app.tar.gz` + `.sig`
- Binary SHA-256 (prefix): `3c5a1a06253622efe50aaa9d`, arch `arm64`

## 2. Signing & Notarization

- `codesign --verify --deep --strict`: exit 0; `TeamIdentifier=5DUM8WPB4C`, hardened runtime flag `0x10000`
- Apple Notary Service: **Accepted** (submission id `2cf74b1c-f07d-4354-a65a-4dd7bc31c173`, keychain profile `FerryxNotary`)
- `xcrun stapler staple` + `stapler validate`: OK
- `spctl -a -vv -t exec`: `accepted` / `source=Notarized Developer ID`

## 3. Daemon-Preserving Replacement

- **Baseline:** daemons `869` (legacy peer) + `65111` (canonical, handover from 869 at 10:58); GUI `69921`; 13 live sessions
- GUI quit gracefully via `osascript` (no signal fallback needed)
- Daemon PID set verified **identical before and after** the bundle swap (argv-based classification on the exact executable path)
- Installed bundle moved to `/Applications/Ferryx.app.bak-20260916073621` (`mv` preserves the inode, so both daemons kept executing from the backup)
- Newly built bundle copied to `/Applications/Ferryx.app`; quarantine xattr cleared; `codesign --verify --deep --strict` exit 0
- GUI relaunched: new pid `46424`

## 4. Post-Install Verification

- Daemon probe (UDS, protocol v4): `handshakeOk pid=65111 epoch=1789523938728` — **unchanged**; `daemonVersion 2026.916.1`
- Sessions: **13/13 preserved**, zero PTY disruption, no daemon restart, no rolling handover
- Installed app: `2026.916.1`, `spctl` accepted (`Notarized Developer ID`)
