# FERRYX ZERO-CONFIG SYNC LIVE E2E REMEDIATION RECEIPT

- **Date:** 2026-09-10T12:27:09.714Z
- **Relay Host:** Omaki (`100.91.254.71:8787`, `https://relay.checka.cc`)
- **Machine ID:** `live-node-230adabc` (ephemeral synthetic test machine)
- **PIN:** `[REDACTED]` (6-digit numeric, expired)
- **Pairing Token:** `[REDACTED]` (ephemeral capability token, expired)
- **Device Token:** `[REDACTED]` (synthetic, never a real device credential)

## SCOPE AND LIMITS OF THIS RECEIPT

This is **relay transport and response-header smoke evidence only**. It is NOT a complete
product end-to-end attestation, and it must not be cited as closure for F01–F10.

What this receipt does NOT establish:

- **Deployed binary/commit identity.** The probe did not record the hash of the running
  `ferryx-relay` binary or its source commit.
- **The daemon side is synthetic.** The `/tunnel/control` peer and the reverse-tunnel
  responder were a purpose-built probe script, not the real Ferryx daemon, GUI, or CLI.
  Consequently the health body shown below (`{"status":"ok","machineId":...}`) is the
  *probe's* response, not the gateway's. The real gateway `HealthResponse`
  (`src-tauri/src/remote/server.rs:146-150`) contains only `status` and `version` and has
  **no `machineId` field**. Nothing here validates a `machineId`-bearing health contract.
- **No authenticated terminal attachment.** A ticket HTTP 200 proves issuance only — not
  grid negotiation, terminal input/output, event delivery, or generation-safe reconnect.
- **Only the warm-cache admission path.** The pair exchange precedes the token checks, so
  the relay's device-token cache was already populated. Before-first-pairing admission,
  cache loss across relay restart, revoked entries, and preexisting valid devices are
  **not** covered here. See `FERRYX_ZERO_CONFIG_SYNC_FINAL_VERDICT_GEN2_2026-09-10.md`
  section 3.1.
- **No multi-host, revocation, expiry, or owner-replacement cases.**

## Behaviors actually observed in this run

- **Control handshake (partial F01/F03 evidence):** A zero-env client connected to
  `/tunnel/control`, completed the Ed25519 handshake, and registered a control session.
  This exercised the relay's handshake verification; it does not establish daemon-owned
  pairing, since the peer was the probe and not the daemon.
- **Pair exchange routing:** `POST https://relay.checka.cc/api/v1/pair/exchange` accepted
  `{ code, deviceName }`, dispatched over the reverse tunnel, and returned the responder's
  JSON metadata (HTTP 200). This validates relay request/response forwarding, not the real
  gateway's exchange contract or identity binding.
- **Domain-separated transcript:** Handshake challenge signed as
  `ferryx-control-v1:live-node-230adabc:relay:{nonce}:{timestamp}` was verified and accepted.
- **Ticket admission, warm cache only:**
  - Unissued token rejected with HTTP `401` Unauthorized.
  - Recorded device token accepted with HTTP `200` OK, issuing a ticket.
  - **Caveat:** this is the populated-cache case. Cold-cache and post-restart admission are
    unverified here and were reproduced as defective by the external audit.
- **Colon-scoped session target:** Ticket target
  `/api/v1/terminal/workspace:session-main` was accepted with colon scoping.
- **Shared-origin response headers (F06 header defects):**
  - Proxy health endpoint `GET /host/live-node-230adabc/api/v1/health` returned HTTP `200` OK with body `{"status":"ok","machineId":"live-node-230adabc"}`.
  - `Content-Security-Policy`: `default-src 'none'`
  - `X-Content-Type-Options`: `nosniff`
  - `Cache-Control`: `no-store, private`
  - `Pragma`: `no-cache`
  - `Set-Cookie`: `none (stripped)`
- **PIN & capability format:** The relay accepted a well-formed 6-digit PIN and capability
  token. Rejection of malformed inputs is covered by unit tests, not by this run. Note the
  relay does **not** bound the capability lease lifetime; see verdict section 3.3.
- **Registration generation:** The ACK recorded `generation: 1`. An ACK echoing a
  generation is **not** proof of monotonic generation enforcement, which the external audit
  disproved by loopback probe.

## Live Execution Output

Credential-like values are redacted. All were ephemeral synthetic test values scoped to a
throwaway machine ID and have since expired; none corresponded to a real device credential.
The `machineId` inside `clientExchange.body` and the health body originated from the **probe's
reverse-tunnel responder**, not from the real gateway.

```json
{
  "pairAck": {
    "generation": 1,
    "pin": "[REDACTED]",
    "machineId": "live-node-230adabc",
    "status": "ready"
  },
  "clientExchange": {
    "status": 200,
    "body": {
      "token": "[REDACTED]",
      "machineId": "live-node-230adabc",
      "displayName": "Omaki Production Host"
    }
  },
  "invalidTicketStatus": 401,
  "validTicketResp": {
    "status": 200,
    "body": {
      "ticket": "[REDACTED]",
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
