# Ferryx Rolling Handover: Session Ownership Transfer Design

**Date:** 2026-09-19  
**Status:** Design proposal only; no implementation in this change  
**Target platforms:** macOS and Linux first  
**Target daemon protocol:** v5 (current `DAEMON_PROTOCOL_VERSION` is 4)

## 한국어 TL;DR

현재 Ferryx의 rolling handover는 세션을 새 daemon으로 **옮기지 않고**, 이전 daemon을 `legacy-*.sock` 뒤에 계속 살려 둔 채 새 daemon이 요청을 프록시합니다. 그래서 이전 daemon은 세션이 하나라도 남아 있으면 종료할 수 없고, 여러 세대 daemon이 동시에 `remote_sessions.json` 같은 상태 파일을 쓰는 구조가 됩니다.

이 설계는 handover를 **실제 소유권 이전**으로 바꿉니다. macOS/Linux에서 전용 Unix-domain handover 채널과 `SCM_RIGHTS`를 사용해 로컬 PTY master FD, SSH bridge의 control/reader stdio FD, canonical listener FD, daemon lock FD를 새 daemon으로 넘깁니다. 동시에 `TerminalOutputHub`의 ring buffer/sequence/resize ledger, remote `RemoteCursor`와 generation/epoch, 세션 메타데이터·worktree identity·`clientRequestId`를 함께 이전합니다. 새 daemon이 모든 상태를 staging에 복원하고 검증한 뒤 최종 commit ACK를 보내면 이전 daemon은 세션 개수와 무관하게 즉시 종료합니다. 최종 ACK 전 오류나 연결 끊김은 이전 daemon이 원본 FD를 계속 보유한 상태에서 rollback합니다.

`handover_routes.json`과 `SessionRouter`의 predecessor-routing은 v5 정상 경로에서 제거합니다. v4 daemon과의 호환은 한시적 degraded mode로만 유지하며, ownership-transfer capability가 없을 때는 활성 세션을 조용히 버리거나 이전 daemon을 강제 종료하지 않습니다.

---

## 1. Problem statement

The current handover is routing, not ownership transfer.

The existing sequence is centered on:

- `src-tauri/src/daemon/handover.rs::HandoverManager::prepare_handover`, which creates a legacy Unix socket and returns the current `TerminalService::list_sessions()` result.
- `src-tauri/src/daemon/handover.rs::HandoverManager::commit_handover`, which writes `handover_routes.json`, removes the canonical socket pathname, releases `DaemonLockFiles`, aborts canonical clients, and moves the predecessor into `HandoverStatus::Draining`.
- `src-tauri/src/daemon/handover.rs::HandoverManager::check_retirement_if_empty` / `check_retirement_locked`, which retire only when the predecessor is draining **and** `terminal_service.list_sessions().is_empty()`.
- `src-tauri/src/daemon/proxy.rs::SessionRouter::adopt_routes_from_manifest`, which reconstructs proxy routes to predecessor daemons rather than reconstructing local ownership.
- `src-tauri/src/daemon/server.rs::run_server_with_handover_and_readiness`, which contacts the predecessor, persists a `HandoverRoute`, invokes `CommitHandover`, then keeps a `LegacyPeer`.
- `src-tauri/src/daemon/server.rs::spawn_legacy_handover_daemon`, which deliberately keeps the predecessor serving its legacy listener after launching the successor.

No `SCM_RIGHTS`, `sendmsg`, or `recvmsg` usage exists under `src-tauri/src` today.

That architecture makes predecessor retirement structurally impossible while old sessions remain owned by the old process. It also creates concurrent daemon generations that can write common persistent state.

### 1.1 Observed failure evidence on 2026-09-19

The motivating incident showed all of the structural problems at once:

1. Three daemon generations coexisted (PIDs 910, 90242, and 16057).
2. Multiple processes raced on `/tmp/rorca-501/remote_sessions.json`. A unique temporary-write suffix mitigation has already been drafted, uncommitted, in `src-tauri/src/session/mod.rs` through `unique_write_suffix`. That hardens atomic replacement, but it does **not** establish single-writer ownership.
3. Sending SIGTERM to the two predecessors killed all 16 sessions they still owned.

The design contract below treats these as ownership failures, not merely routing or file-write bugs.

---

## 2. Repository facts that constrain the design

### 2.1 Daemon handover and canonical ownership

`src-tauri/src/daemon/handover.rs` defines:

