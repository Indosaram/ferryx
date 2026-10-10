# Handover State Review: Portable Handover Production Integration & Fault Tests

**Date:** 2026-10-02T11:25:00Z | **Base Commit:** `9c07425a0f901f76a7c4fb813e161dd2628f7410` (foreign dirty baseline accepted)
**Target:** Portable Handover Transaction & Ownership Fences | **Runtime Verifier:** `st_01a0f8b3` / `st_01a0f80d` (UNRUN)
**Verdict:** **BLOCK** (Whole-objective unapproved: VT parser, resize lease, and staged-paste producers unlinked; runtime tests UNRUN)

## 1. Verified Source Artifact & Baseline Hashes (SHA-256)
- Repository HEAD: `9c07425a0f901f76a7c4fb813e161dd2628f7410` (shared T9 repo READ ONLY, zero commits/builds)
- Scratch Baseline Patch (`exactbaseline.patch`): `7e0c2491da69ae35b4cadb0bfc1123ad55552c31b503a5d20c594d747d4d52de` (828,004 bytes)
- Preserved Integration Patch (`handover-integration-forward.patch`): `cef86b9c773668a0bc971b41681a67b029edd86302d70562717ec009e5647f42` (65,160 bytes)
- Deliverable Forward Patch (`handover-state-forward.patch`): `205f7f8acce8dfdf249a3efbda1a3e3d67fa17c1a514b4d20b9d10cea567fcef` (180,266 bytes, 3,744 lines)
- Deliverable Overlay Patch (`handover-state-overlay.patch`): `e03a509ddace956f2b24cc051d46a201e76d235c2178e91a4aea22c4af101f1f` (17,767 bytes, 461 lines)
- 12 Workcopy Sources in `/tmp/ferryx-superlogical-handover-state/workcopy/`:
  - `src-tauri/src/terminal/session.rs`: `a2eb8a5bf2d7c96781e90b63f1a9c6e1d954f9fea0866a39ecee959d4ebefc28` (108,321 bytes)
  - `src-tauri/src/terminal/pty.rs`: `947e27c733ea8a9188f745e7c97f60fae3caa4d6b29d8715fa1f29afdb675f8d` (80,568 bytes)
  - `src-tauri/src/terminal/service.rs`: `441d9e085d8ae70da93109efbceac732c98e906a2cb6fc272920868e47570c2e` (25,997 bytes)
  - `src-tauri/src/terminal/mod.rs`: `1da74735fceccad2daacae2fd287ac54a798f41f1108c2b4312f49595b3a0f1a` (1,261 bytes)
  - `src-tauri/src/daemon/server.rs`: `2f17a746a97db959b743b237484ab1b846940f8f1687f7f941c89f56ada745f4` (373,087 bytes)
  - `src-tauri/src/daemon/handover.rs`: `31c961ef8ce69c309a9671c328399a4787a067ba0104d649edf25023907dad92` (34,571 bytes)
  - `src-tauri/src/daemon/handover_socket.rs`: `0bfcb40450b79548a95db85e80e7003c1b6a054e6bccfba5f35c1370d2030b3a` (41,964 bytes)
  - `src-tauri/src/daemon/protocol.rs`: `14653ebb23b8d2f6f93219bec8ee08d6336b82e44b0c144e5427297d79a82145` (89,796 bytes)
  - `src-tauri/src/daemon/client.rs`: `1da3a3b988e8ffb5566624fb1756cdc45847ecd86dc8cbee630a210c851e89e9` (278,726 bytes)
  - `src-tauri/src/ipc/error.rs`: `43c60a763c79e7e96a5d43b919b0eed8de408b32b1a60214c8d33846881483fc` (25,933 bytes)
  - `src-tauri/tests/daemon_handover_contract.rs`: `19b0e127b29aae5ece0aa52629d5d8689f9e8b6f1268903c129e6a56d03fd4ff` (40,669 bytes)
  - `src-tauri/tests/daemon_handover_transfer_contract.rs`: `4c3f8e6b9b773542c154cab57d5e945c21e054b8251349bc4f6a79d0728d2090` (55,139 bytes)
