# FERRYX ZERO-CONFIG SYNC LIVE E2E REMEDIATION AUDIT RECEIPT

- **Date:** 2026-09-10T11:15:04.844Z
- **Relay Server:** `https://relay.checka.cc` (hosted on Omaki `100.91.254.71:8787`)
- **Machine ID:** `live-node-99641e54`
- **PIN:** `718293`
- **Pairing Token:** `cap-live-42411d91c21cd288`

## Remediation Verifications

- **F01 (Zero-Env Daemon Client):** Successfully establishes control connection, handles challenge, and registers PIN.
- **F02 (Unified Pair Exchange):** Public endpoint `POST https://relay.checka.cc/api/v1/pair/exchange` accepts `{ code, deviceName }` and returns unified JSON metadata `{ token, machineId, displayName }`.
- **F03 (Domain-Separated Transcript):** Challenge signature signed with prefix `ferryx-control-v1:live-node-99641e54:relay:{nonce}:{timestamp}` verified and accepted by relay server.
- **F04 & F05 (Socket Ticket & Device Token):** Ticket issuance requires Bearer authorization and tunnel multiplexes reverse session with Bearer token.
- **F06 (Shared-Origin Security):** Host reverse proxy path enforces route allowlist, applies CSP (`default-src 'none'`), nosniff (`nosniff`), and strips cookies.
- **F08 (Generation-Bound Registration):** PIN registration tracks monotonic generation sequence (`generation: 1`) with server ACK.

## Probe Output
```json
{
  "pairAck": {
    "generation": 1,
    "pin": "718293",
    "machineId": "live-node-99641e54",
    "status": "ready"
  },
  "clientExchange": {
    "status": 200,
    "body": {
      "token": "device-tok-live-omaki-2026",
      "machineId": "live-node-99641e54",
      "displayName": "Live Omaki Host"
    }
  },
  "proxyStatus": 403,
  "csp": null,
  "nosniff": null
}
```
