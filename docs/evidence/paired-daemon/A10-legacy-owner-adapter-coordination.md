# A10 owner routing adapter coordination (in progress)

Parent owns remote/session_api.rs and machine_operation_journal.rs; child st_01a09869 does not edit them.

Implemented service methods in daemon/machine_owner.rs:

```rust
pub(crate) async fn machine_detail_routed(self: &Arc<Self>, id: &str, epoch: Epoch) -> Result<SessionDetail, String>
pub(crate) async fn machine_sessions_routed(self: &Arc<Self>, epoch: Epoch) -> Result<Sessions, String>
```

Both offload local journal work. They must be awaited outside run_blocking. Current session_api read closure's match must call these routed methods, preserving the parent's admission lifetime, cancellation and deadline guards. Error mappings: HOST_UNAVAILABLE -> 503; MACHINE_OWNER_UNSUPPORTED -> 422. Missing/unknown old owner contract never falls back to Local/mirror. The current HTTP RED will remain RED until adapter integration.

Socket routing instead negotiates MachineGateway on the validated predecessor UDS and proxies real HTTP/WebSocket handshake to the existing owner gateway. This keeps controller map/lease/generation, raw target validation, and cancellable PTY writes on the actual owner. New gateway token is forwarded only inside that validated local UDS; owner independently authenticates it. No permanent bearer URL.

Close transport is implemented as `LegacyPeer::close_machine_http(id, token, body).await -> Result<(u16, Vec<u8>), String>`. Parent adapter boundary: inside mutation's spawned task, after authentication/body/CloseSessionRequest validation but before local journal reconciliation, resolve the peer for close_id, forward unchanged body and original header bearer, return owner status/body/no-store. Existing extract_token is now pub(super), parser unchanged. The owner independently authenticates and applies existing epoch/controller/journal/termination checks. Parent owns adapter integration and admission/cancellation lifetime.

A12 snapshot now awaits machine_sessions_routed after off-thread catalog read, preserving subscribe-before-snapshot sequence ordering. Production child writes frozen at parent request pending adapter composition. The final server upgrade call and function both have six arguments; the intermediate E0061 was a concurrent build between those paired edits, not a remaining signature change.

Actual compiled runtime socket proof is A10-legacy-owner-socket-increment.log: PID67518 and original CWD, replacement generation2, geometry103x37; same run fails HTTP detail expired/running, exit101 with cleanup. Expanded tests after this run remain unverified. No full completion claim. Output queue budget remains parent-owned OPEN; journal contention is separately parent-tested.
