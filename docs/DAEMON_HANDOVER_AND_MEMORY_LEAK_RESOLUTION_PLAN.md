# Ferryx Daemon Handover and Memory Leak Resolution Plan

**Status:** Implementation proposal; runtime changes and production remediation are not performed by this document.

**Prepared:** 2026-09-17. **Repository baseline:** `5736b02bf1dcd2b7b0d76399cf5f4e4bf1046829`.

**Scope:** Stop attachment-lifetime memory retention, preserve existing interactive work, and replace indefinite predecessor proxying with bounded, explicitly committed ownership transfer. Cover local PTYs, legacy routes, remote session responsibilities, and platform limitations.

**Evidence convention:** “Reported” means incident evidence supplied with the task, not a new production measurement. “Verified” means the referenced repository implementation was inspected. Numerical limits and performance criteria below are proposed acceptance contracts, not measured results. Source symbols and line ranges refer to the baseline above.

## 1. Executive summary and decisions

The reported approximately 10 GB outage combines two defects: attachment tasks remain alive without a client, and a replaced daemon retains session ownership for an unbounded period. Fixing one without the other is insufficient. A healthy shell may remain idle for days; neither shell silence nor an open session is a valid reason to retain a dead subscriber or an obsolete application daemon indefinitely.

**Phase 1 must ship independently:** give every attachment an explicit duplex connection lifetime; propagate disconnect and cancellation through every nested pump; release snapshots immediately after transmission; bound memory by bytes and admitted owners; fix both legacy attachment APIs and the client-side reader tasks. Do not wait for PTY migration to stop new leaks.

**Phase 2 target:** exactly one serving Ferryx daemon generation after a bounded upgrade transaction, with the original shell/agent processes still running. Transfer kernel PTY capabilities and application state together. A socket-name change, a PID in JSON, or a scrollback snapshot is not a transfer of a live terminal.

The implementation decision is:

1. Implement a common snapshot, quiescence, fencing, and adoption substrate. Build **Option A: Unix `SCM_RIGHTS` descriptor transfer** as the new-PID handover mechanism. Enable it only for session classes with a tested adoption and process-supervision contract. The predecessor exits with status 0 only after committed activation and responsibility release, not merely after sending descriptors.
2. Build **Option B-live: state-preserving same-PID re-exec** as the Unix alternative where preserving existing child-parent relationships and exact wait status is required. It replaces the old process image and heap without a persistent predecessor. Its success criterion is a new executable image/build and owner epoch, not a changed PID or an old-process exit event.
3. Do not market metadata-only session recreation as seamless migration. A stable PTY supervisor can support a different cross-platform design, but it is an explicit additional process, not a way to claim a one-process architecture. Keep unsupported combinations gated rather than falling back silently to permanent legacy proxying.

Three additional repository hazards make a naive fix unsafe:

- `DaemonClient::attach` uses `into_split()` but its reader task does not retain the owned write half. That half closes its write direction when dropped. Unconditional server EOF handling would disconnect valid existing clients unless the client lifetime and compatibility contract change together. [R2] [S2]
- The vendored `UnixMasterWriter::drop` writes newline plus the terminal's `VEOF` byte. Ordinary writer teardown during migration can terminate an idle shell even when the successor owns another master descriptor. [R9]
- `DaemonLockFile::drop` explicitly calls `LOCK_UN`. Dropping the old wrapper after duplicating its lock descriptor can release the successor's lock. Migration needs a close-only, no-unlock ownership-release path. [R1] [S9]

### 1.1 Non-negotiable invariants

| ID | Required invariant |
| --- | --- |
| I1 | Client disconnect releases attachment resources; it never means closing or killing the underlying terminal session. |
| I2 | A healthy idle attachment stays usable without terminal output. A dead attachment does not require terminal output to be discovered. |
| I3 | Every socket, task, subscription, and buffer has an accountable owner and a bounded lifetime or explicit session-level retention policy. |
| I4 | At most one daemon generation performs PTY reads, input writes, resize, signal, spawn, or close for a session at a time. Duplicating a descriptor does not grant concurrent execution authority. |
| I5 | A migrated live session keeps its session incarnation, child PID identity, kernel PTY, terminal settings, and ordered output history. No implicit shell restart, injected newline/EOF, or synthetic success status is permitted. |
| I6 | Pre-commit failure leaves the old owner usable. Post-commit uncertainty never authorizes both owners to resume. |
| I7 | A successful new-PID upgrade has no predecessor session routes and no live predecessor application daemon after the retirement deadline. A re-exec upgrade has no old image or old heap. |
| I8 | Memory and connection counts depend on live sessions and admitted subscribers, not historical tab switches or upgrade count. |

## 2. Incident timeline, measurements, and diagnostic confidence

### 2.1 Reconstructed timeline

The incident report supplies weekdays and one wall-clock time, not absolute timestamps or its timezone. Preserve that distinction in the incident record. The dates in parentheses below are conditional mappings to the week of this document, not independently verified start times.

| Order | Reported event | Time precision and verification needed |
| --- | --- | --- |
| 1 | PID 869, described as v3, starts on Tuesday. | Tuesday; exact start time unknown. If this is the week ending 2026-09-17, the date is 2026-09-15. Confirm with process start time and daemon logs. |
| 2 | App update on Wednesday at **10:58 AM**, described as a CalVer bump to v4. | Wall time supplied; timezone/build identifiers unknown. Conditional date: 2026-09-16. Obtain both package build IDs and protocol handshake versions. |
| 3 | PID 65111 takes `/tmp/rorca-501/daemon.sock`; PID 869 moves to `/tmp/rorca-501/legacy-869-*.sock`. | Reported handover topology. Correlate prepare/commit, socket ownership, and executable identity. |
| 4 | Over a reported 29-hour interval, 1,066 dead inter-daemon socket connections accumulate. | The interval's precise start/end samples are not supplied. Do not manufacture second-level timestamps. |
| 5 | PID 65111 has 6,427 MB footprint and a reported 16.9 GB peak; PID 869 has 3,600 MB footprint. | Point-in-time/peak observations are different quantities. Sample timestamps and units need reconciliation. |
| 6 | PID 869 still owns five live zsh/omo sessions across maho-workspace, lectures, and content-intel-dashboard. | Reported protected user work. Session IDs and process start identities must be inventoried before intervention. |

The inspected repository has package version `2026.916.1` and daemon wire protocol version `4`. This does not prove either production PID is that exact build, or that the report's “v3/v4” labels exclusively mean wire versions. Keep CalVer, protocol version, migration schema, PID identity, and owner epoch separate. [R3] [R10]

The reported predecessor is a **live legacy session owner**, not an operating-system zombie in the strict exited-but-unreaped sense. Its execution from an obsolete backup inode is reported context, not independently inspected executable evidence.

### 2.2 Memory breakdown and arithmetic reconciliation

| Measurement | Supplied value | Correct interpretation |
| --- | --- | --- |
| PID 65111 current footprint | 6,427 MB | Reported footprint, not necessarily live allocated heap. |
| PID 869 current footprint | 3,600 MB | Reported footprint of the legacy session owner. |
| Combined reported footprint | 10,027 MB | Arithmetic sum, approximately 10.027 GB using decimal MB. Shared-page accounting prevents treating this automatically as unique physical RAM. |
| PID 65111 peak | 16.9 GB | Historical peak; do not add it to the current combined footprint. |
| Heap bucket A | 1,063 × 2 MB | 2,126 MB under decimal interpretation. |
| Heap bucket B | 1,414 × 1.4 MB | 1,979.6 MB under decimal interpretation. |
| Heap bucket C | 2,818 × 512 KB | 1,442.816 MB under decimal interpretation. |
| Listed bucket subtotal | 5,548.416 MB | Approximately **5.548 GB**, not 6.8 GB. Interpreting all sizes as binary gives 5,514.6 MiB, approximately 5.385 GiB. |

The report also attributes approximately 6.8 GB of heap to PID 65111 and approximately 3.6 GB to PID 869. Those totals cannot be reconstructed from the three supplied bucket counts alone. Missing categories, allocator capacity, differing samples, and rounded labels are possible explanations, not established findings. Preserve raw profiler output before assigning the remainder to a particular structure.

Approximately 36.8 dead connections per hour is the arithmetic average of 1,066 over 29 hours, not a measured constant leak rate. The useful prediction is linear growth with historical attachment count; validate it with allocation ownership and live task/socket counts, not just matching allocation sizes.

### 2.3 Evidence still required from the incident

Collect timestamped daemon logs, each PID's start identity and binary version/hash, session-to-process mappings, open descriptor counts, and allocation backtraces. Separate current footprint, RSS, peak footprint, live allocation bytes, and retained allocator arenas. Record whether both processes are on the same measurement basis.

Do not execute speculative cleanup commands against the listed PIDs. PIDs can be reused. Confirm the current socket peer, process start identity, and protected session inventory before any production action. Heap dumps and terminal history may contain private material; keep diagnostic artifacts access-restricted and out of ordinary source control.

## 3. Verified implementation and root-cause analysis

### 3.1 Current architecture

```text
GUI / remote subscriber
    |
    | dedicated Attach connection, replaced on tab/pane/HMR lifecycle
    v
PID 65111: current application daemon
    canonical daemon.sock
    |
    +-- local Attach --> write-only pump --> OutputHub / local PTY
    |
    +-- legacy Attach --> new LegacyPeer connection on every attach
                              |
                              | old connections can remain asleep forever
                              v
                         PID 869: predecessor daemon
                         legacy-869-*.sock
                              |
                              +-- OutputHub / PTY reader / master descriptors
                              +-- five existing shell and agent sessions

Current retirement condition:
    draining AND no local sessions AND no retained in-flight work
    => sessions remaining open can retain the old daemon indefinitely
```

