# Independent Adversarial Review: Task 3 Unit 3-D Held-RPC Controls (Revision 5 Narrow Review)

- **Audit Date:** 2026-10-03
- **Auditor:** Independent Adversarial Reviewer (with Root Audit Invariants)
- **Target Worktree:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`
- **Reviewed Files:**
  - `src-tauri/src/ipc/terminal.rs` (SHA-256: `4546a9dfad5b03011f781128e4f5f7b115a8eac4bc98e804fcdfe07681604030`, 148,829 bytes)
  - `src-tauri/src/daemon/client.rs` (SHA-256: `f6b33a61f8e601aae794236ced99c82444a5a0ca6357e3e1ab75b06effc7e60a`, 129,910 bytes; authorized client edits)
  - Reference Core: `src-tauri/src/ipc/qa_barrier.rs` (SHA-256: `68d78ff532c0a6d2a0413063a02c5b329a7a824e2bbcc468a05341424657283d`, commit prefix `68d78ff`, active `ac`)
- **Scope & Constraints:** Strictly read-only delta review of revision 5 vs prior findings. Zero remote commands or test execution. Zero product edits. Own evidence artifact only.

---

## Executive Summary & Bounded Verdict

| Dimension | Revision 4 | Revision 5 Verdict | Core Technical Evaluation |
|---|---|---|---|
| **Compilation of `client.rs` Initializers** | NEEDS-FIX (E0063) | **CONFIRMED RESOLVED (PASS)** | `upgrade_rpc_client` in `client.rs:1298` includes `token_path: self.token_path.clone()`. The missing field `E0063` compile error is fixed. |
| **No-Spawn Guard on Custom Token Failure** | CRITICAL HAZARD | **CONFIRMED RESOLVED (PASS)** | In `client.rs:1423` (`connect_or_spawn`), an explicit guard checks `if self.token_path.is_some()`. An isolated test client fails fast with `IpcErrorCode::IoError` rather than falling through to launch `ferryx --daemon`. |
| **Cross-Module Field Visibility Leak** | NEW | **QUALIFIED PASS (DESIGN ADVISORY)** | `terminal.rs:3801` added a test asserting `upgraded.token_path`, forcing `token_path` on `DaemonClient` and `upgrade_rpc_client()` to be `pub(crate)`. `client.rs:93–110` already contains this exact test. Redundant test should be dropped so internal fields stay private. |
| **Machine Error vs Prose Assertions** | AUDITED | **VERIFIED IN `client.rs`** | `client.rs:108` asserts structured machine error `err.code == IpcErrorCode::IoError`. The duplicate in `terminal.rs:3808` asserts on prose substring (`contains("auto-spawn disabled")`). |
| **Windows Token Path Isolation** | PASS | **CONFIRMED PASS** | Zero calls to `get_transport_token_path()` in `terminal.rs`. Injects private tempdir token path via `new_with_socket_and_token_path`. |
| **Windows Wire Handshake Framing** | PASS | **CONFIRMED PASS** | Mock responder in `terminal.rs` consumes and responds to `DaemonRequest::Handshake` with `DaemonResponse::HandshakeOk` using valid newline-delimited JSON. |
| **Core Teardown Drain under `ac`** | OPEN | **EXTERNAL RECONCILIATION PENDING** | Core drain (`await_pending_workers` in `run_diagnostic_classifier_headless`) is tracked under `ac` review separately; explicitly kept open per instruction (no resolution by future promise). |
| **Overall Status** | NEEDS-FIX | **SOURCE-APPROVED (SOURCE-SAFE FOR EVENTUAL REMOTE COMPILE/TEST)** | All blocking compiler errors and auto-spawn hazards are resolved. Ready for remote compilation gate. Full Task 3 remains open. |

---

## 1. Prioritized Concrete Findings (Revision 5 Delta)

### Finding 1 [CONFIRMED RESOLVED]: `token_path` Cloned in `upgrade_rpc_client`
- **Audit Findings:**
  In `src-tauri/src/daemon/client.rs` lines 1295–1306:
  ```rust
  pub(crate) fn upgrade_rpc_client(&self) -> Self {
      Self {
          socket_path: self.socket_path.clone(),
          token_path: self.token_path.clone(),
          connection: Arc::new(Mutex::new(None)),
          interactive_connection: Arc::new(Mutex::new(None)),
          remote_connections: Arc::clone(&self.remote_connections),
          local_connections: Arc::clone(&self.local_connections),
          epoch: Arc::new(parking_lot::RwLock::new(None)),
          upgrade_requested: Arc::clone(&self.upgrade_requested),
          spawn_lock: Arc::new(Mutex::new(())),
      }
  }
  ```
  `token_path` is explicitly cloned. The struct literal is complete, resolving `error[E0063]`.
- **Verdict: PASS (Finding 1 from R4 Closed).**

---

### Finding 2 [CONFIRMED RESOLVED]: No-Spawn Guard Inhibits Production Daemon Launch on Custom Token Failure
- **Audit Findings:**
  In `src-tauri/src/daemon/client.rs` lines 1420–1431 (`connect_or_spawn`):
  ```rust
  // Explicit test-isolated client path: must fail immediately without auto-spawn or recovery.
  // Never contact or launch the production daemon when an isolated fixture is unavailable.
  if self.token_path.is_some() {
      return Err(IpcError::new(
          IpcErrorCode::IoError,
          format!(
              "Cannot connect to isolated fixture daemon socket (auto-spawn disabled for isolated test client): {}",
              self.socket_path.display()
          ),
      ));
  }
  ```
  - If a `DaemonClient` is configured with an isolated test token (`token_path: Some(...)`) and connecting to `self.socket_path` fails, `connect_or_spawn` immediately returns `Err(IpcError)`.
  - It NEVER falls through to line 1450 (`crate::util::no_window_tokio_command(&binary_path).arg("--daemon").spawn()`).
  - This guarantees that an unavailable mock socket, a missing token, or an authentication failure during QA/test runs will **never** launch a live host production daemon process or clobber production daemon files.
- **Verdict: PASS (Finding 2 from R4 Closed).**

---

### Finding 3 [DESIGN ADVISORY - Visibility & Redundant Test]: Cross-Module Field Access in `terminal.rs`
- **Source Locations:**
  - `src-tauri/src/ipc/terminal.rs` lines 3788–3812 (`isolated_client_fails_fast_without_spawning_production_daemon`)
  - `src-tauri/src/daemon/client.rs` lines 93–110 (`isolated_test_client_fails_fast_without_spawning_production_daemon`)
- **Analysis:**
  1. **Redundant Test**:
     `client.rs` already contains an in-module test (`isolated_test_client_fails_fast_without_spawning_production_daemon`, lines 93–110) that:
     - Verifies `upgrade_rpc_client()` clones `token_path`.
     - Verifies `connect_and_handshake_once()` fails fast without auto-spawn.
     - Asserts structured machine error: `assert_eq!(err.code, IpcErrorCode::IoError)`.
  2. **Unnecessary Visibility Widening**:
     `terminal.rs` added an almost identical test (`isolated_client_fails_fast_without_spawning_production_daemon`, lines 3788–3812).
     At line 3801, `terminal.rs` asserts:
     ```rust
     let upgraded = client.upgrade_rpc_client();
     assert_eq!(upgraded.token_path, Some(missing_token));
     ```
     Because `terminal.rs` is in `crate::ipc::terminal` and `client.rs` is in `crate::daemon::client`, accessing `upgraded.token_path` forced `client.rs:835` to declare:
     ```rust
     pub(crate) token_path: Option<PathBuf>,
     ```
     and `pub(crate) fn upgrade_rpc_client(&self)`.
  3. **Prose vs Machine Assertion**:
     `terminal.rs:3808` converts the error to string and checks `err_msg.contains("auto-spawn disabled")` instead of inspecting the structured `IpcErrorCode::IoError`.
- **Recommendation for `author-d3`:**
  To maintain clean encapsulation and avoid unnecessary `pub(crate)` visibility on internal client fields:
  - Remove `isolated_client_fails_fast_without_spawning_production_daemon` from `terminal.rs` (it is 100% redundant with `client.rs:93`).
  - Restore `token_path` on `DaemonClient` and `upgrade_rpc_client()` to private `fn` and private field within `src-tauri/src/daemon/client.rs`.

---

### Finding 4 [CONFIRMED RESOLVED]: Windows Token Path Isolation
- Verified that `terminal.rs` has zero executable references to `crate::daemon::server::get_transport_token_path()`.
- The test injects a temporary token file in `daemon_dir.path().join("test-daemon.token")` via `DaemonClient::new_with_socket_and_token_path`.
- No writes or deletions touch the live production `%LOCALAPPDATA%\Ferryx\runtime\daemon.token`.
- **Verdict: PASS.**

---

### Finding 5 [CONFIRMED RESOLVED]: Windows Wire Handshake Framing
- The mock responder in `terminal.rs` lines 3594–3613 correctly consumes `DaemonRequest::Handshake` and replies with `DaemonResponse::HandshakeOk` with trailing `\n`.
- The wire exchange conforms to the newline-delimited JSON framing expected by `DaemonClient`.
- **Verdict: PASS.**

---

### Finding 6 [EXTERNAL RECONCILIATION PENDING]: Core Teardown Drain under `ac`
- In `src-tauri/src/ipc/qa_barrier.rs`, `run_diagnostic_classifier_headless()` teardown does not yet call `await_pending_workers()`.
- Per instruction:
  > *"Core drain still separate activeac, avoid false whole gate."*
- This is tracked under `authorac`'s domain; Unit 3-D is evaluated solely on its owned files and clean boundary contracts.

---

## 2. Remote Verification Selectors (for `executorad`)

```bash
# 1. Feature-gated compilation check (cleanly compiles across all targets)
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features "local-split-qa,native-terminal"

# 2. Baseline feature-off check (verifies no-QA default builds cleanly)
cargo check --manifest-path src-tauri/Cargo.toml --all-targets

# 3. Client unit test for no-spawn guard
cargo test --manifest-path src-tauri/Cargo.toml --lib --features "local-split-qa,native-terminal" -- daemon::client::paired_host_compatibility_tests::isolated_test_client_fails_fast_without_spawning_production_daemon

# 4. Unit 3-D tests in terminal.rs (serial execution)
cargo test --manifest-path src-tauri/Cargo.toml --lib --features "local-split-qa,native-terminal" -- --test-threads=1 ipc::terminal::tests::qa_held_rpc_tests
```

---

## 3. Final Disposition

- Unit 3-D Revision 5 status: **SOURCE-APPROVED (SOURCE-SAFE FOR EVENTUAL REMOTE COMPILE/TEST)**.
- All blocking compiler errors and auto-spawn hazards are resolved.
- Full Task 3 native acceptance remains **OPEN** (Plan Row 3 native acceptance requires real OS window presentation, accessibility traversal, and OCR marker detection).