- `HandoverStatus::{Active, Prepared, Draining, Retired}`.
- `HandoverManager::prepare_handover`.
- `HandoverManager::commit_handover`.
- `HandoverManager::abort_handover`.
- `HandoverManager::retain_request` and `RetirementGuard`.
- `HandoverManager::check_retirement_if_empty`.
- `HandoverManager::retire`.

`commit_handover` records a `HandoverRoute` containing a legacy socket and session IDs, then gives up canonical socket/locks. It does not move process resources.

`src-tauri/src/daemon/manifest.rs` defines `HandoverRoute`, `HandoverManifest`, `HandoverManifest::update_at_path`, `prune_dead_routes`, `add_or_update_route`, and `remove_route`.

The v5 normal path should not require this routing manifest.

### 2.2 Current daemon protocol is v4

`src-tauri/src/daemon/protocol.rs` currently declares:

`pub const DAEMON_PROTOCOL_VERSION: u32 = 4;`

The current handover wire API contains `DaemonRequest::PrepareHandover`, `CommitHandover`, `AbortHandover`, and the corresponding response variants.

This design therefore requires a bump to **v5**.

### 2.3 Canonical listener and lock ownership

`src-tauri/src/daemon/server.rs::run_server_with_handover_and_readiness` currently owns the canonical `tokio::net::UnixListener` as a local variable in its accept loop.

`src-tauri/src/daemon/server.rs` also defines `DaemonLockFile`, `DaemonLockFiles`, `DaemonLockFile::try_lock`, and `acquire_daemon_locks`.

On Unix, `DaemonLockFile::try_lock` uses `libc::flock(..., LOCK_EX | LOCK_NB)`. Its `Drop` explicitly invokes `LOCK_UN`.

A v5 implementation may pass the locked descriptor with `SCM_RIGHTS`, but predecessor final detach must close its copy **without executing `LOCK_UN`**.

### 2.4 Local PTY ownership is process-local today

`src-tauri/src/terminal/pty.rs::PtyManager` owns a `HashMap<String, Arc<PtySession>>` and native `PtySystem`.

`PtyManager::spawn_with_id_and_worktree` opens the PTY, derives nonblocking input from `pair.master.as_raw_fd()`, clones a reader, takes a writer, spawns the child, creates `PtySession`, and starts `PtyManager::start_lifecycle_watcher`.

`src-tauri/src/terminal/session.rs::PtySession` owns the Unix input FD, portable-pty master, writer, child handle, reader task, output sender, worktree path, lifecycle state, and size.

`PtySession::raw_master_fd` exists, but there is no adoption API.

In `src-tauri/vendor/portable-pty/src/unix.rs`, `UnixMasterPty` is private. The public `MasterPty` trait can expose `as_raw_fd`, clone readers, and take a writer, but there is no raw-owned-FD constructor. The implementation needs a small Ferryx-owned Unix adoption extension to the vendored crate.

### 2.5 Output continuity is richer than bytes

`src-tauri/src/terminal/output_hub.rs` defines `TerminalOutputHub`, internal `SessionHub`, `BoundedBuffer`, `OutputChunk`, `ResizePoint`, `ReplayGap`, and `SessionAttachment`.

`BoundedBuffer` tracks retained chunks, `current_size`, exact `next_sequence`, and bracketed-paste mode. `SessionHub` also tracks the resize ledger and retained replay-gap state.

`TerminalOutputHub::record_resize` consumes a sequence number without creating an `OutputChunk`; therefore only copying retained bytes or last chunk sequence is insufficient.

`TerminalOutputHub::subscribe_with_sequence` deliberately subscribes before snapshotting in one critical section. Handover import must preserve the same no-gap boundary.

### 2.6 Remote SSH sessions own live local transport processes

`src-tauri/src/terminal/remote.rs` defines `RemoteRuntime`, `RemoteSessionDescriptor`, `RemoteSessionDetails`, internal `Session`/`Entry`, `RemoteRuntime::create`, `RemoteRuntime::restore`, and `RemoteRuntime::launch`.

`RemoteSessionDescriptor` carries `backend_session_id`, `TargetRef`, `RemoteSessionConfig`, `client_request_id`, `remote_cursor: RemoteCursor`, and size.

The read loop in `RemoteRuntime::launch` reads from the current `RemoteCursor`, validates ordering, publishes into `TerminalOutputHub`, and updates the descriptor cursor.

`src-tauri/src/ssh/bridge.rs` defines `BridgeConnection` and `SshBridgeClient`. `SshBridgeClient` owns independent `control` and `reader` `BridgeConnection` values. `BridgeConnection::drop` calls `child.start_kill()`.