### 3.2 The read side is unobserved during streaming

`DaemonServer::handle_client` splits the stream, creates a `BufReader`, and selects between request reads and handover abort notifications. When it selects an `Attach` request, it awaits the streaming pump **inside that selected branch**. The outer selector is not concurrently watching the connection or abort receiver while that await runs. The pump receives only the writer. [R1: `handle_client`, approximately lines 1543–1568 and 1963–2042]

The supplied diagnosis says the read half is dropped. In the inspected implementation it is more precise to say that the read half remains owned by the surrounding handler but is **not polled during the attachment**. Either design fails to observe client EOF while output is idle; the distinction matters when implementing the fix because the existing buffered reader must be preserved, not reconstructed with potentially lost buffered bytes.

`pump_sequenced_stream_with_agent_state` waits on output and optional agent-state broadcasts. `pump_session_stream` adds another remote-status/duplex layer. Neither has a downstream disconnect source. Writes can also block without a deadline inside a selected branch. Adding a cancellation branch only beside `rx.recv()` is insufficient if cancellation cannot interrupt an already awaited `write_all`, flush, initial snapshot write, or legacy handshake. [R1: approximately lines 2573–2912]

The local Attach path ignores the initial `AttachOk` write and flush errors and proceeds into the pump. It also builds response/history/serialization objects in a scope that spans the pump await. Even where compiler optimization happens to shorten a lifetime, correctness must not depend on that optimization: explicitly scope and release initialization buffers before entering the long-lived loop. [R1]

EOF is observable by a successful read of **zero bytes into a non-empty buffer**; a zero-length read request is not a disconnect probe. EOF indicates closure of the peer's sending direction, which must be interpreted according to the attachment's negotiated half-close contract. The incident's “only detected by writing” description is a property of the present write-only implementation, not a general Unix-domain-socket limitation. [S1]

### 3.3 Legacy proxy and detached-task retention

`LegacyPeer::attach_and_stream` opens a connection for each attachment, reads and forwards the initial response, then waits on upstream lines and agent-state reports. It cannot observe the downstream reader. Its initial attach response read and downstream writes have no encompassing cancellation/deadline contract. Its initial large line allocation can also survive for the function's lifetime. [R4: approximately lines 307–437]

A second path, `LegacyPeer::attach_session`, creates a new 2,048-message broadcast channel and spawns a reader task. That task retains the upstream writer, waits for upstream lines, and ignores failed broadcast sends. When the last downstream receiver disappears during an idle period, nothing wakes the task. This path requires explicit subscriber-lifetime cancellation, not merely a change to the server's direct proxy method. [R4: approximately lines 445–566]

One connection per **live** attachment is not inherently a leak. The defect is failure to retire connections and their allocations after the owning attachment disappears. A generic socket pool does not repair this invariant and cannot safely reuse a connection already committed to a streaming protocol.

### 3.4 Existing client half-close behavior is a hotfix release blocker

`DaemonClient::attach` uses owned socket halves. Its spawned task captures the reader and message sender but not the write half. The write half therefore drops when attach returns, while the caller still expects to receive output. Tokio documents write-half shutdown on that drop. [R2: approximately lines 1343–1535] [S2]

Consequently, **do not deploy unconditional EOF-as-detach to all current protocol-v4 clients**. Change the client to retain both halves for the subscription lifetime and advertise the new liveness contract. Test old/new client combinations explicitly. The message-forwarding task must also wake when its consumer disappears, even without terminal output. Merely dropping a Tokio `JoinHandle` detaches a task rather than cancelling it. [S3]

### 3.5 Why memory is amplified

The output hub has a default 512 KiB history buffer, 1,024-message broadcast capacity, and a 4,096-entry resize ledger. `OutputChunk` already shares payloads through `Arc<[u8]>`. Flattened history and segmented history still contain owned byte vectors. The wire protocol uses base64, so the two history representations can together require roughly 1.33 MiB of payload text for 512 KiB of retained raw history, before metadata and allocation overhead. [R3] [R6]

Tokio broadcast stores each channel item once and supplies clones to receivers; it is not an unbounded full-payload queue independently allocated for every receiver. However, leaked receivers can retain shared items, cloned snapshots create owned allocations, and independently created proxy channels have separate storage. Message-count bounds alone do not provide a byte bound when chunk sizes vary. [S4]

Other retention multipliers are large `AttachOk` strings, simultaneous history and history-segment copies, initial response objects surviving the streaming await, high-water serialization buffers after replay recovery, and untracked proxy/client tasks. `Vec::clear` and `String::clear` do not establish a smaller capacity budget. Attribute exact allocation buckets with backtraces and resource counters; do not present a guessed one-bucket-to-one-type mapping as proof.

### 3.6 Handover currently transfers routes, not PTY ownership

`HandoverManager` creates a legacy listener, records session routes, releases the canonical endpoint/locks, and marks the old daemon draining. `check_retirement_locked` requires `terminal_service.list_sessions().is_empty()`. There is no PTY adoption in this handover path. `PtySession` still owns the master, reader/writer duplicates, and child handle in the old process. [R5] [R7] [R8]

Existing tests correctly protect route persistence before ownership release and prevent delayed old-owner cleanup from unlinking a replacement listener. Preserve those guarantees when replacing the route-based mechanism. [R5]

## 4. Safe incident containment before the hotfix is available

The five reported sessions are protected work. Do not use `kill -9`, daemon-wide shutdown, forced application restart, PTY writer closure, or injected terminal input as a memory-recovery technique without explicit session-owner consent and a recovery plan.

The safe sequence is to inventory sessions and measurements, suppress repeated automatic upgrade attempts/legacy chain creation, reduce unnecessary GUI attach churn, then deploy the coordinated client/server hotfix through a tested path. A new executable on disk does **not** alter the code or reclaim the already retained allocations of an old running daemon. Likewise, a predecessor without an FD-export protocol cannot be retroactively migrated by sending it a new protocol request it does not implement.

For an unpatched predecessor, implement a transitional **single pinned upstream per legacy owner/session** in the patched canonical daemon. New GUI subscribers attach to bounded local fanout rather than reopening old upstreams on every tab change. Keep that upstream until session exit or a supported owner transition. Closing it at every zero-subscriber interval and reopening it later can merely move the continuing leak into the unpatched predecessor.

This is bounded containment, not final retirement and not a way to recover the predecessor's already leaked heap. Mark it visibly as compatibility mode. Move future sessions to the patched owner. Retire a non-migratable old owner when its protected sessions naturally end, or conduct an explicitly approved maintenance restart. If memory pressure makes continued execution unsafe, communicate the session-loss trade-off rather than silently sacrificing user work. Do not create another indefinite legacy generation to hide the problem.

## 5. Phase 1: attachment-lifetime hotfix

### 5.1 Protocol and compatibility contract

Add an optional, defaulted capability list to `HandshakeOk`; old peers without it have no new capability. Add an optional `liveness` field to `Attach` and echo the accepted mode in `AttachOk`. Keep ordinary request/control sockets separate from attachment streams. Validate new fields against captured old-peer fixtures; do not assume every deployed version ignores unknown fields.

Proposed values:

```rust
// Proposed protocol types, not existing implementation.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AttachLiveness {
    DuplexEofV1,
    HeartbeatV1,
}

// HandshakeOk.capabilities, default empty:
// ["attachDuplexEofV1", "attachHeartbeatV1", "streamLimitsV1"]
// Attach.liveness: Option<AttachLiveness>, default None
// AttachOk.accepted_liveness: Option<AttachLiveness>, default None
```

For `DuplexEofV1`, the client must retain both halves until detach; EOF means the attachment is finished. The server must honor the mode only after explicit negotiation. The server responds to unsupported modes with a structured pre-stream error rather than silently changing semantics.

For `HeartbeatV1`, use bounded protocol control frames and a nonce-bearing pong; never write ping bytes to a terminal or treat a normal request-socket `Ping` as implicitly valid inside an attached stream. Proposed remote intervals are 15 seconds with a 45-second acknowledgement timeout, suspended/rebased after host sleep. A transport failure detaches the viewer, not the session. Local duplex EOF remains the immediate path; a keepalive is supplementary protection against peers that remain open but stop functioning.

For legacy half-closing clients, do not interpret read EOF as proof that their receiving half has disappeared. Use an explicitly tested compatibility adapter: on supported local UDS transports, observe peer receive-side closure with a platform-specific socket readiness/error path; for framed transports, use only heartbeat/no-op output that every supported old parser has been proven to tolerate. If neither is safe for a client version, declare it unsupported by the new liveness mode and require coordinated client upgrade. Do not silently label that combination leak-fixed.

| Client / serving peer | Required handling |
| --- | --- |
| Patched client + patched daemon | Retain both halves; negotiated EOF cancellation throughout stream. |
| Patched client + old daemon | Retaining writer remains compatible; old server is still leak-prone, so use the bounded legacy transition and mark limited support. |
| Old half-closing client + patched daemon | Explicit legacy adapter or coordinated version gate; never unconditional EOF cancellation. |
| Patched canonical daemon + old legacy daemon | Pin one bounded upstream per session; no attach-driven upstream churn. |
| Patched canonical daemon + patched legacy daemon | Scoped dedicated upstream or shared session fanout with lease cancellation; upstream count returns to its declared baseline. |

### 5.2 One outer lifetime envelope, including blocked writes

Introduce `daemon/stream_lifecycle.rs`. Its core operation owns the **existing** client `BufReader`, writer, handover cancellation receiver, admission permit, and cleanup guard. It races the *entire* forwarding operation against negotiated disconnect, explicit detach, and owner shutdown. This envelope starts before subscription/snapshot allocation and initial response transmission.

Illustrative structure:

