# Discovery: C4 Genuine iOS/Android Background Web Push — Implementation Contract

Task st_01a070e0 · 2026-09-05 · Report only; no product changes, commits, or session mutations.
**Verdict:** standards-based PWA/Web Push on the existing web frontend is viable, conditional on an HTTPS origin. The daemon already owns a desktop-independent push event source (`agent_state_tx`). No push code, VAPID material, or `web-push` dependency exists anywhere in the repo today.

**Tooling note:** LSP was unavailable this session (official daemon startup failed: `owner_changed_during_cleanup`, dead prior owner); no LSP symbols/references calls were made or claimed. Every symbol, call site, and line citation in this report was verified by structural/source fallback (exact `read` + `rg` against current working-tree content).

## 1. Verified current state (file:line)

- Remote gateway serves **plain HTTP** over `tokio::net::TcpListener` + `axum::serve`; no TLS acceptor — `src-tauri/src/remote/server.rs:1460-1495`. QR connect URL is `http://{localIp}:{port}` — `ui/src/components/settings/RemoteAccessSection.tsx:62`.
- Routes: `/api/v1/health`, `/pair/exchange`, `/sessions`, `/workspace/*`, `/devices`, `/devices/{id}/revoke`, `/events` (WS), `/terminal/{sessionId}` (WS) — `server.rs:1426-1444`. Token auth via `extract_token` Bearer/query — `server.rs:127-137`; `AuthManager::validate_token` — `src-tauri/src/remote/auth.rs:133`.
- Device model: `DeviceInfo{id,name,permission,created_at,last_seen_at}` — `auth.rs:31-43`; revoke deletes device+tokens outright — `auth.rs:162`; `revoke_device` handler — `server.rs:754-767`.
- `ui/public/sw.js` (37 lines) is a **cache-only** worker: install/activate/fetch, no `push`, no `notificationclick` handlers. Registered for non-Tauri runtimes — `ui/src/main.tsx:54-58`.
- Manifest is PWA-ready: `"display": "standalone"`, 512/192 icons — `ui/public/manifest.webmanifest:6,19-31`.
- `#pair=` hash pairing deep link exists; no task deep link (`#ctx=` or query) is parsed — `ui/src/remote/RemoteApp.tsx:247-268`.
- Remote client already upgrades to `wss:` when page protocol is https — `RemoteApp.tsx:179-183`; attention targets computed from `RemoteActiveDesktopSelection` pushed by the desktop — `RemoteApp.tsx:44-118`, setter `cmd_remote_set_active_selection` — `src-tauri/src/ipc/remote.rs:380-425`.
- **Desktop-only notification path today:** `NotificationCoordinator` gates dispatch on desktop-window focus and calls Tauri IPC `cmd_notification_dispatch` — `ui/src/lib/notificationCoordinator.ts:69-135,143-215`; handler `src-tauri/src/ipc/notifications.rs:70`; registered `src-tauri/src/lib.rs:806`. Events: `agent-task-complete`, `terminal-bell` — `src-tauri/src/notification/model.rs:24-31`; preflight — `notification/service.rs:36-52`.
- **Daemon-owned event source:** `DaemonServer.agent_state_tx: broadcast::Sender<(session_id, state, agent, provider_session)>` — `src-tauri/src/daemon/server.rs:840`; listener socket fed by the in-agent extension (`ferryx-agent-state.ts` sends `working|blocked|idle` per session) — `daemon/server.rs:998-1032`, `src-tauri/src/daemon/protocol.rs:394-402`, `src-tauri/src/resources/agent-extensions/ferryx-agent-state.ts`. Listener starts in `run_server_*` — `daemon/server.rs:1127`.
- Daemon hosts its own `RemoteGatewayState` and can start/restore the gateway without any desktop window — `daemon/server.rs:872-941`, restore `daemon/server.rs:1130-1133`; desktop-only embedded mode is `RemoteGatewayManagerInner::State` — `ipc/remote.rs:22-28`.
- Deps: `tokio 1.43` — `src-tauri/Cargo.toml:52`; `axum 0.8` — `:62`; `reqwest 0.12` rustls-tls — `:65`. Cargo.lock has **no** `web-push`, `p256`, `ecdsa`, or `aes-gcm`; VAPID/encryption stack is a new dependency either way.
- No `web-push|vapid|pushManager` match anywhere outside `node_modules` (rg sweep, this session).

## 2. Platform facts (public web docs; versions to confirm on real devices)

