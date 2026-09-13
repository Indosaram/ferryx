# A10 authoritative machine socket output budget integration

2026-09-13, st_01a0986b, macOS arm64, isolated herdr-resume-01a097f8 worktree. Uncommitted candidate. Ownership: only machine sections of remote/server.rs, new remote/machine_output_writer.rs and its focused tests, new tests/a10_output_socket.rs. Parent supplied DaemonSessionService::attach_machine_output. No daemon/service/retirement/PTY lifecycle/relay/UI edits by this integration task. A disjoint Windows test shell edit in server.rs was authorized to its separate composer.

## Delivered behavior

After the existing authoritative owner-routing branch, local machine admission uses `services.sessions.attach_machine_output` instead of `state.session_backend.attach_with_sequence`. Missing session and rejected replay fail admission; no broadcast bridge or local fallback. Snapshot creation remains subscriber-first under the hub lock. Existing owner epoch, target checks, controller lock/generation and outer revocation/fencing select remain.

The writer retains the charged snapshot through attached control, snapshot encoding, send and explicit flush; each live ChargedOutput remains alive through encode/send/flush. It releases guards only afterward. Overflow uses the independent watch receiver selected with priority against EVERY explicit socket write. Overflow terminates the handler instead of reattaching inside an unchanged stream; clients reconnect with a cursor to receive explicit replay/gap. The existing ten-second progress timeout encloses send plus flush. Outer cancellation drops all three read/write/input futures and the controller lease.

Controls are checked before queue admission: eight queued slots, one in-flight slot, one admission-local slot, each at most 1024 bytes including conservative 10-byte WS header = at most 10240 bytes, below the hub's permanent 16384 reservation. Initial attached/status are admitted at the same bound. Oversized serialized controls terminate rather than create another queue. Incoming protocol pings are at most 125 bytes; tungstenite can retain its one automatic reply, also covered by the remaining reservation. Snapshot/live encoded overhead plus 10-byte WS header must fit the charged 512-byte allowance. This is a logical pending wire-byte bound, not RSS, duplicate encoder allocation, or kernel socket buffer accounting. The hub maintains queued+in-flight+control reservations <=1048576.

Current validate_machine_target already uses metadata try_read and catalog try_lock, not a durable journal lock. No unnecessary blocking wrapper/service signature change was added. Closed output also trips termination: lifecycle send is best-effort and cannot bypass shutdown selection.

## Actual socket RED before wiring

A10-output-socket-RED.log, exit101:

```
real machine socket delivered 1100000 queued payload bytes instead of disconnecting
A10_OUTPUT_SOCKET_CLEANUP pid=Some(81615) reaped=true reader_finished=true listener_joined=true root_removed=true
A10_OUTPUT_SOCKET_CLEANUP pid=Some(81632) reaped=true reader_finished=true listener_joined=true root_removed=true
```

The test creates projects and real PTYs via machine HTTP API, attaches actual authenticated WebSockets, holds the consumer, then publishes unequal 700000/400000-byte payloads without yielding between publishes. This deterministically exercises the actual old socket attachment queue, independent of OS scheduling/buffer size. These saturation payloads are explicit hub publishes into a real PTY session, NOT claimed as 1.1MiB emitted by the shell itself. The separate production PTY publisher test covers real reader-to-hub saturation.

## Combined GREEN

Commands (locked, no-default-features):

