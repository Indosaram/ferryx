# Ferryx Relay & Signaling Connectivity Implementation Plan (Revised v2)

## Overview
Replaces the fragile 3-tier discovery stack (custom mDNS packets, unmanaged SSH child tunnels) with a modern, high-performance signaling-and-relay architecture inspired by WebRTC ICE-lite and Tailscale DERP.

### Core Philosophy
1. Zero-Config Accessibility (Relay-First Default): A single HTTPS/WSS URL with pairing PIN works 100% of the time, regardless of NAT, firewall, or cellular data.
2. Opportunistic Direct Path Upgrade: When running in compatible browser environments and networks, the client optionally probes local/Tailscale candidates. If permissions and connectivity pass, traffic seamlessly upgrades to direct WebSocket, offloading relay bandwidth to zero and dropping latency to 1-2ms.
3. Lightweight Multiplexed Native Rust Relay (`ferryx-relay`): Stream data is never piped through costly per-message serverless functions. Instead, a standalone lightweight Rust relay binary handles framed streaming with bounded queues and backpressure.
4. Clean Security & Credential Boundaries: Clear separation between Machine Tunnel Credentials, Short-Lived Pairing PINs, and Device Session Tokens.

---

## Architecture Breakdown

### 1. Security & Credential Hierarchy
- **Machine Tunnel Credential**:
  - Long-lived secret or ed25519 keypair shared between Desktop Daemon and Relay.
  - Used strictly to authorize outbound reverse tunnel registration.
- **Pairing PIN (Rendezvous Grant)**:
  - 6-digit PIN with strict 60-second expiration and single-use invalidation.
  - Exchanged via Signaling for a scoped Device Session Token.
- **Device Session Token**:
  - Bearer token issued upon approval, stored in client localStorage.
  - Authenticates WebSocket connection without exposing raw tokens in query strings where server access logs can leak them.

### 2. Native Rust Relay Engine (`ferryx-relay`)
- Standalone sub-crate: `src-tauri/crates/ferryx-relay` (Tokio + Axum + Tokio-Tungstenite).
- **Reverse Tunnel & Multiplexing Protocol**:
  - Control Channel: Desktop maintains persistent authenticated control connection (`wss://<relay>/tunnel/control`).
  - Session Dispatch:
    - When a client connects to `wss://<relay>/tunnel/session/<session_id>`, relay notifies desktop over the control channel.
    - Desktop immediately opens a dedicated connection for that session (`wss://<relay>/tunnel/data/<session_id>`) or uses Yamux multiplexing over the primary tunnel.
    - Relay bridges the two sockets via kernel zero-copy (`tokio::io::copy_bidirectional`).
- **Operational & Abuse Guards**:
  - Per-IP connection limits and burst protection.
  - Global and per-session byte rate limits and idle connection reaping (30-second ping/pong heartbeat).

### 3. Signaling & Candidate Discovery Layer
- Lightweight HTTP REST service (runnable on Cloudflare Workers or self-hosted HTTP server).
- Responsibilities:
  - 6-digit pairing code coordination.
  - Candidate endpoint exchange:
    - `lan`: e.g. `http://192.168.1.50:43821`
    - `tailscale`: e.g. `http://100.85.12.34:43821`
    - `relay`: e.g. `wss://relay.example.com`
  - Device revocation registry.

### 4. Permission-Aware Direct Path Upgrade Engine (Client)
- **Browser Constraints Handling**:
  - Modern browsers (Chrome 142+, Firefox, Safari) enforce Private Network Access (PNA) and block Mixed Content from HTTPS origins to private HTTP IPs without explicit preflights/permissions.
  - The client treats direct path as **opportunistic**:
    - Default state: Active and stable on Relay WebSocket.
    - Background probe: Sends PNA-compliant preflight/fetch to candidate endpoints with strict 1500ms timeout and catches PNA/CORS blocks gracefully.
    - If probe fails or is blocked: Continues smoothly on Relay without alarming the user.
    - If probe succeeds: Seamlessly switches the terminal WebSocket transport to direct LAN / Tailscale, preserving xterm.js terminal state.