A correct transfer must therefore avoid normal `BridgeConnection::drop` after FD delivery or the predecessor will kill the local SSH bridge process.

### 2.7 Persistent restore is not a transfer substitute

`src-tauri/src/daemon/session_service.rs::persist_remote_sessions_at` writes durable remote descriptors and `StoredSessionMeta` into `remote_sessions.json`.

`DaemonSessionService::restore_remote_sessions_at` deliberately resets:

`record.descriptor.remote_cursor = RemoteCursor(0);`

because cold-start restoration has no local hub backlog.

A v5 successor must not use this cold restore path for transferred live sessions.

### 2.8 Session metadata and idempotency are ownership state

`src-tauri/src/daemon/session_service.rs::StoredSessionMeta` contains machine-session metadata, `client_request_id`, `workspace_id`, `worktree: Option<WorktreeIdentity>`, cwd, provider claim, and spawn fingerprint.

`DaemonSessionService` also owns `spawn_idempotency_cache`, `provider_session_claims`, `machine_controllers`, `machine_lifecycles`, and `session_metadata`.

### 2.9 Epochs have different meanings

`DaemonServer::new_with_paths` creates `DaemonServer::epoch` from current wall-clock milliseconds. That is a daemon-runtime replacement epoch and should advance when a new process takes over.

Separately, `TargetRef::epoch`, `RemoteSessionDetails::generation`, and paired generation state belong to session/remote authority and must be preserved exactly.

### 2.10 Paired-daemon sessions are included

`TerminalService::list_sessions` merges local PTY, remote SSH, and paired-daemon sessions.

`src-tauri/src/terminal/paired_daemon.rs::Descriptor` is durable identity, while `Transport` contains a live WebSocket/TCP/TLS stack. A raw TCP FD cannot reconstruct tungstenite/TLS user-space state. v5 therefore transfers descriptor/controller/output-hub ownership and explicitly reattaches without creating a new remote PTY.

---

## 3. Goals

After successful v5 handover:

1. Successor locally owns every previously live Ferryx session.
2. Every local PTY continues with the same process tree; no respawn.
3. Every direct SSH remote session retains the same target, `TargetRef` epoch, `RemoteCursor`, local sequence stream, and live bridge processes.
4. `TerminalOutputHub` continues from the exact next local sequence and retains replay/resize state.
5. Session metadata, worktree identity, provider claims, and spawn idempotency survive.
6. Successor becomes the only daemon allowed to mutate daemon-owned persistent state.
7. Predecessor exits immediately after final ownership commit, independent of open-session count.
8. Any failure before final commit leaves predecessor able to resume every session.
9. Protocol mismatch never silently loses sessions.

Non-goals include Windows transfer, preservation of already-connected client sockets, and exact exit status for a Unix process the successor cannot legally reap.

---

## 4. Ownership invariants

### I1. One active I/O owner

At most one daemon may actively read a PTY master, issue SSH bridge RPCs, mutate a paired proxy, or advance local sequence state for a session.

Both processes may temporarily hold duplicate FDs, but only one may have active reader/control tasks.

### I2. Originals retained until final commit

The predecessor does not close, detach, kill, or unlock authoritative resources merely because `SCM_RIGHTS` delivery succeeded.

### I3. Successor staging is invisible

Staged sessions do not appear in `TerminalService::list_sessions`, accept mutations, read output, persist state, or accept canonical clients.

### I4. Local sequence continuity is exact

`successor_next_sequence == predecessor_next_sequence_at_freeze`

Transfer all retained output chunks, resize points, replay-gap state, bracketed-paste state, and exact `next_sequence`.

### I5. Remote cursor/generation continuity is exact

Preserve `RemoteSessionDescriptor.remote_cursor`, `RemoteSessionDetails.generation`, and `TargetRef.epoch`.

### I6. Daemon runtime epoch advances

Successor keeps its newly-created `DaemonServer::epoch`; session/remote epochs transfer unchanged.

### I7. Single daemon writer

Before commit only predecessor writes daemon-owned state; after commit only successor writes it.

### I8. No successful v5 predecessor routing

A committed v5 session is local to successor; it does not depend on `LegacyPeer` or `handover_routes.json`.

---

## 5. Proposed v5 handover transaction

Conceptual state machine:

`Active -> Preparing -> Frozen -> Transferring -> CommitReady -> Retired`

Rollback from any pre-commit state returns to `Active`.

A transaction stores:

- unique `transferId`;
- stable `clientRequestId`;
- predecessor/successor PID and daemon epoch;
- ordered session inventory;
- expected FD-role inventory;
- manifest digest;
- per-frame receipt/ACK state;
- final commit nonce.

