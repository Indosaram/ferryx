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

### Deployed artifact identity

Recorded after the fact on the Omaki host (`systemctl is-active ferryx-relay.service` ->
`active`):

- Binary: `/home/indo/bin/ferryx-relay`
- SHA-256: `bb1b5520068495759cc437e2cb9bbf251971f2c7615e9c3a03ba7cebfb52902d`
- Size / mtime: `4710200` bytes, `2026-09-10T21:26:55` (host local time)
- Release profile. Installed binary is byte-identical to the build artifact at
  `/home/indo/ferryx-relay-src/src-tauri/target/release/ferryx-relay` (both
  `bb1b5520...52902d`), so the installed copy is the one that was compiled on the host.

Source attribution, verified rather than asserted: the relay source synced to Omaki at
`/home/indo/ferryx-relay-src/src-tauri/src/remote/relay_server.rs` hashes
`198dd3c18201b39e9b4c2f969e1cb682ef9285adc9ec3cbb2c86d13def12032e`, which equals
`git show 54b0801:src-tauri/src/remote/relay_server.rs` and also `git show HEAD:...` for the
same path (commits after `54b0801` on this branch are documentation-only). The deployment is
synced source plus an on-host `cargo build --release`, not a git checkout, so this is a
content-hash correspondence for the relay module rather than a whole-tree provenance proof.

Caveat: the binary hash was captured after the probe run, not atomically with it. It matches
the binary in place during the run because the service was neither rebuilt nor restarted
between the probe and this capture, but that ordering is an operational assertion.

What this receipt does NOT establish:
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

## Addendum: F04 cold-cache fail-open reproduced on the live relay

The external Gen2 audit reported that relay ticket admission fails open when the relay has no
cached device token for a machine. This was independently reproduced against the deployed
production relay, confirming the finding and superseding this receipt's earlier
"F04 verified" reading (which exercised only the warm-cache path).

Method: complete a real Ed25519 control handshake for a fresh machine ID, then request a
socket ticket **without performing any pair exchange**, presenting a bearer that was never
issued anywhere.

- Machine ID: `coldcache-e3508e3a` (ephemeral)
- Request: `POST https://relay.checka.cc/host/coldcache-e3508e3a/api/v1/socket-ticket`
  with `Authorization: Bearer never-issued-anywhere-000`
- **Result: HTTP 200 with an issued ticket.** Expected: 401.

Root cause, confirmed in source at `src-tauri/src/remote/relay_server.rs`:

- line 227 `is_valid_device_token` returns the cache lookup combined with `is_none_or`, so a
  cache **miss** yields `true`;
- line 204 initializes `paired_tokens` as an empty in-memory `HashMap`, so the hole reopens
  on every relay restart, not just before a machine's first exchange.

The line 225 comment ("Machines not yet paired through this relay retain gateway-side
authentication") does not hold for this path: the relay issues the ticket itself and never
consults the gateway on a miss.

Scope of impact: this is relay admission only. The gateway remains the authenticating and
revocation-enforcing authority for the subsequent stream, so this reproduction does not
demonstrate unauthorized terminal control. It does allow an unauthenticated caller to consume
the shared ticket budget and induce backend work.

## Addendum 2: F04 fix verified live on the hardened relay

After deploying the remediation (worktree commit `c5fea8b`), the same probe that
previously succeeded now fails closed.

Deployed binary: `/home/indo/bin/ferryx-relay`, SHA-256
`afd03af3a5b8a9dbac78a89644cc8a87d5b25b8b4ed6ba5791eedd5d1b4890d4`
(`systemctl is-active` -> `active`). Supersedes the earlier `bb1b5520...52902d` build.

### Negative case - gateway rejects the bearer

- Machine `coldcache3-d6e4a4bd`: real Ed25519 control handshake, no pair exchange, bearer
  `never-issued-anywhere-000`. The relay consulted the owning gateway over the reverse
  tunnel; the gateway answered `401`.
- **Result: HTTP 401** `{"error":"Device token not authorized for this machine"}`.
- Before the fix this identical request returned **HTTP 200 with a valid ticket**.

### Positive case - gateway confirms the bearer

- Machine `warmpath-*`, bearer `gateway-approved-token-001`, target
  `/api/v1/terminal/workspace:session-main`; gateway answered `200`.
- **Result: HTTP 200** with an issued ticket, so admission is not simply denying everything.

### Unknown machine

- `POST /host/unknown-machine-xyz/api/v1/socket-ticket` -> **HTTP 404**, disclosing no
  information about whether a machine or token exists.

Residual limits unchanged: the control peer and gateway responder are still a synthetic
probe, so this validates relay-side admission and its gateway-consultation path, not the real
daemon's token store or an authenticated terminal attachment. F10 remains open.

## Addendum 3: commit-bound in-process E2E (F10)

The auditor required an E2E receipt bound to a commit, covering a real
browser/relay/gateway chain rather than a synthetic responder. The synthetic
probes above cannot satisfy that, so the coverage now lives in an executable test
instead of a hand-written transcript.

- Commit: `a50a465` (extends `coordinator_pairs_through_relay_to_real_gateway` in
  `src-tauri/src/remote/relay_server.rs`).
- Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::`
  -> `test result: ok. 163 passed; 0 failed` (exit 0) at HEAD
  `6261a9f0b1ed661fd2462bdaccf061a35081b162`.

### What is real in that test

- Real relay router and real `RelayClient` control channel (Ed25519 handshake).
- Real `RemoteGatewayState` behind the real `create_remote_router`, on a real socket.
- Real pairing via `PairingCoordinator`, real `/api/v1/pair/exchange` over the relay,
  and the issued token validated through the gateway's own `auth_manager`.
- Real PTY: `/bin/sh` spawned via `spawn_in_worktree` in a real `git init` worktree,
  on the same `TerminalService`/`WorkspaceRegistry` the gateway serves.

### Properties asserted

- Unissued bearer is refused a socket ticket; the issued device token is granted one.
- A pairing code cannot be exchanged twice (replay rejected).
- A valid ticket streams no terminal data while the session is not the active desktop
  selection.
- After the desktop declares the selection, a fresh ticket attaches over the relay and
  a marker written into the shell comes back out through the relay-proxied socket.
- The single-use ticket cannot authorize a second attachment.
- No permanent credential appears in the WebSocket URL; only the single-use ticket.

### Proof the attachment assertion is not vacuous

Writing a different marker into the shell makes the test consume its full 20s
deadline and fail with "real PTY output must traverse the relay to the paired
browser", so the echo is genuinely observed rather than assumed.

### Residual gap

This is an in-process Rust harness: the browser side is the relay's HTTP/WS client
surface, not a real browser engine, and it does not cover two machines, revocation
mid-stream, expiry, owner replacement, or restart recovery. Those remain open.
