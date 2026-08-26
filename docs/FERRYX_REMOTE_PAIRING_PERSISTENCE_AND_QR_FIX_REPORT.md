# Ferryx Remote Pairing Persistence and QR Generation Fix Report

Date: 2026-08-26

## 1. Summary of Changes

This fix resolves two issues in the Ferryx Remote Access subsystem:
1. **Remote Authorization Persistence Across Browser Reload/Reopen**:
   - Remote browser client (`ui/src/remote/RemoteApp.tsx` and `ui/src/lib/remoteClient.ts`) now retains paired tokens across page reload and reopen on the same browser profile.
   - `loadWorkspace()` and `selectContext()` now selectively disconnect only on authentication rejections (`401 Unauthorized` or `403 Forbidden`, such as when a device is revoked from Ferryx Desktop), rather than wiping the token on transient network or 5xx gateway errors.
   - Unified token storage using canonical helper methods `getRemoteAuthToken`, `setRemoteAuthToken`, and `clearRemoteAuthToken` in `ui/src/lib/remoteClient.ts` with transparent migration support for legacy storage keys.
2. **Desktop Remote Access Settings QR Code Generation**:
   - In `ui/src/components/SettingsDialog.tsx`, when Remote Access is already Active (e.g. on settings dialog open or restored listener), the UI now immediately triggers QR code generation and displays the pairing PIN and QR image rather than getting permanently stuck in `Generating...`.
   - `generatePairing` callback is stabilized via `statusRef` with empty dependencies (`[]`), preventing infinite re-render loops or duplicate `createPairingCode` calls on mount.
   - Any QR code generation failure is explicitly surfaced in the UI with a `Retry` action rather than leaving the user with an indefinite loading placeholder.
   - The user is always provided with a "New Code" / "Generate Code" action to refresh the QR pairing code on demand while Active, even when paired devices exist.
   - Updated the stale Remote Access copy to accurately reflect that existing authorized browser profiles reconnect while Remote remains enabled, and only require re-pairing when browser storage is cleared, a device is revoked, or a different profile/device is used.

## 2. Files Changed

- `ui/src/lib/remoteClient.ts`: Canonical auth token helpers supporting legacy migration and unified clearing.
- `ui/src/lib/remoteClient.test.ts`: Regression tests for storage helpers.
- `ui/src/remote/RemoteApp.tsx`: Uses canonical storage helpers; restricts `disconnect()` to explicit disconnect actions and 401/403 auth errors.
- `ui/src/remote/RemoteUI.test.tsx`: Regression tests for transient server error retention, 401 revocation disconnect, and legacy token migration.
- `ui/src/components/SettingsDialog.tsx`: Stable `generatePairing` callback, initial mount QR auto-generation when Active, error surface with retry, and updated accuracy copy.
- `ui/src/components/SettingsDialog.test.tsx`: Regression tests asserting single `createPairingCode` on Active mount, "New Code" refresh, error retry, and updated policy copy.

## 3. Red -> Green Test Evidence

### Initial Red Run
1. `SettingsDialog > automatically generates and displays a new QR code when Remote Access is already Active with paired devices present`:
   - **Failed**: `AssertionError: expected "spy" to be called 1 times, but got 0 times`
2. `SettingsDialog > surfaces a QR generation failure with a retry option rather than leaving indefinite Generating... loading`:
   - **Failed**: `TestingLibraryElementError: Unable to find an element with the text: /Pairing creation failed on daemon/i`
3. `Remote UI Components > retains authorization across normal page reload when server returns transient error`:
   - **Failed**: `AssertionError: expected null to be 'paired-device-token'`

### Green Run
- `bun run --cwd ui test src/components/SettingsDialog.test.tsx src/remote/RemoteUI.test.tsx src/lib/remoteClient.test.ts`: PASS (43 tests passed)
- Full UI suite (`bun run --cwd ui test`): PASS (89 test files, 782 tests passed)
- TypeScript build (`bun run --cwd ui build`): PASS (`tsc && vite build` 0 errors)
- Rust remote tests (`cargo test --manifest-path src-tauri/Cargo.toml --lib remote`): PASS (48 tests passed)

## 4. Residual Limitations

- Browser privacy modes or automated profile clearing (e.g. Incognito closing) will naturally clear `localStorage`, requiring re-pairing as designed.
- Native desktop IPC commands (`cmd_remote_pairing_create`) still enforce standard 60-second PIN expiration and single-use exchange semantics.
