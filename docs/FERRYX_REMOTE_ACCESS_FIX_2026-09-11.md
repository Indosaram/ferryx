# Ferryx Remote Access Diagnosis & Resolution (2026-09-11)

## Overview
Investigation and resolution of remote access failures spanning relay web serving, local LAN pairing PIN redemption, ticket issuance for terminal WebSockets, and daemon remote status metadata.

## Root Causes Identified

1. **Relay Server Static Web App 404 (Missing Fallback)**
   - `relay_router` on `ferryx-relay` (deployed at `https://relay.checka.cc`) only registered API tunnel routes (`/tunnel/*`, `/api/v1/pair/exchange`, etc.) without a fallback route for web clients.
   - Accessing `https://relay.checka.cc/` or QR code link `https://relay.checka.cc/#pair=...` returned 404 Not Found, preventing users from opening the Ferryx Remote Web App.

2. **Direct LAN Pairing Code Redemption Failure**
   - When generating pairing codes via `ferryx-cli pair generate` or GUI `RemoteAccessSection`, `PairingCoordinator` registered only the 32-character hex `pairing_token` into the local `AuthManager`, omitting the 6-digit numeric `pin`.
   - As a result, direct LAN connection (`POST /api/v1/pair/exchange`) using the 6-digit PIN failed with HTTP 400 `Invalid pairing code`.

3. **Frontend WebSocket Ticket Bypass / Query Token Rejection**
   - In `RemoteTerminal.tsx`, direct socket URLs constructed `${proto}//${host}:${port}/api/v1/terminal/${terminalId}?token=${deviceToken}`.
   - The remote gateway's security enforcement rejected query parameter tokens with `a permanent device token in the query string is rejected`.
   - The frontend needed to issue a short-lived ticket via `POST /api/v1/ws/ticket` using `remoteSocketUrl` and connect to `/ws/terminal/<id>?ticket=<ticket>`.

4. **Daemon Status Metadata Omission**
   - `DaemonRemoteStatus` and `RemoteGatewayStatusResponse` lacked `machine_id`, `relay_connected`, and `control_channel_connected` fields, making it impossible for the GUI/CLI to report relay connection health accurately.

## Changes Made

1. **`src-tauri/src/remote/relay_server.rs` & `server.rs`**
   - Exposed `serve_static_or_index` as `pub(crate)` and attached `.fallback(axum::routing::get(crate::remote::server::serve_static_or_index))` to `relay_router`.
   - Deployed release build of `ferryx-relay` to `omarchy` (`100.91.254.71:8787` / `https://relay.checka.cc`), and synced built `ui/dist` assets.
   - Verified `curl https://relay.checka.cc/` serves `index.html` (HTTP 200) and assets with correct MIME types.

2. **`src-tauri/src/remote/relay_client.rs` & `auth.rs`**
   - In `PairingCoordinator::generate_pairing_with_permission`: registered both `pin` and `pairing_token` into `self.auth` with the requested device permissions.
   - Added automated unit test `test_pairing_coordinator_pin_can_be_redeemed_directly` verifying that generated PINs can be exchanged directly via `AuthManager::exchange_pairing_code_with_installation`.

3. **`src-tauri/src/daemon/protocol.rs`, `server.rs`, `client.rs`, `ipc/remote.rs`**
   - Added optional fields `machine_id`, `relay_connected`, `control_channel_connected` to `DaemonRemoteStatus` and `RemoteGatewayStatusResponse`.
   - Added `pairing_token`, `machine_id` to `DaemonResponse::RemotePairingCodeOk` and `CreatePairingCodeResponse`.
   - Implemented `remote_create_pairing_code_detailed` on `DaemonClient`.

4. **`ui/src/remote/RemoteTerminal.tsx`**
   - Updated `terminalSocketUrl` to issue a short-lived ticket via `remoteSocketUrl` and connect with `?ticket=` on production direct/all paths, avoiding insecure token query strings.

5. **`tests/zero_config_gen5_regression.rs`**
   - Updated pattern match for `DaemonResponse::RemotePairingCodeOk` to `{ code, .. }`.

## Verification Results

- `cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_gen5_regression`: 6/6 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote`: 222/222 passed (excluding unprivileged local root sshd test).
- `bun run --cwd ui build`: Built in 2.27s cleanly.
- `vitest run --maxWorkers=1 src/remote/`: 145/145 passed across 12 test suites.
- `vitest run --maxWorkers=1 src/components/settings/RemoteAccessSection.test.tsx`: 20/20 passed.
- Public Relay Deployment (`https://relay.checka.cc`):
  - `GET /` -> HTTP/2 200 text/html (Ferryx Web App)
  - `GET /assets/index-owSRIDIs.js` -> HTTP/2 200 text/javascript
  - `GET /manifest.webmanifest` -> HTTP/2 200 application/manifest+json
  - `GET /sw.js` -> HTTP/2 200 text/javascript