---

## Detailed Task Breakdown

### Phase 1: Security & Credential Hierarchy
- [ ] Task 1.1: Refactor `AuthManager` to support separate Machine Credentials and Short-Lived Pairing Grants (`src-tauri/src/remote/auth.rs`).
  - Separate machine authorization key from client device tokens.
  - Ensure pairing PIN state is persisted or daemon-managed so CLI processes can interact with the running daemon authority.
- [ ] Task 1.2: Authenticate all remote API routes (`src-tauri/src/remote/server.rs`, `push.rs`).
  - Enforce Bearer token authorization and device validation on `POST /api/push/subscribe` and `POST /api/push/unsubscribe`.
  - Rate-limit subscription size per device.

### Phase 2: Native Rust Relay Server (`ferryx-relay`)
- [ ] Task 2.1: Implement standalone `ferryx-relay` crate with session bridging.
  - Implement `/tunnel/control` for desktop daemon registration and heartbeat.
  - Implement on-demand data channel pairing (`/tunnel/data/:session_id` <-> `/tunnel/client/:session_id`).
  - Use `tokio::io::copy_bidirectional` for fast pass-through byte stream forwarding.
- [ ] Task 2.2: Implement Desktop Daemon Reverse Tunnel Client (`src-tauri/src/remote/relay_client.rs`).
  - Maintains outbound control connection to relay server with auto-reconnect backoff.
  - Spawns dedicated local PTY forwarding pipe whenever relay signals an incoming client session.
- [ ] Task 2.3: Add relay server integration tests and backpressure benchmarks.
  - Verify disconnect/reconnect recovery, token rejection, and burst streaming without memory leak.

### Phase 3: Permission-Aware Direct Path Probing Engine (Client)
- [ ] Task 3.1: Implement `DirectPathUpgrader` in `ui/src/lib/directPathUpgrade.ts`.
  - Probes candidates in priority order (LAN > Tailscale).
  - Handles PNA (Private Network Access) errors and Mixed-Content restrictions without throwing uncaught exceptions.
  - Provides reactive status (`probing` -> `upgraded` or `relay_fallback`).
- [ ] Task 3.2: Integrate Upgrader into `ui/src/remote/RemoteApp.tsx`.
  - Boots on Relay WebSocket immediately.
  - On upgrade confirmation, handshakes direct WebSocket and closes relay socket cleanly.
  - Displays non-intrusive connection pill: `Relay (Active)`, `LAN (Direct 2ms)`, or `Tailscale (Direct 12ms)`.

### Phase 4: UI & Settings Simplification
- [ ] Task 4.1: Refactor Settings Remote Access Section (`ui/src/components/settings/RemoteAccessSection.tsx`).
  - Replaces fragmented mode switches with unified "Remote Access" master switch.
  - Configurable Relay / Signaling Server URL.
  - Single universal QR code generating `https://<relay_or_signaling_url>/#pair=<PIN>`.
- [ ] Task 4.2: Mobile Host Drawer & Top Chrome cleanup (`ui/src/remote/MobileHostDrawer.tsx`).
  - Displays connection transport status (Relay vs Direct) and candidate diagnostics.

### Phase 5: Deprecation & Cleanup
- [ ] Task 5.1: Remove obsolete mDNS and SSH tunnel discovery files:
  - Delete `src-tauri/src/remote/discovery/mdns.rs`.
  - Delete `src-tauri/src/remote/discovery/ssh_tunnel.rs`.
  - Update `discovery/mod.rs` and remove obsolete unit tests.
- [ ] Task 5.2: Final End-to-end verification.
  - Automated test run across backend and frontend.
  - Build checks (`cargo check --lib`, `cargo check --bin ferryx`, `bun run build`).
