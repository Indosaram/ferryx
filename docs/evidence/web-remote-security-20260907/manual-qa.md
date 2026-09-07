# Web Remote Security Manual QA Evidence

Executed on 2026-09-06 at 23:28 UTC against the running isolated QA fixture (`src-tauri/examples/web_remote_security_qa`) in worktree `/Users/indo/code/project/ferryx-web-remote-security-20260907`.
Fixture runs real production Axum router (`create_remote_router`), real `AuthManager`, real PTY manager, and loopback listener.

## Matrix

| # | Scenario | Exact command / action | Expected | Observed | Verdict | Artifact |
|---|----------|------------------------|----------|----------|---------|----------|
| 1 | Static path traversal blocked | `curl --path-as-is /../../etc/hosts` | HTTP 400 Bad Request, 0 bytes leaked, normal assets/index served with 200 OK | HTTP 400 Bad Request (0 bytes), index.html served 200 OK, CSS asset served 200 OK | PASS | #scenario-1 |
| 2 | Device revocation invalidates open sockets | Connect events+terminal WebSockets on Device A, revoke Device A via Device B token | Both Device A WebSockets close immediately upon revocation; Device B WebSockets unaffected | Device A events and terminal WebSockets closed immediately on revocation; Device B remained open and received broadcast event | PASS | #scenario-2 |
| 3 | View device permission boundaries | Viewer tries revoking another device (403 expected), then self-revokes (204 expected) | HTTP 403 for other device (victim remains), HTTP 204 for self-revoke, token rejected 401 after | HTTP 403 on revoking other device, victim remained paired; HTTP 204 on self-revoke, subsequent calls returned 401 | PASS | #scenario-3 |
| 4 | Pairing attempt rate limiting | Submit 5 invalid PINs, then submit the valid PIN on 6th attempt | 5x HTTP 400 Bad Request, 6th attempt returns HTTP 429 with `{"code":"pairing_rate_limited"}` | First 5 attempts returned 400; 6th attempt returned HTTP 429 `{"code":"pairing_rate_limited"}` locking out even the valid PIN | PASS | #scenario-4 |

---

## Detailed Transcripts

### Scenario 1: Static Path Traversal Blocked
```http
curl --path-as-is -i http://127.0.0.1:50948/../../../../../../../../../../../../../../../../etc/hosts
HTTP/1.1 400 Bad Request
vary: origin, access-control-request-method, access-control-request-headers
access-control-allow-origin: *
content-length: 0
date: Sun, 06 Sep 2026 23:27:44 GMT
```
- Normal asset `/assets/index-zcQS6jqy.css`: HTTP 200 OK, content-type `text/css`.
- Normal SPA route `/`: HTTP 200 OK, returns `<!doctype html>...<div id="root"></div>`.

### Scenario 2: Device Revocation Invalidates Open WebSockets
- Device A (`VictimControl`) paired -> token issued.
- Device B (`AdminControl`) paired -> token issued.
- WebSocket `/api/v1/events?token={tokenA}` -> `open`.
- WebSocket `/api/v1/terminal/{sessionId}?token={tokenA}` -> `open`.
- WebSocket `/api/v1/events?token={tokenB}` -> `open`.
- Revoke Device A via Device B: `POST /api/v1/devices/{deviceA_id}/revoke` with `Bearer {tokenB}` -> HTTP 204 No Content.
- Device A events WebSocket -> `close` event received immediately (readyState = CLOSED).
- Device A terminal WebSocket -> `close` event received immediately (readyState = CLOSED).
- Device B events WebSocket -> remains OPEN.
- Broadcast test message `live-check-b` via `state.emit_event` -> received on Device B events WebSocket payload `live-check-b`.

### Scenario 3: View Device Permission Boundaries
- Viewer paired with permission `View` -> token issued.
- Victim paired with permission `Control` -> token issued.
- Viewer sends `POST /api/v1/devices/{victim_id}/revoke` -> HTTP 403 Forbidden: `View-only device cannot revoke another device`.
- `GET /api/v1/devices` confirms victim device still exists.
- Viewer sends `POST /api/v1/devices/{viewer_id}/revoke` (self-revoke) -> HTTP 204 No Content.
- Subsequent calls with viewer token return HTTP 401 Unauthorized.

### Scenario 4: Pairing Attempt Rate Limiting
- Pairing code generated.
- Attempt 1-5 with invalid code `000000`:
  - Request: `POST /api/v1/pair/exchange`
  - Status: HTTP 400 Bad Request (`Invalid pairing code`)
- Attempt 6 with the VALID generated code:
  - Request: `POST /api/v1/pair/exchange`
  - Status: HTTP 429 Too Many Requests
  - Body: `{"code":"pairing_rate_limited"}`
