# A10 output budget repair boundary

Status: source-backed budget gap, not measured saturation.

`terminal/output_hub.rs:7-8` retains 512 KiB of replay history but uses
1024-message broadcast channels. `OutputChunk.bytes` is an Arc-backed payload;
sequence consumers share allocations, rather than each owning a full copy.
Therefore neither "1024 messages means 1 MiB" nor "each consumer duplicates
every payload" is a valid characterization.

`register_session_channels` creates both sequenced and raw broadcasts.
`publish_with_read_timestamp` sends a shared sequenced chunk and a raw Vec
copy. The history's byte cap does not itself bound those channels' unread
payloads to 1 MiB. `SessionAttachment.receiver` exposes a broadcast receiver,
and attachment subscribes before snapshot under the same hub lock.

`remote/server.rs:1227-1390` sets a 1 MiB WebSocket write buffer, sends one
frame at a time with a ten-second timeout and reads directly from that
broadcast receiver. While send is blocked, subsequent output remains in the
count-bounded shared channel. There is no observed per-machine-consumer
queued-byte accounting or immediate overflow signal in the inspected path.
Lagged receivers currently reattach for a replay snapshot.

## Required repair and evidence

Preserve the subscriber-before-snapshot invariant, sequence domain and shared
payload economy. Bound machine socket pending output by bytes, including
in-flight data where the contract counts it, without blocking PTY readers or
unrelated native/mirror consumers. Overflow must detach that consumer and
recover through replay/gap, not spawn a replacement shell or silently discard
bytes under an unchanged sequence cursor.

Prove exact-boundary and over-boundary behavior using unequal chunk sizes;
message-count tests alone cannot prove a byte budget. Hold the real socket
consumer while a real PTY produces output, capture peak accounting, then
verify disconnect/reconnect and the original PID. Concurrently exercise
another pane to prove independent progress. Use subscribed barriers and a
bounded external watchdog, not sleeps or an unbounded producer.

The ten-second write-progress deadline needs controlled-time proof and the
real stalled-transport surface. A tiny fake sink can test timeout selection,
but does not establish the real queue's retained-byte limit.

The gateway and event/session wiring currently belong to A12. This note does
not authorize concurrent edits there. Compose the byte-budget repair after
that write phase, retaining Local/SSH/mirror regressions in the single
aggregate verification. No change to the global history capacity is justified
solely by this machine-socket requirement.
