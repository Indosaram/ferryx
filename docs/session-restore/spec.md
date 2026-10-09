# Ferryx Session Preservation & Restore: Normative Specification v11.2 (Final)

- Location: `docs/session-restore/spec.md` (canonical). The local Korean working copy `.omo/plans/session-restore-architecture-fix.md` matches v11.1 and lists the v11.2 amendments in a separate section.
- Modified: 2026-10-09
- History: v1 → pass-1 REVISE → v2 → pass-2 STILL_BLOCKED → v2.0 → pass-3 REJECT → v3.0 → pass-4 REJECT → v4.0 → pass-5 REJECT → v5.0 → pass-6 REJECT → v6.0 → pass-7 REJECT → v7.0 → pass-8 REJECT → v8.0 → pass-9 REJECT → v9.0 → pass-10 REJECT → v10.0 → full self-audit → v11.0 → check of remaining state-machine models → v11.1 → Rust implementation property tests (§9.1) → **v11.2**
- Normative terms: MUST / MUST NOT / SHOULD have the meanings defined in RFC 2119.
- Review evidence: `.omo/evidence/astra-*-session-restore.md` (local, not committed). Executable state-machine models: `docs/session-restore/spec-models/`. P0 experiment evidence: `docs/session-restore/p0-evidence/`.

---

## 0. Scope of Guarantees

### 0.1 Guarantees (MicroHost-owned sessions)

"PTY write point" means `write()` on the PTY master fd on POSIX, and `WriteFile()` on the ConPTY input pipe handle on Windows.

| ID | Kind | Guarantee |
|---|---|---|
| G1 | Safety | GUI exit, crash, or Force Quit, and policy daemon exit, panic, SIGKILL, or update, do not end the session's PTY write point reference or the execution of the child process. |
| G2 | Safety+Convergence | The state applied by a display client always equals the host's replicated state S (§2.2) at some revision `R`. While the subscription is maintained, the client either applies all subsequent revisions in order or, if it cannot apply them, converges to the same state through resynchronization. Convergence of resynchronization is guaranteed when all of the following hold: scrollback rewrites (§2.3) stop. The consumption throughput of the client connection is large enough to receive one snapshot within the transfer deadline (§2.5, 30 seconds). After that, the Delta generation rate is lower than the consumption throughput, or output stops long enough to receive one snapshot and drain the queue. The session actor keeps processing events. A connection that does not satisfy these conditions is closed with `SLOW_CONSUMER`, and G2 does not promise convergence for that subscription. UI events (§2.6) are not included. |
| G3-S | Safety | Input bytes accepted by the host (§3.4 `accepted`) are written to the PTY write point in acceptance order, at most once. Accepted bytes that have not been written are retained until they are written or reported as `pending_dropped`. Outbox bytes that have not been accepted are not written. |
| G3-L | Convergence | While all of the following conditions hold, accepted bytes are eventually written: the child reads PTY input so that the write point becomes writable. While all of the following conditions hold, outbox bytes are eventually accepted: the writer and the host are alive, the daemon-path connection is restored, the writer holds the input lease, and the child reads input so that the write queue has room. If the lease is lost, bytes that were assigned to that epoch but not accepted are confirmed and displayed as `not delivered` via the classification query (§3.6). If the final value cannot be obtained because the ledger was evicted or the owner has disappeared, they are displayed as `outcome unknown`. Bytes not yet assigned to any epoch carry over to the epoch of the next lease. |
| G4 | Safety | A live session is not judged `terminated` because of timeouts, partial failures, or delayed responses. `terminated` is confirmed only by the positive evidence of §5.5. |
| G5a | Safety | Each pane's child view frame, visibility, surface configuration, and present reflect only the viewport target that the pane last accepted. Stale asynchronous work does not change them. |
| G5b | Safety+Convergence | The host applies Resize only in `(resize_epoch, resize_seq)` order. Once the daemon-path connection is restored and the pane holding the Resize lease remains visible and Ready, the PTY size converges to that pane's latest desired size (with `normalize_size` applied, §2.2). During the transition, a stale size may be applied briefly. Panes that do not hold the lease do not determine the PTY size. |

### 0.2 Out of Scope (Explicit Exceptions)
- Host process death, OS logout/reboot: no guarantee of PTY survival. `terminated(owner_lost)` only when positive evidence exists (§5.5).
- Legacy sessions (`LegacyDaemon` partition): only G4 and G5a apply. G1, G2, G3, and G5b do not apply, and the existing behavior of the old daemon is followed. If the old daemon dies, its PTYs disappear. Drain is not a transfer of ownership.
- Scrollback is preserved with the per-session `scrollback_max_lines` (default 10,000) as the target. Because the VT engine evicts in page units, the actual number of retained lines may differ from this value by several hundred lines.
- UI events (clipboard, notifications, etc.) are delivered best-effort without duplication.

---

## 1. Process Domains

```
GUI (Tauri/React + Native Viewport)    Remote clients
       │  Tauri IPC (JSON, u64 = decimal string)  │
       ▼                                           ▼
Policy Daemon (ferryx-daemon): gateway, relay, git, registry aggregator, router, intent DB
       │  FXSH protocol, per-user local IPC
       ▼
Session Host instance(s) (ferryx-host): PTY owner, VT engine, input ledger, leases
```

### 1.1 Execution Ownership (Fail-closed)
- Each host instance has one service management unit: macOS LaunchAgent label `com.ferryx.host.<instance_id>`, Linux systemd user template unit instance `ferryx-host@<instance_id>.service`, Windows Task Scheduler user task `\Ferryx\Host-<instance_id>`. Each unit runs that instance's dedicated executable `<data>/ferryx.v6/hosts/<instance_id>/ferryx-host`. This file is not changed after installation until the instance ends. Stop/restart of one unit does not affect other instances. The host is started only through this service management unit. On Linux the user manager must outlive the login session: the installer enables lingering for the user (`loginctl enable-linger`); if lingering is off, logout stops `user@<uid>.service` and every host unit, and the UI MUST show that sessions do not survive logout. On Windows the task principal is the user's SID (not the account name) with `LogonType = Interactive` and `RunLevel = Limited` (P0-4).
- **Instance registry**: `{instance_id, endpoint, host_version, state: active|retired}` is written via atomic rename to `<data>/ferryx.v6/hosts/<instance_id>.json` (common to all OSes). The daemon finds instances through this directory (Windows named pipes are also found through the endpoint in this registry). New Spawns are sent only to the most recently registered of the `active` instances. If there is no `active` instance, the daemon installs the executable with a new instance_id and registers and starts the service management unit (the daemon is not the parent of that process). The host rereads its own registry entry at session termination and every 60 seconds. It does not belong to the GUI/daemon process tree or Job Object. It does not depend on `CREATE_BREAKAWAY_FROM_JOB`.
- The daemon/GUI MUST NOT launch the host as a child process. If it cannot start or connect via the service manager, the daemon MUST return `SERVICE_MANAGER_UNAVAILABLE` to the UI and refuse creation of new sessions.
- The host reports `supervisor`, `pid`, and `process_start_time` in `HelloAck`. The daemon MUST cross-verify the PPID chain and start time using OS APIs. On mismatch, fail-closed.
- The updater and the daemon restart logic MUST NOT stop/restart/kill a host instance that has sessions (§6).

### 1.2 Local IPC Security
- Endpoints: macOS `$TMPDIR/ferryx/host-<instance_id>.sock`, Linux `$XDG_RUNTIME_DIR/ferryx/host-<instance_id>.sock`, Windows `\\.\pipe\ferryx-host-<user_sid>-<instance_id>`.
- The directory MUST be verified with `lstat` to have owner=current UID, mode 0700, and not be a symlink. On failure, refuse to start.
- Peer credentials MUST be verified: macOS `getpeereid`, Linux `SO_PEERCRED`, Windows `GetNamedPipeClientProcessId` + token SID. On mismatch, terminate immediately.
- Trust model: processes of the same user account are trusted (explicit assumption). Session-mutating commands additionally require a lease (§3.3, §4.3).
- Limits: frame payload 16 MiB (excluding header), Spawn argv+env 256 KiB, 512 sessions per host, 8 subscribers per session, 64 outstanding requests per connection, WriteInput bytes 64 KiB, 64 input epochs with unwritten bytes remaining in the write queue, VT response pending bytes 64 KiB (§3.5), host-wide snapshot budget 512 MiB, session state size `S_MAX` 64 MiB (§2.2). Exceeding a limit yields `LIMIT_EXCEEDED`. However, the snapshot budget and `S_MAX` do not cause rejection and are instead handled by the rules of §2.2 and §2.5.

---

## 2. Screen State Replication

### 2.1 Session Actor
- Each session has one single serial actor inside the host. Only this actor mutates the VT engine, `state_revision: u64`, scrollback, hyperlink table, subscribers, input ledger, write queue, and leases.
- The actor processes events one at a time in arrival order. Events are FXSH requests (all of the C→H commands in §7.4 that target a session) and internal events (`PtyOutputChunk, PtyWritable, PtyEof, ChildExited, SnapshotBudgetAvailable, SnapshotFrameWritten, SnapshotBufferReleased, SnapshotDeadline, ConnectionClosed`). There are no events outside this list.
- The PTY reader thread puts `read()` results into the actor mailbox (cap 8 MiB). When the mailbox is full, the reader waits. The actor does not wait for subscriber transmission or snapshot memory acquisition (§2.4). Therefore mailbox congestion depends only on VT processing speed.

### 2.2 Replicated State and revision
The replicated state S is as follows: cols, rows, screen cells, cursor (position, shape, visibility, blink), modes (all of §A.4), palette (all of §A.1), title, hyperlink table (id → URI referenced by cells of the current screen or scrollback), scrollback (line_id and content of every retained row), exit_info.
- `state_revision` increases by exactly 1 at each actor step in which S changes. It does not increase at steps in which S does not change (such as when only an incomplete escape was buffered).
- **Row width**: every RowData in S (screen and scrollback) has a number of cells equal to the current cols. A scrollback row is the row stored by the engine projected onto the current cols (excess cells are truncated and missing cells are filled with blank cells). Therefore a step in which cols changes is always a scrollback rewrite (§2.3), regardless of whether the engine reflows.
- Parser internal state (incomplete UTF-8/escape, character set designations not reflected on the screen, etc.) exists only in the host VT engine and is not part of S. Clients do not parse VT, so they do not need this state.
- `state_digest = xxh3_128(canonical_encode(S))` (§A.6). The host and the client use the same function.
- **Size normalization**: `normalize_size(cols, rows) = (clamp(cols, 1, 1024), clamp(rows, 1, 512))`. The host applies this function to the size received by Spawn and Resize and applies the result, and reports the applied size in ResizeAck and in the `STALE_RESIZE` detail. A pane applies the same function when computing the desired PTY size (§4.4). Therefore cols in S is 1~1024 and rows is 1~512.
- **Size caps**: Cell grapheme_extra ≤ 32 bytes (combining characters beyond this are discarded), title ≤ 4 KiB, hyperlink URI ≤ 2 KiB, hyperlinks referenced by S ≤ 1024 (beyond this, new links are recorded without a hyperlink). Under these caps, the encoding of S excluding scrollback is 40 MiB or less.
- If at some step the length of `canonical_encode(S)` would exceed `S_MAX` (64 MiB), then within that step scrollback rows are evicted starting from the oldest until it no longer exceeds it. This is ordinary eviction and is conveyed via `scrollback_evicted_before`. Therefore S at every revision is at most `S_MAX`.

