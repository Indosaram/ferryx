# A10 routed-owner repair boundary

Status: source-backed missing contract; not a runtime reproduction or accepted fix.
This note narrows the existing routed-owner repair task, not the approved scope.

## Current loss points

- `daemon/proxy.rs:80-111`: `LegacyPeer::send_request` accepts a
  `HandshakeOk` but discards its fields. A handshake epoch identifies that
  responding daemon, not necessarily a session's transitive routed owner.
- `daemon/proxy.rs:135-158`: list responses discard their epoch, and describe
  returns only `DaemonSessionDetails`.
- `daemon/protocol.rs:90-102`: that details DTO carries geometry, paths,
  running state and output sequences, but no machine scope, machine target
  or authoritative session owner epoch.
- `daemon/session_service.rs:1303-1384`: describe projects workspace/CWD
  metadata but does not transmit `StoredSessionMeta.machine_session`.
- `daemon/proxy.rs:726-770`: routed describe converts the legacy DTO into
  the mirror-oriented `RemoteSessionDetails`, with no machine authority.
- `daemon/session_service.rs:219-235`: machine attachment requires current
  process-local metadata and a current process-local PTY. A route adopted
  from a manifest cannot satisfy this merely by being listed by SessionRouter.

LSP identified these definitions and the machine-only predicate's callers.
The parent read the implementations, not just the symbol names.

## Required repair proof

Use two isolated real owners and the existing handover path, preserving the
original remote PTY. Prove detail, ticket, attachment, input and close use the
original owner epoch while the gateway epoch differs. Neither raw ID lookup,
gateway handshake nor cached route existence alone establishes machine scope.

Keep controller authority singular across old and new gateways: a second
gateway must not acquire an independent controller for the same process.
Stale controller input/resize/close and old tickets must remain fenced at the
actual owner. Old peers lacking the machine contract must fail explicitly;
missing fields cannot silently become Local, SSH or mirror privileges.

Mirror filtering currently uses the durable journal ownership predicate at
`remote/state.rs:587-592` and `remote/server.rs:657,1149`. Preserve that
classification across handover and verify it with actual mirror list/socket
requests. This observation is not evidence of a current disclosure: the
durable journal may retain the classification even though native metadata
does not survive in memory.

Also test incomplete/unavailable legacy inventory: it must not erase paired
rows or trigger Create. Verify old-format compatibility separately from a
new-owner handover, and retain exact process cleanup receipts.

## Scheduling boundary

This repair necessarily reaches protocol, proxy, session authority and socket
wiring. A12 currently owns part of that source, so the parent has not edited
it or spawned a conflicting producer. Run the repair after A12's write phase
settles, alongside only disjoint repairs, then verify the combined batch.
Production PTY input cancellation is already assigned separately.
