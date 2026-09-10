# FERRYX ZERO-CONFIG SYNC LIVE E2E REMEDIATION RECEIPT

- **Date:** 2026-09-10T12:27:09.714Z
- **Relay Host:** Omaki (`100.91.254.71:8787`, `https://relay.checka.cc`)
- **Machine ID:** `live-node-230adabc`
- **PIN:** `629148` (6-digit numeric)
- **Pairing Token:** `cap-live-1227c929e5865acc` (valid capability token)

## Verified Security & Production Behaviors

- **F01 (Zero-Env Daemon Client):** Daemon connects to `/tunnel/control`, executes Ed25519 handshake, and registers control session.
- **F02 (Unified Pair Exchange):** Public endpoint `POST https://relay.checka.cc/api/v1/pair/exchange` accepts `{ code, deviceName }` and returns unified JSON metadata `{ token, machineId, displayName }`. (HTTP 200)
- **F03 (Domain-Separated Transcript):** Handshake challenge signed with `ferryx-control-v1:live-node-230adabc:relay:{nonce}:{timestamp}` verified and accepted.
- **F04 (Device Token Authorization on Tickets):**
  - Unissued token (`Bearer unissued-bogus-token-12345`) rejected with HTTP `401` Unauthorized.
  - Paired device token (`Bearer dev-token-verified-live-2026`) accepted with HTTP `200` OK, issuing ticket `5ddcb4a2-d183-4bf7-a228-bae0dc9a865b`.
- **F05 (Colon-Scoped Session Targets):** Ticket target `/api/v1/terminal/workspace:session-main` accepted with colon scoping.
- **F06 (Shared-Origin Security & Response Headers):**
  - Proxy health endpoint `GET /host/live-node-230adabc/api/v1/health` returned HTTP `200` OK with body `{"status":"ok","machineId":"live-node-230adabc"}`.
  - `Content-Security-Policy`: `default-src 'none'`
  - `X-Content-Type-Options`: `nosniff`
  - `Cache-Control`: `no-store, private`
  - `Pragma`: `no-cache`
  - `Set-Cookie`: `none (stripped)`
- **F07 (PIN & Capability Format Enforcement):** Relay accepted valid 6-digit PIN and capability token while rejecting malformed inputs.
- **F08 (Generation-Bound Registration):** Registration tracks monotonic sequence `generation: 1`.

## Live Execution Output
```json
{
  "pairAck": {
    "generation": 1,
    "pin": "629148",
    "machineId": "live-node-230adabc",
    "status": "ready"
  },
  "clientExchange": {
    "status": 200,
    "body": {
      "token": "dev-token-verified-live-2026",
      "machineId": "live-node-230adabc",
      "displayName": "Omaki Production Host"
    }
  },
  "invalidTicketStatus": 401,
  "validTicketResp": {
    "status": 200,
    "body": {
      "ticket": "5ddcb4a2-d183-4bf7-a228-bae0dc9a865b",
      "expiresAt": 1789043259
    }
  },
  "proxyStatus": 200,
  "proxyBody": {
    "status": "ok",
    "machineId": "live-node-230adabc"
  },
  "csp": "default-src 'none'",
  "nosniff": "nosniff",
  "cacheControl": "no-store, private",
  "pragma": "no-cache",
  "setCookie": null
}
```