- `PushManager`/`pushManager.subscribe` is secure-context-only; iOS Safari delivers Web Push **only to Home-Screen-installed PWAs** (iOS 16.4+); Android Chrome uses the standard Push API + FCM. Vendored capability data exists but is compact-encoded and was not decoded here — `site/node_modules/caniuse-lite/data/features/push-api.js`. Marked: iOS-version specifics unverified in-repo.
- Consequence: plain-HTTP LAN serving (current state, §1) **cannot** subscribe any phone. The C4 plan itself forbids assuming HTTP-LAN PWA push — `docs/FERRYX_COMPETITIVE_GAPS_PLAN_2026-09-05.md:127`.

## 3. HTTPS / VAPID / credentials

- **HTTPS:** must terminate TLS somewhere for the phone origin (gateway-native rustls, reverse proxy, or Tailscale serve). Decision open; blocks all device QA. No cert/key infrastructure exists in-repo.
- **VAPID:** one P-256 keypair per host, generated on first push enable, persisted `0600` via `write_private_json` (`auth.rs:206-234`) into `remote_data_dir()` (`state.rs:431-447`, honors `FERRYX_DATA_DIR`). Public key served to clients; private key never leaves the daemon.
- **Push services:** third-party (FCM for Chrome; Apple's push service for Safari) — needs daemon outbound HTTPS (`reqwest` rustls already available). No vendor account credentials beyond VAPID required.

## 4. Rust crate

Candidate: `web-push` crate (VAPID + `aes128gcm` payload encryption). crates.io/docs.rs were unreachable from this session, so exact version and feature flags (hyper vs reqwest client) are **unverified** — first implementation step is `cargo add web-push` + inspect features. Fallback: hand-rolled VAPID JWT-ES256 + aes128gcm on `p256`/`aes-gcm` (also new deps). Both keep `reqwest` for transport.

## 5. Event source independence from desktop

- Safe: agent-state edges (`working → blocked|idle`) originate **inside the daemon** via `agent_state_tx` and exist regardless of the desktop window; the sender task subscribes there and reads subscriptions from the daemon's `RemoteGatewayState`. Today these reports are only re-injected into per-session output streams for the desktop UI (`daemon/server.rs:1451`) — no push consumer yet.
- Gap: terminal-bell (`\x07`) detection is frontend-only (`notificationCoordinator.ts:69-135`); the daemon has no bell/output tap. Bell-driven push requires a new daemon-side output scanner — out of this contract's minimal scope; decide separately (§11).
- Non-daemon embedded mode has no agent-state listener (listener lives in `run_server_*`, daemon only) — push there is best-effort desktop-hosted and not background-guaranteed; contract targets daemon mode.

## 6. Proposed minimal API/types

Rust — new `src-tauri/src/remote/push.rs`:
```rust
pub struct PushKeysDto { pub p256dh: String, pub auth: String }
pub struct PushSubscriptionDto { pub endpoint: String, pub keys: PushKeysDto,
    pub expiration_time: Option<u64> }                       // serde camelCase
pub struct StoredPushSubscription { pub device_id: String, pub endpoint: String,
    pub p256dh: String, pub auth: String, pub created_at: u64 }
pub enum PushSendOutcome { Sent, Gone }                      // Gone = HTTP 404/410 -> prune
pub trait PushSender: Send + Sync {   // real web-push client in prod, fake in tests
    fn send_to(&self, sub: &StoredPushSubscription, payload_json: &str) -> Result<PushSendOutcome, String>;
}
```
HTTP (all behind existing `extract_token` + `validate_token`, device bound to token's `device_id`):
- `GET  /api/v1/push/vapid-public-key` → `{"publicKey":"..."}` (`server.rs` router §1)
- `POST /api/v1/push/subscribe` body `{"subscription": PushSubscriptionDto}` → 204; upsert per device
- `POST /api/v1/push/unsubscribe` body `{"endpoint":"..."}` → 204
Persistence: `remote-push.json` next to `remote-auth.json` via `write_private_json`; cascade: `revoke_device` (`auth.rs:162` / handler `server.rs:767`) deletes that device's subscriptions. Sender task (daemon): after `handle_remote_configure` (`daemon/server.rs:1530`), subscribe `agent_state_tx`; notify on `working → blocked|idle` edges, dedupe per session, prune `Gone` endpoints.

UI: `ui/public/sw.js` gains `push` (show notification with payload `{title, body?, url}`) and `notificationclick` (`clients.openWindow(url)`). `RemoteApp.tsx` parses `#ctx=<workspaceId>:<worktreeSlug|>:<tabId|>` alongside `#pair=` (`:247-268`) and routes through the existing `selectContext` flow. Subscribe call (`pushManager.subscribe({userVisibleOnly:true, applicationServerKey})`) fires only from an explicit user tap; Notification permission is never requested implicitly (convention: `src-tauri/src/notification/mod.rs:10-12`).

## 7. Ownership / write scopes

- NEW: `src-tauri/src/remote/push.rs` (+ unit tests), `ui/src/remote/pushClient.ts`, `ui/src/remote/pushDeepLink.ts`.
- EDIT: `remote/mod.rs` (module/re-export), `remote/server.rs` (3 routes + handlers), `remote/auth.rs` or server revoke handler (cascade delete), `daemon/server.rs` (sender task spawn), `ui/public/sw.js`, `ui/src/remote/RemoteApp.tsx` (`#ctx` parse only).
- UNTOUCHED: `src-tauri/src/notification/*`, `ui/src/lib/notificationCoordinator.ts`, desktop IPC commands — push is a separate remote-domain sender; no shared state with desktop notification dispatch.
- Storage keys `ferryx.*` only; no local paths in push payloads (remote AGENTS anti-patterns); never log tokens/endpoints in full.

## 8. RED test seam

- Rust (`src-tauri/src/remote/tests.rs`, existing harness `RemoteGatewayState::new_with_paths_backend` — `state.rs:118-137`): (a) `push_subscribe_requires_valid_token_and_binds_device`; (b) `working_to_blocked_edge_sends_one_push_per_subscribed_device` using a fake `PushSender` recording sends — no network; (c) `gone_endpoint_is_pruned_and_revoked_device_receives_nothing` (revoke via `auth.rs:162`).
- UI: `ui/src/remote/pushDeepLink.test.ts` — `parsePushDeepLink("#ctx=ws:slug:tab")` → exact `{workspaceId, worktreeSlug, tabId}` target; malformed/absent hash → `null`. sw.js payload parsing stays in a pure helper so bun test needs no ServiceWorker runtime.

## 9. Literal real-surface QA

- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote` (remote/AGENTS.md) and `... --lib notification::` (notification/AGENTS.md; untouched pipeline must stay green).
- `bun run --cwd ui test` (ui/package.json:9, `vitest run --maxWorkers=1`); `bun run --cwd ui build` (:8 typecheck).
- `cargo run --manifest-path src-tauri/Cargo.toml -- --daemon` (root AGENTS.md), then `curl -fsS http://127.0.0.1:43821/api/v1/health` → `{"status":"ok","version":"0.1.0"}` (`server.rs:1427`); `curl -fsS -X POST http://127.0.0.1:43821/api/v1/pair/exchange -H 'Content-Type: application/json' -d '{"code":"<PIN>","deviceName":"qa"}'` (`server.rs:1428,199`); `curl -fsS -X POST .../api/v1/push/subscribe -H "Authorization: Bearer <token>" -d '{"subscription":{...}}'` → 204.
- Real device (the only acceptable C4 evidence): HTTPS origin (§3) → iOS Safari ≥16.4: Share → Add to Home Screen → launch from icon → tap "Enable notifications" (explicit) → lock screen / swipe app away → run a real agent to `blocked` → lock-screen push arrives → tap opens PWA on that exact tab. Android Chrome: ⋮ → Install app, same steps. Negative: revoke device from desktop settings → next daemon send hits 404/410 → subscription pruned (verify via `remote-push.json`); deny browser permission → no subscription; expired subscription re-subscribes on next visit. An in-page waiting badge or desktop notification never counts as pass (plan :128).

## 10. Blockers — true external prerequisites (no synthetic substitute exists)

1. **HTTPS origin for the phone** — unavoidable; nothing in-repo provides it. Without TLS there is no `pushManager` on any real device; simulating receipt from a page-open WS event proves nothing about background delivery.
2. **Outbound internet from daemon to push services** (FCM/Apple) during QA.
3. VAPID keypair provisioning per host (generated, but needs a stable per-host data dir — `FERRYX_DATA_DIR` or OS dir, `state.rs:431-447`).

## 11. Remaining decisions (owner: user)

- HTTPS termination owner: gateway-native rustls vs reverse proxy vs Tailscale serve (blocks all device evidence).
- Crate: `web-push` (verify version/features online) vs hand-rolled p256+aes128gcm.
- Bell-driven push in C4 scope, or agent-state edges only for now (§5 gap).
- Push payload body exposure default (plan requires a body-visibility setting; propose title-only default, body opt-in on subscribe).
- Minimum iOS version acceptance (16.4) and Android WebView/other-browser fallback policy (declare unsupported rather than fake delivery).