```rust
// Pseudocode: helper types and error conversions are implementation work.
async fn serve_attachment<R, W>(
    reader: BufReader<R>,
    writer: W,
    owner_cancel: OwnerCancellation,
    lease: AttachmentLease,
) -> Result<(), StreamError> {
    // Keep buffered bytes; do not replace reader with its raw inner stream.
    let termination = watch_client_lifetime(reader, lease.liveness());
    let forward = send_snapshot_then_forward(writer, lease.context());
    tokio::pin!(termination, forward);

    let result = tokio::select! {
        reason = &mut termination => Err(StreamError::Disconnected(reason)),
        _ = owner_cancel.cancelled() => Err(StreamError::OwnerChanged),
        result = &mut forward => result,
    };
    // Dropping the losing future releases its upstream socket, subscriptions,
    // and partially serialized frame. The whole connection is then closed.
    result
}
```

Use a one-byte non-empty read only for a mode where no additional inbound bytes are legal; unexpected bytes produce a bounded protocol error. A heartbeat/control mode instead uses a bounded frame parser. Preserve any already-buffered bytes following `Attach`. Never run two readers on the same client connection. Account for read errors and half-close tests. [S1]

Do not spawn the forwarding future and assume dropping its handle will cancel it. Prefer structured concurrency with an owned future. If a child task is unavoidable, retain its handle in a supervised task set, cancel it, and join it within the teardown budget. Do not hold synchronous locks across an await. [S3]

Wrap individual writes/flushes with progress deadlines as well as the outer lifetime envelope. Proposed defaults: 5 seconds for handshake and a stalled write, 10 seconds for initial attach completion. A partially written stream frame cannot be retried from byte zero on the same connection; close that attachment and let sequence-aware reconnection recover. A slow subscriber must not stall PTY collection or other subscribers.

### 5.3 Server changes

Refactor the Attach branch to create an attachment context and hand both directions to the lifetime envelope. Pass cancellation through `pump_session_stream`, its remote-status duplex adapter, and `pump_sequenced_stream_with_agent_state`. Make handover cancellation observable for the full streaming lifetime, not just between requests. Treat initial `AttachOk` write/flush failure as terminal and do not enter the pump.

Put snapshot response creation/transmission in a short inner scope. Move the receiver into the pump only after the response succeeds and all owned initial history/serialization copies have been dropped. On replay recovery, release temporary segment copies after the recovery frame is sent; replace oversized serialization storage with the normal small buffer before returning to idle.

Audit `SubscribeRemoteEvents`, machine metadata subscriptions, direct remote transports, and any other output-only loops for the same ownership pattern. Existing duplex machine gateway paths are not automatically defective, but their task guards and cancellation paths must participate in the regression matrix. [R1]

### 5.4 Proxy and client changes

`LegacyPeer::attach_and_stream` should be an owned forwarding future receiving a cancellation context, not a function capable of outliving its downstream writer. Validate a successful `AttachOk` before treating the upstream as streaming; bounded errors before the first success frame are request errors, while errors after streaming begins close the stream or use a defined terminal stream error. Own both upstream halves explicitly and release them on every exit path.

For `LegacyPeer::attach_session`, add a lease to `SessionAttachment` or return an owned stream abstraction. The spawned reader must select between upstream input, owner cancellation, and **the last subscriber disappearing**. Tokio broadcast exposes `Sender::closed()` in the inspected runtime documentation; use it rather than periodically checking receiver count only after output. Protect the subscribe/last-drop race with one registry operation. Do not leave a task alive because `tx.send` failures are ignored. [R4] [S5]

For `DaemonClient::attach`, capture the owned writer in the reader task or another explicitly scoped owner. Select on the message sender's closed notification to release idle sockets when the UI receiver is dropped. Audit all `DaemonAttachment` destructuring/drop sites: dropping an outer handle must not detach a running reader task. Attach/detach operations must be idempotent and independently scoped for multiple windows/panes. [R2]

### 5.5 Proposed lifecycle structures

```rust
// Proposed ownership model. Use tokio::sync::watch for cancellation initially;
// adding CancellationToken would require an explicit dependency decision.
struct AttachmentKey {
    session_id: String,
    owner_epoch: u64,
    client_instance_id: uuid::Uuid,
    attachment_id: uuid::Uuid,
}

struct AttachmentLease {
    key: AttachmentKey,
    cancel: tokio::sync::watch::Sender<bool>,
    // Owned capacity permit + accounting guard; Drop cancels exactly once.
    // The registry holds metadata/weak ownership, not a cycle back to this lease.
}

struct LegacySessionKey {
    peer_instance: uuid::Uuid,
    owner_epoch: u64,
    session_id: String,
}

struct LegacySessionStream {
    key: LegacySessionKey,
    // One supervised upstream, bounded replay, subscriber leases, and byte budget.
    // Compatibility-pinned status is explicit and observable.
}
```

Prefer a scoped dedicated stream for a patched predecessor unless shared fanout is needed. If implementing shared fanout, key it by authenticated peer instance and session ownership, not socket path alone; preserve access checks per subscriber, isolate cursors, and emit a replay gap when a subscriber falls behind. Maintain one shared replay store rather than cloning full history into a queue for every viewer.

Optional control-connection pooling is separate: at most four validated idle control connections per legacy peer, one request in flight per connection unless request IDs are multiplexed, 30-second idle expiry, and eviction on any framing/timeout/version error. Never return an attached streaming socket to the control pool. Do not add a pool merely to reduce a metric while leaving detached tasks alive.

### 5.6 Memory budgets and cleanup policy

Initial defaults below are conservative design inputs to be adjusted only with recorded benchmark evidence. They are not a global guarantee about every subsystem in the Tauri application.

| Resource | Proposed budget / policy |
| --- | --- |
| Session replay history | Keep existing 512 KiB raw history default; include synthetic mode prefixes and resize-ledger metadata in accounting. |
| Output transport | Enforce 4 MiB of unique retained payload per session across the shared transport/replay references, plus an independently bounded metadata count. Chunk oversized publications into at most 64 KiB payloads. Count shared allocations once. |
| Subscribers | Default 16 live attachments per session and 128 per daemon; reject excess before history serialization. Limits count multi-window/remote subscribers, not merely tabs. |
| Concurrent snapshots | At most 8 initial/recovery snapshot encoders; at most 8 MiB transient working allocation each. Budget includes raw history, segments, base64 expansion, and encoded frame. |
| Compatibility frame | 4 MiB maximum encoded Attach/Lagged frame under the default history limit; enforce before unbounded string growth and before base64 decode. Larger configured history requires negotiated chunked snapshots, not an implicit larger cap. |
| Regular control frame | 64 KiB; migration and snapshot protocols have their own bounded framing. Input retains the existing separate input-size policy. |
| Serializer after a large frame | Normal initial capacity 8 KiB; retain at most 64 KiB at idle. Replace oversized storage after send rather than relying on `clear()`. |
| Legacy compatibility stream | One pinned upstream and one bounded shared replay store per legacy session; no historical-attach accumulation. |
| Disconnected attachment | Release its task/socket/receiver and attachment-owned bytes within 1 second in local tests, 5 seconds on loaded CI. |
| Healthy idle session | Preserve the PTY and bounded session history. Never terminate it merely for silence or to satisfy an attachment memory limit. |

Implement a daemon resource-budget object with byte permits, attachment permits, and snapshot permits. A proposed initial subsystem soft limit is 256 MiB and hard admission limit is 512 MiB, but baseline non-terminal services must be separately measured. If the process already exceeds its pressure threshold, stop new snapshot work and evict slow/disconnected viewers; preserve shell processes and produce actionable telemetry. Do not use an address-space hard limit or an automatic daemon kill as the primary control.

When output exceeds a subscriber's permitted backlog, disconnect that subscriber or deliver an explicit gap and resnapshot under the snapshot semaphore. Never keep retrying full-history recovery without a retry/budget cap. Bound pending subscribers waiting on semaphores and all protocol strings, session IDs, segment counts, and request counts. Avoid per-session metric labels containing unbounded user data; detailed IDs belong in bounded diagnostic records.

## 6. Phase 2 architecture: true single-owner rolling upgrades

### 6.1 Target states

```text
Option A: new PID, after successful committed FD/state handover

GUI / remote clients -- reconnect with cursor --> NEW daemon
                                                 canonical listener
                                                 owner epoch E+1
                                                 OutputHub snapshot + live sequence
                                                 adopted PTY master(s)
                                                        |
                                                        v
                                                 SAME shell / agent PIDs

OLD daemon: no session ownership, no stream tasks, no legacy routes; exit(0)
Optional lifecycle supervisor, only when explicitly selected: not a legacy proxy.
```

```text
Option B-live: same PID, new image and heap

old daemon image [PID P, epoch E]
    -- quiesce, snapshot, allowlisted inherited descriptors, exec -->
new daemon image [PID P, epoch E+1]
    -- restore hub / session objects --> SAME PTYs and child processes

No persistent predecessor. Child-parent relationships remain with PID P.
```

“Immediate” means a bounded transaction independent of how long a shell remains open. Initial service-level targets for five sessions are a 250 ms p95 input pause, a 2-second hard freeze budget, and predecessor retirement within 5 seconds after activation acknowledgement. Exceeding pre-commit limits aborts or defers the upgrade; it does not justify force-closing a live session. These targets require measurement and may not be claimed as achieved by the plan.

### 6.2 Option A: PTY descriptors plus state over SCM_RIGHTS

Unix descriptor passing makes another process hold a reference to the same open file description; descriptor numbers are local to each process. It does not copy terminal data into a new PTY or transfer exclusive application authority. Linux and Darwin provide ancillary descriptor transfer through Unix sockets. [S6] [S7]

