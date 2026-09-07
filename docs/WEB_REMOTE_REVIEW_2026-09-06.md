# Review Work - Final Report

## Overall Verdict: PASSED

Scope: Ferryx web/mobile remote access security repairs and verification across Axum HTTP/WebSocket surfaces and authentication manager.

| Lane | Verdict | Confidence |
|------|---------|------------|
| Manual QA (4 rows, 4 artifacts) | PASS | - |
| Gate review | APPROVE | HIGH |

## Blocking Issues
(None. All four previously blocking security issues are resolved and verified.)

## Key Findings

### 1. Static Path Traversal Blocked (P1 Resolved)
- **Implementation**: `src-tauri/src/remote/server.rs:1424-1482`. `static_relative_path` performs percent-decoding, strictly rejects forbidden portable components (`\`, `:`, `%`, `\0`, `.`/`..`), and verifies canonical path containment under the asset root via `tokio::fs::canonicalize` and `.starts_with(&root)`. Out-of-root symlink targets return HTTP 404; invalid path traversals return HTTP 400.
- **Verification**: `curl --path-as-is http://127.0.0.1:<port>/../../etc/hosts` returned HTTP 400 Bad Request with 0 bytes leaked. Valid UI assets (`/assets/index-zcQS6jqy.css`) and SPA root navigation (`/`) returned HTTP 200 OK. Unit tests in `security_tests.rs` pass.

### 2. Device Revocation Invalidates Active WebSockets (P1 Resolved)
- **Implementation**: `src-tauri/src/remote/auth.rs:74, 199-232` and `src-tauri/src/remote/server.rs:826-859, 897-940, 979-1078, 1148-1366`. Per-device latched `watch` channels ensure cancellation is never lost even if upgrade has not started. All socket pump loops (events, raw terminal, native grid terminal) are pinned local futures (`std::pin::pin!`) driven by biased cancellation (`while_device_authorized`).
- **Verification**: Live QA fixture verified that revoking Device A immediately forces both its active events WebSocket and active terminal WebSocket to close (code 1006). Device B WebSockets remained open, unaffected, and continued receiving broadcast events.

### 3. View Device Permission Boundaries Enforced (P2 Resolved)
- **Implementation**: `src-tauri/src/remote/server.rs:796-815`. A device with `View` permission attempting to revoke another device ID receives HTTP 403 Forbidden (`View-only device cannot revoke another device`). A `View` device revoking itself (self-revoke / logout) succeeds with HTTP 204 No Content, after which its token is rejected with HTTP 401 Unauthorized. `Control` devices retain full peer device revocation.
- **Verification**: Live QA fixture verified that a `View` device attempting to revoke a peer returned HTTP 403, and the peer remained active. Self-revoke returned HTTP 204, and subsequent requests with the revoked token returned HTTP 401.

### 4. Global Server-Side Pairing Attempt Budget (P1 Resolved)
- **Implementation**: `src-tauri/src/remote/auth.rs:10, 41-60, 128-145` and `src-tauri/src/remote/server.rs:212-218`. A strict 5-failure limit per 60-second window is tracked under `parking_lot::RwLock`. Consecutive invalid PIN attempts return HTTP 400. Once 5 failures occur, the 6th attempt (even with a valid generated PIN) returns HTTP 429 Too Many Requests with JSON payload `{"code":"pairing_rate_limited"}`. Window refresh retains unexpired codes (`created_at < 60s`), decoupling PIN lifespan from window rollover.
- **Verification**: Live QA fixture verified 5 consecutive invalid PIN attempts returned HTTP 400, followed by the 6th attempt with the genuine PIN returning HTTP 429 `{"code":"pairing_rate_limited"}`. Unit test `late_generated_pin_survives_failure_window_rollover` proves legitimate codes crossing window boundaries retain their full 60-second lifetime.

## Manual QA Matrix

| # | Scenario | Exact command / action | Expected | Observed | Verdict | Artifact |
|---|----------|------------------------|----------|----------|---------|----------|
| 1 | Static path traversal blocked | `curl --path-as-is /../../etc/hosts` | HTTP 400 Bad Request, 0 bytes leaked, normal assets/index served with 200 OK | HTTP 400 Bad Request (0 bytes), index.html served 200 OK, CSS asset served 200 OK | PASS | docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-1 |
| 2 | Device revocation invalidates open sockets | Connect events+terminal WebSockets on Device A, revoke Device A via Device B token | Both Device A WebSockets close immediately upon revocation; Device B WebSockets unaffected | Device A events and terminal WebSockets closed immediately on revocation (code 1006); Device B remained open and received broadcast event | PASS | docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-2 |
| 3 | View device permission boundaries | Viewer tries revoking another device (403 expected), then self-revokes (204 expected) | HTTP 403 for other device (victim remains), HTTP 204 for self-revoke, token rejected 401 after | HTTP 403 on revoking other device, victim remained paired; HTTP 204 on self-revoke, subsequent calls returned 401 | PASS | docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-3 |
| 4 | Pairing attempt rate limiting | Submit 5 invalid PINs, then submit the valid PIN on 6th attempt | 5x HTTP 400 Bad Request, 6th attempt returns HTTP 429 with `{"code":"pairing_rate_limited"}` | First 5 attempts returned 400; 6th attempt returned HTTP 429 `{"code":"pairing_rate_limited"}` locking out even the valid PIN | PASS | docs/evidence/web-remote-security-20260907/manual-qa.md#scenario-4 |

## Recommendations
1. **Uncommitted Working-Tree Notice**: The security repairs and evidence files are uncommitted in the working tree. Concurrent agent sessions sharing the repository could affect these files. It is recommended to commit these changes when appropriate.
2. **Reverse Proxy Considerations**: For deployments behind Cloudflare Tunnels or reverse proxies, ensure HTTP 429 status codes and WebSocket close frames are forwarded transparently to web/mobile clients.
3. **Pre-existing Scoped Control Test**: The pre-existing failure in `tests/scoped_control.rs` (`control::lease::rejects_competing_controller_until_expiry_or_revoke`) should be addressed separately as part of the `ferryx_scope` feature track.
