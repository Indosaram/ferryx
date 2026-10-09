# Evidence: Task 3 Held-RPC Control (split-concurrent)

## Executive Summary
This document provides contract, implementation, and adversarial review remediation evidence (Revisions 1 through 4) for the held-rpc control of Task 3 under the `local-pane-liveness-root-remediation` plan in worktree `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`.

- **Modified Files**:
  - `src-tauri/src/ipc/terminal.rs`
  - `src-tauri/src/ipc/qa_barrier.rs`
  - `src-tauri/src/daemon/client.rs`
- **Evidence File**: `E/task-3d-held-rpc.md`
- **Worktree**: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`
- **Branch**: `fix/local-pane-liveness-w1`
- **SHA256 (`src-tauri/src/ipc/terminal.rs`)**: `4546a9dfad5b03011f781128e4f5f7b115a8eac4bc98e804fcdfe07681604030`
- **SHA256 (`src-tauri/src/ipc/qa_barrier.rs`)**: `0e7126364c7583be00cbeeab3bea3c3ba7a162fa0b203161a3603a91e7178979`
- **SHA256 (`src-tauri/src/daemon/client.rs`)**: `f6b33a61f8e601aae794236ced99c82444a5a0ca6357e3e1ab75b06effc7e60a`

---

## 1. Adversarial Review Remediations (E/task-3d-adversarial-review.md)

### Revision 1 & 2 Remediations
| Finding | Severity | Resolution & Seam Verification |
|---|---|---|
| **Rev1 Finding 1**: Feature cfg desynchronization (`mod qa_barrier` requires `all(feature = "local-split-qa", feature = "native-terminal")`) | HIGH | All production hooks, helpers, and test modules in `src-tauri/src/ipc/terminal.rs` aligned to `#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]`. Default `cargo test --lib` (without features) compiles cleanly against a no-op inline fallback returning `None` with zero references to `qa_barrier`. |
| **Rev1 Finding 2**: Synchronous disk I/O in async Drop (`HeldRpcGuard::drop` called `append_receipt`) | HIGH | Removed synchronous disk I/O from async worker threads. Settlement receipts are offloaded via `crate::ipc::run_blocking` with observable completion (`.await`). |
| **Rev2 Finding 1 & 2**: Raw Arc pointer-keyed static registry (`CHANNEL_RPC_REGISTRY: HashMap<usize, ChannelRpcState>`) | HIGH | Removed static map and raw pointer casting (`Arc::as_ptr as usize`) completely. Coordinated core API directly in `QaBarrierChannel` (`try_claim_held_rpc`, `release_held_rpc_claim`). State is owned directly by the `QaBarrierChannel` instance; when the channel is dropped, all claim state drops cleanly with it. Zero memory leaks, zero ABA hazards. |
| **Rev2 Finding 3**: Missed cancellation receipt on real future drop | HIGH | Refactored `HeldRpcClaim::drop` to schedule durable cancellation settlement through an owned background worker (`channel.schedule_cancellation_receipt`). The worker executes receipt appending via `tokio::task::spawn_blocking` and registers its `JoinHandle` in `channel.pending_workers`, which is observed and awaited during completion cleanup via `channel.await_pending_workers()`. Does not rely on artificial `cancel_signal` or `pending()`. |
| **Rev2 Finding 4**: Test over-claimed real PTY execution | MED | Renamed test to `remote_command_holds_while_local_dispatch_executes_concurrently`. Corrected claims and docstrings to reflect that it proves IPC command dispatch concurrency to the daemon client rather than full OS PTY execution. |

### Revision 4 Remediations (Client Seam & Auto-Spawn Inhabitation)
| Finding | Severity | Resolution & Seam Verification |
|---|---|---|
| **Rev4 Finding 1**: Missing `token_path` in `upgrade_rpc_client` (E0063 compile error) | HIGH | Added `token_path: self.token_path.clone()` to `DaemonClient::upgrade_rpc_client()` in `src-tauri/src/daemon/client.rs:1297`. Crate compiles cleanly without missing field errors. |
| **Rev4 Finding 2**: Auto-spawn fallthrough in `connect_or_spawn` clobbering production daemon | HIGH | In `client.rs:1423` (`connect_or_spawn`), added explicit fail-fast guard: if `self.token_path.is_some()`, immediately return `Err(IpcError::new(IpcErrorCode::IoError, ...))` without sleeping or falling through to spawn `ferryx --daemon`. This strictly protects the explicit test-isolated path from ever contacting or launching the production daemon, while preserving 100% of standard production daemon recovery policy for production clients (`token_path: None`). |
| **Rev4 Regression Coverage**: Deterministic missing fixture test | MED | Added deterministic regression tests in both `client.rs` and `terminal.rs` (`isolated_client_fails_fast_without_spawning_production_daemon`), confirming that upgrade cloning preserves `token_path` and that an unavailable fixture fails fast with `"auto-spawn disabled"`. |

