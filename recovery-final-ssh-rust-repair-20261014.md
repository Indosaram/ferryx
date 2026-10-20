# Recovery Final SSH Rust Repair (2026-10-14)

## Executive Summary

Diagnosed and repaired the two Rust test failures in `terminal::remote` reported in `final-r2/gate-rust-terminal-remote.raw.log`:
1. `terminal::remote_runtime_tests::ssh_close_of_gone_target_succeeds_and_forgets_the_session` (line 1572 unwrap panic on `RemoteFailure { kind: Missing }`)
2. `terminal::remote_runtime_tests::ssh_reconnect_to_exited_target_publishes_its_final_output` (line 193 state deadline timeout waiting for `RemoteConnectionState::Expired`)

Both root causes were isolated in `src-tauri/src/terminal/remote.rs` and resolved with minimal targeted fixes, preserving existing deterministic tests without test weakening, maintaining strict identity fences, prohibiting automatic spawn and uncertain replay, and keeping execution unexecuted per instructions.

---

## File Identification & Exact SHA-256 Hashes

### Candidate Worktree (`/Volumes/T9-Mac/project/ferryx-wt/recovery-final-20261014`)
- `src-tauri/src/terminal/remote.rs`: `c8c0d02fedb7f03b8b4ee960f64a35b33f5f53d730a942dae081294565c9acbd`
- `src-tauri/src/terminal/remote_runtime_tests.rs`: `547a1b8af6ccedaff16c6050a35ce41ac6bfb9096c0f047a746fab25f605a8ae`

### Canonical Repository (`/Volumes/T9-Mac/project/ferryx`)
- `src-tauri/src/terminal/remote.rs`: `685da35644237867e5f791e92efe4bf8c53b11346333a2ac63c8654a82c7ea2f`
- `src-tauri/src/terminal/remote_runtime_tests.rs`: `5a92308a0929c749bcfc3f21c7b89c32925c3ed8066baaa3a58cee511f4b409e`

---

## Root Cause Analysis & Invariants

### 1. `ssh_close_of_gone_target_succeeds_and_forgets_the_session`
- **Failure**: `runtime.close("local-stable").await.unwrap()` panicked at `remote_runtime_tests.rs:1572:45` with `Err(RemoteFailure { kind: Missing, message: "Target not found on remote helper" })`.
- **Source Mechanism**:
  In `RemoteRuntime::close(&self, id: &str)`, when stopping an explicit session target whose helper reports `BridgeError::TargetNotFound`, the bridge error is mapped to `RemoteFailureKind::Missing`.
  Line 950 evaluated:
  `let target_gone = matches!(f.kind, RemoteFailureKind::Expired);`
  omitting `RemoteFailureKind::Missing`. Consequently, `target_gone` evaluated to `false`, and `close()` returned `Err(f)` instead of `Ok(())`.
- **Contract & Discipline**:
  `close()` represents explicit user intent to terminate and forget a terminal session. If the remote target is already missing or expired on the helper, there is nothing left to stop on the remote host. Explicit close must idempotently succeed and forget the session locally (`self.sessions.lock().remove(id); self.hub.remove_session(id); return Ok(())`).
  *Boundary Distinction*: Typed `Missing` is permitted to idempotently forget in explicit user `close()`, but must NEVER be used to infer automatic process exit during autonomous reconnect loops.
- **Repair**:
  In `src-tauri/src/terminal/remote.rs`, line 950 updated to:
  `let target_gone = matches!(f.kind, RemoteFailureKind::Expired | RemoteFailureKind::Missing);`

### 2. `ssh_reconnect_to_exited_target_publishes_its_final_output`
- **Failure**: `state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await` timed out with `state deadline: Elapsed(())` at `remote_runtime_tests.rs:193:6`.
- **Source Mechanism**:
  In `run()` reconnect loop (lines 1283–1318), upon reconnecting to an exited target:
  1. `info = client.describe(&d.target).await?;` confirmed `info.target == d.target` (matching identity).
  2. `if info.exited` detected affirmative process exit.
  3. The final output tail was read and published to `TerminalOutputHub` under the active generation guard (`s.details.generation == generation && read.target == d.target`).
  4. However, line 1318 returned `Err(BridgeError::TargetNotFound)`.
  5. Returning `BridgeError::TargetNotFound` mapped to `RemoteFailureKind::Missing`.
  6. In `fail()`, `RemoteFailureKind::Missing` maps to `RemoteConnectionState::Disconnected` (unconfirmed/retryable absence), NOT `RemoteConnectionState::Expired`.
  7. Consequently, the session remained in `Disconnected`, never transitioned to `Expired`, and the test's state watcher timed out waiting for `Expired`.
- **Contract & Discipline**:
  Under approved Task 5 and affirmative exit invariants:
  - Source absence never implies affirmative exit; typed `Missing` alone represents unknown liveness.
  - However, when `client.describe(&d.target)` affirmatively confirms `info.target == d.target` AND `info.exited == true`, this constitutes grounded matching-identity exit proof directly from the helper authority.
  - Returning `BridgeError::RemoteTargetExpired` represents this confirmed exit truthfully, transitioning the session to `RemoteConnectionState::Expired` and terminating the dial loop, consistent with live read exit handling (`read.exited -> BridgeError::RemoteTargetExpired` at line 1447 and `run_live` at line 1606).
  - The late generation guard (`s.details.generation == generation`) remains fully enforced to protect hub output from being overwritten by stale dials, and early return ensures old generation settlement is suppressed.
- **Repair**:
  In `src-tauri/src/terminal/remote.rs`, line 1318 updated from:
  `return Err(BridgeError::TargetNotFound);`
  to:
  `return Err(BridgeError::RemoteTargetExpired);`

---

## Scope & Execution Discipline

1. **Owned Files**: Only `src-tauri/src/terminal/remote.rs` was modified in candidate worktree `/Volumes/T9-Mac/project/ferryx-wt/recovery-final-20261014`. Existing test file `src-tauri/src/terminal/remote_runtime_tests.rs` already contains exact assertions and was verified intact.
2. **Execution Boundary**: Strictly unexecuted per instruction (`No execution/build/LSP/children/process/GUI/commit`). No cargo test, cargo build, bun test, or LSP processes were invoked.
3. **Patch Tooling**: All changes and artifact creation applied exclusively via `/Users/indo/.local/bin/apply_patch` with stdin patch format.
4. **Preserved Invariants**:
   - Zero automatic spawn: reconnection never creates replacement shells or processes.
   - Zero uncertain replay: writes and mutations are not replayed across reconnects.
   - Grounded identity: process exit strictly requires matching target proof from helper authority.
