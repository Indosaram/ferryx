# Direct peer path (P2P) and direct trust

> Status: source only. Not deployed in any release.
> Verification: PENDING. Compile, tests, and a Windows smoke run have not been
> reported yet. Nothing here claims the direct path works end to end.

## What it is

Paired machines normally carry all traffic through the relay. The direct path lets
two paired machines talk over a peer-to-peer QUIC connection instead. The relay is
still used for signaling (offer/answer), but it is **untrusted**.

## Trust model

- A peer's public key is **never** learned from the relay, an offer, or an answer.
  Offers are verified only against a key held in the local trust store.
- Each machine keeps its own pinned keys in `<data>/remote/direct-trust.json`, next
  to `identity.json`. `<data>` follows `FERRYX_DATA_DIR`, or the platform default
  (`%LOCALAPPDATA%\Ferryx` on Windows, `~/.ferryx` elsewhere). The file is version 2
  (`peers: {id: {key, source: "manual"|"account", epoch?}}` plus `revoked` tombstones);
  version 1 files are still read (their pins count as manual) and rewritten as v2.
- Keys enter the store in exactly two ways:
  - **Manual**: `ferryx direct-trust provision` (below).
  - **Account-attested (automatic)**: when both machines are enrolled with an account and
    belong to the **same owner** (same user, or same team `org:<id>`), a trust miss makes
    the machine fetch the peer's record from `GET /api/account/v1/machines/{id}/peer-record`
    on its own account origin. The request is signed with the machine's enrolled identity
    key; the account answers only callers of the same owner (401 without valid machine
    auth, 403 cross-owner, 404 unknown). The record is signed under the domain
    `ferryx-account-direct-peer-v1`, expires after 24 h, and is verified against the
    operator-pinned `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`. The host additionally requires a
    valid account lease and pins only when the record's `ownerKey` equals its own lease
    owner. Failures are logged, briefly negative-cached on the host, and the pair stays on
    the relay.
- Precedence: a manual pin always wins over an account key; an account key may only change
  for a strictly newer enrollment epoch; `revoke` tombstones the machine id so it is never
  auto-repinned until you `provision` it manually again.
- **Shared trust root caveat**: in production the account service runs inside
  `ferryx-relay`, so the account signing key is not independent of the relay host. Whoever
  controls that host's account key can attest a key for any machine of the owner. If that is
  not acceptable, pin manually (manual pins win) or opt out.
- **Opt-out**: `FERRYX_DIRECT_AUTO_TRUST=0` (also `false`/`off`/`no`) disables
  account-attested pinning on that machine, on either side. Default is on.
- A missing pin, an unreadable store, or a corrupt store means "not trusted". The
  connection stays on the relay and the failure is logged, never raised as an error
  to the user flow.

## Exchanging keys manually

Needed only for machines that are not enrolled with the same account owner, or when
auto-trust is disabled. Run on **each** machine, then carry the output to the other machine out of band (for
example, copy it over an existing terminal session you already trust):

```
ferryx direct-trust identity
```

This prints one JSON line with only the public half of the existing machine
identity. It never rotates the identity and never prints the private key:

```
{"machineId":"<id>","publicKey":"<base64 ed25519 public key>"}
```

On the other machine, pin those values:

```
ferryx direct-trust provision <machineId> <publicKeyBase64>
```

Repeat in the opposite direction. Both sides must provision before the direct path
can be considered.

## CLI reference

Available as `ferryx direct-trust ...` and `ferryx-cli direct-trust ...`. On Windows use
`ferryx-cli.exe`, because release `ferryx.exe` is a GUI-subsystem binary and its
stdout is not shown in a console. None of these commands start or contact the daemon.

| Command | Effect | Output |
|---|---|---|
| `identity` | Print the local public identity | `{"machineId","publicKey"}` |
| `provision <machineId> <key> [--replace]` | Pin a peer key | `{"provisioned":"<id>"}` |
| `revoke <machineId>` | Remove a pin and block automatic re-pinning | `{"machineId","revoked":true\|false}` |

- Provisioning the same key again retains the same pin.
- Provisioning a **different** key for an already pinned machine fails unless you pass
  `--replace`. Use it only when that machine really was re-keyed.
- `revoke` returns `false` when nothing was pinned. After a revoke, subsequent
  negotiations refuse that peer; existing authenticated streams follow their
  bearer/session lifetimes.
- Errors go to stderr with exit code 1.

## Network requirements and defaults

- IPv4 only. Address discovery (STUN) rejects non-IPv4 mapped addresses.
- Uses arbitrary UDP ports, not 443. Networks that block outbound UDP or
  non-443 traffic will keep the pair on the relay.
- QUIC keep-alive is 15 s and the idle timeout is 20 s
  (`src-tauri/src/paired_host/direct/quic.rs`).

## Verification status

| Item | State |
|---|---|
| Rust compile (Windows) | pending |
| `cargo test --lib -- cli::direct_trust` | pending |
| Windows CLI smoke (identity / provision / revoke) | pending |
| End-to-end direct connection between two machines | pending |

Update this table only with results that have been measured.