Pass one canonical master descriptor per local session plus a validated capability manifest. The successor duplicates it into its own reader/input/resize roles as required by its backend. Do not pass every incidental reader/writer clone. Pass listener and lock descriptors separately with explicit roles. Inventory auxiliary FDs and services rather than treating the master as the daemon's only resource.

The old owner first stops new mutation admission, finishes or explicitly rejects in-flight input, pauses PTY readers at a defined boundary, and drains already-read bytes into a final sequenced snapshot. The successor stages all sessions but does not read or write PTYs before commit. After activation is acknowledged, the predecessor releases its copies without terminal control writes, removes migrated entries without normal close semantics, drains non-session responsibilities, and exits.

### 6.3 Parentage and process supervision: descriptors are not Child objects

The current `PtySession` contains a `portable_pty::Child` and calls `try_wait`/`wait`. A successor that receives the PTY master does not become that shell's parent. Unix waiting APIs apply to children; PID metadata alone cannot reconstruct a valid ordinary `Child` object with the same wait rights. A Linux subreaper helps only with the relevant ancestor relationship; setting it on an arbitrary newly spawned successor does not adopt existing sibling/unrelated children. [R7] [S8]

Introduce an explicit process-handle model:

```rust
// Proposed semantic variants; platform implementations remain separate.
enum SessionProcessHandle {
    DirectChild { /* owned portable-pty child, normal wait/reap */ },
    AdoptedUnix {
        identity: ProcessIdentity,
        // PTY EOF + OS process-exit observation; not ordinary waitpid ownership.
        // Exact exit code is unavailable unless a verified lifecycle source supplies it.
    },
    Supervised { /* authenticated stable supervisor reference */ },
}

struct ProcessIdentity {
    pid: u32,
    start_identity: String,
    shell_process_group: Option<u32>,
}
```

For a strict one-application-process **new-PID** deployment, `AdoptedUnix` is the implementable contract: preserve interaction and process identity; report an unknown exit code as `None` when the platform cannot supply it, never as a successful zero. The existing state/stream types already permit optional exit codes, but all lifecycle consumers must be audited and tested before this policy is enabled. Distinguish process exit, PTY closure, lost observation permission, and unknown status. Use supported process identity/exit facilities and PTY state, not periodic `kill(pid, 0)` as the sole authority. Refuse signalling when identity cannot be safely established; refresh foreground job-control identity rather than caching it through an upgrade.

If exact wait/reap semantics are mandatory for a session class, choose B-live, or spawn that class under a stable lifecycle supervisor from its creation. A supervisor is an explicitly supported extra process with a small versioned protocol; it is not the obsolete Ferryx application daemon and must not retain a copy of every GUI stream. Such a design changes the strict one-process requirement and requires approval. It cannot retrospectively become the parent of existing macOS children through JSON or FD passing.

Release gate A-PROCESS: demonstrate session survival, signal/resize correctness, accurate known/unknown exit reporting, and reliable reaping by the actual parent on every supported OS. Disable A for classes that fail; do not conceal the failure with indefinite proxying.

### 6.4 Option B: serialization, scrollback, and fast re-adoption

There are three materially different meanings of “state handover”:

| Variant | Preserves arbitrary live zsh/agent execution? | Appropriate use |
| --- | --- | --- |
| Snapshot plus passed/inherited live PTY capabilities | Yes, subject to ownership and process-lifecycle gates. | Required application-state companion to A or B-live. |
| Metadata/history plus reconnection to an independently persistent session owner | Only if that owner already preserves the processes and supports authenticated reattachment. | Explicit broker/remote backend architecture; not zero-extra-process migration. |
| Serialize cwd, PID, geometry, history and start a fresh shell | No. It loses in-memory shell/agent state and running jobs. | User-approved restart/recovery, labelled as such. |

B-live keeps the existing PID and executes the replacement binary after serializing bounded state and retaining an allowlist of PTY/listener/lock/snapshot descriptors. Unix exec replaces the process image while preserving the process identity and eligible descriptors. Current code already uses re-exec for upgrades with no active sessions; extending it requires a new restore path, not simply removing the empty-session check. [R1] [S10]

B-live implementation sequence: preflight the verified replacement and schema; stop all spawn/mutation admission; quiesce readers and state producers; produce the final snapshot; establish the exact descriptor allowlist; clear close-on-exec only for those descriptors under a global spawn/exec gate; execute the verified binary in restore mode. Restore close-on-exec immediately after adoption and rebuild runtime registrations, output hubs, child supervision, and services without spawning replacement shells. No thread may create a child during the temporary descriptor-inheritance window.

If exec itself returns an error, restore descriptor flags and resume the old owner. If the replacement successfully execs and then crashes during restore, the former heap cannot be rolled back. Preflight, a minimal restore bootstrap, versioned checkpoints, and optionally a **bounded temporary FD escrow process** can reduce that risk; escrow is an explicit additional process during the transaction and cannot be counted as zero-process overhead. A persisted state file alone does not keep a PTY alive after its last master closes.

Checkpointing arbitrary process memory is not assumed by this plan. A portable guarantee cannot be built from saving shell environment/cwd/history alone. Preserve existing live kernel objects or state explicitly which recovery semantics are weaker.

### 6.5 Cross-platform decision matrix

| Platform / mechanism | Plan |
| --- | --- |
| macOS local Unix PTY | Implement SCM_RIGHTS and B-live. Gate raw-FD adoption, wakeable reader shutdown, foreground/job-control behavior, descriptor inheritance, and process-status policy on supported macOS CI versions. Apple manual pages cited here are archived semantics, not a substitute for current-OS tests. |
| Linux local Unix PTY | Same substrate. Optional pidfd/subreaper capabilities must be detected and must not be mistaken for automatic wait rights over non-children. Test the oldest supported kernel as well as current kernels. |
| Windows current daemon | Repository currently reports handover/upgrade unsupported. Phase 1 applies to its socket and subscription lifetimes. Do not advertise Unix FD transfer as a Windows implementation. [R1] [R5] |
| Windows ConPTY redesign | Treat `HPCON`, process handles, pipe endpoints, console lifecycle, and IO cancellation as a separate backend. Duplicating pipe/process handles is not evidence that the pseudoconsole can be imported into another host. A stable ConPTY broker is a concrete alternative, with explicit extra-process and broker-upgrade policy. |
| Remote SSH / paired daemon sessions | Classify whether the local daemon owns a local ssh PTY, a reconnectable remote handle, or transport state that cannot be serialized. Transfer/reconnect only through the corresponding backend contract; preserve remote generation/cursors and authentication. |

Microsoft documents that closing a pseudoconsole affects attached applications. `ReleasePseudoConsole` exists on Windows 11 24H2/build 26100 and Windows Server 2025, but is not by itself a general serialized-HPCON import protocol. Validate the actual supported host/child ownership sequence, including output draining and cleanup, before relaxing the Windows gate. Do not use ordinary `CloseHandle` assumptions for `HPCON`. [S11] [S12]

## 7. Handover protocol and state schema

### 7.1 Transport and security boundary

Use a dedicated authenticated control channel for migration, separate from newline-delimited Attach traffic. Prefer a private Unix socketpair explicitly inherited by the verified candidate at spawn. If a pathname is required, place it in the existing validated runtime directory, enforce directory/socket ownership and restrictive permissions, and bind a one-use random transaction nonce to the expected peer identity.

Validate same-user peer credentials using platform facilities, verify the launched executable identity and permitted upgrade source, and reject unexpected peers/roles. Do not accept arbitrary session IDs, executable paths, or descriptor counts from an unauthenticated connection. A nonce/hash detects transaction confusion or corruption; it is not a substitute for authenticated transport or trusted executable validation.

On receipt, validate ancillary lengths/types, frame sizes, role counts, and actual descriptors; immediately place every received descriptor in an RAII-owned container. Reject truncation and close all descriptors on every rejection path. Do not mix `BufReader::read_line` with ancillary-bearing reads: migration channel reads must retain the association between frame data and received rights. Linux `MSG_CMSG_CLOEXEC` can be used where available; other platforms need immediate `FD_CLOEXEC` under the process-spawn gate. [S6] [S7]

### 7.2 Proposed messages and bounded framing

Use independent `migration_schema = 1`, not a CalVer string or the current daemon wire version as a compatibility proxy. Negotiate minor-compatible capabilities; refuse an unsupported mandatory field or session backend before quiescence.

```text
Hello(handover_id, nonce, source_identity, candidate_build, schemas, capabilities)
Prepare(expected_owner_epoch, session_inventory_digest, requested_mode)
Prepared(resource_requirements, supported_session_classes, limits)
Quiesce(handover_id, deadline)
SessionHeader(session_id, incarnation, state_digest, lengths, fd_roles)
SessionDescriptors(batch_index, descriptor_roles + SCM_RIGHTS)
SnapshotChunk(session_id, chunk_index, bounded_payload, checksum)
SnapshotComplete(session_id, output_boundary, input_boundary, digest)
Ready(handover_id, inventory_digest, validated_session_count)
Commit(handover_id, new_owner_epoch, commit_record_digest)
Activated(handover_id, new_owner_epoch, adopted_inventory_digest)
Retired(handover_id) / Abort(handover_id, structured_reason)
Status(handover_id) -> last_durable_phase and owner identity
```

Proposed frame envelope: fixed magic, schema, message type, payload length, transaction ID, monotonically increasing frame index, descriptor count, and integrity checksum. Limit ordinary migration payload frames to 64 KiB, descriptor batches to 16 FDs, and per-session snapshot size to the configured bounded state budget. Preflight a total transaction budget, initially 64 MiB, rather than staging unbounded history for all sessions. Raise a limit only through explicit configuration and tested negotiation.