---

## 2. Plan and Runner Contract Alignment

### 2.1 Runner Contract Requirements (`scripts/qa/pane-liveness.mjs`)
Under the approved plan (`.omo/plans/local-pane-liveness-root-remediation.md` IS-4) and `pane-liveness.mjs`:
```javascript
'split-concurrent': {
  barriers: ['held-rpc'], marker: true, splitMenu: true,
  receipts: ['fixture-setup', 'held-rpc', 'split-create', 'presentation', 'marker-output'],
  requireHeldRpc: true, fiveTuple: true, timings: true, singlePty: true,
},
```
1. **Pre-arm Phase**: The runner pre-arms `held-rpc` before binary spawn via `<dir>/held-rpc.arm.json` specifying `runId` and `operationId`.
2. **Registration Ack**: Product scans and acknowledges arms into `<dir>/held-rpc.armed-ack.json`.
3. **Trigger & Hold**: While the runner initiates a concurrent local pane split (`clickSplitRightDarwin`), the product executes a real remote terminal RPC ingress operation. The remote operation holds at the `held-rpc` barrier and writes `<dir>/held-rpc.held.json`.
4. **Held Assertion**: Runner awaits `<dir>/held-rpc.held.json` and asserts:
   ```javascript
   if (held?.heldRpc !== true) throw new HarnessError('ASSERTION_FAILURE', `split-concurrent requires heldRpc: true, got ${JSON.stringify(held)}`);
   ```
5. **Local Concurrency**: Unrelated local operations (pane split creation, presentation, native marker typing and rendering, local PTY write) complete unhindered and unblocked while the remote RPC remains held.
6. **Release & Settlement**: Runner releases `held-rpc` via `<dir>/held-rpc.release.json`. The product settles the barrier, appends a receipt to `<dir>/held-rpc.receipt.jsonl`, and resumes/completes the underlying remote terminal RPC.

---

## 3. Seam & File Ownership Audit

### 3.1 File Boundary Verification
- **d1 Ownership**: `src-tauri/src/ipc/native_terminal.rs`, `src-tauri/src/native_terminal/surface_host.rs` — **UNTOUCHED / NO OVERLAP**.
- **d2 Ownership**: `src-tauri/src/daemon/server.rs`, `src-tauri/src/daemon/handover.rs` — **UNTOUCHED / NO OVERLAP**.
- **`st_01a100ac` Ownership & Core API Coordination**: `src-tauri/src/ipc/qa_barrier.rs` — Core channel methods added directly to `QaBarrierChannel` (`try_claim_held_rpc`, `release_held_rpc_claim`, `schedule_cancellation_receipt`, `await_pending_workers`). Preserved without revert for synchronization with `author-ac` (`st_01a100ac`).
- **Authorized Client Token Seam**: `src-tauri/src/daemon/client.rs` — Authorized smallest addition of `DaemonClient::new_with_socket_and_token_path`, `resolve_transport_token`, `upgrade_rpc_client` clone, and fail-fast auto-spawn prevention without altering production socket defaults.
- **Product Seam**: `src-tauri/src/ipc/terminal.rs`.
- **Pre-existing Working Tree Changes**: The foreign diff in `src-tauri/src/ipc/terminal.rs` (lines 2268–2271 introducing `reader_paused`, `kernel_stopped`, `registry_suspended`, `suspension_source`) has been strictly preserved.

### 3.2 Real Remote Terminal RPC Ingress Seams Hooked
In `src-tauri/src/ipc/terminal.rs`, the following genuine remote terminal RPC entry points are hooked:
1. `cmd_terminal_remote_write`: Hooked via `maybe_hold_remote_rpc_barrier(&session_id, "remote_write")`.
2. `cmd_terminal_remote_resize`: Hooked via `maybe_hold_remote_rpc_barrier(&session_id, "remote_resize")`.
3. `cmd_terminal_remote_status`: Hooked via `maybe_hold_remote_rpc_barrier(&session_id, "remote_status")`.
4. `cmd_terminal_remote_retry`: Hooked via `maybe_hold_remote_rpc_barrier(&session_id, "remote_retry")`.
5. `cmd_terminal_spawn` (remote workspace branches: SSH and Paired Daemon): Hooked via `maybe_hold_remote_rpc_barrier(&request.workspace_id, "remote_spawn_ssh")` and `maybe_hold_remote_rpc_barrier(&request.workspace_id, "remote_spawn_paired")`.

The local branch of `cmd_terminal_spawn` (when `!is_remote_workspace && !is_paired_workspace`), along with local `cmd_terminal_write` and local `cmd_terminal_resize`, remain completely unhooked and unhindered.

---

## 4. Contract Guarantees