- Patch Applicability: `git apply --check -p1` on shared repo (EXIT: 0), baseline (EXIT: 0), and reverse on workcopy (EXIT: 0, 0 diff).

## 2. Source-Backed Audit Findings & Defect Resolutions
1. **Fresh Placement Fix & Authenticated OS Peer Identity Binding (`handover_socket.rs:79-115`, `server.rs:2605-2640`):**
   - *Placement Fix:* Moved `socket_peer_pid` from erroneous placement inside `impl HandoverSocketListener` (lines 86-153 in candidate preimage) to module-scope free function (lines 79-115), resolving free function call site `crate::daemon::handover_socket::socket_peer_pid` in `server.rs:2609`.
   - *Identity Fallback Elimination:* Requires kernel OS peer PID (`LOCAL_PEERPID` on macOS, `SO_PEERCRED` on Linux, `creds.pid` fallback). Fails closed immediately (`AdoptionFailed`) on lookup failure; claimed wire PID NEVER counts as authenticated identity.
   - *Mismatch & Metadata Validation:* Wire predecessor PID validated against authenticated peer PID; wire/peer mismatch strictly fails closed. Inconsistent wire PIDs across session frames trigger immediate rejection.
2. **Deterministic Test Liveness Boundary & Magic Number Elimination (`server.rs:8186-8204`):**
   - Hardcoded magic number `999999` eliminated. Tests spawn real isolated child process and reap via `dead_child.wait()`, guaranteeing deterministic `ESRCH` via kernel exit events.
   - Regression Test 6: active live legacy socket listener (`UnixListener::bind`) rejects activation even when PID is ESRCH (`ownership uncertain`).
   - Regression Test 7: positive test proving verified dead PID (`ESRCH`) with removed legacy socket activates successfully.
   - Handover socket unit tests (`handover_socket.rs`): peer PID extraction, peer/wire mismatch fail-closed, and missing peer proof fail-closed.
3. **Predecessor Retirement Proof & Socket Inactivity Verification (`server.rs:2440-2500`):**
   - Predecessor PID is mandatory (`quarantined.predecessor_pid`); verifies `libc::kill(pid, 0)` returns `ESRCH` (`EPERM`, `EINVAL`, or alive rejected).
   - Predecessor legacy socket path is mandatory; asynchronous check `tokio::fs::try_exists(path).await` prevents blocking async runtime.
   - If socket file exists on disk, connection failure does NOT count as retirement (`ownership uncertain`); quarantine strictly retained.
   - Record is removed from quarantine only after all retirement proofs pass; caller boolean alone never bypasses proof.
4. **Exact Transfer-Bound Abort & Unfreeze (`server.rs:4120-4240`, `pty.rs:952-972`):**
   - `DaemonRequest::AbortHandover` requires exact non-empty, non-wildcard `transfer_id`, resuming ONLY sessions matching that specific transfer ID. Deprecated dangerous `resume_confirmed_handover_sessions_all` global unfreeze.
5. **Adopted Reader Startup Barrier & Writer Seal Rollback Guard (`session.rs:833-867`, `1897-1903`):**
   - Adopted sessions initialize with `pause_requested=true` and `reader_paused=true`; reader thread blocks on `reader_pause_notify.notified()` barrier until commit, eliminating early un-gated PTY reads.
   - `quiesce_input_output_async` uses RAII `SealRollbackGuard`; cancellation/timeout while awaiting `input_gate` unseals writer.
6. **Full Logical Write Admission & Typed Outcome (`service.rs:83-110`, `error.rs:401`):**
   - Entire logical payload admitted under single write permit (no 64 KiB chunk interleaving). Partial OS write returns typed `PtyError::PartialDelivery { written, total, reason }`.
7. **Windows ConPTY Invariant & Natural Draining (`handover.rs:100-130`):**
   - Win32 `HPCON` cannot duplicate across processes; `UpgradeAction::Refuse` preserves predecessor as stable owner in Draining mode without fake handle clones.
