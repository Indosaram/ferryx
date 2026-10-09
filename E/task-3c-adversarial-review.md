# Independent Adversarial Review: Task 3 Unit 3-C Daemon Fault Controls (Revision 3 Delta)

- **Audit Date:** 2026-10-03
- **Auditor:** Independent Adversarial Reviewer (with Root Audit Invariants)
- **Target Worktree:** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`
- **Reviewed Files:**
  - `src-tauri/src/daemon/server.rs` (SHA-256: `8ab86e2efeefebd4a5f933db3cb3cab12ef5fdd1add4883be44d05b7d5f448d6`, 350,147 bytes; unchanged from R2)
  - `src-tauri/src/daemon/handover.rs` (SHA-256: `676d9ede05711b675e4925a4004c4cb7720b1d3d1ce31def045f6517157941cf`, 46,312 bytes)
  - Reference Core: `src-tauri/src/ipc/qa_barrier.rs` (SHA-256: `68d78ff532c0a6d2a0413063a02c5b329a7a824e2bbcc468a05341424657283d`, commit prefix `68d78ff`, active `ac`)
- **Scope & Constraints:** Bounded delta review strictly evaluating the previous ACK blocker against current core `68d78ff` contract. Zero remote commands or test runs. Zero product edits. Own evidence artifact only.

---

## Executive Summary & Bounded Verdict

| Dimension | Prior Status (R2) | Revision 3 Verdict | Core Technical Evaluation |
|---|---|---|---|
| **Role Filter Before ACK Write** | NEEDS-FIX | **SOURCE-APPROVED (ALGORITHMICALLY RESOLVED)** | `self.role.accepts_barrier(&name)` is evaluated at line 919 BEFORE any file read/write or ACK generation. Even when arm files omit `targetRole`, Successor will skip `predecessor-export`, `commit`, and `abort`, eliminating the ACK overwrite hazard. |
| **Duplicated Scan & Private Field Access** | NEW | **EXACT COMPILER INTERFACE BLOCKER** | In duplicating `scan_and_ack_arms` into `DaemonQaControl`, `handover.rs` accesses private fields of `QaBarrierChannel` (`.dir`, `.run_id`, `.pid`, `.arms`). In current core `68d78ff`, these fields are private, producing compilation errors (`E0616`). Requires `ac` to expose `pub(crate)` fields or provide filtered arm registration. |
| **Synchronous File I/O on Async Paths** | AUDITED | **QUALIFIED PASS (ISOLATED RUNTIME GAP)** | Startup scan in `init_daemon_qa_barrier_channel` runs during early daemon boot. Dynamic fallback scan in `maybe_hold_barrier` (line 996) runs on Tokio worker threads; acceptable if arms are pre-scanned, but dynamic scans risk blocking executor threads. |
| **Filtered Arms Availability to `channel.spec`** | AUDITED | **VERIFIED (LOGICALLY ALIGNED)** | Lines 971–974 explicitly insert accepted specs into `self.channel.arms`, ensuring `channel.spec(name)` returns `Some` during `maybe_hold_barrier`. (Blocked solely by private field access on `.arms`). |
| **Canonical Filename Alignment with `scriptab`** | AUDITED | **CONFIRMED ALIGNED** | Writes `{name}.armed-ack.{role}.json` and canonical `{name}.armed-ack.json`. Aligns 1:1 with `scripts/lib/qa-scenarios/common-harness.mjs:346-380` role verification and overwrite detection. |
| **Regression Test Scoping** | AUDITED | **SCOPING CORRECTION ENFORCED** | Test `test_regression_predecessor_ack_preserved_when_successor_scans_shared_dir_with_absent_target_role` proves logical role-based preservation in-process; claims of "empirical/proven concurrent two-process execution" are rejected. |
| **Overall Status** | NEEDS-FIX | **SOURCE-APPROVED (PENDING EXACT COMPOSED COMPILER INTERFACE RECONCILIATION WITH `ac`)** | Algorithmic logic is approved. No task completion; full Task 3 remains open. |

---

## 1. Concrete Technical Audit (Revision 3 Delta)

### 1.1 Resolution of Previous ACK Overwrite Blocker
- **Prior Hazard (R2):**
  In R2, `DaemonQaControl::scan_and_ack_arms()` delegated directly to `self.channel.scan_and_ack_arms()`. If the runner omitted `targetRole` from an arm file (e.g. `predecessor-export.arm.json`), `qa_barrier.rs` did not filter by role, causing the Successor daemon to overwrite `predecessor-export.armed-ack.json` with its own PID and role.
- **R3 Fix Audit (`handover.rs` lines 917–922):**
  ```rust
  // Gate: Role acceptance check BEFORE scanning or writing anything!
  // Absent targetRole MUST NEVER cause successor to claim predecessor arm!
  if !self.role.accepts_barrier(&name) {
      continue;
  }
  ```
  `self.role.accepts_barrier(&name)` is now evaluated immediately upon parsing the arm file name:
  - `DaemonRole::Successor` accepts ONLY `successor-adopt`.
  - `DaemonRole::Predecessor` accepts ONLY `predecessor-export`, `commit`, and `abort`.
  - When Successor encounters `predecessor-export.arm.json` (even with `targetRole: None`), it immediately skips the entry. It never reads the file body, never generates an ACK, never writes `predecessor-export.armed-ack.successor.json`, and never touches `predecessor-export.armed-ack.json`.
- **Verdict: The ACK overwrite race condition is algorithmically resolved.**

---

### 1.2 Exact Compiler Interface Blocker: Private Field Access on `QaBarrierChannel`
- **Source Locations:**
  - `src-tauri/src/daemon/handover.rs` lines 897, 939, 963, 969, 970, 971:
    - Line 897: `let Ok(entries) = std::fs::read_dir(&self.channel.dir)`
    - Line 939: `if spec.run_id != self.channel.run_id`
    - Line 963: `"producerPid": self.channel.pid`
    - Line 969: `write_json_atomic(&self.channel.dir, ...)`
    - Line 970: `write_json_atomic(&self.channel.dir, ...)`
    - Line 971: `if let Ok(mut arms) = self.channel.arms.lock()`
  - Reference Core `src-tauri/src/ipc/qa_barrier.rs` (commit `68d78ff`):
    ```rust
    pub struct QaBarrierChannel {
        dir: PathBuf,
        run_id: String,
        role: Option<String>,
        pid: u32,
        arms: Mutex<HashMap<String, ArmSpec>>,
        ...
    }
    ```
- **Hazard & Build Blocker:**
  `QaBarrierChannel` is defined in `crate::ipc::qa_barrier`. In Rust, struct fields without `pub` or `pub(crate)` are strictly private to `qa_barrier.rs`.
  `handover.rs` resides in `crate::daemon::handover::qa_control`.
  Attempting to directly access `self.channel.dir`, `self.channel.run_id`, `self.channel.pid`, and `self.channel.arms` violates Rust visibility rules and produces compiler error `E0616 (field is private)`.
- **Interface Reconciliation Required with `authorac`:**
  To compile cleanly without violating encapsulation, `st_01a100ac` (`authorac`) must either:
  1. Add `pub(crate)` visibility to `dir`, `arms`, `pid`, `run_id` on `QaBarrierChannel` in `qa_barrier.rs`, OR (preferred architecture):
  2. Provide a filtered arm scanning method on `QaBarrierChannel`:
     ```rust
     pub fn scan_and_ack_arms_filtered<F>(&self, predicate: F) -> (Vec<String>, Vec<String>)
     where F: Fn(&str) -> bool;
     ```
     This allows `DaemonQaControl` to pass `|name| self.role.accepts_barrier(name)` into `QaBarrierChannel` without duplicating filesystem traversal or prying into private mutexes.

---

### 1.3 Synchronous File I/O on Async Startup and Seams
- **Audit Findings:**
  - `DaemonQaControl::scan_and_ack_arms()` and `write_json_atomic` execute synchronous `std::fs` operations (`read_dir`, `read_to_string`, `write`, `rename`).
  - **Startup Path (`server.rs:2393`)**: `init_daemon_qa_barrier_channel(role)` runs synchronously during daemon boot before the UDS socket listener starts accepting connections. In this phase, reading small JSON arm files is bounded and presents zero deadlocks.
  - **Seam Fallback (`handover.rs:996`)**:
    ```rust
    if self.channel.spec(name).is_none() {
        let _ = self.scan_and_ack_arms();
    }
    ```
    If `spec(name)` was already populated during startup, this branch is bypassed. If a dynamically-armed barrier is encountered, it executes synchronous I/O on the active Tokio worker thread handling `TransferSessions`, `CommitHandover`, or `AbortHandover`. While bounded to the local barrier directory, offloading to `tokio::task::spawn_blocking` is recommended if dynamic arms are used.

---

### 1.4 Availability of Filtered Arms to `channel.spec`
- **Audit Findings:**
  - In `handover.rs` lines 971–974:
    ```rust
    if let Ok(mut arms) = self.channel.arms.lock() {
        arms.insert(spec.name.clone(), spec);
    }
    ```
  - When `maybe_hold_barrier` executes:
    ```rust
    let spec = self.channel.spec(name)?;
    self.channel.write_held(&spec, session_id, stage, extra_held);
    let outcome = self.channel.wait_for_release(&spec).await;
    ```
    `channel.spec(name)` queries the same `arms` map. Because accepted arms are inserted into `arms`, `channel.spec` will successfully find the spec.
  - **Verdict:** The design correctly ensures filtered arms are available for barrier holds.

---

### 1.5 Alignment of Canonical Filenames with `scriptab`
- **Audit Findings:**
  - Lines 968–970 write:
    1. Role-scoped ACK: `{spec.name}.armed-ack.{role}.json` (e.g. `predecessor-export.armed-ack.predecessor.json`)
    2. Generic canonical ACK: `{spec.name}.armed-ack.json`
  - In `scripts/lib/qa-scenarios/common-harness.mjs:346-380`, `awaitRegistered`:
    - Waits for `${name}.armed-ack.${targetRole}.json` when `targetRole` is configured.
    - Inspects `${name}.armed-ack.json` and asserts:
      ```javascript
      if (genericAck.role && genericAck.role !== targetRole) {
        throw new HarnessError('ASSERTION_FAILURE', `barrier ${name}: generic ACK role '${genericAck.role}' does not match targetRole '${targetRole}' (ACK overwrite detected)`);
      }
      ```
  - Because `handover.rs` suppresses unowned ACKs, Successor never touches `predecessor-export.armed-ack.json`.
  - **Verdict: 100% aligned with runner protocol.**

---

### 1.6 Scoping and Proof Limits of Regression Test
- **Source Location:** `src-tauri/src/daemon/handover.rs` lines 1089–1180.
- **Analysis:**
  - Test `test_regression_predecessor_ack_preserved_when_successor_scans_shared_dir_with_absent_target_role`:
    - Instantiates `pred_control` and `succ_control` concurrently within the **same test process**.
    - Both report `producer_pid: std::process::id()`.
    - Asserts that `pred_ack_before == pred_ack_after` when Successor scans the shared directory.
  - **Proof Boundary Enforced:**
    - This test provides proof of in-process logical role filtering and non-clobbering on disk.
    - It does **not** prove multi-process operating system scheduling concurrency between two distinct daemon PIDs.
    - Claims of "empirically proven concurrent multi-process execution" are explicitly rejected.

---

## 2. Remote Verification Selectors (for `executorad`)

```bash
# 1. Check feature-gated daemon compilation (will expose E0616 until interface reconciled with ac)
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --features "local-split-qa,native-terminal"

# 2. Run isolated Unit 3-C tests
cargo test --manifest-path src-tauri/Cargo.toml --lib --features "local-split-qa,native-terminal" -- daemon::handover::qa_handover_tests

# 3. Baseline feature-off check
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
```

---

## 3. Final Disposition

- Unit 3-C Revision 3 status: **SOURCE-APPROVED (PENDING EXACT COMPOSED COMPILER INTERFACE RECONCILIATION WITH `ac`)**.
- The logic resolving the ACK overwrite blocker is sound, role-filtered before I/O, and aligned with the runner. Full compilation awaits visibility reconciliation on `QaBarrierChannel` with `authorac`.
- Full Task 3 native acceptance remains **OPEN**.