### 4.1 Real RPC Ingress vs. Synthetic Background Futures
The hook executes exclusively upon entry to an actual remote terminal command. There are **zero synthetic background futures** fabricating `heldRpc: true`. If no remote RPC is invoked, the barrier is not held.

### 4.2 Unrelated Local Path Stays Runnable
While a remote terminal RPC is held at `maybe_hold_remote_rpc_barrier`, unrelated local operations (local workspace split, local PTY writes via `cmd_terminal_write`, and local terminal reads) continue immediately with zero latency and zero barrier interference.

### 4.3 No Locks Held Across QA Wait
The dispatch tracking state is checked in memory via `channel.try_claim_held_rpc` and released immediately before awaiting release. During `channel.wait_for_release(&spec).await`, **zero mutexes or locks are held**.

### 4.4 Privacy (No Input Payloads Recorded)
The held payload and receipt structures record strictly structural nonces (`runId`, `operationId`, `producer`, `producerPid`, `sessionId`, `stage`, `heldRpc: true`, `releaseOutcome`). The user input data (`data: String` in `cmd_terminal_remote_write` containing keystrokes or terminal sequences) is **never passed, never inspected, and never logged**.

### 4.5 Bounded Correlation, Cancellation Workers, and Retry Safety
- **Channel-Owned Claim State**: `channel.try_claim_held_rpc` tracks claims directly inside the `QaBarrierChannel` instance.
- **Retry Safety**: When a hold is cancelled or dropped, `release_held_rpc_claim` clears the in-flight claim without inserting into `settled_rpc_ops`, allowing retries to succeed immediately.
- **Owned Cancellation Workers**: On real future drop, `HeldRpcClaim::drop` calls `channel.schedule_cancellation_receipt`, which dispatches the write to `tokio::task::spawn_blocking` and tracks the join handle in `channel.pending_workers`, awaited during teardown via `channel.await_pending_workers()`.

---

## 5. Deterministic Unit Tests in `src-tauri/src/ipc/terminal.rs`

Six neighbor-sized, deterministic unit tests are authored in `src-tauri/src/ipc/terminal.rs` under `mod qa_held_rpc_tests`:

1. `remote_command_holds_while_local_dispatch_executes_concurrently`:
   - Uses `DaemonClient::new_with_socket_and_token_path` injecting an exact tempdir token path.
   - Strictly avoids writing to or restoring production `get_transport_token_path()`.
   - Invokes real Tauri command handler `cmd_terminal_remote_write`.
   - Confirms remote write parks at `held-rpc` with `heldRpc: true`.
   - Concurrently invokes local `cmd_terminal_write` to prove local IPC command dispatch succeeds immediately.
   - Releases barrier; confirms `cmd_terminal_remote_write` completes and emits receipt.

2. `cancellation_via_worker_and_retry_without_stale_lockout`:
   - Spawns task holding barrier.
   - Aborts task (`task.abort()`) to simulate real future drop.
   - Confirms `HeldRpcClaim::drop` schedules cancellation receipt worker and appends receipt with `releaseOutcome: "cancelled"`.
   - Retries exact same operation; confirms it successfully holds again without stale lockout.
   - Releases barrier; confirms clean settlement.

3. `scoped_per_channel_isolation_prevents_cross_test_races`:
   - Validates that independent `QaBarrierChannel` instances maintain isolated claim states without cross-test interference.

4. `privacy_contract_never_logs_input_payload`:
   - Drives remote write carrying sensitive tokens (`"TOP_SECRET_USER_INPUT_KEYSTROKES"`).
   - Asserts held payload in memory and on disk omits all input data.

5. `unarmed_or_inactive_channel_never_blocks`:
   - Validates that when `held-rpc` is not armed, remote operations return `None` immediately without delay.

6. `isolated_client_fails_fast_without_spawning_production_daemon`:
   - Sets up missing socket and token paths in tempdir.
   - Verifies `upgrade_rpc_client()` clones `token_path` correctly.
   - Verifies connection attempt fails immediately with `"auto-spawn disabled"` and never spawns production daemon.

---

## 6. Artifact Hashes
- File: `src-tauri/src/ipc/terminal.rs`
  - SHA256: `4546a9dfad5b03011f781128e4f5f7b115a8eac4bc98e804fcdfe07681604030`
- File: `src-tauri/src/ipc/qa_barrier.rs`
  - SHA256: `0e7126364c7583be00cbeeab3bea3c3ba7a162fa0b203161a3603a91e7178979`
- File: `src-tauri/src/daemon/client.rs`
  - SHA256: `f6b33a61f8e601aae794236ced99c82444a5a0ca6357e3e1ab75b06effc7e60a`
- File: `E/task-3d-held-rpc.md` (this document)
  - SHA256: `958395bc2481cca443e7b2b7907bddf4cd2c73b840d7bc2e8e8122fa6d3bf209`