- `cargo test --manifest-path src-tauri/Cargo.toml --lib machine_output_writer`: 3 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --test a10_output_socket --test a10_output_budget --test a10_output_budget_pty --test machine_terminal_stream -- --nocapture`: 7 budget + 2 PTY + 1 actual socket + 1 existing machine socket scenario passed.

Complete successful log: A10-output-socket-GREEN2.log. First attempted batch A10-output-socket-GREEN.log stopped at compile error: axum Message has no len method; fixed with exhaustive variant matching. No tests were skipped or weakened. LSP no errors for server/writer/new test files; one initial server request timed out, subsequent request succeeded. `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib` passed (A10-output-socket-build.log). Existing warnings remain visible. Tracked server diff whitespace check passed.

Actual socket signatures:

```
A10_OUTPUT_SOCKET original_pid=94982 sibling_pid=94999 unequal_pending=1100000 overflow_closed=true reconnect_gap=true
A10_OUTPUT_SOCKET_CLEANUP pid=Some(94999) reaped=true reader_finished=true listener_joined=true root_removed=true
A10_OUTPUT_SOCKET_CLEANUP pid=Some(94982) reaped=true reader_finished=true listener_joined=true root_removed=true
```

Sibling proof executes printf in the actual shell via WS binary input, parses its PID, and compares to the backend-owned PTY. Reconnection uses afterSequence=0, decodes the actual binary replay metadata, requires a gap, and checks original PID unchanged. Existing machine_terminal_stream also exercises focus independence, controller replacement, stale input/resize, cross-device conflict, authorization/revocation, input/control caps, ticket reuse, suffix replay and gap on the integrated path; PIDs95086/95145 reaped.

Writer transport tests use a real ephemeral TCP pair, a tungstenite server writer, and a peer that never reads. Sender socket buffer is 1024; a 16MiB test-only frame ensures actual I/O is pending before cancellation/clock advance. Overflow test sets the independent watch signal and requires immediate completion with no peer progress. Deadline test pauses time only after socket construction, observes pending write, advances exactly ten seconds, and requires failure. These exercise the production machine_send helper, not a mock sink. The oversized 16MiB test frame bypasses production frame admission solely to saturate the transport; it is NOT a claim that production admits that frame or that this test alone proves the 1MiB budget. Full socket test proves wiring separately. No single combined test yet holds an actual HTTP-upgraded write pending, triggers overflow from continuous PTY output, and reconnects; the evidence is composed across the real surfaces above, not mislabeled as that stronger scenario.

Owned PTY integration in the same batch: PID94903 normal and94904 injected failure, full2MiB sibling output, peak1047560, reaped/reader_finished/root_removed true. That fixture retains its drain during teardown. Reader completion is observed; production watcher JoinHandle ownership and dropped-receiver cleanup remain the separate parent followthrough, not repaired here.

No fixed sleeps or polling readiness were added. Tests use observed messages/EOF, manually polled pending I/O, and bounded watchdogs. All private listeners are joined; exact PID checks for94903,94904,94982,94999,95086,95145 returned no processes. Fresh /tmp/a10-ws.* HOME/runtime/data/session/XDG/TMPDIR roots were removed per invocation; canonical daemon and parent supervisor untouched. Existing private target, jobs2, debug0, incremental0, RUSTC_WRAPPER empty, explicit /Users/indo/.cargo and /Users/indo/.rustup.

## Review and after-source hashes

Writer owns bounded socket sends (34 pure LOC); writer tests own transport admission/deadline (49); integration test owns machine socket budget scenario (113). Existing oversized server was not broadly refactored: its shared legacy socket behavior remains unchanged, narrow writer logic was extracted. No new unsafe, casts, production unwrap/expect, logging, detached producer task, service abstraction, or parameter bag. All new functions have <=3 parameters. Existing machine handler's six-argument signature remains except its attachment type; it represents existing socket/session/controller context rather than a new abstraction. Assertions remain behavioral; no prose pins.

SHA-256 at successful verification (server hash includes inherited/disjoint parent changes):

| Path | SHA-256 |
| --- | --- |
| src-tauri/src/remote/server.rs | c59fb9f97d404606be024335045c8d60ad8f2429ba1347853f87bc22a746f063 |
| src-tauri/src/remote/machine_output_writer.rs | 27a052576b45352747aca46879ab4133af26e0a6e0b95ef677df684ced5fb115 |
| src-tauri/src/remote/machine_output_writer_tests.rs | 830d04cfcc36b8fe994c376aaac998b53ec19c2d2b3b85ff1960aefe84f6c62d |
| src-tauri/tests/a10_output_socket.rs | 959605f554461e906ef9b1c042617264009638f5fde88057a2a374c9e13cfd80 |

Retirement, routed-tunnel aggregate buffering, Windows runtime and PTY lifecycle followthrough are not claimed complete. The owner-tunnel source was not edited; this bound applies to the authoritative owner's machine subscription/writer, not a promise about total buffers across relay/tunnel hops.