With stream sockets, handle partial writes and interrupted syscalls. Once ancillary rights have been sent with part of a frame, continue its remaining payload without sending those rights again. The receiver must handle split/coalesced payloads and ancillary records without assuming one send equals one receive. Retry an unacknowledged transaction message by its idempotency key, not by creating extra untracked descriptor owners. Receive/truncation and interrupted-transfer tests are mandatory. [S6]

Check each PTY role for a valid master/backend capability and usable size/control operations, not merely `isatty`. Reject unexpected regular files, duplicate session assignments, incompatible access modes, or descriptor roles not in the authenticated inventory. Do not mutate shared file status flags while the old owner is still using that open file description. Readiness flags, nonblocking mode, and close-on-exec need explicit per-platform tests.

### 7.3 Required transferable state

```rust
// Proposed schema sketch. Raw OS descriptor integers never become portable IDs.
struct SessionTransferV1 {
    session_id: String,
    session_incarnation: uuid::Uuid,
    source_owner_epoch: u64,
    backend_kind: SessionBackendKind,
    process: ProcessIdentity,
    supervision: ProcessSupervisionMode,
    master_fd_role: Option<u16>,
    cols: u16,
    rows: u16,
    workspace_id: Option<String>,
    worktree_path: Option<String>,
    cwd_metadata: Option<String>,
    lifecycle_state: TransferLifecycle,
    output: OutputSnapshotV1,
    input: InputBoundaryV1,
    agent: AgentSnapshotV1,
    remote: Option<RemoteAdoptionV1>,
}

struct OutputSnapshotV1 {
    first_retained_sequence: Option<u64>,
    last_assigned_sequence: u64,
    next_sequence: u64,
    // Bounded chunk records, retaining original sequence numbers.
    chunks: Vec<TransferChunk>,
    resize_ledger: Vec<TransferResize>,
    bracketed_paste_enabled: bool,
    replay_gap: Option<TransferGap>,
}
```

The concrete schema must additionally define bounded IDs/strings/counts, chunk digests, input operation IDs and accepted byte offsets, child-exit tombstones, session ownership metadata, agent-state revision/provenance, and remote generation/recovery descriptors. Do not serialize channel receivers, locks, Tokio tasks, trait-object memory, or raw credentials into ordinary JSON.

Preserve the kernel terminal settings rather than resetting them from a default template. Carry dimensions and resize-ledger boundaries for replay. Preserve sequence allocation even when the history buffer is empty: `next_sequence` must not reset to 1. Carry pending exit/status transitions and ensure they are published exactly once. Keep session incarnation stable across ownership transfer while owner epoch increments; a newly spawned replacement shell must receive a new incarnation.

Only bounded metadata/history is copied; the old allocator heap and already dead attachments are not part of the migration snapshot. Release each transferred snapshot chunk after acknowledgement and avoid holding full old and new serialized transactions simultaneously.

## 8. Transaction, fencing, and failure handling

### 8.1 State machine

```text
ACTIVE(E)
   |
   v
PREPARING --> abort --> ACTIVE(E)
   |
   v
QUIESCING --> pre-commit failure --> resume ACTIVE(E)
   |
   v
STAGED / READY --> pre-commit failure --> discard candidate copies, ACTIVE(E)
   |
   | durable commit record; old mutation gate remains permanently fenced
   v
COMMITTED(E+1) --> candidate activates from validated staged state
   |
   v
ACTIVATED --> release predecessor responsibilities --> RETIRED

After COMMITTED, a timeout is uncertainty, not permission to resume the old owner.
```

### 8.2 Prepare and quiesce

First take an upgrade coordinator gate covering automatic upgrades, manual upgrades, and candidate startup. Inventory every local/remote/paired session, legacy route, foreground observer, agent-state endpoint, workspace service, active mutation, watcher, and gateway responsibility. Refuse candidate admission when another transaction exists. Preflight code/schema compatibility, FD headroom, memory budget, and process-supervision support before stopping any PTY reader.

Stop new spawn/close/resize/input mutations or return a structured retryable `OWNER_TRANSITION` before accepting them. Drain already admitted durable operations; cancellation of their initiating client must not erase accepted work. Input writes that partially reached the PTY are recorded with accepted offsets and are not replayed wholesale by the successor.

Replace the current indefinite PTY poll with a wakeable read worker: on Unix, use a pollable wake pipe/event mechanism alongside PTY readiness, or move to an appropriately managed nonblocking async reader. A quiescence command must produce an acknowledgement and join/park the old reader at a known point. Aborting an already running `spawn_blocking` task is not a valid pause mechanism. [R7] [S13]

Pause the producer, drain its bounded PTY-to-output channel, and wait for the `TerminalService` output pump to acknowledge the final published sequence. A channel closing for migration must not invoke the current ordinary session-removal path. Freeze input admission and snapshot/output/resize state at a consistent boundary. Short PTY backpressure during the pause is permitted; an unbounded blocked producer is not. Do not freeze user processes with signals as a substitute for this protocol.

### 8.3 Stage, commit, and activate

Transfer snapshots and capabilities while the candidate is inert. It validates every session and reports `Ready` only after it can reconstruct its backend and output hub. It may register readiness sources while staging, but it must not consume PTY output or issue terminal operations yet.

Use a durable transaction journal with atomic replacement and verified write/fsync error handling. Record source and target identities, inventory digest, phase, new epoch, and staged capability roles. The **commit record selecting the target generation** is the ownership linearization point. All participating old-owner mutation paths remain fenced from quiescence onward and may resume only following a durable pre-commit abort. New-owner mutation paths activate only after validating the matching committed record.

Prefer passing the canonical listener and its existing lock capabilities, avoiding a pathname unlink/rebind gap. Pause old accepts and reconnect existing clients through a defined owner-change event. The same listener may be referenced during staging, but only the committed generation is admitted to accept and serve. Test queued connects across the boundary.

Change `DaemonLockFile` transfer semantics: duplicate/receive the locked capability, then detach the old wrapper **without `LOCK_UN`**. On pre-commit candidate rollback, close its duplicate without unlocking the source. Do not reopen the same lock pathname and assume it is the same lock ownership. Linux flock locks are associated with the open file description; closing all references or explicitly unlocking releases them. Test Darwin behavior and retain inode/owner checks. [R1: approximately lines 565–603] [S9]

If listener passing is unavailable for a backend, use a staged endpoint plus an atomic, generation-checked discovery record and a separately serialized ownership transition. Do not retain the present unguarded idea that removing a socket path proves the previous owner is finished. Cleanup must verify the exact endpoint/generation and must never unlink the replacement's listener.

### 8.4 Activation and retirement

The candidate restores the original history/sequence boundaries, starts exactly one reader per adopted PTY, exposes the new owner epoch, and acknowledges the committed inventory. Reconnecting viewers provide session incarnation and last received sequence. Replay follows the same gap semantics as normal reconnect; no silent duplication, sequence reset, or concatenation of flat and segmented history.

Before `exit(0)`, the predecessor must establish all of the following: each session is transferred or has a recorded terminal tombstone; no pending mutations or durable jobs remain assigned to it; old reader/output tasks are joined; attachment/observer/gateway tasks are cancelled; migrated objects are detached using no-EOT/no-kill teardown; no predecessor routes remain; and lock/listener cleanup cannot affect the successor.

Replace “wait for user sessions to become empty” with “all responsibilities released by a committed transfer or ordinary completion.” Migration removes entries through a transfer-specific API, never `Close`, `Hibernate`, or a user-visible exit event. The old daemon is not retained as an indefinite session proxy or reaper. A short retirement deadline detects a bug; it does not authorize killing a predecessor that still owns untransferred user work.

### 8.5 Failure matrix

| Failure point | Required outcome |
| --- | --- |
| Candidate spawn, signature, schema, or resource preflight fails | Remain ACTIVE on the old generation; no listener/PTY mutation. Report the reason and back off repeated upgrades. |
| Client detaches during prepare/snapshot | Cancel only that viewer. Session migration continues only under the independently owned upgrade transaction. |
| Quiescence exceeds hard deadline | Before commit, discard staged candidate state and resume old readers/input at the recorded boundary; verify no bytes were consumed twice. |
| FD transfer is partial, malformed, truncated, or exceeds FD limits | Close every received candidate FD, reject the transaction, retain old capabilities. No unaccounted descriptor survives an error. |
| Snapshot hash/sequence/inventory does not match | Reject before commit and retain old owner; do not partially activate sessions. |
| Child exits while preparing | Capture an exit/tombstone at the barrier; include it once. Do not resurrect an already exited session from stale metadata. |
| Old dies before candidate has a complete validated snapshot | Candidate may hold some PTYs, but cannot claim complete recovery. Fail closed per session, preserve any valid capability, and surface loss; a journal cannot recreate lost kernel objects. |
| Old dies after complete Ready and durable Commit | Candidate activates using the authenticated staged inventory and committed epoch. Resolve ownership from the journal, not the absence of a message alone. |
| Candidate dies before commit | Old resumes after a durable abort and confirmed candidate fencing; candidate duplicates are closed. |
| Candidate dies after commit but old still holds escrow copies | Never automatically resume both. Confirm candidate death by stable identity, serialize a recovery transaction, and restore from a known boundary. Any possibly delivered input is not blindly retried. |
| Both holders die / last master is closed | Live-PTY continuity is lost without independent escrow/supervision. Surface this limitation; history on disk is not live-session recovery. |
| Activated acknowledgement is lost | Query durable status and target identity. Retransmit idempotent status/ack messages; do not roll back merely because a timer expired. |
| Cleanup sees a different socket inode or epoch | Leave that endpoint intact; record cleanup conflict. Preserve the replacement-listener regression test. |
| Concurrent second update or old legacy chain | Serialize; refuse another migration candidate. Inventory and resolve every existing owner; one migrated canonical owner does not imply its predecessors migrated. |
| Unsupported remote/Windows/old-binary session class | Reject before freeze or use an explicitly approved alternative. No automatic permanent-proxy fallback. |
| Disk full or journal/fsync failure | No commit acknowledgement; pre-commit abort while old capabilities remain. Once committed, use recovery rules rather than rewriting history as uncommitted. |