No shared routing manifest is required for v5.

---

## 6. UDS handover transport and framing

`DaemonServer::handle_client` currently uses a split stream and `BufReader::read_line`, so it cannot represent ancillary FD data.

The v5 handover socket should therefore be a dedicated Unix transport using raw `sendmsg`/`recvmsg`.

Each frame carries:

- magic `FXHO`;
- framing version;
- message kind;
- payload length;
- `transferId`;
- monotonic `frameSequence`;
- declared `fdCount`;
- flags.

Descriptor frames send first frame bytes and `SCM_RIGHTS` in one `sendmsg`.

Receiver must:

- reject `MSG_CTRUNC`;
- reject ancillary FD-count mismatch;
- bound payload and descriptor count;
- immediately set `FD_CLOEXEC`;
- close all received FDs on validation failure;
- map descriptors using an explicit ordered `fdRoles` list.

Authenticate handover peer using existing private runtime path protections plus same-user credentials (`SO_PEERCRED` on Linux, `getpeereid` on macOS).

---

## 7. FDs transferred

### 7.1 Daemon-level authority

Transfer:

1. canonical Unix listener FD;
2. legacy daemon lock FD;
3. persistent daemon lock FD, when present.

The canonical listener is staged without accepting. On commit the successor adopts the existing listening socket instead of unlinking/rebinding `daemon.sock`.

Lock FDs are staged as raw owned descriptors. The predecessor final-detach path must close its copies without invoking current `DaemonLockFile::drop`/`LOCK_UN`.

### 7.2 Local PTY

Transfer **one canonical PTY master FD per local PTY session**.

The successor derives new input/reader/writer clones from this FD.

Also transfer child/session-leader PID, process-group identity, size, worktree path, and lifecycle state.

### 7.3 Direct SSH remote session

For each `SshBridgeClient` transfer both bridge connections:

- `control.stdin`
- `control.stdout`
- `control.stderr`
- `reader.stdin`
- `reader.stdout`
- `reader.stderr`

A fully connected SSH session therefore transfers six pipe FDs.

Also transfer local SSH child PIDs, captured stderr bytes, healthy poison/closed state, read prefetch, writer-buffer state, handshake identity, host ID, owner ID, and bridge epoch.

A poisoned or mid-frame bridge is not transferable; wait for a clean RPC boundary or abort handover.

### 7.4 Paired session

Do not pass a raw WebSocket/TCP FD as a substitute for TLS/tungstenite state.

Transfer `paired_daemon::Descriptor`, controller generation, local output-hub state, and descriptor persistence ownership. Successor explicitly reattaches to the same remote target and must not create a replacement remote PTY.

---

## 8. Per-session payload

Common payload includes:

- transfer/session identity and kind;
- predecessor daemon epoch;
- lifecycle state;
- complete output-hub snapshot;
- `StoredSessionMeta` equivalent;
- `clientRequestId`;
- workspace/worktree identity;
- cwd;
- provider claim;
- spawn fingerprint;
- machine-session metadata;
- workspace registration;
- FD-role list.

Output-hub snapshot includes:

- capacity;
- retained `OutputChunk` values with sequence/bytes/timestamp/replay-gap;
- exact `BoundedBuffer.next_sequence`;
- bracketed-paste state;
- resize ledger;
- `SessionHub.replay_gap`;
- transport-owner flag when relevant.

Validate strictly increasing chunk sequences, ordered resize ledger, all sequence values below `next_sequence`, retained byte size within capacity, and valid replay-gap ranges.

Broadcast subscribers and machine senders are process-local and reconnect instead of transferring.

For SSH remote sessions additionally transfer full logical `RemoteSessionDescriptor`, `RemoteSessionDetails`, pending size, bridge snapshots, and stdio FD roles.

For `spawn_idempotency_cache`, serialize **remaining TTL** instead of `Instant`, plus `clientRequestId`, session ID, and `SpawnRequestFingerprint`.

---

## 9. Freeze ordering

### 9.1 Admission gate

Close admission for new session creation and new mutations. Let already-admitted mutations finish.

### 9.2 Local PTY freeze

Current `PtySession::new` creates a `spawn_blocking` reader with no cooperative pause boundary.

Add a quiesce seam:

1. request pause;
2. reader finishes current read;
3. already-read bytes enter PTY output channel;
4. reader ACKs pause before another read;
5. `TerminalService::register_output` pump drains queued chunks into hub;
6. snapshot hub;
7. duplicate PTY master FD.

Unread output then remains in kernel PTY buffering while predecessor is frozen.

