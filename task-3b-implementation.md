# Task 3 Unit 3-B Implementation Evidence (Post-Review3B Final Remediation)

**Target Component:** Unit 3-B Client Attach & Real Native Presentation Hooks  
**Target Files Owned:**
- `src-tauri/src/ipc/native_terminal.rs` (EXCLUSIVE)
- `src-tauri/src/native_terminal/surface_host.rs` (EXCLUSIVE)
- `task-3b-implementation.md` (Evidence Artifact)  
**Worktree Location:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`  
**Date:** 2026-10-03  
**Status:** REPAIRED IN SOURCE (Unrun Gate: Awaiting Authorized Remote GUI QA)

---

## 1. Minimal 2-Site Repair for Frozen Candidate `b8956fb4` (for `executorad`)

The minimal delimiter fix applied to `84a8975f` yielded `b8956fb484fedef2631d8124e2db92fd45b98db9973a1f21491237520c49b0dc` (exact size: 365,229 bytes). In remote QA feature checks (`task-3-cargo-check-qa-v3.log`), compiler errors occurred at lines 3264 and 3321 due to bare `PRESENTATION_PRODUCER_ID` inside `impl NativeTerminalSurfaceHostState`.

### 1.1 Standalone 2-Site Patch against `b8956fb4`
```diff
--- a/src-tauri/src/native_terminal/surface_host.rs
+++ b/src-tauri/src/native_terminal/surface_host.rs
@@ -3261,7 +3261,7 @@ impl NativeTerminalSurfaceHostState {
                 "coordinatorEvidence": "coordinator-pending",
                 "presentationEvidence": "coordinator-pending",
                 "hasUnpresentedFrames": true,
-                "producerComponent": PRESENTATION_PRODUCER_ID,
+                "producerComponent": Self::PRESENTATION_PRODUCER_ID,
             }),
         );
         let outcome = channel.wait_for_release(&spec).await;
@@ -3318,7 +3318,7 @@ impl NativeTerminalSurfaceHostState {
                     .unwrap_or(serde_json::Value::Null),
                 "coordinatorEvidence": "coordinator-consumed",
                 "presentationEvidence": "coordinator-consumed",
-                "producerComponent": PRESENTATION_PRODUCER_ID,
+                "producerComponent": Self::PRESENTATION_PRODUCER_ID,
                 "channelProducer": CHANNEL_PRODUCER,
             }),
         );