## 9. Step-by-step implementation roadmap

All new modules, types, tests, flags, and scripts in this section are **planned additions**, not claims that they already exist. Each change must land with its associated tests. The only change delivered by this planning task is this Markdown document.

| Step | Files / symbols | Implementation and acceptance evidence |
| --- | --- | --- |
| P0 — reproducible baseline | New `src-tauri/tests/daemon_attach_detach_stress.rs`; existing server/client fixtures | Build isolated five-session idle reproduction with fixed prefilled history. Capture a failing resource-count test on pre-fix code. Do not require production access. |
| P1 — observable ownership | New `src-tauri/src/daemon/stream_lifecycle.rs`; `daemon/mod.rs`; `terminal/metrics.rs` | Attachment/task/socket/byte guards, bounded diagnostic snapshot, unique ownership labels and cancellation reasons. Prove all guards return to baseline after controlled drop. |
| P2 — compatible client lifetime | `daemon/client.rs`: `DaemonClient::attach`, `DaemonAttachment`; `daemon/protocol.rs` | Retain owned writer, consumer-closed cancellation, supervised task teardown, optional capability negotiation. Pass old/new half-close matrix before enabling EOF mode. |
| P3 — local and remote stream envelope | `daemon/server.rs`: `handle_client`, all three stream pumps, remote event subscriptions | Own existing reader and writer, cancel entire forward path including AttachOk and stalled writes, release snapshot scope, bound framing. Idle EOF and handover abort must work without any output. |
| P4 — both proxy APIs | `daemon/proxy.rs`: `LegacyPeer`, `attach_and_stream`, `attach_session`, `SessionRouter`; `terminal/output_hub.rs`: `SessionAttachment` | Lease-owned upstreams and subscriber-closed wakeup; transitional pinned fanout for unpatched owners; no ignored no-subscriber send failures. Both direct proxy and backend attachment tests pass. |
| P5 — bounded data plane | `terminal/output_hub.rs`; `daemon/protocol.rs`; `daemon/server.rs`; `daemon/proxy.rs` | Byte budgets, bounded decoder, shared history ownership where practical, snapshot semaphore, replay retry limits, shrink oversized buffers. Existing replay/agent-state behavior preserved. |
| P6 — lifecycle audit and hotfix release | `daemon/machine_peer.rs`, `daemon/session_metadata_forward.rs`, `daemon/session_metadata_events.rs`, `terminal/remote.rs`, `terminal/paired_runtime.rs`, relevant client attachment consumers | Search all spawned streaming readers and output-only loops; document owner/cancel/join for each. Ship Phase 1 only after resource and compatibility gates. |
| P7 — migratable PTY primitives | `terminal/session.rs`, `terminal/pty.rs`, `terminal/service.rs`; new `terminal/transfer.rs` | Wakeable pause/drain/resume; tracked output pump; explicit Migrating/Migrated state; adopt without spawning; ProcessHandle variants; no normal close/kill/removal during transfer. |
| P8 — portable-pty and lock teardown | `src-tauri/vendor/portable-pty/src/lib.rs`, `src-tauri/vendor/portable-pty/src/unix.rs`; `daemon/server.rs`: `DaemonLockFile` | Audited owned-master import/export API; close-only writer detach with no newline/VEOF; close-only lock transfer with no LOCK_UN. Avoid `mem::forget` as an ownership design. Test failed transfers as well as successful ones. |
| P9 — descriptor transport | New `daemon/fd_transfer.rs`; `daemon/mod.rs`; `src-tauri/Cargo.toml` only if dependency additions are justified | Bounded ancillary framing, RAII descriptor batches, credential/nonce validation, platform CLOEXEC handling, partial-send/receive tests and fuzz/property tests. Prefer narrow existing libc usage or an explicitly audited safe wrapper. |
| P10 — snapshot and transaction | New `daemon/migration.rs`; `daemon/handover.rs`; `daemon/manifest.rs`; `terminal/output_hub.rs`; `daemon/agent_state.rs` | Versioned bounded snapshot/journal, all-session inventory, input/output barriers, generation fencing, resume/commit/tombstone rules, authenticated migration capabilities. Retain old route format for read-only compatibility until retired. |
| P11 — startup and retirement | `daemon/server.rs`: upgrade/startup/listeners; `src-tauri/src/cli.rs`; `daemon/launchd.rs`; `daemon/session_service.rs`, `daemon/session_lifecycle.rs` | Candidate restore entrypoint, serialized upgrades, adopted listener/locks, no restart loop, responsibility-based retirement. Wire supervisor configuration so intentional retirement does not respawn an obsolete daemon. |
| P12 — B-live restore | New `daemon/reexec.rs`; existing `perform_daemon_exec_with_path`; `cli.rs`; PTY transfer modules | Allowlisted descriptor inheritance, verified restore bootstrap and error rollback, same-PID image/epoch verification. Preserve direct-child wait behavior. |
| P13 — platform/backend gates | Unix transfer backend; vendored Windows `win/conpty.rs` and `win/psuedocon.rs` only for a separately approved Windows design; remote/paired runtimes | Full supported-OS matrix, explicit Windows/remote capability reporting, documented unknown exit-code policy or exact-status alternative. Never enable a backend based only on compilation. |
| P14 — stress, crash, and rollout | New `src-tauri/tests/daemon_handover_migration.rs`, `src-tauri/tests/daemon_reexec_restore.rs`; new `scripts/qa/daemon-attach-detach-stress.py` | 1,000/10,000-cycle tests, 29-hour soak, repeated generation handovers, per-phase crash injection, automated resource report and final go/no-go. |

The critical ordering is P2 before unconditional EOF behavior, P7/P8 before any old PTY/lock wrapper is dropped during migration, and P9–P13 before removing the legacy path for supported live sessions. Implementing FD send/receive first and then trying to retrofit lifetime ownership is unsafe.

## 10. Test specifications

### 10.1 Phase 1 deterministic unit and integration tests

Use actual Unix socket pairs/listeners for Unix close semantics and actual TCP loopback sockets for the non-Unix transport. A `tokio::io::duplex` fixture is useful for stalled writes but does not prove OS half-close semantics.

| Proposed test name / family | Required contract |
| --- | --- |
| `idle_local_attach_full_close_releases_all_resources` | Prefill history, attach, consume snapshot, cease all output, close client. Server task, receiver, owned bytes, and socket return to baseline within deadline. Session remains alive. |
| `idle_attach_retained_writer_does_not_false_disconnect` | Patched client stays attached through an idle period; later output arrives exactly once. |
| `legacy_half_close_requires_compatibility_mode` | Reproduce current client's dropped owned writer while reader stays open. New server does not misclassify it as a full disconnect in legacy mode; capability mode is enforced separately. |
| `client_receiver_drop_cancels_idle_reader` | Drop UI message receiver and attachment owner without terminal output; no detached reader task or open socket remains. |
| `legacy_attach_and_stream_downstream_close_cancels_upstream` | Two patched daemon fixtures, idle legacy session. Downstream close causes both canonical and predecessor attachment resources to return to baseline. |
| `legacy_attach_session_last_receiver_drop_wakes_reader` | Exercise the second proxy API. No output or failed send is required to observe zero receivers. |
| `unpatched_predecessor_upstreams_are_pinned_and_bounded` | Simulate old idle leak behavior. After 1,000 GUI reattachments, at most one intentional upstream exists per legacy session; no per-tab upstream growth. Mark this containment, not old-heap recovery. |
| `attach_snapshot_failure_never_enters_pump` | Fail/close during AttachOk; subscription and temporary snapshot copies are released. |
| `stalled_write_disconnect_and_owner_abort_are_cancellable` | Fill output socket; stop reading; then detach or commit owner cancellation. The task exits even while write/flush is pending. |
| `remote_status_adapter_propagates_cancel` | Cancel through the nested duplex/status pump. Both layers and the remote subscription are released. |
| `handover_abort_cancels_active_stream_not_only_request_loop` | Attach first, then trigger owner cancellation with no output. No old canonical client remains hidden inside an await. |
| `agent_state_storm_cannot_starve_disconnect_or_output` | Stress the biased proxy path with agent updates and verify bounded fairness and cancellation latency. |
| `oversized_attach_or_lagged_frame_is_bounded` | Enforce bytes before growing strings/decode buffers; bound segment/string counts and base64 expansion; structured failure without memory spike beyond the test budget. |
| `two_subscribers_one_detaches_other_survives` | Ownership/cursor accounting is per attachment; one close never kills the other viewer or PTY. |
| `subscribe_drop_race_has_no_orphan_task` | Repeat last-receiver-drop/new-subscribe races, startup cancellation, handshake failure, timeout, and session exit. Counters remain exact. |
| `replay_recovers_sequence_and_geometry_without_duplicate_history` | Preserve gaps, resize-ledger ordering, bracketed-paste state, agent snapshots, and bounded replay behavior. |

### 10.2 PTY and migration tests

Use fixture-owned children with known process identities and a controlled counter/input protocol. Also run interactive zsh and a representative agent/TUI fixture. Never infer survival only from a PID still existing: verify preserved shell variables, a continuing background job, controlled input/output, geometry, and job-control behavior after handover.

Required test families include master-FD adoption without `openpty`/spawn, parentage-aware exit observation, exact known or explicitly unknown exit status, final PTY-byte ordering across a reader pause, and no second reader consuming the same PTY. Exercise `SIGWINCH`, Ctrl-C to the current foreground job, raw/canonical mode, alternate-screen content, bracketed paste, resizing during output, and a child exiting during every transaction phase.

