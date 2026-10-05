# Task 1 Source Defect Report: Missing DTO Closure in src-tauri/src/daemon/protocol.rs

**Reported By:** Sole Remote Verifier  
**Target:** Source Owner / Parent Orchestrator  
**Date:** 2026-10-03  
**Status:** BLOCKING SOURCE DEFECT (Stops Task 1 Remote Rust Verification)  

---

## 1. Exact Compiler Output on maho-win

When executing `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_diagnostics` and `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier`, rustc terminates with exit code **101** (compilation failure):

```text
error[E0609]: no field `reader_paused` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:839:36
    |
839 |                 assert_eq!(session.reader_paused, None);
    |                                    ^^^^^^^^^^^^^ unknown field

error[E0609]: no field `kernel_stopped` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:840:36
    |
840 |                 assert_eq!(session.kernel_stopped, None);
    |                                    ^^^^^^^^^^^^^^ unknown field

error[E0609]: no field `registry_suspended` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:841:36
    |
841 |                 assert_eq!(session.registry_suspended, None);
    |                                    ^^^^^^^^^^^^^^^^^^ unknown field

error[E0609]: no field `suspension_source` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:842:36
    |
842 |                 assert_eq!(session.suspension_source, None);
    |                                    ^^^^^^^^^^^^^^^^^ unknown field

error[E0609]: no field `reader_paused` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:865:36
    |
865 |                 assert_eq!(session.reader_paused, Some(true));
    |                                    ^^^^^^^^^^^^^ unknown field

error[E0609]: no field `kernel_stopped` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:866:36
    |
866 |                 assert_eq!(session.kernel_stopped, Some(false));
    |                                    ^^^^^^^^^^^^^^ unknown field

error[E0609]: no field `registry_suspended` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:867:36
    |
867 |                 assert_eq!(session.registry_suspended, Some(true));
    |                                    ^^^^^^^^^^^^^^^^^^ unknown field

error[E0609]: no field `suspension_source` on type `daemon::protocol::DaemonSessionDetails`
   --> src\ipc\debug.rs:868:36
    |
868 |                 assert_eq!(session.suspension_source.as_deref(), Some("unknown"));
    |                                    ^^^^^^^^^^^^^^^^^ unknown field

error[E0599]: no associated function or constant named `new` found for struct `daemon::protocol::DaemonSessionDetails` in the current scope
   --> src\ipc\debug.rs:873:45
    |
873 |         let details = DaemonSessionDetails::new(
    |                                             ^^^ associated function or constant not found in `daemon::protocol::DaemonSessionDetails`
```

---

## 2. Root Cause Analysis

1. In `manifest-r2.json`, `src-tauri/src/daemon/protocol.rs` is placed under `excludedFiles`:
   ```json
   {
     "path": "src-tauri/src/daemon/protocol.rs",
     "linesInW": "+50 / -0",
     "reason": "Unfrozen DTO protocol extensions. Belongs to Task 2 contract freeze."
   }
   ```
2. In base commit `d82b35e4`, `DaemonSessionDetails` does not have `reader_paused`, `kernel_stopped`, `registry_suspended`, `suspension_source`, or the `DaemonSessionDetails::new` constructor.
3. However, candidate file #6 (`src-tauri/src/ipc/debug.rs`), which **is** part of the Task 1 candidate composition, directly tests and references these exact fields in lines 839-873.
4. Therefore, omitting `src-tauri/src/daemon/protocol.rs` breaks the Rust crate compilation across both `pane_liveness_diagnostics` and `qa_barrier` tests.

---

## 3. Remediation Required from Source Owner

1. Re-include `src-tauri/src/daemon/protocol.rs` (from worktree `W`) into candidate worktree `C`.
   - Specifically the 4 optional fields on `DaemonSessionDetails` (`reader_paused`, `kernel_stopped`, `registry_suspended`, `suspension_source`) and the `pub fn new(...)` constructor.
2. Promote `src-tauri/src/daemon/protocol.rs` from `excludedFiles` to `selectedFiles` in `manifest.json` (bringing candidate files to 22).
3. Notify the Remote Verifier to re-stage and execute the Rust gates.