```

### 1.2 Snapshot Provenance & Posthash Delegation
- **Frozen Input Candidate:** `b8956fb484fedef2631d8124e2db92fd45b98db9973a1f21491237520c49b0dc` (365,229 bytes).
- **Posthash Execution:** Per instructions, the actual post-patch candidate hash will be computed directly by `executorad` on remote host `maho-win` during candidate staging. Zero hypothetical local posthashes are claimed.
- **Current Owned Source:** Confirmed absent in our current `surface_host.rs` (lines 3463, 3476, and 3536 all reference `Self::PRESENTATION_PRODUCER_ID`).

---

## 2. Review 3-B Source Defects & Dispositions

### Defect 1: Synchronous `teardown()` Left Unchanged; Outer Async QA Shutdown Coordinated with `ac`
- **Defect:** Calling `rt.block_on` inside synchronous `teardown()` when called inside an active Tokio context causes nested runtime panics and blocks UI threads.
- **Remediation:** Removed the `block_on` block completely from `NativeTerminalSurfaceHostState::teardown(&self)`. Production synchronous `teardown(&self)` is 100% unchanged.
- **Coordinated Async Lifecycle:** The awaited QA shutdown is coordinated at the outer async test/harness boundary with `coreac` via `channel.drain_and_verify_workers().await`.

### Defect 2: Explicit Typed Binding Failure on Missing Target
- **Defect:** Missing `targetBackendSessionId` was previously logged with `tracing::error!` and silently returned, which bypassed the barrier rather than reporting an explicit failure to the test harness.
- **Remediation:** Both `dispatch_owned_render` (`surface_host.rs`) and `hold_attach_handshake_barrier_qa` (`native_terminal.rs`) explicitly check `spec.target_backend_session_id`. If absent or empty on an armed barrier, a typed failure receipt is emitted:
  - `stage: "presentation_binding_failed"` / `stage: "attach_handshake_binding_failed"`
  - `status: "failed"`
  - `error: "BINDING_FAILURE: missing required targetBackendSessionId on armed barrier"`
  - `actionable: false`
- This receipt is correlated and asserted by the harness, ensuring explicit nonzero test failure rather than silent bypass. If `targetBackendSessionId` is present and does not match `session_id`, the pane safely bypasses without claiming.

### Defect 3: Channel-Owned Claims & Zero Custom Globals
- **Defect:** Custom static globals (`PRESENTATION_TRACKER`, `PENDING_RECEIPT_WORKERS`) caused architectural fragmentation.
- **Remediation:** Removed all custom globals from `surface_host.rs`. Uses channel-owned operations directly:
  - `channel.try_claim(PRESENTATION_BARRIER, &session_id, &spec.operation_id)`
  - `channel.release_claim(PRESENTATION_BARRIER, &session_id, &spec.operation_id)`
  - `channel.schedule_cancellation_receipt(PRESENTATION_BARRIER, &operation_id, settlement)` for non-blocking GPU completion receipt logging.

### Defect 4: Prearmed Signal & Spawned Execution in Attach Timeout Test
- **Defect:** `tokio::pin!(attach_fut)` alone without polling before `tokio::time::advance` meant the attach future never started and its deadline timer was never registered in the timer wheel before time advanced.
- **Remediation:** In `qa_barrier_attach_tests::attach_handshake_timeout_leaves_prior_live_session_tasks_intact`:
  1. Prearms held event subscriber: `let mut held_rx = channel.subscribe_held();`.
  2. Spawns the production command: `let attach_handle = tokio::spawn(async move { cmd_native_terminal_attach(...).await });`.
  3. Awaits the actual held signal: `QaBarrierChannel::await_held_event(&mut held_rx, ATTACH_HANDSHAKE_BARRIER).await;`. At this point, the command has entered `hold_attach_handshake_barrier_qa` and its 500ms deadline timer is genuinely active in the reactor.
  4. Advances virtual time: `tokio::time::advance(std::time::Duration::from_millis(600)).await;`.
  5. Awaits the spawned command join handle: `let result = attach_handle.await.expect("join failed");`.
  6. Asserts `result.unwrap_err().code == IpcErrorCode::Timeout`, `!stream_abort_handle.is_finished()`, and `state.has_session_host(session_id)`.

---

## 3. Five-Tuple Status Preservation

- The five-tuple requirement (`frontendSessionId`, `paneIdentity`, `backendSessionId`, `bindingKey`, `attemptGeneration`) remains an unresolved dependency.
- The Rust backend only possesses authoritative `backendSessionId` and monotonic `attemptGeneration`. Visual `paneIdentity` and frontend `bindingKey` are **never fabricated** in Rust backend code.
- Final five-tuple acceptance remains open pending Architect91's cross-layer seam.

---

## 4. Current File Hashes & Structural Balance

| File | Baseline Pre-Unit-3-B SHA-256 | Current Remediated SHA-256 | Byte Size |
|---|---|---|---|
| `src-tauri/src/ipc/native_terminal.rs` | `b641184b746131bb786fbff69df42a7cdf0c058a3c3e51b77c37ddde0006acfc` | `2f9dbf0889a46d77f1473d39bc47f13c9d5645e8b072bc6058ece280a07e0172` | 167,529 B |
| `src-tauri/src/native_terminal/surface_host.rs` | `84a8975f8fa4bec20204b5ec710480e2455ec31087cb7f2a6f30baa06e09f82c` | `e06f83cb6e608440cebf2acb9f48657a3aae7b1f4d7fa1c886502219727caa5a` | 378,847 B |

- **Structural Delimiter Balance:**
  - `native_terminal.rs`: Braces balance: 0, Parens balance: 0, Brackets balance: 0 (4,265 lines).
  - `surface_host.rs`: Braces balance: 0, Parens balance: 0, Brackets balance: 0 (9,271 lines).

---

## 5. Explicit Unrun Gates

Per session instructions and host boundaries:
1. `cargo check --lib`: UNRUN on this Mac host.
2. `cargo test --lib`: UNRUN locally or remotely.
3. `pane-liveness.mjs`: UNRUN (requires authorized remote GUI desktop session).
4. No compilation or test-pass claims are made; verified strictly through static source delimiter balance and contract auditing.
5. Handed back to `root` and `executorad` for incorporation into the next candidate overlay.