### 2.3 Scrollback line_id
- `line_id` increases monotonically from 1 within a session incarnation and is never reused.
- When a new row is pushed off the top of the screen into the scrollback, it receives the next line_id.
- A step that changes the content or order of existing scrollback rows is called a **scrollback rewrite**. This includes a cols change (§2.2 row width), a rows change, ED 3 (clear scrollback), RIS, an active screen switch (primary↔alternate), and engine behavior that changes scrollback rows. The screen and scrollback in S are those of the active screen. While the alternate screen is active, the scrollback is empty, and the step that returns to the primary screen is a scrollback rewrite.
- **Rewrite detection**: before and after each actor step, the host compares (cols, rows, active screen) and the position and content hash of a tracking reference (the engine's tracked grid ref) placed on the most recent scrollback row. If the position has disappeared, has changed in a way not explained by the number of rows evicted from the front, or the content hash differs, the step is treated as a rewrite. This determination MUST never miss a rewrite; judging a non-rewrite step as a rewrite (unnecessary resynchronization) is permitted. A scrollback rewrite reassigns new contiguous line_ids to all remaining rows and, at that step, forces resynchronization (§2.5) on all subscribers.
- An empty scrollback is represented as `scrollback_count = 0`.

### 2.4 Subscription, Baseline, Snapshot Memory
**Subscribe(subscriber_id, attach_seq, client_known_revision?, client_known_incarnation?)** is processed in a single actor step. `subscriber_id` is the UUID of the subscribing display client instance (pane or remote view), and `attach_seq` is a u64 that the instance increments by 1 each time it sends Subscribe (it continues across connection changes).
0. If an existing subscription with the same `subscriber_id` exists: if that subscription's `attach_seq ≥` the request's `attach_seq`, return `STALE_SUBSCRIBE{current_attach_seq}` and stop. If it is smaller, close that subscription by the same rules as Unsubscribe (§2.5 return (b)). Therefore there is at most one subscription per `subscriber_id`.
1. If the number of subscribers is at the limit, `LIMIT_EXCEEDED(subscribers)`.
2. Register a new `subscription_id` (monotonically increasing within the session) and record `subscriber_id` and `attach_seq`.
3. Let the current revision be `R`. If `client_known_revision == R` and `client_known_incarnation` equals the current incarnation, enqueue `SubscribeAck{attach_seq, snapshot_follows: false, revision: R}` and set the baseline `base = R`.
4. Otherwise, first enqueue `SubscribeAck{attach_seq, snapshot_follows: true, revision: 0}`, then execute the baseline reset of §2.5. The snapshot's revision is contained in the SnapshotFrame.

The send queue is FIFO. Therefore the client always receives `SubscribeAck`, then the SnapshotFrame, then the Deltas after that snapshot, in that order.

- When a connection closes, all subscriptions created over that connection are closed (§2.5 return (b)).

**Snapshot**: a snapshot is the S at the time the buffer is created, serialized into an immutable buffer (§A.3 Body encoding). One buffer is at most `S_MAX` + 1 MiB. If the Body length is 1 MiB or less, it is sent as a single tag 0 (Full) frame; if larger, it is sent as tag 1 (Chunk) frames with bytes ≤ 1 MiB. The sum of reservations issued host-wide MUST NOT exceed 512 MiB, and buffers are always created only within a reservation (§2.5).
### 2.5 Resync and Snapshot Budget
**Budget allocator**: The host has exactly one budget allocator, which is a single serial actor. The allocator owns the budget (512 MiB), a FIFO queue, and the set of reservations it has issued. The size of one reservation is fixed at `S_MAX` + 1 MiB. A reservation is identified by `reservation_id` (monotonically increasing within the host, never reused). `ReturnReservation{reservation_id}` is idempotent; an id that has already been returned is ignored. Allocator messages (`RequestReservation`, `CancelRequest`, `ReturnReservation`, `SnapshotBudgetAvailable`) are host-internal messages and do not appear on the FXSH wire.

**Request key**: The key of a queue entry and of a reservation is `(session_id, subscription_id, request_seq)`. `request_seq` increases monotonically from 1 per subscription, and is incremented by 1 each time the subscription requests a new reservation. The queue holds at most one entry per `(session_id, subscription_id)`.

**Per-subscription budget state**: `Idle`, `Waiting{request_seq}`, `Reserved{reservation_id}`, `Sending{reservation_id, snapshot_id, deadline}`, `Draining{reservation_id, snapshot_id, deadline}`, `Closed`. `Reserved{reservation_id}` exists only inside the allocation event step, which enters it and leaves it through step 4; no other event observes a subscription in `Reserved`.

**Send path**: Each connection has one sender, and the session actor hands SnapshotFrames to the sender frame by frame. Once the sender starts a frame, it either writes it to the end or closes the connection (it does not stop in the middle of a frame). The sender may discard frames it has not yet started writing via `DropPending{subscription_id, snapshot_id}`. The sender sends the following internal events to the session actor. Every event carries `(subscription_id, reservation_id, snapshot_id)`.
- `SnapshotFrameWritten{..., last: bool}`: One frame has been written to the end. `last` indicates whether it is the last frame of that snapshot.
- `SnapshotBufferReleased{...}`: All of the sender's references to that snapshot buffer have been released (one of: completion, discard, connection closed).
- The session actor separately sets a `SnapshotDeadline{...}` timer at `deadline`.
The session actor processes these three events **only when the subscription state is Sending or Draining with the same `(subscription_id, reservation_id, snapshot_id)`**. As an exception, a `SnapshotBufferReleased` that matches an unreturned-reservation record of a closed subscription executes only `ReturnReservation{r}` and clears that record. All others are ignored.

**Triggers**: Subscribe step 4, client `Resync`, queue overflow (§2.6), scrollback rewrite (§2.3). The session actor executes the following within one step:
1. Discard all not-yet-sent Deltas from the subscriber queue and set `resync_pending = true`. Deltas are not accumulated for a subscriber that is `resync_pending`.
2. Per-state handling:
   - `Sending{r, id, d}`: Send `DropPending{subscription_id, id}` to the sender and change the state to `Draining{r, id, d}`. The reservation is returned when `SnapshotBufferReleased` is received.
   - `Draining`, `Reserved{r}`, `Waiting{q}`: Do not change (since `resync_pending` is already true, the next transition of each state handles it). Do not register a duplicate in the queue.
   - `Idle`: Execute **Request** below.
3. **Request**: Set `request_seq += 1`, send `RequestReservation{session_id, subscription_id, request_seq}` to the allocator, then become `Waiting{request_seq}`.
4. **Buffer creation** (only from `Reserved{r}`): Serialize S at the current revision `R` into a Body to create an immutable buffer. Attach a new `snapshot_id` that increases monotonically within the subscription and hand the SnapshotFrames to the sender. Set `base = R`, `resync_pending = false`, and change the state to `Sending{r, snapshot_id, deadline: now + 30 seconds}`.

**Allocation**: For the request at the head of the queue, when the budget headroom becomes greater than or equal to the reservation size, the allocator creates a reservation and sends `SnapshotBudgetAvailable{subscription_id, request_seq, reservation_id}` to that session actor. Queue order is never skipped. In that event step, if the subscription state is `Waiting{q}` and `q == request_seq`, the session actor changes it to `Reserved{r}` and executes step 4. In all other cases (no subscription, `Closed`, a different state, a different `request_seq`), it sends `ReturnReservation{r}`.

**Sending·Draining transitions** (matching events only):
- `SnapshotFrameWritten{last: true}` in `Sending`: Cancel the `SnapshotDeadline` timer, change the state to `Draining`, and set `resync_count = 0`. The sender releases its buffer reference immediately after writing the last frame.
- `SnapshotBufferReleased` in `Draining`: `ReturnReservation{r}`. Then, if `resync_pending`, execute **Request**; otherwise `Idle`.
- `SnapshotBufferReleased` in `Sending`: This is the case where the sender closed the connection. After `ReturnReservation{r}`, close the subscription ((b) below).
- `SnapshotDeadline` in `Sending` or in `Draining` (entered via DropPending): Close the subscription with `SLOW_CONSUMER`. If the sender is in the middle of a frame, close that connection. The reservation is returned upon receipt of `SnapshotBufferReleased`, and that return rule remains in effect even after the subscription is closed (the reservation record of a closed subscription is kept until it is returned).

**Return and cancellation**: (b) When a subscription is closed by Unsubscribe, a new Subscribe from the same subscriber, connection close, or `SLOW_CONSUMER`: if `Reserved{r}`, `ReturnReservation{r}`. If `Sending{r, id, _}` or `Draining{r, id, _}`, send `DropPending` to the sender and `ReturnReservation{r}` upon receiving `SnapshotBufferReleased`. If `Waiting{q}`, send `CancelRequest{session_id, subscription_id, request_seq: q}` to the allocator; the allocator removes only the queue entry whose key is exactly the same. If a reservation has already been issued, the allocation event that arrives afterward is returned according to the rule above. The state becomes `Closed`.

**Send deadline**: `deadline` is set once when the reservation enters Sending, and is not extended in Draining either. A single reservation enters Sending only once.

**Fairness**: The queue is FIFO with at most one entry per subscription. An issued reservation either becomes `Sending` or is returned in the step in which the session actor processes the allocation event. `Sending`·`Draining` close the subscription at `deadline` (30 seconds), and close the connection if in the middle of a frame. When the connection closes, the sender releases its buffer reference. Therefore, a reservation is returned within 30 seconds + the sender's connection close time + the session actor's event processing delay. A subscription that needs a reservation again enters at the tail of the queue. Therefore, the request at the head of the queue eventually receives a reservation.

### 2.6 Delta, Merging, Queue Limits
The fields of `Delta` are defined in §A.2. A `?` field is present only if it changed within that range, and if present, it is the full value.
- The client MUST apply a Delta only when `subscription_id` equals the current subscription, snapshot assembly is complete (§2.7), and `base_revision == local_revision`. If `subscription_id` is the same but `base_revision ≠ local_revision`, the client does not apply it and MUST `Resync`, unless a Resync is already outstanding, in which case the Delta is buffered (§2.7).

**Merge rules** (only within the actor, for consecutive not-yet-sent Deltas `D1..Dn`):
| Field | Rule |
|---|---|
| base/new | `D1.base`, `Dn.new` |
| size, cursor, modes, palette, title, exit_info | The last present value |
| dirty_rows | If any `Di` has size, all rows of the final size. Otherwise, the union of the last value per row number |
| hyperlinks_added | Union of the last value per id |
| scrollback_appended | Concatenate in order, then remove line_ids smaller than the final `scrollback_evicted_before` |
| scrollback_evicted_before | Maximum value |
| ui_events | Concatenate in order |

**Hyperlink table pruning**: The host S's hyperlink table always contains only ids referenced by the screen·scrollback. Immediately after applying all fields of a Delta (including a merged Delta), in the same apply step, the client MUST reduce the hyperlink table to the ids referenced by the screen·scrollback. Even if a merged Delta's `hyperlinks_added` has entries not referenced in the final state, they disappear through this pruning.

**Queue limits**: For each subscriber, if the sum of the encoded sizes of unsent Deltas (including all fields) exceeds 4 MiB or the number of messages exceeds 256, execute §2.5 in that step and `resync_count += 1`. Snapshot buffers are not counted toward this limit (they are managed by the §2.5 budget). When the last frame of one snapshot is written, `resync_count = 0` (§2.5). When `resync_count` reaches 3, that subscription is disconnected with `SLOW_CONSUMER`. A disconnected client re-subscribes via `disconnected → resolving → attaching` according to the §5.5 FSM.

### 2.7 Client Snapshot Assembly
- When the client adopts a new subscription via SubscribeAck, it resets any in-progress assembly, buffered Deltas, the last applied and last discarded snapshot ids, and the outstanding-Resync flag.
- The client first checks the SnapshotFrame's outer fields `subscription_id`, `session_incarnation`, `snapshot_id` (§A.3). If `subscription_id` differs from the current subscription or the incarnation differs from the binding, the frame is ignored. This check is performed before changing the assembly state.
- For a frame of the current subscription, if `snapshot_id` is smaller than the id of the in-progress assembly or less than or equal to the larger of the last applied and the last discarded snapshot id, it is ignored. If it is greater than the id of the in-progress assembly, discard the in-progress assembly and the Deltas buffered in the meantime, and start a new assembly with this id. During assembly, the existing screen is left as is.
- tag 0 (Full) carries the entire Body in one frame. tag 1 (Chunk) carries `index, total, total_len, bytes`. Chunks of the same snapshot_id must arrive in order with `index` increasing by 1 starting from 0. `total` and `total_len` must be the same in all chunks, `1 ≤ total`, `total_len ≤ S_MAX + 1 MiB`, and the sum of bytes lengths must equal `total_len`. If any of these is violated, discard the assembly, record its `snapshot_id` as the last discarded snapshot id, and send `Resync`. The remaining chunks of that snapshot are then ignored by the rule above instead of each starting a new assembly and another Resync.
- Since the send queue is FIFO, Deltas of the same subscription arrive after that snapshot. Deltas of the current subscription that arrive during assembly are buffered.
- **Outstanding Resync**: From sending `Resync` (assembly violation, `base_revision` mismatch, digest mismatch) until the next snapshot is installed, the client buffers Deltas of the current subscription as during assembly and sends no further Resync for them. Deltas already in flight when the host processed the Resync are absorbed by the snapshot instead of each causing another Resync.
- When assembly completes, replace all of S at once with the Body contents, and update `local_revision = Body.revision` and the last applied snapshot id. Among the buffered Deltas, discard those with `base_revision < Body.revision`, and apply the rest in `base` order according to the §2.6 rules. The client SHOULD recompute `state_digest` and compare it. If it differs, send `Resync`.
- After receiving `SubscribeAck{snapshot_follows: true}`, the client does not apply Deltas until the first snapshot assembly completes. A snapshot arriving due to a forced resync in the attached state is assembled under the same rules.

### 2.8 Terminal Protocol Side Effects
- Responses that go back to the PTY, such as DSR/DA, are produced only by the host VT engine and placed into the session write queue (§3.5) in generation order. A response that would cause the sum of VtReply bytes remaining in the queue to exceed 64 KiB is not placed into the queue but discarded, and `vt_replies_dropped` is incremented by 1 (exposed via SessionState). The actor does not wait because of this. Display clients MUST NOT produce such responses. Resync does not regenerate responses.
- OSC 52 clipboard writes, notifications (OSC 9/777), and the bell are delivered via the Delta's `ui_events` (payload in §A.1). `event_id` increases by 1 starting from 1 within a session incarnation and is never reused.
- A snapshot carries `next_ui_event_id` (the id of the next event to occur). The client's deduplication key is `(session_incarnation, event_id)`. Immediately after assembly, set `last_ui_event_id = next_ui_event_id - 1`; thereafter, ignore events with `event_id ≤ last_ui_event_id`, and advance `last_ui_event_id` for each processed event.
- Events discarded due to resync are not resent. The snapshot's `ui_event_gap` is true when there are unsent events between the event id last sent to this subscription and `next_ui_event_id - 1`. It is false in the snapshot of the first Subscribe.

---

## 3. Input

### 3.1 Roles
- **Origin author**: The client instance that produced the key input. An author belongs to one pane (desktop) or remote terminal view, and the target session and `binding_generation` follow the current binding of that pane·view. When the binding changes, voluntary surrender (§3.6) is executed in that step. `client_instance_id` is unique per author. It is the sole holder of input before it is accepted.
- **Policy daemon**: A relay that does not retain input bytes. It MUST NOT retain input. It maintains routing metadata: (1) For each request forwarded over a host connection, it keeps `(host connection, forwarding request_id) → (client connection, original request_id)` until response·Error·connection loss, to return the response to the original requester. (2) It keeps `client_instance_id → client connection` (the connection that last sent a request containing that id) to route `LeaseRevoked`·`InputAck` events by the event's instance id. (3) It keeps `(session_id, subscription_id) → client connection` to route SnapshotFrame·Delta, and sends `ChildExited` to all client connections that hold a subscription to that session. When a client connection is lost, it Unsubscribes at the host the subscriptions created through that connection.
- **Session actor**: Holds the ledger and the write queue.

### 3.2 Input Stream
- Input belongs to stream `(session_id, input_epoch)`. `input_epoch` is incremented by 1 within the session each time an input lease is issued.
- Stream offsets start at 0 and are not shared between epochs.
- **Immutable stream contract**: The unit of immutability is the byte position. An author MUST NOT produce different bytes at the same `(input_epoch, offset)`. Chunk boundaries may differ on each send.
### 3.3 Input lease (host side)
- Identification and retry of input lease requests and responses follow the **lease request rules** in §4.4. The writer-side state machine is §3.6.
- `AcquireLease{client_instance_id, lease_request_id, scope: Input}`: the host processes requests in arrival order and grants to the last requester. However, if there are already 64 epochs with unwritten bytes remaining in the write queue, `LIMIT_EXCEEDED(input_epochs)` (the writer retries).
- The response is `LeaseGranted{client_instance_id, lease_request_id, scope: Input, epoch, prior?}`. `prior` contains `{epoch, final_accepted, committed}` when there was a previous epoch.
- Each input epoch is in the `active` (has a holder) or `fenced` state. A new lease grant, `ReleaseLease`, and §3.4-5 change that epoch to `fenced`. A `fenced` epoch never becomes `active` again. The `accepted` of a `fenced` epoch is fixed at that value and accepts no more, but already-accepted pending bytes remain in the write queue and are written (G3-S).
- In the step that grants a new lease, the previous epoch is fenced and a `LeaseRevoked{holder_instance_id, scope: Input, revoked_epoch, new_epoch}` event is sent to the previous holder.
- `ReleaseLease{client_instance_id, lease_request_id, scope: Input, epoch}`: if the epoch (when `epoch = 0`, the current epoch) is `active` and the holder is `client_instance_id`, that epoch is fenced. Otherwise, nothing is done. The response is `LeaseReleased{client_instance_id, lease_request_id, scope, epoch, released}`.
- `Reclaim{client_instance_id, lease_request_id, scope: Input, epoch}`: if the requested epoch is `active` and the holder is the same, `ReclaimResult{..., ok: true, current_epoch: requested_epoch, input: {accepted, committed}}`. Otherwise, `ReclaimResult{..., ok: false, current_epoch(0 if there is no active holder), input_final: {final_accepted, committed, pending_dropped} of the requested epoch}`. If there is no ledger for the requested epoch (§3.4 retention), `EPOCH_UNKNOWN`.

### 3.4 Ledger
For each epoch, the session actor maintains the holder `client_instance_id`, `accepted`, `committed`, `pending_dropped`, and a `retained` ring (`[retained_start, accepted)`) holding the most recent 4 MiB of accepted bytes verbatim. Invariants: `retained_start ≤ accepted`, `committed ≤ accepted`, `accepted` and `committed` are monotonically increasing.

**Ledger retention**: The host retains the ledgers (values excluding `retained`) of the following epochs: the current epoch, epochs with unwritten bytes remaining in the write queue, and the most recent 64 epochs. Ledgers of other epochs are deleted. The `retained` ring is kept only for the current epoch. `GetEpochState`·`Reclaim` for a deleted epoch is `EPOCH_UNKNOWN`. Therefore `pending_dropped_by_epoch` contains only retained epochs with `pending_dropped > 0`, and has at most 128 entries.

`WriteInput{epoch, start, bytes, crc32c}` is processed in one actor step:
1. If `epoch` is not the current input epoch in the `active` state, `STALE_LEASE`. If `bytes` exceeds 64 KiB, `LIMIT_EXCEEDED(input_chunk)`.
2. `crc32c(bytes) ≠ crc32c` → `CORRUPT_FRAME`. The CRC is used only for transmission error detection and is not used as proof of identity.
3. `end = start.checked_add(len)`. On overflow, `LIMIT_EXCEEDED(offset_overflow)`.
4. `start > accepted` → `INPUT_GAP`.
5. If the overlap `[start, min(end, accepted))` is not empty: if that range is outside `retained`, `INPUT_UNVERIFIABLE`. If inside, the original bytes are compared and, if they differ, `INPUT_DIVERGED`. In both cases, this epoch is fenced and `LeaseRevoked{new_epoch: 0}` is sent to the writer.
6. `end ≤ accepted` → return `InputAck{client_instance_id, epoch, accepted, committed}` without changing state.
7. `start ≤ accepted < end` → put the suffix `[accepted, end)` into the write queue, append it to `retained`, and set `accepted = end`. However, if the total InputSegment bytes in the write queue would exceed 1 MiB, do not accept and return `INPUT_BACKPRESSURE`.
8. Return `InputAck{client_instance_id, epoch, accepted, committed}`.

### 3.5 Session write queue
- There is one write queue per session, and it is FIFO. Entries are `InputSegment{epoch, from, to}` or `VtReply{bytes}`, and enter in order of acceptance/generation. The InputSegment total byte limit is 1 MiB (§3.4-7) and the VtReply total byte limit is 64 KiB (§2.8); they are independent of each other.
- The actor writes the queue head entry to the non-blocking write endpoint. The next entry is not written until one entry has been completely written. Therefore VT replies are not interleaved inside an accepted input chunk.
- `EINTR` is retried immediately, and `EAGAIN` (Windows: overlapped not complete) resumes on the `PtyWritable` event. Each time bytes are written, the `committed` of that epoch is advanced, and an `InputAck` event is sent to that epoch's holder or last holder.
- When the write endpoint becomes unwritable due to `EIO`/EOF/child exit, the remaining queue bytes are added to the per-epoch `pending_dropped` and the queue is emptied. The values are included in `ChildExited` and `SessionState`.

### 3.6 Writer state machine
**State**: `cur: {epoch, accepted_known, committed_known}?`, `needs_reclaim: bool`, `pending` (§4.4 lease request), `disowned` (§4.4 lease request rules), `retry_timer?` (for lease request retry only), `send_timer?` (for WriteInput retry only), `suppressed: bool`, `unassigned` (bytes not yet assigned to an epoch), `segments` (unconfirmed bytes assigned per epoch), `finalizing: map epoch → {query_pending, retry_timer?}`, per-epoch inflight slots.

**Byte assignment**: If `cur` exists, new bytes are immediately assigned to `cur.epoch` and receive the next offset. Otherwise, they are appended to the end of `unassigned`.

**Acquire condition**: The writer has input focus, `suppressed = false`, and either `unassigned` is not empty or the user is typing. **Retain condition**: The writer has input focus and the binding is the same as when the lease was obtained.

**Reevaluate** (exactly once at the end of every handler; only the first applicable one, in the order below, is executed): (b) If `cur` exists and the retain condition is false: if the binding changed, voluntary release immediately; if only focus was lost, voluntary release when that epoch has no unaccepted bytes. (c) If `cur` exists, `needs_reclaim` holds, and there is no `pending`·`retry_timer`, Reclaim (§4.4). (p) If the §4.4 confirmation Reclaim condition holds, confirmation Reclaim. (a) If `cur` does not exist, the acquire condition is true, and there is no `pending`·`retry_timer`, Acquire (§4.4). A release deferred due to unaccepted bytes is re-decided by the Reevaluate at the end of the ACK application or lease loss handler. Invariants are the same as §4.4.

**OnGranted**: If it passes the same correspondence/context checks as §4.4 OnGranted (the writer's target session and writer binding generation are the same as at request time, and the acquire policy is true), set `cur = {epoch, 0, 0}`, `needs_reclaim = false`, `suppressed = false`, assign `unassigned` to that epoch in order starting from offset 0, and then start sending.

**Binding change**: In the step where the writer's binding changes, after the voluntary release of (b), the bytes remaining in `unassigned` are classified **not delivered** for the previous session and removed, the typing state is cleared, and a `pending` Acquire for the previous session is abandoned (its session is added to `disowned`, §4.4). Bytes typed while bound to one session are therefore never assigned to an epoch of another session. (In v11.1 `unassigned` survived the change, so the next Grant on the newly bound session would have delivered those bytes there.)

**Sending**: Of the bytes of the `cur` epoch, only the range `[accepted_known, accepted_known + 1 MiB)` is sent. This range is split into contiguous chunks of 64 KiB or less, and at most 4 WriteInputs are in flight concurrently. Each WriteInput is tagged with a local `attempt_id`. Because bytes outside one attempt's window are not sent, retransmission overlap is 1 MiB or less and lies within `retained` (4 MiB).

**ACK application**: `InputAck` (whether response or event) and `EpochState` are applied only to the epoch they contain. If `epoch == cur.epoch`, set `accepted_known = max(accepted_known, accepted)`, `committed_known = max(...)`, and delete bytes below `accepted_known` from that epoch's segment. If the epoch is in `finalizing`, only that epoch's high-water is raised. Other epochs are ignored. Responses for other epochs do not change `cur` or its slots.

**Retry**: If an attempt ends with `INPUT_BACKPRESSURE`, `LIMIT_EXCEEDED(inflight)`, `OWNER_UNREACHABLE`, or there is no response within 5 seconds, mark all slots of that epoch as discarded (subsequent responses to that attempt only perform ACK application and do not change slots·timers) and set `send_timer`. The interval starts at 50ms and doubles up to 1 second; on expiry, sending restarts from `accepted_known`. If ACK application increases `accepted_known` or `committed_known`, clear `send_timer`, resume sending immediately, and reset the interval to 50ms. On `INPUT_GAP`, set `accepted_known = max(accepted_known, detail.accepted)` and resend in the same way. `send_timer` is cleared when `cur` changes or disappears.

**Lease loss**: All of the following are paths that lose `cur` epoch E: a `LeaseRevoked` matching `cur`, `STALE_LEASE` for a request sent with `cur.epoch`, `INPUT_DIVERGED`, `INPUT_UNVERIFIABLE`, a negative `ReclaimResult`, voluntary release. In this case, in one step: mark all inflight slots of E as discarded, set `cur = none`, `pending = none` if `pending` is a Reclaim for E, move E's segment to `finalizing[E]`, and set `needs_reclaim = false`, `send_timer = none`. If the response contained E's final values (`input_final`), classify immediately; otherwise start a `GetEpochState{E}` query. If the loss is because another instance received a new lease (`LeaseRevoked.new_epoch ≠ 0`, negative Reclaim's `current_epoch ≠ 0`, `STALE_LEASE` detail's `current_epoch ≠ 0`), set `suppressed = true`. Then Reevaluate.

**Voluntary release**: Send `ReleaseLease{E}` and execute the lease loss above (`suppressed` is not changed).

**Classification query**: For each `finalizing[E]`, send `GetEpochState{E}` independently. On error·connection loss·no response within 5 seconds, resend at intervals starting at 200ms and doubling up to 2 seconds. If the response is `fenced = false` (the release was lost), resend `ReleaseLease{E}` and retry the query. If `fenced = true`, classify as below. If `EPOCH_UNKNOWN`, or the session has become `terminated`, or `OwnerDead` of the owning partition is observed, mark all remaining bytes as **outcome unknown** (if `ChildExited` contained E's `pending_dropped`, classify as much as possible using that value and the last high-water). When classification is done, delete only `finalizing[E]` and E's segment. This processing does not change `cur`, `pending`, or other epochs.

**Classification** (E's final values `final_accepted, committed, pending_dropped`):
  - offset `< committed`: **written** (including the case where only the ACK was lost).
  - `committed ≤ offset < final_accepted - pending_dropped`: **accepted, pending write**. It is subsequently written according to G3-S/G3-L.
  - `final_accepted - pending_dropped ≤ offset < final_accepted`: **accepted then dropped** (child exit, etc.).
  - `≥ final_accepted`: **not delivered**.

**Reconnect**: `pending = none`, `retry_timer = none`, mark all inflight slots as discarded. If `cur` exists, `needs_reclaim = true` (Reevaluate sends Reclaim). While `needs_reclaim`, WriteInput is not sent. If Reclaim is `ok: true` and `current_epoch == requested_epoch`, set `needs_reclaim = false`, `accepted_known = max(accepted_known, input.accepted)`, and resume sending. Reclaim Errors·timeouts are retried via the `retry_timer` of the §4.4 lease request rules. Queries in `finalizing` are resent according to their own retry rules.

**Clearing suppressed and confirmation Reclaim**: The suppressed set·clear·confirmation Reclaim rules of §4.4 are used as is. A user's new input intent is a key press·paste·focus gain occurring in this writer. The response to a confirmation Reclaim does not change `cur` or `finalizing`. Therefore two writers do not keep taking the lease from each other without user events.

- Bytes that the daemon received from the writer but died before passing to the host remain in the `cur` segment or `unassigned`, and are therefore retransmitted.

### 3.7 GetSessionState
`SessionState` (§A.5) is built with consistent values in one actor step.

---

## 4. Viewport and PTY size

### 4.1 Target token
`ViewportTarget = { window_instance_id, pane_instance_id, binding_generation: u64, revision: u64 }`
- `pane_instance_id` is a new UUID each time the pane is mounted.
- `binding_generation` increments when the pane binds a different session.
- `revision` increments within the same generation when any of the payload fields (bounds, scale_factor, visible, focused, session) changes.
- Ordering is lexicographic on `(binding_generation, revision)`. If a different payload arrives with the same token, it is reported as `PROTOCOL_VIOLATION` and ignored.
### 4.2 GPU device actor
- Each window has one GPU device actor. This actor owns the `wgpu` device/queue and manages `device_generation: u64`.
- The device lost callback is delivered only to the device actor mailbox. In that step, the device actor changes its state to `Lost`, performs `device_generation += 1`, and then sends `DeviceLost{new_generation}` to all pane actors of the window. It then attempts reconstruction.
- A reconstruction attempt carries the `device_generation` at the time it started as a token. The result returns to the device actor mailbox and is applied only when the token equals the current `device_generation` and the state is `Lost`. If they differ, the device created by the result is discarded.
- If reconstruction succeeds, the state is changed to `Ready` and `DeviceReady{generation}` is sent to all pane actors. If it fails, it retries indefinitely at intervals of 100ms, 300ms, 1s, and 5s thereafter. Starting from the third failure, it sends `DeviceDegraded{generation, reason}` to the pane actors.
- **Pane registration**: When a pane actor is created, it sends `Register` to the device actor, and in that step the device actor adds the pane to the broadcast targets and returns the current `{state, device_generation}`. The pane initializes `gpu` and `device_generation` with this value. Before registration, Prepare is not started (`deferred = true`). A Destroyed pane sends `Unregister`.
- Messages from the device actor to each pane arrive in the order they were sent.

### 4.3 Pane actor
Each pane is owned by one pane actor on the native UI thread. Shared display state (child view frame, visibility, `wgpu::Surface` configuration, present) changes only in the pane actor's Apply step.

Pane actor state: `latest_accepted`, `committed`, `last_visible_committed` (including size), `attempt_id: u64`, `device_generation`, `gpu: Ready|Lost|Degraded`, `tombstoned`, Resize state (§4.4).

1. **Accept(target)**: If `tombstoned`, reject. If `target ≤ latest_accepted`, `STALE_VIEWPORT`. Otherwise, in one step: if `target.binding_generation` differs from `latest_accepted`, or `target` is Hidden, or `target.focused` is false, execute §4.4 Relinquish. Set `latest_accepted = target`, `attempt_id += 1`, and start Prepare.
2. **RetryLatest** (internal trigger): If not `tombstoned`, `attempt_id += 1` and start Prepare with `latest_accepted`. The target does not change.
3. **Prepare** (worker): Produces only a candidate that does not touch shared state.
   - If the target is Hidden (`visible = false` or either the width or height of the physical size is 0), `Candidate{kind: Hidden}` without GPU work.
   - Otherwise, if pane `gpu ≠ Ready`, do not start Prepare and set `deferred = true`.
   - Otherwise, compute the physical size, render the current CPU grid model frame into an offscreen render target of the new size, and compute cols/rows. The result is `Candidate{kind: Visible, target, attempt_id, device_generation, ...}`.
4. **Apply(candidate)** (actor step, no internal await): If `tombstoned` or `candidate.attempt_id ≠ attempt_id`, discard. If `kind: Visible` and `candidate.device_generation ≠ device_generation`, also discard.
   - `kind: Hidden`: hide the child view → `committed = target` → receipt `Hidden` → §4.4 Relinquish. Do not configure/present.
   - `kind: Visible`: child view frame and show → `surface.configure` → blit candidate frame → `present` → `committed = target`, `last_visible_committed = target` → receipt `Ready` → update Resize state (§4.4 OnVisibleApplied).
   - If any Visible step fails, leave `committed` unchanged. Send receipt `Degraded{reason, next_retry_ms}` and schedule RetryLatest. Surface errors (`Outdated`/`Lost`/`Timeout`) do not change the device state and only perform RetryLatest.
5. **DeviceLost{g}**: If `g ≤ device_generation`, ignore. Otherwise, `gpu = Lost`, `device_generation = g` (all previous candidates are invalid). §4.4 Relinquish. Receipt `Degraded{reason: "device_lost"}`.
6. **DeviceReady{g}**: If `g < device_generation`, ignore. Otherwise, `gpu = Ready`, `device_generation = g`, RetryLatest (including any pending Prepare).
7. **DeviceDegraded{g}**: If `g ≠ device_generation` or `gpu = Ready`, ignore. Otherwise, `gpu = Degraded`. Send receipt `Degraded` and display a user retry button. When the button is pressed, request an immediate retry from the device actor.
8a. **Registered{state, g}** (§4.2 registration response): `gpu = state`, `device_generation = g`. If `deferred` and `gpu = Ready`, RetryLatest.
8. **Destroy**: In the same step, hide the child view, detach it from the window, and release the surface. `tombstoned = true`. Thereafter, discard all Prepare results and RetryLatest. §4.4 Relinquish.

- All handlers (1–8, Apply) finish without any side effects (receipt, retry scheduling, UI display, lease request) if `tombstoned`. The Relinquish in 8 is the exception.
- After each handler finishes, execute §4.4 Reevaluate exactly once. Relinquish within a handler does not execute Reevaluate.

Receipt `{target, state: Ready{presented_state_revision} | Hidden | Degraded{reason, next_retry_ms}}`. `Ready` means "the latest accepted target has been applied and the frame of that revision has been submitted to present", and is not proof that the compositor actually displayed it.
### 4.4 Resize lease and PTY synchronization
**Host side**
- The Resize lease is independent of the input lease. `AcquireLease{client_instance_id = pane_instance_id, scope: Resize}` requests are processed in arrival order and the lease is issued to the last requester: `LeaseGranted{client_instance_id, lease_request_id, scope: Resize, epoch: resize_epoch, resize_applied: {epoch, seq, cols, rows}}`. `resize_epoch` increases by 1 on each issuance. The previous holder is sent `LeaseRevoked{holder_instance_id, scope: Resize, revoked_epoch, new_epoch}`.
- `Resize{resize_epoch, resize_seq, cols, rows}`: if the epoch is not current, `STALE_LEASE`. If `(epoch, seq) ≤ applied`, `STALE_RESIZE`. Otherwise, apply it and return `ResizeAck{epoch, seq, cols, rows}`.
- `ReleaseLease{client_instance_id, lease_request_id, scope: Resize, epoch}`: if it is the current epoch (or `epoch = 0`) and the holder is `client_instance_id`, clear the holder. The response is `LeaseReleased`. Afterwards, a Resize for that epoch becomes `STALE_LEASE`.
- `Reclaim{client_instance_id, lease_request_id, scope: Resize, epoch}`: if the requested epoch is the current epoch and the holder is the same, `ReclaimResult{client_instance_id, lease_request_id, requested_epoch, ok: true, current_epoch: requested_epoch, resize: applied}`; otherwise `ReclaimResult{client_instance_id, lease_request_id, requested_epoch, ok: false, current_epoch(0 if there is no holder), resize: applied}`.

**Vacant lease notification** (common to input and Resize, host side): The host keeps a `revokees` set per scope. It adds the previous holder that was sent `LeaseRevoked{new_epoch ≠ 0}`, and a Reclaim requester that received `ok: false` because another holder exists. An instance that receives a new lease is removed. In a step where the holder goes away (ReleaseLease, the §3.4-5 fence of the input epoch), the host sends a `LeaseVacated{holder_instance_id, scope, epoch}` event to each instance in `revokees` and clears the set.

**Lease request rules** (common to input lease and Resize lease)
- A client instance (pane or writer) issues `lease_request_id: u64` monotonically increasing from 1 and does not reuse it. This value continues across connection changes. AcquireLease, Reclaim, and ReleaseLease requests and their responses all carry `client_instance_id` and this value. Responses are matched to in-flight requests by `(client_instance_id, lease_request_id)`. The host puts the request's `client_instance_id` into the response unchanged. Error frames do not carry this value, so the client matches an Error to the original request by the header `request_id` on the same connection (the daemon restores the original request_id via §3.1 (1)).
- If an in-flight request ends with an Error or has no response for 5 seconds, and that request is still `pending`, set `pending = none` and start `retry_timer`. If it was an AcquireLease, add its session to `disowned` (an Error relayed after the request reached the host does not prove that no Grant was issued). The interval doubles from 200ms up to 2s, and is reset to 200ms when a success response is received. Retries use a new `lease_request_id`.
- If the connection drops, `pending = none`. After reconnection, run the reconnection rule below.
- If a `LeaseGranted` arrives that does not match an in-flight request, return it by sending `ReleaseLease` with that Grant's session and epoch, and add that session to `disowned` unless the client holds a lease on it.
- **Stale request filter** (host): For each session, scope and `client_instance_id`, the host keeps the largest `lease_request_id` it has processed. An AcquireLease·Reclaim·ReleaseLease whose `lease_request_id` is less than or equal to that value was overtaken by a newer request of the same instance (for example, it was still buffered on a dropped connection or on another daemon connection) and is discarded without a response. Without this filter a delayed AcquireLease revokes the lease its own instance obtained later and suppresses that instance (§9.1). The table is deleted with the session and holds at most 1,024 entries per session and scope; when it is full, the entry processed longest ago is removed.
- **Disowned sessions** (client): `disowned` is the set of sessions where the host may record this instance as holder although the client holds no lease there. A session is added when an AcquireLease to it ends without a Grant being kept (Error, timeout, connection drop, Relinquish or binding change while it is `pending`, a returned Grant) and when a held lease is relinquished. It is removed when a Grant for it is kept, when the `LeaseReleased` of a confirming release for it arrives, and when the session terminates or its owner is lost. The **confirming release** sends `ReleaseLease{epoch: 0}` to one session in `disowned` with a new `lease_request_id` and sets `pending = {id, Release, session}`; Errors and timeouts are retried per the rules above. Without it, an AcquireLease abandoned by the client but granted later leaves an orphan holder that never releases, and a client suppressed by that Grant never receives `LeaseVacated` (§9.1).

**Pane-side state**: `lease: {session_id, binding_generation, epoch}?`, `needs_reclaim: bool`, `pending: {lease_request_id, kind: Acquire|Reclaim|Probe|Release, session_id, binding_generation, requested_epoch?}?`, `disowned: set<session_id>`, `retry_timer?`, `suppressed: bool`, `suppressed_epoch: u64`, `needs_probe: bool`, `next_seq: u64`, `latest_sent: (seq, cols, rows)?`, `pty_acked: (seq, cols, rows)?`, `pty_desired: (cols, rows)?`. The sizes in `pty_desired` and `latest_sent` are always values with `normalize_size` (§2.2) applied. Below, "lease match" means the message's session equals `lease.session_id` and its epoch equals `lease.epoch`.

**Acquisition policy**: True when all of the following hold. Not `tombstoned`. `latest_accepted` is bound to a session and is Visible and focused (key focus on desktop; user input/tab selection on remote clients). `last_visible_committed` exists and its `binding_generation` equals the current binding (a Visible Apply has succeeded at least once in the current binding). `gpu = Ready`. `suppressed = false`. Below, "current binding" is the session and `binding_generation` of `latest_accepted`.

**Rules**
- **Reevaluate**: Each handler in §4.3 and each handler below runs Reevaluate exactly once at its end. Reevaluate runs only the first applicable one, in the following order: (b) If the acquisition policy is false and `lease` exists, Relinquish. (c) If `lease` exists, `needs_reclaim`, and there is no `pending`/`retry_timer`, Reclaim. (p) If there is no `lease`, `suppressed ∧ needs_probe`, and there is no `pending`/`retry_timer`, probe Reclaim. (a) If the acquisition policy is true and `lease`, `pending`, and `retry_timer` are all absent, Acquire. (r) If `disowned` is not empty and `pending` and `retry_timer` are absent, confirming release (lease request rules). Invariant: **After any handler finishes, the state never remains (acquisition policy true ∧ no lease), (lease exists ∧ needs_reclaim) or (`disowned` not empty) with both `pending` and `retry_timer` absent. A lease never remains when the acquisition policy is false.**
- **Retry timer expiry**: First set `retry_timer = none`, then Reevaluate.
- **Lease invalidation**: `lease = none`, `needs_reclaim = false`, `latest_sent = none`, `pty_acked = none`. If `pending` is a Reclaim for that lease, `pending = none`.
- **Acquire**: Send `AcquireLease` with a new `lease_request_id` and set `pending = {id, Acquire, session_id of the current binding, binding_generation}`.
- **Reclaim**: Send `Reclaim{lease.epoch}` with a new `lease_request_id` and set `pending = {id, Reclaim, lease.session_id, lease.binding_generation, requested_epoch: lease.epoch}`.
- **OnGranted(lease_request_id, epoch, applied)**: If `pending` is not the Acquire with this id, send `ReleaseLease{epoch}` to that Grant's session and add it to `disowned` unless the pane holds a lease on it. If it is, copy `pending`'s context locally and set `pending = none`. If the current binding's session and `binding_generation` equal the copied values and the acquisition policy is true, set `lease = {session, generation, epoch}`, remove the session from `disowned`, `needs_reclaim = false`, `suppressed = false`, `next_seq = 1`, `latest_sent = none`, `pty_acked = none`, `pty_desired = normalize_size(cols/rows of last_visible_committed)` and run PtySync. Otherwise, send `ReleaseLease{epoch}` and add the session to `disowned`.
- **OnVisibleApplied**: `pty_desired = normalize_size(cols/rows of this target)`. If a lease exists, PtySync.
- **PtySync**: If `lease` and `pty_desired` exist, `needs_reclaim` is false, the acquisition policy is true, `lease.binding_generation` equals the current binding, and `latest_sent` is absent or the size in `latest_sent` differs from `pty_desired`: `seq = next_seq; next_seq += 1`, send `Resize{lease.epoch, seq, pty_desired}` and set `latest_sent = (seq, pty_desired)`. The criterion is the last sent target, not the last ACK.
- **Awaiting confirmation and retry**: A state where `latest_sent` exists and `pty_acked` is absent or `pty_acked.seq < latest_sent.seq` is called "unconfirmed". If unconfirmed continues for 2 seconds, or the last Resize ends with an Error other than `STALE_LEASE`/`STALE_RESIZE` or a 5-second timeout, set `latest_sent = none` and run PtySync again. The interval doubles from 200ms up to 2s, and is reset to 200ms when an ACK is received. Retries always use a new seq, so even if an earlier request arrives late, the larger seq sent last wins.
- **OnResizeAck(epoch, seq, size)**: Only when lease match and (`pty_acked` is absent or `seq > pty_acked.seq`), `pty_acked = (seq, size)`. Then PtySync.
- **OnStaleResize(applied)**: If a lease exists and `applied.epoch == lease.epoch`, `next_seq = max(next_seq, applied.seq + 1)`, `latest_sent = none`, PtySync.
- **OnStaleLease** (`STALE_LEASE` in response to a Resize request): If lease match, lease invalidation. If the detail's `current_epoch ≠ 0`, `suppressed = true`.
- **Reconnection**: If `pending` is an Acquire, add its session to `disowned`. `pending = none`, `retry_timer = none`. If a lease exists, `needs_reclaim = true`. (Reevaluate runs Reclaim or Acquire.)
- **OnReclaimResult(lease_request_id, requested_epoch, ok, current_epoch, applied)**: If `pending` is not the Reclaim with this id, ignore. If it is, copy the context locally and set `pending = none`. If `lease` is absent or `lease.session_id`/`lease.binding_generation`/`lease.epoch` differ from the copied values, stop processing here (the Reevaluate at the end still runs). If `ok` and `current_epoch == requested_epoch`, set `needs_reclaim = false`, `next_seq = max(next_seq, applied.seq + 1)`, `latest_sent = none` and run PtySync. This seq is larger than every seq sent so far, so it overrides delayed requests sent before the disconnection. In other cases (`ok: false`, or `ok` but the epoch differs), lease invalidation, and if `current_epoch ≠ 0`, `suppressed = true`.
- **Relinquish** (internal cleanup operation; does not run Reevaluate): If a lease exists, send `ReleaseLease{lease.epoch}` to that lease's session, add the session to `disowned`, and perform lease invalidation. If `pending` is an Acquire, add its session to `disowned` and set `pending = none` (if that Grant arrives later, it is returned per OnGranted).
- **OnRevoked(revoked_epoch, new_epoch)**: Only when the session matches and `revoked_epoch == lease.epoch`, lease invalidation, and if `new_epoch ≠ 0`, `suppressed = true`. A Revoked for a different epoch does not change state.
- **Setting suppressed**: When setting `suppressed = true`, record the lost epoch in `suppressed_epoch`.
- **Clearing suppressed**: If any of the following occurs, `suppressed = false`, `needs_probe = false`: a new focus gain or user input occurs on this pane, `LeaseVacated` for this scope is received, a new lease is acquired, or `current_epoch == 0` in the probe Reclaim below. Therefore, two clients do not keep taking the lease from each other without user events, and when the holder goes away, the suppressed client competes again.
- **Probe Reclaim**: On reconnection, if `suppressed`, `needs_probe = true` (to compensate for a `LeaseVacated` lost during reconnection). If lease, pending, and retry_timer are absent and `needs_probe`, Reevaluate sends `Reclaim{suppressed_epoch}` with a new `lease_request_id` and sets `pending = {id, Probe}`. On receiving the response, set `needs_probe = false`, and if `current_epoch == 0`, clear suppressed. Errors and timeouts are retried per the lease request rules.
- A `LeaseReleased` matching the `pending` confirming release sets `pending = none` and removes that session from `disowned`; any other `LeaseReleased` does not change pane state. A ReleaseLease sent with a specific epoch is not retried; the confirming release covers its loss. (v11.1 assumed that "the next Acquire replaces that epoch", but a client suppressed by the orphan Grant never sends that Acquire.)
- A pane without a lease draws the host size as is (letterbox).

**A→B→A example**: Applied size A, `latest_sent=(1,A)`. B is Applied and `(2,B)` is sent (delayed). When A is Applied again, `pty_desired=A ≠ latest_sent.B`, so `(3,A)` is sent. Even if the delayed `(2,B)` is applied first, `(3,A)` is applied after it. If `(3,A)` is lost, the reconnection rule resends A with a new seq.

### 4.5 Miscellaneous
- Render data is rebuilt from the CPU-side grid model built from the last Snapshot+Delta. Losing the GPU does not lose the screen state.
- Even a new session with no output MUST present a valid frame with background and cursor on the first Visible Apply.
- The per-call IPC timeout (5s) is only a failure of that request; there is no permanent failure latch.

---

## 5. Owner registry and liveness determination

### 5.1 Partitions
`OwnerPartition = MicroHost{host_instance_id, pid, process_start_time} | LegacyDaemon{legacy_instance_id, pid, process_start_time, endpoint}`
### 5.2 Creation intent recording and orphans
- The daemon first sends `ReserveOperation{kind: Spawn}` to the target host and receives an `operation_id` (§7.5). Then, **before** sending Spawn, it MUST record `intent{operation_id, partition, workspace, layout_hint, spawn_payload, state: pending}` in the DB with WAL fsync. `spawn_payload` is the full set of Spawn's `cols, rows, program, args, env, cwd`. On receiving `SpawnResult`, it updates to `state: created{session_id, session_incarnation, created_table_revision}` (fsync) and then responds to the UI. The partition is already in the intent.
- The host preserves `creation_operation_id` for as long as the session is alive. This value is exposed in `SessionList` entries and in `SessionState`.
- On restart, a `pending` intent is completed by resending Spawn with the same `operation_id` and the stored `spawn_payload` (because of §7.5 idempotency, there is exactly one session). If `OPERATION_EXPIRED`, `INSTANCE_RETIRED`, or a stored-Spawn execution error (§7.5) is received, or `OwnerDead` for that partition is observed, the intent is changed to `failed` and the UI is notified. If the daemon dies after only reserving and before recording the intent, that reservation is evicted, unused by anyone, under the §7.5 rules.
- A session that is in the inventory but has no DB intent is exposed to the UI as `owner_metadata_missing` and is not terminated.

### 5.3 Restart recovery and routing
1. Load intents, mappings, and the partition list from the DB.
2. Find host instances in the instance registry (§1.1), and also add instances not in the DB as partitions.
3. For each partition, perform connect → Hello (identity verification) → `ListSessions`.
4. Session commands (input, Resize, lease, Subscribe, Kill) are sent only to the mapped partition. If it cannot be connected, return `OWNER_UNREACHABLE`. Forwarding to a different partition MUST NOT occur.

### 5.4 Inventory and freshness
- The daemon has, per instance, an `aggregator_instance_id` (UUID) and a monotonically increasing `inventory_seq: u64`. `request_token = (aggregator_instance_id, inventory_seq)`.
- The response to MicroHost `ListSessions{request_token}` is `SessionList` (§A.5). `complete` is true when the host enumerated the entire session table in a single actor step. `table_revision` increments by 1 on every session creation, child exit, and table deletion.
- **Retention of exited sessions**: When a child exits, the session actor, in that step, cleans up the write queue (§3.5), records S.exit_info (state_revision +1), increments `table_revision`, and sends `ChildExited`. The session remains in the table with `child_running = false` and responds to Subscribe·GetSessionState·GetEpochState. Once 24 hours have passed since exit and there are no subscriptions, it is deleted from the table (`table_revision` +1).
- LegacyDaemon adapter: the old daemon's `ListSessions` enumerates the entire in-process session map. Therefore, a successful response on a connection whose identity (pid, start_time) was confirmed by Hello has `complete = true`, `table_revision = 0`. No new sessions are created in a Legacy partition. Failures, timeouts, and partial responses are `complete = false`.
- The UI aggregate response is `AggregatedInventory{request_token, partitions: [{partition, reachability: reachable|unreachable|owner_dead, complete, table_revision?}], sessions}`. There is no single top-level `complete`. When the daemon observes `OwnerDead` for a partition, it records that fact in the DB and thereafter reports that partition as `owner_dead` in every inventory. Therefore, a binding that missed the `OwnerDead` event also learns of the death in the next inventory.
- A UI binding retains the last processed `request_token`. It ignores responses whose `aggregator_instance_id` is the same and whose `inventory_seq` is smaller, or responses from an aggregator other than the instance the daemon announced as the current aggregator.

### 5.5 Binding FSM
States are `unbound, resolving, attaching, attached, disconnected, terminated`. A binding retains `{session_id, session_incarnation?, partition?, created_table_revision?, adopting: bool, attach_seq, subscription_id?, last_request_token}`. Each time it enters attaching, it sets `attach_seq += 1` and includes it in Subscribe. The MicroHost absence-determination rows below apply only to trusted bindings that have `created_table_revision`.

| from | event | condition | to |
|---|---|---|---|
| unbound | layout load | stored session_id exists | resolving |
| resolving | inventory (latest token) | owning partition Reachable, session exists, incarnation matches, `child_running` | attaching |
| resolving | inventory (latest token) | owning partition Reachable, session exists, incarnation matches, `¬child_running` | terminated(exited), record exit_info |
| resolving | inventory | owning partition Unreachable or complete=false | resolving (retry) |
| resolving | no response to inventory request for 5 seconds, or Error | — | resolving (retry) |
| resolving | inventory | MicroHost: Reachable ∧ complete ∧ `table_revision ≥ created_table_revision` ∧ absent | terminated(absent) |
| resolving | inventory | MicroHost: `table_revision < created_table_revision` | resolving |
| resolving | inventory | Legacy: Reachable ∧ complete ∧ absent | terminated(absent) |
| resolving | inventory | an entry with the same session_id exists but its incarnation differs from the binding | treat that entry as absent and apply the absence rows above |
| attaching | SubscribeAck | `attach_seq` equals the current attempt, incarnation matches. Record subscription_id | attached |
| attaching, attached | SubscribeAck | `attach_seq` differs from the current attempt | ignore. That subscription is closed when a Subscribe with a larger attach_seq from the same subscriber_id is processed at the host (§2.4-0). In the meantime, frames of that subscription are ignored due to subscription_id mismatch |
| attaching | `SESSION_NOT_FOUND` | — | resolving |
| attaching | `STALE_SUBSCRIBE{current}` | — | set `attach_seq = current + 1` and attaching (resend) |
| attaching | `LIMIT_EXCEEDED(subscribers)` | — | disconnected |
| attached | exit_info present in Delta·Snapshot | incarnation matches | terminated(exited) (subscription may be kept for display) |
| attaching, attached | disconnection, no SubscribeAck for 10 seconds, `OWNER_UNREACHABLE`, `SLOW_CONSUMER`, any other Error not in the rows above | — | disconnected |
| disconnected | partition connection available (reconnection completed, or the connection is still alive as with SLOW_CONSUMER). Backoff from 200ms, doubling up to 5 seconds | — | resolving |
| resolving, attaching, attached, disconnected | `ChildExited{incarnation}` or `¬child_running` in `SessionState` | incarnation matches | terminated(exited) |
| resolving, attaching, attached, disconnected | `OwnerDead{partition}` event, or `reachability = owner_dead` for that partition in inventory (§5.6) | partition matches | terminated(owner_lost) |
| any | incarnation mismatch, stale request_token, different subscription_id | — | ignore |
| terminated | user New Shell | — | new intent. On receiving SpawnResult, attaching with a new trusted binding. If Spawn ends with an error, remain terminated and display the error |

- **resolving retry**: While in resolving, if there is no inventory request awaiting a response, request inventory with a new `inventory_seq`. The retry interval starts at 200ms and doubles up to 5 seconds, and is reset upon becoming attached. Therefore, the binding does not stay in resolving due to lost requests/responses.
- `terminated` retains the session_id, the last snapshot, and the scrollback. They are deleted only when the pane is closed.
- **Final screen**: If a binding that has become `terminated(exited)` has no applied snapshot, then while the session remains in the table, it Subscribes display-only, receives one snapshot, and then Unsubscribes. Failure of this subscription does not change the lifecycle.
- **Trusted bindings and hints**: A binding whose DB intent is `created` has all of `{session_id, incarnation, partition, created_table_revision}`. A binding with only a session_id hint from the layout (no intent) resolves in the **adoption** state: it looks for that session_id in the inventories of all partitions, and if it is found in exactly one partition, it records that SessionEntry's incarnation·created_table_revision and that partition, promotes itself to a trusted binding, and then writes a `created` mapping to the DB. If all known partitions are Reachable ∧ complete and it is found nowhere, `terminated(absent)`. Otherwise, it remains resolving. An unknown created_table_revision is not treated as 0.
- The speculative-revival clause and the `backendSessionId = null` handling in `sessionPersistence.ts` are removed. The localStorage layout provides only the pane structure and session_id hints.

### 5.6 Positive evidence of owner death
- Immediately after partition Hello, the daemon confirms `(pid, process_start_time)` with the OS and registers an exit watch. After registering, it re-reads the start time to rule out a PID-reuse race. Per-platform mechanisms: macOS `kqueue EVFILT_PROC NOTE_EXIT` + `proc_pidinfo`, Linux `pidfd_open` + `/proc/<pid>/stat` starttime, Windows `OpenProcess(SYNCHRONIZE)` + `GetProcessTimes`.
- `OwnerDead{partition}` is emitted only when one of the following is observed: (a) an exit-watch event, (b) an OS report that the pid does not exist or that the start time differs. Timeouts and connection failures are not evidence.
- When the daemon restarts, it checks each partition's `(pid, start_time)` in the DB in the same way.

### 5.7 Legacy retirement
- Automatic retirement is permitted only when a response is received in which the Legacy partition is Reachable ∧ complete ∧ has 0 sessions.
- Otherwise, retirement occurs only when the user explicitly executes "Terminate all remaining Legacy sessions". This action shows the list of remaining sessions and obtains confirmation. Closing a tab is not a retirement condition.

---

## 6. Deployment·Update

### 6.1 Initial transition
1. When installing the new app, the updater MUST NOT terminate the existing daemon.
2. The new-policy daemon uses a new path (socket·lock·DB under `ferryx.v6/`). The existing daemon's `/tmp/rorca-{uid}/daemon.sock` is registered as a LegacyDaemon partition.
3. Existing sessions continue to be used via Legacy routing. New sessions are created only on MicroHost.
4. Retirement follows §5.7.

### 6.2 Host update
- Host updates are performed only by adding a new instance. The updater installs a dedicated executable with a new `instance_id` and registers·starts that instance's service management unit (§1.1). Once the new instance is registered as `active` in the registry and the daemon confirms it via Hello, the updater changes the old instance's registry state to `retired` and notifies the old instance. The old instance's service unit is not stopped/restarted.
- A `retired` instance rejects `ReserveOperation{Spawn}` with `INSTANCE_RETIRED` (the daemon re-reads the registry and reserves again on the active instance). Only a `retired` instance, when it has 0 sessions and 0 operations in the `reserved` state, stops accepting new requests together with that determination at the serialization point of the operation table, terminates, and removes its own service unit registration. An `active` instance does not terminate even if it has 0 sessions. New Spawns are not sent to a `retired` instance.
- If the old instance must be forcibly terminated due to a security update, this is an exception to G1. It requires a per-session termination warning and user confirmation.

---

## 7. FXSH wire protocol

### 7.1 Header (40 bytes, big-endian, manual per-field encode/decode)
| offset | size | field |
|---|---|---|
| 0 | 4 | magic `0x46585348` |
| 4 | 2 | major (=1) |
| 6 | 2 | minor (=0) |
| 8 | 2 | command |
| 10 | 2 | flags: bit0 response, bit1 error, bit2 unsolicited event, the rest MUST be 0 |
| 12 | 16 | session_id (all 0 = not session-specific) |
| 28 | 8 | request_id: monotonically increased by the requester within a connection. Responses carry the original value, unsolicited events carry 0. Not used for freshness determination across connections |
| 36 | 4 | payload_len (≤ 16 MiB) |

### 7.2 Basic encoding
- Integers are big-endian fixed-width. `bool` is u8 (0/1, other values are `PROTOCOL_VIOLATION`). `str` = u32 len + UTF-8 (invalid UTF-8 is `PROTOCOL_VIOLATION`). `bytes` = u32 len + raw. `[T]` = u32 count + elements. `opt<T>` = u8 tag(0/1) + T. `UUID` = 16 bytes. `pair(a,b)` = a followed by b.
- For `opt` fields, the tag is always encoded regardless of the presence condition. Violating a "present iff" condition in the schemas below is `PROTOCOL_VIOLATION`.
- Payloads are encoded in schema order. Fields added in a minor version are appended only at the end, and the receiver MUST ignore residual bytes after the fields it knows.

### 7.3 Connection establishment
- The first frame is `Hello`, and the response is `HelloAck` (§7.4).
- If major differs, send `VERSION_UNSUPPORTED` and drop the connection. A command without the capability gets `UNSUPPORTED_CAPABILITY` and has no effect on session state.
- On magic mismatch, payload_len exceeded, truncated frame, or undefined flags bits, drop the connection immediately. For an unknown command, send `UNKNOWN_COMMAND` and keep the connection.
### 7.4 Commands and schemas
enum: `client_kind` u8 = 1 gui_window, 2 remote_client, 3 policy_daemon. `supervisor` u8 = 1 launchd, 2 systemd, 3 taskscheduler. `scope` u8 = 1 Input, 2 Resize. `signal` u8 = 1 hangup, 2 interrupt, 3 terminate, 4 kill.

| id | Name | Direction | Payload |
|---|---|---|---|
| 0x01 | Hello | C→H | major u16, minor u16, capabilities u64, client_kind u8, client_instance_id UUID (the connecting principal. When the daemon relays, this is the daemon's own id; the principal ids for lease, input, and subscription are carried in each command payload) |
| 0x02 | HelloAck | H→C | major u16, minor u16, capabilities u64, host_instance_id UUID, supervisor u8, pid u32, process_start_time u64 (OS-specific start identifier: macOS `proc_bsdinfo.pbi_start_tvsec`×10⁶+`pbi_start_tvusec`, Linux `/proc/<pid>/stat` starttime tick, Windows `GetProcessTimes` creation FILETIME. PID reuse verification is done only by equality comparison of this value), host_version str |
| 0x03 | Spawn | C→H | operation_id OperationId, cols u16, rows u16, program str, args [str], env [pair(str,str)], cwd str |
| 0x04 | SpawnResult | H→C | operation_id OperationId, session_id UUID, session_incarnation UUID, pid u32, created bool, created_table_revision u64 |
| 0x05 | AcquireLease | C→H | client_instance_id UUID, lease_request_id u64, scope u8 |
| 0x06 | LeaseGranted | H→C | client_instance_id UUID, lease_request_id u64, scope u8, epoch u64, prior opt{epoch u64, final_accepted u64, committed u64} (present iff scope=Input and a previous input epoch exists), resize_applied opt{epoch u64, seq u64, cols u16, rows u16} (present iff scope=Resize) |
| 0x07 | ReleaseLease | C→H | client_instance_id UUID, lease_request_id u64, scope u8, epoch u64 |
| 0x08 | LeaseReleased | H→C | client_instance_id UUID, lease_request_id u64, scope u8, epoch u64, released bool |
| 0x09 | LeaseRevoked | H→C event | holder_instance_id UUID, scope u8, revoked_epoch u64, new_epoch u64 (0 if there is no holder) |
| 0x0A | Reclaim | C→H | client_instance_id UUID, lease_request_id u64, scope u8, epoch u64 |
| 0x0B | ReclaimResult | H→C | client_instance_id UUID, lease_request_id u64, scope u8, requested_epoch u64, ok bool, current_epoch u64 (equal to requested_epoch if ok, otherwise the current active epoch or 0), input opt{accepted u64, committed u64} (present iff scope=Input ∧ ok), input_final opt{final_accepted u64, committed u64, pending_dropped u64} (present iff scope=Input ∧ ¬ok, values for the requested epoch), resize opt{epoch u64, seq u64, cols u16, rows u16} (present iff scope=Resize) |
| 0x0C | WriteInput | C→H | client_instance_id UUID, epoch u64, start u64, bytes, crc32c u32 |
| 0x0D | InputAck | H→C (response and event) | client_instance_id UUID (the holder or last holder of that epoch), epoch u64, accepted u64, committed u64 |
| 0x0E | GetEpochState | C→H | client_instance_id UUID, epoch u64 |
| 0x0F | EpochState | H→C | epoch u64, final_accepted u64, committed u64, pending_dropped u64, fenced bool |
| 0x10 | Resize | C→H | resize_epoch u64, resize_seq u64, cols u16, rows u16 |
| 0x11 | ResizeAck | H→C | epoch u64, seq u64, cols u16, rows u16 |
| 0x12 | Subscribe | C→H | subscriber_id UUID, attach_seq u64, client_known_revision opt u64, client_known_incarnation opt UUID |
| 0x13 | SubscribeAck | H→C | attach_seq u64, subscription_id u64, session_incarnation UUID, revision u64, snapshot_follows bool |
| 0x14 | SnapshotFrame | H→C event | §A.3 |
| 0x15 | Delta | H→C event | §A.2 |
| 0x16 | Resync | C→H | subscription_id u64 |
| 0x17 | Unsubscribe | C→H | subscription_id u64 |
| 0x18 | GetSessionState | C→H | (none) |
| 0x19 | SessionState | H→C | §A.5 |
| 0x1A | ListSessions | C→H | request_token pair(UUID, u64) |
| 0x1B | SessionList | H→C | §A.5 |
| 0x1C | Kill | C→H | operation_id OperationId, signal u8 |
| 0x1D | KillResult | H→C | operation_id OperationId, delivered bool |
| 0x1E | ChildExited | H→C event | session_incarnation UUID, exit_code opt i32, posix_signal opt u8 (present iff terminated by a signal, POSIX only), table_revision u64, pending_dropped_by_epoch [pair(u64,u64)] (those §3.4 retained epochs whose value > 0, at most 128) |
| 0x1F | Error | H→C | code u16, message str, detail bytes (encoded with the §7.6 schema) |
| 0x20 | ReserveOperation | C→H | kind u8 (1 Spawn, 2 Kill) |
| 0x21 | OperationReserved | H→C | operation_id OperationId |
| 0x22 | LeaseVacated | H→C event | holder_instance_id UUID (notification target), scope u8, epoch u64 (the epoch that became vacant) |

`Kill.signal` handling: On POSIX, 1→SIGHUP, 2→SIGINT, 3→SIGTERM, 4→SIGKILL are sent to the child process group. On Windows, 2→`GenerateConsoleCtrlEvent(CTRL_C_EVENT)`, 1/3→close ConPTY, 4→Job Object `TerminateJobObject`.

### 7.5 Idempotency
- `OperationId = (host_instance_id UUID, op_seq u64)` and only the host issues it. On receiving `ReserveOperation{kind}`, the host creates an id with `op_seq = ++max_issued`, records `{kind, state: reserved, reserved_at}` in the table, and then returns it in `OperationReserved`.
- All time judgments in this section use the host process's sleep-inclusive monotonic clock (macOS `mach_continuous_time`, Linux `CLOCK_BOOTTIME`, Windows `QueryInterruptTime`). The wall clock is not used.
- `Spawn`/`Kill` decisions are made at a single serialization point of the host operation table, in the following order. This decision is made before session routing and existence checks (Kill does not return `SESSION_NOT_FOUND`):
  1. If `host_instance_id` is not this host, `OPERATION_UNKNOWN`.
  2. If `op_seq > max_issued`, `OPERATION_UNKNOWN`.
  3. If an entry exists in the table: if `kind` differs from the command, `OPERATION_CONFLICT`. If `reserved`, execute it, store the result and the request hash, change it to `completed`, and then return the result. The request hash is `xxh3_128(command u16 ‖ header session_id ‖ payload)`. The execution result is stored whether it is success or failure: for a Spawn exec failure, the `Error` response itself (code, detail) is stored as the result, and for a Kill whose target session does not exist or has already terminated, `KillResult{delivered: false}` is stored. The response is not sent before the execution result is stored. If `completed`, return the stored result when the payload hash is the same (Spawn has `created=false`), and `OPERATION_CONFLICT` if it differs.
  4. If no entry exists in the table (`op_seq ≤ max_issued` but absent = evicted), `OPERATION_EXPIRED`. It is not executed again.
- Eviction: a `reserved` entry is evicted 24 hours after `reserved_at`. A `completed` Spawn is not evicted while the session is alive, and is evicted 25 hours after the session terminates. A `completed` Kill is evicted 25 hours after completion.
- Evicted ids and never-seen ids are distinguished by comparison with `max_issued`, not by time. Therefore, no matter how the clock moves, an evicted id is never executed again. If the host instance changes, ids of the previous instance are rejected at step 1.

### 7.6 Error codes and detail schema
| code | name | detail |
|---|---|---|
| 1 | STALE_LEASE | scope u8, current_epoch u64 (0 if there is no active holder), epoch_final_accepted u64 (0 if scope=Resize) |
| 2 | INPUT_GAP | accepted u64 |
| 3 | INPUT_DIVERGED | epoch u64, accepted u64, committed u64 |
| 4 | INPUT_UNVERIFIABLE | epoch u64, accepted u64, committed u64, retained_start u64 |
| 5 | INPUT_BACKPRESSURE | accepted u64 |
| 6 | CORRUPT_FRAME | (none) |
| 7 | STALE_RESIZE | applied_epoch u64, applied_seq u64, cols u16, rows u16 |
| 8 | SESSION_NOT_FOUND | (none) |
| 9 | OPERATION_CONFLICT | operation_id OperationId |
| 10 | OPERATION_EXPIRED | operation_id OperationId |
| 11 | OPERATION_UNKNOWN | operation_id OperationId |
| 12 | SLOW_CONSUMER | subscription_id u64 |
| 13 | VERSION_UNSUPPORTED | host_major u16, host_minor u16 |
| 14 | UNSUPPORTED_CAPABILITY | capability_bit u8 |
| 15 | UNKNOWN_COMMAND | command u16 |
| 16 | LIMIT_EXCEEDED | limit_kind u8 (1 frame, 2 spawn_args, 3 sessions, 4 subscribers, 5 inflight, 6 offset_overflow, 7 input_chunk, 8 input_epochs), limit u64 |
| 17 | PROTOCOL_VIOLATION | reason u8 (1 bad_bool, 2 bad_utf8, 3 opt_condition, 4 bad_enum, 5 bad_tag, 6 truncated, 7 chunk_inconsistent, 8 bad_order: a list whose keys must strictly ascend does not, 9 bad_shape: a structural constraint of a record is violated, such as screen row count or row width, attribute or mode bits outside their mask, hyperlink id 0, notification text length, `pending_dropped_by_epoch` longer than 128, or `ReclaimResult{ok: true}` with `current_epoch ≠ requested_epoch`) |
| 18 | STALE_VIEWPORT | (Tauri side only; not used in FXSH) |
| 19 | STALE_SUBSCRIBE | current_attach_seq u64 |
| 20 | EPOCH_UNKNOWN | epoch u64 |
| 21 | INSTANCE_RETIRED | (none) |
| 22 | OWNER_UNREACHABLE | (none. Sent to the client by the daemon when it cannot connect to the owning partition. The host does not send it) |

### 7.7 capability bits
bit0 Input lease (including LeaseVacated), bit1 Resize lease (including LeaseVacated), bit2 Snapshot/Delta, bit3 UI events, bit4 EpochState. A v1 host MUST provide all of bit0–4.

### 7.8 Tauri JSON boundary
- All u64 values (revision, offset, epoch, seq, generation, attempt) MUST be decimal strings in JSON. TS parses them with `BigInt`.
- `LogicalRect {x, y, width, height}` is in CSS pixels as f64, relative to the top-left of the window content area. Physical pixels = round(css × scale_factor).

---

## Appendix A. Payload schemas
### A.1 Common types
- `Color` = tag u8 (0 Default, 1 Indexed: u8, 2 Rgb: u8 u8 u8). Any other tag is `PROTOCOL_VIOLATION`.
- `Cell` = codepoint u32 (0 = empty cell), grapheme_extra opt str (present iff combining characters exist), width u8 (0 continuation cell of a wide character, 1, 2), fg Color, bg Color, underline_color Color, attrs u16, hyperlink_id u32 (0 = none).
- `attrs` bits: 0 bold, 1 dim, 2 italic, 3 underline, 4 double underline, 5 curly underline, 6 blink, 7 inverse, 8 invisible, 9 strikethrough, 10 overline, 11–15 0 MUST.
- `RowData` = wrapped bool, cells [Cell]. The number of cells MUST equal cols at that point in time. An empty cell is `Cell{codepoint 0, grapheme_extra none, width 1, fg Default, bg Default, underline_color Default, attrs 0, hyperlink_id 0}`.
- `Cursor` = row u16, col u16, style u8 (0 block, 1 underline, 2 bar), blinking bool, visible bool.
- `Palette` = default_fg Color, default_bg Color, cursor_color opt Color, overrides [pair(u8 index, pair(u8 r, pair(u8 g, u8 b)))]. overrides MUST be in ascending index order, with no duplicates.
- `Hyperlink` = id u32 (≥1, must not be reused within an incarnation), uri str.
- `ExitInfo` = exit_code opt i32, posix_signal opt u8.
- `UiEvent` = event_id u64, kind u8, followed by kind-specific fields:
  - 1 clipboard_write = target u8 (1 clipboard, 2 primary), text str. text is the UTF-8 obtained by decoding the base64 of OSC 52. If decoding fails or the result is not UTF-8, the host does not create an event. A read request (`?`) also does not create an event.
  - 2 notification = title str (≤ 256 bytes, empty string if absent), body str (≤ 4 KiB).
  - 3 bell = no fields.
  - Any other kind value is `PROTOCOL_VIOLATION`.
- `OperationId` = host_instance_id UUID, op_seq u64.
- `Modes` = §A.4.

### A.2 Delta
subscription_id u64, base_revision u64, new_revision u64, size opt pair(u16 cols, u16 rows), cursor opt Cursor, modes opt Modes, palette opt Palette, title opt str, dirty_rows [pair(u16 row, RowData)] (ascending row order, no duplicates), hyperlinks_added [Hyperlink] (ascending id order), scrollback_appended [pair(u64 line_id, RowData)] (ascending line_id order), scrollback_evicted_before opt u64, exit_info opt ExitInfo, ui_events [UiEvent].

### A.3 SnapshotFrame
subscription_id u64, session_incarnation UUID, snapshot_id u64 (monotonically increasing within a subscription, never reused), tag u8, followed by:
- tag 0 Full: Body.
- tag 1 Chunk: index u32, total u32, total_len u32, bytes. Concatenating the bytes of the same snapshot_id in index order yields Body (validation rules in §2.7).
- `Body` = revision u64, state_digest 16 bytes, next_ui_event_id u64, ui_event_gap bool, **SBody**.
- `SBody` (= canonical S) = cols u16, rows u16, screen [RowData] (count = rows), cursor Cursor, modes Modes, palette Palette, title str, hyperlinks [Hyperlink] (only those referenced in the current S, ascending id order), scrollback [pair(u64 line_id, RowData)] (everything currently retained, ascending line_id order), exit_info opt ExitInfo.

### A.4 Modes
flags u32 bits: 0 alt_screen, 1 app_cursor_keys, 2 app_keypad, 3 bracketed_paste, 4 focus_reporting, 5 origin, 6 autowrap, 7 reverse_video, 8 insert, 9 cursor_visible, 10 sync_output, 11–31 0 MUST. Followed by mouse_mode u8 (0 off, 1 x10, 2 normal, 3 button, 4 any), mouse_encoding u8 (0 default, 1 utf8, 2 sgr, 3 urxvt, 4 sgr_pixels). The scroll region and tab stops are used only for interpreting subsequent output and not for display, so they are not included in S (parser internal state, §2.2).

### A.5 State and list responses
- `SessionState` = session_incarnation UUID, state_revision u64, state_digest 16 bytes, input_epoch u64, input_accepted u64, input_committed u64, resize_epoch u64, resize_seq u64, cols u16, rows u16, child_running bool, exit_info opt ExitInfo (present iff ¬child_running), pending_dropped_by_epoch [pair(u64,u64)] (at most 128 entries), vt_replies_dropped u64, creation_operation_id OperationId.
- `SessionList` = request_token pair(UUID, u64), owner_instance_id UUID, table_revision u64, complete bool, sessions [SessionEntry]. 512 sessions × entry size is far smaller than 16 MiB.
- `SessionEntry` = session_id UUID, session_incarnation UUID, created_table_revision u64, creation_operation_id OperationId, child_running bool, exit_info opt ExitInfo (present iff ¬child_running).

### A.6 canonical_encode(S)
`canonical_encode(S)` = the encoded byte sequence of `SBody` in A.3. It follows the ordering, no-duplicates, and empty-cell representation rules of A.1. session_incarnation is not included in S. The same logical state always produces the same byte sequence.

---

## 8. Fault Injection Exit Gates

### 8.1 Measurement conditions
- Platforms: macOS 15 (Apple M series), Ubuntu 24.04 x86_64 (Wayland and X11 each), Windows 11 x64.
- Load: 32 sessions. Of these, 4 each produce 20 MiB/s of output (a mix of `yes` and random SGR), and the rest are idle. Grid 200×60, scrollback 10,000 lines. The input measurement session uses 64 KiB/s bursts. GPU is each platform's integrated GPU.
- Clock: the test harness's monotonic clock. Unless otherwise noted, the time from the "fault clearance point" (for one-shot faults, the injection point) until the convergence event is observed must be within 5 seconds.
- Input verification method (all platforms): the test build feature `fault-tap` logs, verbatim, the bytes successfully handed to the PTY write point. The logged byte sequence is compared against the expected byte sequence for an exact match. On POSIX, the record of the child process `ferryx-pty-probe` (`cfmakeraw`, echo off, writes received bytes to a file) is additionally compared against the same expected value.
- State comparison: send an output-stop command to the probe, and after the client `local_revision` becomes equal to the host `state_revision`, compare the two `state_digest` values.
### 8.2 Gates
| Gate | Injection | PASS |
|---|---|---|
| 1 | GUI Force Quit / `kill -9` | PID·start_time of MicroHost sessions unchanged. After GUI restart, each pane's digest matches via the §8.1 method |
| 2 | Daemon `kill -9` during an input burst (the point right after daemon receipt and before host delivery is forced via an instrumentation hook) | tap log (and POSIX probe) == byte sequence generated by the writer's outbox |
| 3 | Induce partial write·EAGAIN with a small PTY buffer + large paste, drop InputAck frames, child resumes reading after `INPUT_BACKPRESSURE` continues for 10 seconds with the connection kept open, single 2 MiB paste (Windows: 4 MiB, because the ConPTY input pipe accepted a 1 MiB paste while the child was not reading, P0-1); a run in which no `INPUT_BACKPRESSURE` occurred is invalid, not a pass | Same as Gate 2. Bytes of a single WriteInput ≤ 64 KiB |
| 4a | Bytes not yet accepted for the old writer at lease handover | Those bytes shown as `not delivered`, absent from tap log |
| 4b | Bytes accepted but not yet written at lease handover. Probe pauses PTY reads until after the classification query response | Those bytes appear in the tap log after reads resume, in acceptance order, and precede the new writer's bytes. Old writer UI shows `accepted, pending write` |
| 4c | Only the ACK lost after write, followed by lease handover | Old writer UI `written`, exactly once in tap log |
| 4d | A writer whose lease was taken regains focus and types; bytes typed while it had no lease exist. Classification response for previous epoch E delayed until after the Grant for new epoch F. GetEpochState response dropped 2 times. InputAck of the previous epoch injected with delay while F is sending. Unconfirmed bytes exist just before voluntary release due to focus loss | In the new epoch those bytes are accepted·written in order from offset 0. F's lease·outbox unchanged. E's bytes are classified per 4a~4c and classification completes via retries |
| 4e | Two remote clients keep input focus continuously, one takes the lease | Input lease grants ≤ 1 during 10 seconds without user events. When the side whose lease was taken presses a key, it reacquires once |
| 4f | Classification of an old epoch after ledger eviction (65 or more epochs created), classification while the owning host is kill -9'd | Each shown as `outcome unknown`, no stall |
| 5 | In the same epoch, retransmit the entire already-accepted range within `retained` (while probe reads are paused), retransmit with changed chunk boundaries | Retransmission handling does not change `accepted`·`committed`, no duplicates in tap log |
| 6 | Partially overlapping retransmit (overlap matches), overlap mismatch, overlap outside `retained` | Respectively: only suffix accepted / `INPUT_DIVERGED` + fence / `INPUT_UNVERIFIABLE` + fence. In every case no duplicates in tap log |
| 7 | Output injected just before·just after Subscribe, output only `\e[?1049h`, change only the palette (`OSC 4`), after hyperlink output overwrite that cell to remove the last reference (`OSC 8`), inject Chunk frames of a previous subscription delayed until after the new subscription's ACK, inject out-of-order Chunks or Chunks with a different total_len | digest matches. Receive order is SubscribeAck → SnapshotFrame → Delta. Previous subscription frames do not affect assembly. Invalid Chunks cause assembly to be discarded, then Resync |
| 8 | Queue overflow due to subscriber stopping consumption, heavy scrolling during snapshot transmission, resize (reflow) during snapshot transmission | reader keeps making progress. digest matches after resume. On 3 consecutive overflows, SLOW_CONSUMER, then resubscribe per FSM and digest matches |
| 9 | Simultaneous resync on 16 sessions whose state size is close to `S_MAX`, with 2 of the subscribers stopped consuming | Actor keeps making progress. Subscribers that stopped consuming get SLOW_CONSUMER 30 seconds after the snapshot is enqueued. All remaining subscribers match digest (judgment deadline 3 minutes). S encoding ≤ `S_MAX` at every revision. Additionally, when injecting 3 resyncs on the same subscription in Waiting state, and Unsubscribe in each of Waiting·Sending·Draining states, entries per subscription in the wait queue ≤ 1, and after injection ends and all subscriptions become Idle·Closed, sum of issued reservations = 0. With all 7 budget slots occupied by Sending, when scrollback rewrite is injected into each Sending subscription every 10 seconds for 60 seconds, another subscription at the head of the wait queue receives a reservation and digest matches. When two identical subscription_ids from different sessions are Waiting and only one is Unsubscribed, the other remains in the wait queue and eventually digest matches. When snapshot X's last-frame-completion event and buffer-release event are injected delayed into Y's Sending, Y's reservation and resync_count are unchanged. When the deadline is exceeded mid-frame, only that connection is closed and FXSH stream violations 0. The judgment deadline is 30 seconds + measured actor latency |
| 10 | viewport requests completed in reverse order, new Accept during Prepare, new Accept injected right after Apply validation (instrumentation hook), delayed completion after pane deletion, Hidden target (including size 0) | Final visibility matches the last accepted target, and if the last accepted target is Visible, child frame·surface size also match. New Accept is processed in the next actor step. No reappearance of deleted panes. Hidden target yields Hidden receipt, configure/present calls 0 |
| 11 | Resize arriving in reverse order, A→B→A (B delayed), ResizeAck received in reverse order, new pane resize after pane re-creation, daemon kill during Resize transmission, Resize request dropped while the connection is kept (daemon instrumentation hook), re-show after Hidden transition, Grant arriving late after Hidden transition right after AcquireLease, LeaseRevoked of a previous epoch arriving late, AcquireLease failing with `LIMIT_EXCEEDED`·timeout while the connection is kept, Reclaim upon reconnect after another pane took the lease during disconnection, LeaseRevoked arriving before that Reclaim's negative response, Reclaim failing with `LIMIT_EXCEEDED`·timeout while the connection is kept, A's Grant arriving during binding change A→B, Grant arriving after Hidden target accepted but before Apply, two remote clients keeping tab selection continuously, viewport of 1200×100 cells size, lease contention (two windows alternating focus) | After faults are cleared, PTY cols/rows == latest desired size of the lease-holding pane (with `normalize_size` applied). After Hidden target acceptance·deletion, Resizes newly issued by that pane 0 (excluding previously issued delayed requests), delayed Grant immediately returned via ReleaseLease. A valid lease is not cleared by a delayed Revoked. After Reclaim failure, Reclaim retry observed while the connection is kept. Resize lease grants ≤ 1 during 10 seconds without user events. After the holder Releases (including reconnect after notification loss), another suppressed focused pane obtains the lease within 5 seconds |
| 12 | Inject device lost once then clear it (including during Prepare), second device lost while reconfiguration is in progress, new pane created during device Lost, DeviceReady·DeviceDegraded arriving after pane deletion | No PTY impact. Ready within 5 seconds after clearing. Apply of previous device_generation candidates 0, Ready published from previous reconfiguration results 0. New pane starts in the state from the registration response. Receipts·UI for deleted panes 0 |
| 13 | Persistent device lost injection | Degraded receipt within 5 seconds. 3 or more retries observed over the following 30 seconds. No PTY impact |
| 14 | Legacy partition Unreachable | Legacy sessions terminated 0 |
| 15 | Actual `kill -9` of Legacy daemon | Those sessions terminated(owner_lost), no impact on other partitions |
| 16 | inventory responses in reverse order, stale ChildExited arriving late, response of a previous aggregator instance arriving late, reconnect after child exit during disconnection (inventory `child_running=false`), attaching timeout repeated 10 times by delaying only SubscribeAck, recovery of a session that has only a layout hint | Erroneous termination of the latest binding 0. A session that exited during disconnection shows terminated(exited) and its final screen. After repeated timeouts, the host's subscription count for that session ≤ 1 (+ other clients), digest matches. Hint session is adopted and attached |
| 17 | (a) Daemon kill just before Spawn response, then restart. (b) Re-request of an operation for a live session. (c) Re-request of an operation evicted after 25 hours elapsed since session termination (monotonic clock acceleration hook). (d) Move wall clock +48h → wait 30 minutes → revert −48h, then re-request (b)·(c). (e) op_seq never issued, different host_instance_id. (f) Re-request a completed Kill operation with only the header session_id changed. (g) Re-request the same Kill operation after session termination·table deletion. (h) Daemon kill right after Spawn intent fsync·before Spawn transmission | (a) Exactly 1 child, intent created. (f) `OPERATION_CONFLICT`. (g) Stored KillResult. (h) After restart, exactly 1 child from the stored payload. (b) Same result, `created=false`, new children 0. (c) `OPERATION_EXPIRED`, new children 0. (d) Results identical before and after the wall clock change, new children 0. (e) `OPERATION_UNKNOWN` |
| 18 | Service manager startup failure, host update while an old instance with sessions exists | Former: session creation refused, GUI child host processes 0. Latter: old instance PID·start_time·session count unchanged, new sessions on the new instance, old instance exits after reaching 0 sessions |
| 19 | Host `kill -9`, block only the host connection (process alive) | Former: terminated(owner_lost). Latter: while blocked, only moves between disconnected·resolving, terminated 0; attached after the block is lifted |
| 20 | App replacement with the actual updater (Legacy sessions present) | Legacy daemon PID·start_time·session count unchanged, new sessions are MicroHost |

---

## 9. State Machine Executable Model Verification

The core state machines of v11.0 were ported to executable models and run with a mix of random message ordering (arbitrary reordering), bidirectional loss on connection disconnection, request errors, timer expiry, and focus·binding changes. At the end of each run, fault injection was stopped and convergence was checked. The model sources for §4.4, §3.3~§3.6, §5.5, §4.2~§4.3 are in `docs/session-restore/spec-models/` (`lease.js`, `input.js`, `binding.js`, `viewport.js`). For the §7 wire table, the commands·error codes·limit kinds used in the body were cross-checked by script, and the missing `OWNER_UNREACHABLE` code was added. The §2.5 model was run only within the verification session, and its source was not retained.

| State machine | Runs | Invariants checked | Result |
|---|---|---|---|
| §4.4 lease client (3 panes)·host | 5,000 × 3 variants | lease the client believes = host holder, an eligible pane does not stall without a lease, does not remain suppressed when there is no holder, final Resize applied | The first round (v10 rules) found 938 stalls → cause: suppression (`suppressed`) was not lifted even after another client released the lease. After adding `LeaseVacated` and confirmation Reclaim (§4.4), stalls 0 |
| §2.5 snapshot budget·allocator | 20,000 | reservation leaks 0, wait queue residue 0, stalls in states other than Idle·Closed 0, `resync_pending` residue 0 | failures 0 |
| §5.5 binding FSM (1 host, 1 session) | 3,000 × 5 variants (connection disconnect, child exit·table deletion, host death, hint adoption, subscription limit 1) | a live session is eventually attached, an exited session is terminated(exited), a vanished session is terminated, a dead host is terminated(owner_lost), wrongful terminated 0, host subscriptions per subscriber ≤ 1 | The first round found two defects: if an inventory request is lost, permanent stall in resolving (1,484/1,500); if the `OwnerDead` event is missed, the death is never learned (39/3,000). After adding the resolving retry rule and the `owner_dead` state in inventory, failures 0 in all variants |
| §4.2~§4.3 viewport·GPU (many panes, 1 device) | 400 | On queue drain after fault injection ends: live panes are displayed per the last accepted target, displayed generation = current device generation, displays of deleted panes 0, device Ready | The first round found a defect where the child view of a deleted pane remained (390/400). After fixing Destroy to remove the child view·surface in the same step, failures 0 |
| §3.3~§3.6 input ledger·writer | 3,000 × 2 (request errors 0%, 5%) | tap log duplicates 0, per-writer order preserved, if `written` then actually written, if `not delivered` then actually not written, `accepted, pending write` is eventually written, bytes vanished without classification 0, finalizing stalls 0, all bytes of the focused writer accepted | failures 0 |

### 9.1 Implementation property tests (v11.2)

The host and client state machines were implemented as pure Rust crates (`crates/fxsh` codec, `crates/session-core`) and the binding FSM as a TypeScript reducer (`ui/src/state/sessionBinding.ts`). The property tests drive the implementations themselves, not models: each connection direction is FIFO, deliveries across connections interleave arbitrarily, a response travels back on the connection its request arrived on and is lost with it, and events go to the instance's current connection.

| State machine | Cases | Invariants checked | Result |
|---|---|---|---|
| §4.4 Resize lease, 3 panes · host | 10,000 | at quiescence: the focused pane holds the lease the host records, unfocused panes hold none, PTY size equals the focused pane's size, `disowned` and `pending` empty, no Grant after quiescence | two defects found and fixed (below) |
| §3.3~§3.6 input ledger · writers, 2 writers × 2 sessions with rebinding | 10,000 | tap duplicates 0, per-writer order, every byte written only to the session it was typed for, `written`/`accepted, pending write` really written, `not delivered` never written, unclassified loss 0, finalizing stalls 0, no orphan holder on any session | failures 0 after the §3.6 binding-change rule |
| §2.6~§2.8 replica client | 10,000 + 4 scenarios | final digest and revision equal the host's, UI events strictly increasing and never duplicated, stale-subscription chunks have no effect, a broken chunk stream costs exactly one Resync | failures 0 after the discarded-snapshot and outstanding-Resync rules |
| §2.2~§2.6 diff · merge | 10,000 each | applying the diff reproduces the state; a merged Delta equals sequential application | failures 0 |
| §2.5 budget · subscription | 10,000 | reservation leaks 0, allocator queue residue 0, stalls outside Idle·Closed 0 | failures 0 |
| §5.5 binding FSM | 300 seeds × 6 variants | the binding.js invariants plus: attached only to the subscription the host holds; an adopted identity is persisted | failures 0 |

Defects found by these tests and fixed in v11.2:
1. **Self-revocation** (§4.4): a delayed AcquireLease of an instance reached the host after the lease that instance obtained later and revoked it, leaving the focused pane suppressed with no lease. Fixed by the host stale request filter.
2. **Orphan holder** (§4.4): an AcquireLease the client had abandoned (timeout, reconnect, focus loss) was granted afterwards; the host recorded the client as holder while the client held nothing, so a client suppressed by that Grant never received `LeaseVacated`. v11.1 assumed "the next Acquire replaces that epoch", but the suppressed client never sends it. Fixed by `disowned` and the confirming `ReleaseLease{epoch: 0}`.

Defects found while implementing, each pinned by a test that fails when the fix is removed:
3. **Cross-session input** (§3.6): `unassigned` survived a binding change, so the next Grant on the newly bound session delivered bytes typed for the previous one.
4. **Resync storm** (§2.7): after one chunk violation every remaining chunk of the same snapshot started a new assembly and another Resync; Deltas in flight behind an outstanding Resync each caused another Resync. Fixed by the discarded-snapshot watermark and the outstanding-Resync buffering.

Each test target was checked by mutation: 16 deliberate faults in the Rust state machines and 5 in the binding reducer each make at least one test fail. Three equivalent or dead-code mutants found on the way were resolved by removing the unreachable `Reserved` variant and by adding deterministic tests for the INPUT_GAP accepted offset and for outstanding-Resync buffering.

## 10. RCA Narrative Corrections
- 2026-10-08 Force Quit death: The observed fact is "the GUI and the daemon were in the same process group and terminated in the same second". The termination signal path is an inference. Gates 1/18 are the measured judgment criteria.
- In the handover failure, who closed the last master reference has not been confirmed. Because this specification removes the handover path for new sessions, this defect remains only in Legacy sessions (§0.2).