### 9.3 SSH freeze

For every `RemoteRuntime` entry:

1. close control admission;
2. finish current control RPC;
3. allow current reader long-poll to finish a valid frame;
4. stop before next read;
5. flush each `BufWriter`;
6. capture/validate `BufReader` prefetch;
7. pause stderr pump and preserve both FD and captured prefix;
8. snapshot details/cursor under control/state locks;
9. snapshot output hub after final publish.

Do not cancel `BridgeConnection::request` mid-I/O because its cancellation semantics intentionally poison the connection.

### 9.4 Paired freeze

Pause each paired owner actor at a message boundary. If it cannot become safely reattachable before timeout, abort the whole transfer.

### 9.5 Persistence freeze

Close predecessor state-writer admission and wait for active `persist_remote_sessions_at`, `watch_remote_session`, and paired descriptor writes.

Successor staging has no writer authority.

---

## 10. Protocol sequence

### Phase A — handshake

1. Predecessor creates a mode-0600 dedicated handover UDS.
2. Successor starts with `--handover-from <path>`.
3. Successor sends `HandoverHello` with protocol 5, `sessionOwnershipTransferV1`, `scmRightsV1`, PID/epoch, stable `clientRequestId`, and `transferId`.
4. Predecessor authenticates peer and returns `HandoverOffer` containing PID/epoch, ordered session inventory, kinds, and FD counts.

Reject unsupported session kinds before freeze.

### Phase B — freeze

5. Successor sends `FreezeRequest`.
6. Predecessor drains mutation, pauses session readers/transports and canonical accept, and freezes persistence.
7. Predecessor sends `Frozen` with final inventory and manifest digest.

Canonical listener and locks remain held; `daemon.sock` is not removed.

### Phase C — transfer

8. Transfer listener/lock FD frames.
9. For each session:
   - send `SessionBegin` metadata/hub state;
   - send one or more `FdBatch` frames;
   - successor validates/stages;
   - successor returns `SessionImported`.
10. Predecessor sends `TransferEnd`.
11. Successor validates complete inventory, FD roles, hub state, metadata, idempotency, and reconstructability.
12. Successor returns `ImportReady`.

Predecessor still retains all originals.

### Phase D — commit

13. Predecessor sends one-time `CommitChallenge { nonce }`.
14. Successor replies `CommitAck { transferId, nonce, manifestDigest }` only when all staging is verified.
15. Receipt of matching `CommitAck` is the irreversible ownership point.

Then:

- predecessor never resumes session I/O;
- successor promotes staging;
- successor activates transferred canonical listener;
- successor activates PTY/SSH readers and paired reattach actors;
- successor opens daemon state-writer authority;
- predecessor transfer-detaches resources without child kill or lock unlock;
- predecessor aborts old client connections so they reconnect to successor;
- predecessor removes only the temporary handover control socket;
- predecessor exits immediately.

It does not wait for `TerminalService::list_sessions()` to become empty.

---

## 11. In-process reconstruction

### PTY master adoption

Add an owned-FD constructor to the vendored Unix portable-pty layer, conceptually:

`master_from_owned_fd(OwnedFd, tty_name) -> Box<dyn MasterPty + Send>`

It takes ownership, validates PTY semantics, sets/verifies close-on-exec, and does not reopen by pathname.

### Adopted process lifecycle

A transferred PTY child is not a child of successor. Do not fabricate a waitable `portable_pty::Child`.

Use a process-control abstraction equivalent to:

- `SpawnedChild(Box<dyn Child + Send + Sync>)`
- `AdoptedUnixProcess { pid, process_group }`

For adopted sessions preserve signal semantics and liveness observation; exact exit code may be unknown when successor cannot reap.

### `PtyManager` adoption

Add an internal `PtyManager::adopt_transferred_session`-equivalent path. It rejects duplicate IDs, constructs the adopted session, inserts only at promotion, starts adopted lifecycle observation, connects a fresh output receiver, and never calls `spawn_command`.

### Output-hub import

Add export/import APIs for full `SessionHub` state. Restore `next_sequence` exactly before any new publish. Create fresh process-local broadcast senders.

### SSH reconstruction

Add transfer-aware extraction/import.

Predecessor extraction must stop stderr cooperatively, flush writer, capture prefetch, detach three stdio FDs per bridge, retain local bridge child PID, and suppress current drop-time child kill only after commit.

Successor rebuilds both bridge connections and `SshBridgeClient` with original host/owner/epoch/handshake identity.