8. **Explicit Unlinked Upstream Producer Dependencies (Why Verdict is BLOCK):**
   - *VT Engine Owner:* `AuthoritativeVtSnapshot` DTO exists, but live `ghostty_terminal_t` parser snapshot/restore is UNLINKED in daemon PTY loops.
   - *Resize Lease Owner:* `ResizeLeaseSnapshot` DTO exists, but live lease coordinator actor is UNLINKED.
   - *Writer Sequence / Staged Paste Owner:* `WriterFencedStateSnapshot` and `StagedPasteSnapshot` exist as DTOs, but live sequence coordinator is UNLINKED.
   - *Supplementary Glyph / Kitty State:* UNLINKED in daemon loops.

## 3. Authored Fault Tests Matrix (ALL STRICTLY UNRUN)
- `daemon_handover_transfer_contract::tests::test_v5_isolated_process_lost_ack_retains_quarantined_sessions` [UNRUN]
  Spawns real isolated daemons D1 & D2 via intercepting proxy; drops D2 wire connection after D1 commit; verifies D1 retires/exits, D2 survives beyond startup in quarantine holding PTY master, child shell remains alive; verifies wildcard/empty/unknown resolution rejected; resolves via exact transfer ID upon verified D1 retirement via ESRCH; verifies attach and bidirectional input/output on D2.
- `terminal::session::tests::test_session_freeze_token_and_confirmed_unfreeze` [UNRUN]
- `terminal::session::tests::test_full_logical_write_admission_and_drain` [UNRUN]
- `terminal::session::tests::test_quiesce_input_output_async_cancel_drop_guard` [UNRUN]
- `terminal::session::tests::test_partial_os_delivery_typed_outcome_no_blind_retry` [UNRUN]
- `terminal::session::tests::test_bounded_async_reader_quiescence_barrier` [UNRUN]
- `terminal::session::tests::test_adopted_reader_paused_until_ack_atomic_activation` [UNRUN]
- `terminal::session::tests::test_isolated_process_lost_ack_old_retire_retention_and_eventual_resolution` [UNRUN]
- `terminal::session::tests::test_confirmed_abort_detaches_adopted_fds_without_killing_child` [UNRUN]
- `terminal::pty::tests::test_export_session_for_handover_freeze_and_abort` [UNRUN]
- `daemon::handover_socket::tests::test_session_transfer_metadata_freeze_token_roundtrip` [UNRUN]
- `daemon::handover_socket::tests::test_handover_transaction_reader_starts_only_after_actual_commit_ack` [UNRUN]
- `daemon::handover_socket::tests::test_handover_socket_peer_pid_and_meta_validation` [UNRUN]
- `daemon::handover_socket::tests::test_handover_socket_peer_identity_mismatch_fails_closed` [UNRUN]
- `daemon::handover_socket::tests::test_handover_socket_missing_peer_proof_fails_closed` [UNRUN]
- `daemon::server::tests::test_quarantined_handover_ownership_fences_regression` [UNRUN]
- `daemon::handover::upgrade_action_tests::windows_conpty_stable_owner_fallback_preserves_handles_without_duplication_fiction` [UNRUN]

## 4. Remote Verification Protocol (Sole Verifier: st_01a0f8b3 / st_01a0f80d, UNRUN)
- Local runtime execution is strictly UNRUN (no cargo test/check, bun test, or GUI execution on Mac).
- Authoritative remote test execution commands:
  - Unix Contract Gate: `cargo test --test daemon_handover_contract -- --nocapture`
  - Transfer Contract Gate: `cargo test --test daemon_handover_transfer_contract -- --nocapture`
  - Ownership Fences Unit: `cargo test --lib daemon::server::tests::test_quarantined_handover_ownership_fences_regression -- --nocapture`
  - Peer PID Validation Unit: `cargo test --lib daemon::handover_socket::tests::test_handover_socket_peer_pid_and_meta_validation -- --nocapture`
  - Peer Mismatch Unit: `cargo test --lib daemon::handover_socket::tests::test_handover_socket_peer_identity_mismatch_fails_closed -- --nocapture`
  - Missing Proof Unit: `cargo test --lib daemon::handover_socket::tests::test_handover_socket_missing_peer_proof_fails_closed -- --nocapture`