Dedicated regression tests must prove that migration teardown sends **zero terminal input bytes** and does not invoke the vendored writer's newline/VEOF behavior; transferring or abandoning duplicate lock FDs does not issue `LOCK_UN` against the surviving owner; and cleanup cannot remove the replacement socket. A third contender should fail to acquire the daemon lock throughout staging, commit, and predecessor exit.

Fault-inject before and after every durable state transition, including lost Ready/Activated messages, failed journal writes, FD exhaustion, truncation, stale peer credentials, duplicate batch frames, wrong inventory digests, and simultaneous upgrade attempts. At each injected failure assert both session reachability and **single active mutation ownership**, not simply that one process eventually starts.

For input, assign operation IDs and byte offsets in fixtures. Issue a partially accepted write during freeze and verify it is neither dropped as an acknowledged success nor replayed from byte zero. For output, log the ordered fixture stream and compare the complete post-reconnect result against expected sequences, with explicitly declared retention gaps only.

### 10.3 Stress and soak contract: 1,000+ tab switches

The minimum automated fixture has five persistent sessions, each prefilled to the configured history limit before measurement. Warm up 200 attach/detach cycles, settle, capture baseline, then perform **1,000 measured cycles** and a **10,000-cycle extension**. Record metrics every 100 cycles and after a teardown settling window. Keep terminal output quiet during the idle case so a later write cannot mask failed disconnect detection.

Run direct local attachments, both proxy APIs, patched-to-patched dual daemon, patched-to-unpatched compatibility fanout, TCP/remote-status transport, and multiple simultaneous viewers. Add a separate high-output/slow-consumer run; do not mix its intentionally growing legitimate history with the idle leak baseline. A UI-level run must exercise actual pane mount/unmount, tab switching, close, remote reconnect, and development HMR disposal in an isolated fixture.

The long run is a 29-hour soak with a fixed session/subscriber population and repeated attaches. A separate upgrade soak performs at least 20 successive candidate generations while preserving the same sessions. Record the predecessor lifetime after every activation. Tests own their daemons/children and must terminate/reap only those fixture processes, including on failure.

## 11. Verification commands and measurable acceptance criteria

### 11.1 Existing commands that can be run now

From the repository root, the following existing test filters are grounded in inspected source. They are useful regressions, not proof that the new leak/migration tests already exist. Use `--list` and verify the intended tests are present; Cargo can succeed after running zero matching tests.

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib handover_commit -- --list
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib handover_commit -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib test_handover_protocol_serde_roundtrip -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib test_client_dedicated_attach_stream_does_not_monopolize_control_connection -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib cancelled_gateway_retains_close_operation_until_it_drains -- --nocapture
```

These commands require the repository's supported Rust/native build prerequisites. A missing system dependency is an environment failure to report, not permission to record an unrun test as passed. Do not run a fixture that resolves production endpoints: the harness must provide private runtime/state/config directories. `FERRYX_RUNTIME_DIR` is an existing Unix runtime override; all other mutable path sources must also be explicitly isolated rather than assuming this single variable isolates everything. [R1]

### 11.2 Commands to add with the planned implementation

The following test targets, module filters, QA script, and options are **new contracts to implement** in P0/P14. They are intentionally not presented as commands that currently work in this checkout. Test manifest registration and test-count assertions are part of delivering the targets.

```sh
# New unit-test module wired into daemon/mod.rs or the tested owner module.
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib stream_lifecycle_tests -- --nocapture

# New ignored, fixture-isolated integration stress target; default 1,000 cycles.
cargo test --locked --release --manifest-path src-tauri/Cargo.toml --test daemon_attach_detach_stress -- --ignored --nocapture --test-threads=1

# New migration and re-exec integration targets.
cargo test --locked --manifest-path src-tauri/Cargo.toml --test daemon_handover_migration -- --nocapture --test-threads=1
cargo test --locked --manifest-path src-tauri/Cargo.toml --test daemon_reexec_restore -- --nocapture --test-threads=1

# New QA wrapper: creates isolated daemons, runs the selected scenario,
# measures BOTH owners, validates the report schema, and exits nonzero on failure.
python3 scripts/qa/daemon-attach-detach-stress.py --cycles 1000 --warmup 200 --sessions 5 --scenario local --report artifacts/daemon-qa/local-1000.json
python3 scripts/qa/daemon-attach-detach-stress.py --cycles 10000 --warmup 200 --sessions 5 --scenario legacy-patched --report artifacts/daemon-qa/legacy-10000.json
python3 scripts/qa/daemon-attach-detach-stress.py --cycles 1000 --warmup 200 --sessions 5 --scenario legacy-unpatched --report artifacts/daemon-qa/compatibility-1000.json
python3 scripts/qa/daemon-attach-detach-stress.py --duration-hours 29 --sessions 5 --scenario idle-soak --report artifacts/daemon-qa/idle-29h.json
python3 scripts/qa/daemon-attach-detach-stress.py --upgrades 20 --sessions 5 --scenario migration --report artifacts/daemon-qa/migration-20.json
```

The script must reject conflicting modes, missing binaries, zero tests/cycles, unsafe production runtime paths, unidentified peer processes, and incomplete reports. It creates artifact directories itself with private permissions, records the exact builds/OS/configuration, and always cleans up its own process groups. It must not silently attach to a daemon discovered at the normal user socket or use the production PIDs as defaults.

After implementation, run the supported platform build checks separately:

```sh
cargo check --locked --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Run these natively in each supported OS CI environment; one macOS run does not prove Linux or Windows behavior. Preserve the existing Unix handover tests and relevant SSH/paired-session regression suites in addition to the new targeted tests.

### 11.3 Resource acceptance contract

“Zero memory growth” means zero retained **attachment-owned** resource growth with historical attachment count, plus a bounded, non-accumulating process-memory envelope. It does not mean that an allocator must return every page to the OS after every detach.

| Metric | Required pass condition |
| --- | --- |
| Orphan attachment tasks | Exactly zero after settling; distinguish intentional session-level compatibility streams. |
| Attachment-owned bytes / permits | Return exactly to pre-cycle live-owner baseline, excluding declared session replay state. No ownership guard remains for a dead viewer. |
| Subscriber count | Exactly the configured live subscribers after each settle window; zero for detached sessions' viewer layer. |
| Patched-to-patched upstream connections | Return to baseline after detach; no growth with cycle count. |
| Unpatched compatibility upstreams | At most one intentional stream per inventoried legacy session, stable across 1,000/10,000 GUI cycles. Existing old leaks must be reported separately. |
| Fixture socket/FD count | Return to the declared baseline, allowing only explicitly recorded persistent pool entries; no new baseline after every batch. |
| Live heap | No positive retained-allocation slope attributable to completed attachments; instrumented owned bytes are the primary zero-growth proof. Proposed profiler-noise tolerance: 1 MiB after warmup, investigated with backtraces when exceeded. |
| OS memory | Proposed idle-run tolerance: final footprint/private-memory delta no more than the larger of 16 MiB or 5% of warm baseline, and fitted slope at most 1 KiB per cycle. This is a secondary noise-tolerant bound, not a substitute for exact ownership counters. |
| Long-run plateau | The 10,000-cycle and 29-hour tests remain in the same bounded envelope; a small repeatable allocation leak fails even if it fits the OS-noise allowance in the 1,000-cycle run. |
| Disconnect latency | Local EOF release within 1 second; loaded-CI hard limit 5 seconds. Negotiated network heartbeat failures meet their separately stated lease deadline. |
| Session continuity | Same five session incarnations and process identities; shell variables/jobs remain usable; output/input contracts and geometry checks pass. |

Measure both canonical and predecessor processes. On macOS, use timestamped `vmmap -summary`/appropriate footprint instrumentation and live-heap profiling; on Linux, record RSS/private/PSS fields consistently; on Windows, record private bytes and handle counts. Label the exact metric rather than comparing unlike quantities across platforms. If diagnostic tools are unavailable, retain exact internal counters and mark the missing OS/heap evidence; do not replace it with an assertion that memory “looks stable.”

A practical read-only macOS incident collection, after confirming the PID identities, is:

```sh
ps -p 65111,869 -o pid,ppid,lstart,etime,rss,command
lsof -nP -a -p 65111,869 -U
vmmap -summary 65111
vmmap -summary 869
```

These are diagnostic examples, not commands executed by this planning task and not recovery actions. Run with the required local permissions, record collection timestamps, and never equate the `ps` RSS column with the supplied footprint or peak measurements.

### 11.4 Retirement and upgrade acceptance contract

For **Option A**, the fixture must retain an OS child handle for each daemon so it can positively reap the old daemon and assert status 0. If the real upgrade candidate is spawned by the old daemon, instrument the test supervisor to observe its identity and termination instead of asserting that an arbitrary unrelated observer can `waitpid` it. Within 5 seconds of `Activated`, no old application daemon remains, no old session routes remain, and the canonical endpoint answers with the new owner epoch/build. Verify all five original shell/agent processes are still usable.

For **B-live**, assert the same daemon PID/start identity, a different verified running image/build and owner epoch, an independently reinitialized heap, and the same live sessions. A missing old PID is the wrong assertion for re-exec. No additional persistent predecessor process or legacy route may remain.

For either mode, a third candidate cannot acquire ownership during the transfer, old cleanup cannot unlink the canonical listener, old mutation requests are fenced, and repeated upgrades do not accumulate observers, locks, descriptors, routes, or child waiters. Record activation/retirement timestamps in the report rather than using a fixed sleep and assuming success.

### 11.5 Required report schema