`RemoteRuntime` needs a new live-import path distinct from `RemoteRuntime::restore`; it preserves details, generation, attempts, failure, pending size, bridge, output hub, and exact `RemoteCursor`.

Successful direct-SSH transfer does not reconnect or spawn.

### Paired reconstruction

Recreate paired proxy from transferred descriptor against imported hub and explicitly reattach to same target/generation before global commit.

### Session-service state

Promotion restores metadata, provider claims, TTL-adjusted spawn idempotency entries, workspace registrations, and relevant machine-controller generation/reservation state.

---

## 12. Canonical listener and lock transfer

Move canonical listener ownership behind a pauseable lease. During freeze, predecessor accept loop ACKs pause; successor stages a duplicate listener FD without accepting.

At commit, successor activates accept and predecessor closes its copy without unlinking `daemon.sock`.

For locks, the successor stages raw descriptors. Rollback closes only successor duplicates. Final commit converts successor descriptors into active lock guards and predecessor closes its copies without `LOCK_UN`.

This must be proven with real subprocess tests on macOS and Linux.

---

## 13. Old-daemon immediate exit

For v5, `HandoverManager::check_retirement_if_empty` / `check_retirement_locked` must no longer decide daemon lifetime from `TerminalService::list_sessions().is_empty()`.

Either remove them from v5 or repurpose them to check only final commit ACK/request draining.

`RetirementGuard` may still protect request framing, but not session lifetime.

Final commit directly invokes `HandoverManager::retire` after transfer detach.

The legacy socket is now a temporary transfer endpoint, not a session-serving endpoint.

`handover_routes.json` and `SessionRouter::adopt_routes_from_manifest` are absent from successful v5 startup.

---

## 14. Failure, rollback, ambiguous delivery, idempotency

Before final `CommitAck`, the predecessor retains every original resource.

On partial transfer or disconnect:

- successor closes staged FDs and discards staging without starting readers;
- predecessor resumes PTY readers, SSH/paired actors, canonical accept, and persistence;
- predecessor returns to `Active`.

Each frame is identified by `transferId`, `frameSequence`, session ordinal, FD roles, and payload digest.

Duplicate retry with identical content closes newly-received duplicate FDs and replays prior ACK. Same frame identity with different content is a protocol conflict.

Each mutating handover request carries stable `clientRequestId`. Same ID + same fingerprint reuses transaction result; same ID + different fingerprint is rejected.

Rollback is guaranteed for channel failures before predecessor receives a valid final `CommitAck`.

After that ACK, ownership is irrevocably assigned to successor and predecessor may never resume I/O. A successor crash after issuing final commit ACK is equivalent to a daemon crash after completed handover and is outside rollback guarantee.

Any one-session import failure aborts the **entire** transfer. Never commit a subset and proxy the remainder.

Pre-commit timeout always rolls back; never SIGTERM predecessor as timeout recovery.

---

## 15. Protocol v5 backward compatibility

Increment `DAEMON_PROTOCOL_VERSION` from 4 to 5 and advertise `sessionOwnershipTransferV1`.

### v5 successor -> v4 predecessor

Probe capability before current destructive handover semantics.

With zero active sessions, safe restart can proceed.

With active sessions:

- never claim ownership transfer;
- never kill predecessor;
- never silently drop sessions.

During a migration window, either explicitly retain degraded legacy `HandoverRoute`/`LegacyPeer` mode or defer upgrade and leave predecessor canonical.

### v5 predecessor -> older successor

With active sessions, reject/defer legacy `CommitHandover` when successor lacks ownership-transfer capability.

### No mid-flight downgrade

Once v5 freezes or sends FDs, fully rollback before any legacy fallback.

---

## 16. Persistent-state single-writer design

The uncommitted unique-temp-suffix work in `src-tauri/src/session/mod.rs` is valuable but only prevents temporary pathname collision; it does not establish ownership.

Introduce a process-level daemon state-writer lease tied to canonical ownership.

Writers including `DaemonSessionService::persist_remote_sessions_at`, persistence in `DaemonSessionService::watch_remote_session`, and paired descriptor persistence only run with this lease.

Ordering:

- predecessor closes writer admission and waits for active writes;
- successor staging has no writer lease;
- final commit promotes canonical lock and writer lease together;
- successor may persist one consolidated post-transfer snapshot;
- predecessor performs no post-commit write.

For v5 handover startup, do not call `SessionRouter::adopt_routes_from_manifest` or cold `restore_remote_sessions_at` for transferred sessions. Import live ownership first. Cold startup without handover remains unchanged.

---

## 17. Platform notes

### macOS and Linux

Both support AF_UNIX `SCM_RIGHTS`.

Handle partial reads, ancillary truncation, FD_CLOEXEC, FD count bounds, EINTR, and platform-specific peer credentials.

Linux uses `SO_PEERCRED`; optional pidfd observation may improve adopted process monitoring but cannot be required because macOS is first-class.

macOS uses `getpeereid` and portable Unix process observation.

The flock-transfer assumption must be tested with real processes on both platforms.

### Windows

Current behavior remains explicit:

- non-Unix `HandoverManager::prepare_handover` returns `"Handover unsupported on Windows"`;
- non-Unix `DaemonServer::handle_upgrade_binary` returns `DaemonResponse::UpgradeUnsupported`.

Do not advertise ownership-transfer capability, fake transfer through persistence, or terminate active Windows daemon to force upgrade.

Future Windows transfer requires a separate Win32/ConPTY design.

---

## 18. Tests and executable invariants

### Real PTY zero-session-loss contract

Use two daemon processes and real PTYs:

1. start predecessor;
2. create multiple PTYs;
3. record session IDs and child PIDs;
4. emit unique pre-handover markers;
5. attach and record local sequences;
6. trigger v5 handover;
7. emit data during freeze to exercise kernel buffering;
8. complete commit;
9. assert predecessor exits while sessions remain live;
10. assert successor lists same session IDs;
11. assert original child PIDs remain and were not respawned;
12. write input through successor;
13. observe post-handover output;
14. assert strict sequence monotonicity;
15. assert no missing/duplicate marker or new replay gap.

### Output-hub state round trip

Export/import a hub containing chunks, resize points, a resize sequence after last chunk, bracketed-paste mode, and replay gap. First successor publish must allocate exact predecessor `next_sequence`.

### PTY FD transfer

Pass a real PTY master through `SCM_RIGHTS`, reconstruct it, and prove read/write/resize after sender closes its duplicate.

### SSH bridge transfer

Use controlled local child fixtures for control/reader bridges and verify all six FDs, stderr capture, prefetch, writer flush, no sender-side child kill, and continued framed RPC.

Inject a poisoned/in-flight RPC and prove handover aborts.

### Remote cursor/generation continuity

Seed nonzero `RemoteCursor`, remote generation, and local next sequence. Assert successor reads and publishes from exact transferred values.

### Partial-transfer rollback matrix

Inject disconnect after hello, freeze, daemon listener FD, lock FD, first PTY FD, one SSH batch, final session, `TransferEnd`, `ImportReady`, and immediately before `CommitAck`.

Every pre-commit failure must leave predecessor alive and all sessions usable, with staging closed and canonical accept/persistence resumed.

### Idempotency

Repeat same `clientRequestId`/`frameSequence`; prove no duplicate registry entry, duplicate FDs close, prior ACK replays.

Reuse same ID with different fingerprint and assert conflict.

### Single-writer state-file contract

Instrument persistence writer PID. Predecessor may write before freeze; nobody writes during staging; only successor writes after commit; predecessor never writes after commit.

Run repeated handovers under persistence churn and verify `remote_sessions.json` remains valid.

### Immediate retirement with nonempty sessions

Complete transfer while predecessor `terminal_service.list_sessions()` remains nonempty. Final `CommitAck` must fire retirement immediately without killing transferred children.

### Listener/lock transfer

Prove successor accepts on transferred canonical listener, predecessor does not unlink `daemon.sock`, third process cannot acquire locks through predecessor exit, and successor eventual release allows acquisition.

### Protocol compatibility

Test v5->v4 active, v5->v4 empty, and older-successor->v5 active behavior.

### Windows

Assert capability absent and existing unsupported behavior unchanged.

---

## 19. Migration and rollout

### Stage 0

Land the existing unique-temp-suffix persistence hardening separately. Do not call it the handover fix.

### Stage 1

Add transfer primitives disabled by default: Unix framing, SCM_RIGHTS codec, PTY import/export, hub state import/export, bridge freeze/import/export, listener/lock transfer, transaction/idempotency state.

### Stage 2

Bump protocol to v5 and advertise transfer capability only on supported Unix builds.

### Stage 3

Enable v5 behind a rollout flag. Measure freeze duration, session/FD counts, rollback reason, commit latency, predecessor exit latency, and writer PID. Do not log session bytes or credentials.

### Stage 4

Make v5 default after soak verifies zero PTY loss, sequence continuity, single-writer behavior, and immediate predecessor exit.

### Stage 5

After v4 compatibility window, remove handover routing manifest, route adoption, and long-lived legacy predecessor routing from rolling upgrades.