The QA JSON report must include schema version, scenario, OS/build identities, configuration and limits, warmup/measured cycle counts, session incarnations and process start identities, baseline/final/max resource counts for every daemon, per-batch samples, cancellation-latency distribution, footprint/private-memory metric names, live-heap evidence availability, output/input consistency results, migration phase timings, old-daemon exit/image-replacement evidence, and explicit pass/fail reasons.

Return a nonzero exit status for an incomplete scenario, failed continuity check, missing required metric, unsafe fixture isolation, unsupported migration backend, or any resource contract violation. A compatibility-containment report must not be labelled a full single-daemon migration pass.

## 12. Release, rollback, and operational ownership

### 12.1 Rollout gates

| Gate | Owner role | Evidence required before proceeding |
| --- | --- | --- |
| G0 — diagnosis | Daemon maintainer + incident operator | Baseline reproduction; protected-session inventory; reported vs measured evidence separated. |
| G1 — hotfix correctness | Daemon/client maintainers | Full half-close matrix; idle local/proxy/client task tests; blocked-write cancellation; no injected terminal control bytes. |
| G2 — leak containment | Performance/QA owner | Exact resource counters, 1,000/10,000-cycle tests, both-process metrics, bounded unpatched compatibility scenario. |
| G3 — migration primitives | PTY/platform maintainer | Wakeable quiescence, no-EOT teardown, safe lock transfer, FD framing/security tests, process-status contract decision. |
| G4 — atomic handover | Daemon/platform maintainers | All-session inventory, failure matrix, generation fencing, listener safety, same-process continuity, retirement evidence. |
| G5 — supported-platform release | Release/QA owner | Native supported-OS matrix, remote backend gates, soak results, explicit disabled-mode behavior and recovery runbook. |

Start with isolated synthetic fixtures, then an opt-in canary with protected-session inventory and resource monitoring. Expand only when session-loss, false-disconnect, resource-growth, and retirement thresholds remain clean. Do not automatically retry a failed migration in a tight loop. Limit one candidate and one transaction per owner; during compatibility operation, refuse upgrades that would add another unbounded predecessor chain.

### 12.2 Rollback rules

Before commit, rollback closes candidate-only copies without EOT or unlocking the source and resumes the old owner from the recorded boundary. After commit, rollback is another fenced ownership transaction, not a client-side choice to reconnect to an old socket. Keep the previous executable available only as a verified rollback image, not as an indefinitely running application daemon.

A hotfix rollback must preserve the client/server half-close compatibility matrix. Rolling back only one side can turn valid attachments into immediate disconnects. Never roll back from bounded stream ownership to the known leaking behavior solely to reduce a new metric alert; use the guarded compatibility path and investigate.

Existing unpatched predecessors are a bootstrap constraint. This plan cannot make them execute migration code they do not contain. Document which sessions need natural drain or approved maintenance before the machine can meet the final single-owner invariant. That limitation does not justify allowing future patched-to-patched upgrades to create the same architecture again.

### 12.3 Exit criteria for the project

Phase 1 is complete when all supported attach paths are cancellation-owned, no historical-attach growth is observed under the specified tests, compatibility behavior is explicit, and protected sessions remain usable. Phase 2 is complete only when supported live sessions transfer without a persistent old application daemon, the platform/process-status contract is met, crash/rollback gates pass, and telemetry can demonstrate bounded retirement in production.

A document review or passing protocol serialization test alone is not completion of either implementation phase.

## 13. Known uncertainties and explicit decisions to record

The incident's absolute timestamps, deployed build identities, raw allocation backtraces, and the unexplained heap subtotal remain unverified. Collect those as incident evidence without blocking the attachment-lifetime fixes demonstrated by source inspection.

Before enabling Option A, record whether unknown exit status for adopted non-child sessions is an accepted product contract for every consumer. If not, choose B-live or an explicitly approved supervisor architecture for that class. Before enabling Windows live migration, provide a supported ConPTY ownership/import/release proof on the minimum supported Windows build; Unix success is not that proof.

The memory defaults, pause deadlines, and soak tolerances are proposed release budgets. Tune them using controlled measurements, retaining exact zero-orphan/zero-retained-attachment invariants. Do not loosen a threshold to hide a monotonic leak. Clarify process-supervisor restart policies and remote-session responsibility transfer before promising unconditional daemon retirement.

## 14. Repository evidence index

These references point to inspected source files. Symbols and approximate baseline line ranges are supplied to make the plan auditable without implying that line numbers remain stable after implementation.

| Ref | Source and verified anchors |
| --- | --- |
| R1 | [Daemon server][R1]: runtime override; `DaemonLockFile` and explicit unlock (565–603); `handle_client` (1543 onward); Attach (approximately 1963–2042); remote event stream and handover request dispatch (2300 onward); upgrade (2450 onward); stream pumps (2573–2912). |
| R2 | [Daemon client][R2]: `DaemonClient::attach` (1343–1535), owned split and reader task capture; existing dedicated-attach/control-connection test (2581 onward). |
| R3 | [Wire protocol][R3]: protocol version 4, base64 serialization, history segment model, handshake/Attach/handover messages, `test_handover_protocol_serde_roundtrip`. |
| R4 | [Legacy proxy][R4]: `LegacyPeer`, `connect_and_handshake`, `attach_and_stream` (307–437), `attach_session` (445–566), recovery stream, and manifest-based routing. |
| R5 | [Handover manager][R5]: state model, prepare/commit/abort, in-flight retirement guard, empty-session retirement condition, persistence and replacement-listener regression tests. |
| R6 | [Output hub][R6]: history/broadcast/resize capacities, Arc-backed chunks, owned attachment snapshots, sequence allocation, segmented replay. |
| R7 | [PTY session][R7]: descriptor/Child ownership, blocking reader (approximately 148–190), process wait/reap, ordinary Drop (531 onward). |
| R8 | [PTY manager][R8] and [terminal service][R8b]: spawn/lifecycle ownership, output registration pump, output removal on ordinary channel closure. |
| R9 | [Vendored Unix PTY backend][R9]: private master wrapper, reader/writer cloning, `UnixMasterWriter::drop` (393–405) writes newline/VEOF. |
| R10 | [Cargo manifest][R10]: package `2026.916.1`, Tokio `1.53`, local portable-pty `0.9`, library/binary/test configuration. |

[R1]: ../src-tauri/src/daemon/server.rs
[R2]: ../src-tauri/src/daemon/client.rs
[R3]: ../src-tauri/src/daemon/protocol.rs
[R4]: ../src-tauri/src/daemon/proxy.rs
[R5]: ../src-tauri/src/daemon/handover.rs
[R6]: ../src-tauri/src/terminal/output_hub.rs
[R7]: ../src-tauri/src/terminal/session.rs
[R8]: ../src-tauri/src/terminal/pty.rs
[R8b]: ../src-tauri/src/terminal/service.rs
[R9]: ../src-tauri/vendor/portable-pty/src/unix.rs
[R10]: ../src-tauri/Cargo.toml

## 15. Primary platform/runtime references

References were consulted on 2026-09-17. They establish API semantics; the proposed Ferryx structures, budgets, protocol, and release gates are design recommendations. Archived Apple references must be supplemented by current supported-macOS integration tests.

- **[S1]** Tokio `AsyncReadExt`: read/EOF and cancellation semantics.
- **[S2]** Tokio Unix `OwnedWriteHalf`: dropping the owned half shuts down its write direction.
- **[S3]** Tokio `JoinHandle`: dropping a handle detaches the task.
- **[S4]** Tokio broadcast channel: shared item retention, bounded capacity, and lag behavior.
- **[S5]** Tokio broadcast `Sender::closed`: notification when receivers disappear.
- **[S6]** Linux `unix(7)`: ancillary descriptor transfer, framing constraints, credentials, and truncation.
- **[S7]** Apple archived `recvmsg(2)`: Darwin ancillary-message and stream-read semantics.
- **[S8]** Linux/Apple wait APIs and Linux child-subreaper semantics: process parentage is not transferred by FD passing.
- **[S9]** Linux/Apple `flock(2)`: duplicate-descriptor lock ownership and release semantics.
- **[S10]** Linux/Apple `execve(2)`: image replacement and descriptor/process continuity.
- **[S11]** Microsoft `ClosePseudoConsole`: pseudoconsole teardown affects attached applications.
- **[S12]** Microsoft `ReleasePseudoConsole`: supported versions and release ownership semantics.
- **[S13]** Tokio `spawn_blocking`: a started blocking task cannot be stopped merely by aborting its handle.

[S1]: https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html
[S2]: https://docs.rs/tokio/latest/tokio/net/unix/struct.OwnedWriteHalf.html
[S3]: https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html
[S4]: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html
[S5]: https://docs.rs/tokio/latest/tokio/sync/broadcast/struct.Sender.html
[S6]: https://man7.org/linux/man-pages/man7/unix.7.html
[S7]: https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/recvmsg.2.html
[S8]: https://man7.org/linux/man-pages/man2/waitpid.2.html
[S9]: https://man7.org/linux/man-pages/man2/flock.2.html
[S10]: https://man7.org/linux/man-pages/man2/execve.2.html
[S11]: https://learn.microsoft.com/en-us/windows/console/closepseudoconsole
[S12]: https://learn.microsoft.com/en-us/windows/console/releasepseudoconsole
[S13]: https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html

Additional primary references for the platform-specific claims: [Apple waitpid](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/waitpid.2.html), [Linux child subreaper](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html), [Apple flock](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html), and [Apple execve](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/execve.2.html).

---

**Delivery boundary:** This artifact is the resolution plan. Runtime fixes, stress harnesses, migration protocol extensions, production memory reclamation, and single-daemon upgrade certification remain implementation work governed by the acceptance contracts above.