---

## 20. Risks

- **Adopted PTY child cannot be reaped by successor:** use separate adopted-process control and allow unknown exact exit code.
- **Bytes stranded between reader and hub:** cooperative reader pause plus output-channel drain.
- **SSH BufReader prefetch:** transfer prefetch at clean RPC boundary.
- **SSH BufWriter unsent bytes:** flush before export; abort on failure.
- **Predecessor drop kills SSH bridge:** dedicated transfer detach path that bypasses current `BridgeConnection::drop` kill.
- **Transferred lock gets unlocked by predecessor:** bypass current `DaemonLockFile::drop` `LOCK_UN`; prove cross-process.
- **Successor reads before rollback is impossible:** staging does no I/O.
- **Paired WebSocket user-space state cannot be raw-FD reconstructed:** explicit reattach to same target.
- **Ancillary descriptor limits:** bounded batches and `MSG_CTRUNC` rejection.
- **Multiple state writers despite unique temp paths:** daemon writer lease tied to canonical ownership.
- **v4 fallback keeps predecessor alive:** compatibility-only, measured, removed after migration window.

---

## 21. Required implementation seams

| Area | Current anchor | Required new capability |
| --- | --- | --- |
| Handover transaction | `daemon/handover.rs::HandoverManager` | freeze/stage/commit, idempotency, immediate retire |
| Raw transfer channel | `daemon/server.rs` handover path | Unix `sendmsg`/`recvmsg` + `SCM_RIGHTS` |
| Protocol | `daemon/protocol.rs::DAEMON_PROTOCOL_VERSION` | v5 capability/messages |
| Canonical listener | `DaemonServer::run_server_with_handover_and_readiness` | pause/extract/import listener lease |
| Daemon locks | `DaemonLockFile` / `DaemonLockFiles` | transfer without predecessor `LOCK_UN` |
| PTY import | `terminal/pty.rs::PtyManager` | adopt existing master FD |
| PTY process state | `terminal/session.rs::PtySession` | spawned-child vs adopted-process control |
| portable-pty | `vendor/portable-pty/src/unix.rs::UnixMasterPty` | owned-FD constructor/adapter |
| Output continuity | `terminal/output_hub.rs::TerminalOutputHub` | full `SessionHub` export/import |
| Remote runtime | `terminal/remote.rs::RemoteRuntime` | live import preserving cursor/generation |
| SSH bridge | `ssh/bridge.rs::BridgeConnection` / `SshBridgeClient` | freeze, stdio FD export/import, no-kill detach |
| Session authority | `daemon/session_service.rs::DaemonSessionService` | metadata/idempotency/controller export/import |
| Paired runtime | `terminal/paired_runtime.rs::Runtime` | pause/export/promote + explicit reattach |
| Legacy routing | `daemon/proxy.rs::SessionRouter` | no v5 predecessor route adoption |
| Persistence | `persist_remote_sessions_at` / `watch_remote_session` | process-level writer lease |

---

## 22. Acceptance criteria

1. Handover with nonempty local PTYs ends with only successor daemon alive.
2. Same PTY children survive; no respawn.
3. Direct SSH sessions retain same remote targets and live bridge processes.
4. `RemoteCursor`, session generations/target epochs, and local output sequences continue monotonically.
5. Successor serves all transferred sessions locally; successful v5 sessions do not depend on `LegacyPeer`.
6. `handover_routes.json` is not written on successful v5 path.
7. Predecessor exits immediately after final ownership commit regardless of session count.
8. Any injected pre-commit transfer failure rolls back without session loss.
9. Only one daemon writes daemon-owned state at a time.
10. macOS/Linux pass real cross-process listener/lock/PTY transfer tests.
11. Windows remains explicitly unsupported.
12. v4 compatibility never silently drops active sessions.

---

## 23. Recommended final architecture

> **A rolling handover is successful only when the successor owns the kernel resources and the in-memory continuity state, not when it merely knows where the predecessor lives.**

Under v5, the legacy socket becomes a short-lived transfer control plane. `SCM_RIGHTS` moves kernel capabilities; typed transfer payloads move the state needed to use them correctly; freeze/import/commit moves logical authority; listener/lock transfer moves daemon authority; and final commit makes predecessor retirement independent of open sessions.

This replaces:

`new daemon -> SessionRouter -> LegacyPeer -> old daemon -> old PTY/SSH resources`

with:

`new daemon -> local TerminalService -> transferred PTY/SSH/paired ownership`

Once that invariant holds, the predecessor has no session-serving responsibility and can exit immediately.
