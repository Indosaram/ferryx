# Task 1 Exit Measurement Calibration Forward Addendum

**Author:** Sole Remote Verifier  
**Target:** Parent Orchestrator / Evidence Ledger  
**Date:** 2026-10-03  
**Status:** Forward Calibration for Revision 3  

---

## 1. Exit Measurement Calibration (Inferred vs Measured)

Per evidence review, the following distinctions between **measured** and **inferred** exit states are calibrated:

1. **Mac Test List (`cargo test --lib --features local-split-qa -- --list`)**:
   - **Measured Exit**: Pipeline exit `0` (from `grep -F "error[E"`).
   - **Historical Inner Exit**: **`UNKNOWN`** (compiler failure observed via 9 `error[E0609]` / `error[E0599]` lines in output; conventional rustc `101` was inferred rather than directly measured by PIPESTATUS).
   - **R3 Protocol**: Use `set -o pipefail` and direct `PIPESTATUS[0]` capture to a dedicated `.exit` file before any pipeline or filter.

2. **Windows `qa_barrier` (`cargo test --lib --features local-split-qa qa_barrier`)**:
   - **Measured Exit**: SSH wrapper exit `1` (remote process failure).
   - **Historical Native Exit**: **`UNKNOWN`** (compiler failure observed via 9 `error[E0609]` / `error[E0599]` lines in output; rustc `101` was inferred, but unlike `rust-diag-r2.exit` where `$LASTEXITCODE` was explicitly written to file, no dedicated `.exit` file was recorded).
   - **R3 Protocol**: Wrap in dedicated `run-rust-qa-barrier-r3.ps1` script writing `$LASTEXITCODE` to `rust-qa-barrier-r3.exit` immediately after command execution.

3. **Confirmed Measured Exits on R2**:
   - `ui-build-r2.exit`: **`0`** (directly measured from `$LASTEXITCODE`).
   - `scoped-vitest-r2.exit`: **`0`** (directly measured from `$LASTEXITCODE`, 42/42 tests passed).
   - `canonical-runner-vitest.exit`: **`0`** (directly measured from `$LASTEXITCODE`, 19/19 tests passed).
   - `rust-diag-r2.exit`: **`101`** (directly measured from `$LASTEXITCODE`, 9 compiler errors).
   - `disposable-fixture-receipt.json`: **`1`** (directly measured from `git apply --check`).

---

## 2. R3 Execution Readiness & Staged Preconditions

- **No Retesting Needed on Frontend**: The finished UI build (`dist/`), 42 scoped frontend vitest tests, and 19 canonical runner vitest tests are verified and passed exit 0; they do not need repetition on R3.
- **R3 Scope**:
  1. Verify and restage only the repaired `src-tauri/src/daemon/protocol.rs` and `manifest-r3.json`.
  2. Execute `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_diagnostics` on `maho-win` capturing native exit.
  3. Execute `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier` on `maho-win` capturing native exit to file.
  4. Query exact qualified test name from `cargo test --lib --features local-split-qa -- --list` on `maho-mac` with direct PIPESTATUS capture.
  5. Execute the qualified coordinator test on `maho-mac` with direct exit capture.
- **Compiler Hold**: All compiler runs are on hold until official R3 notice and frozen `manifest-r3.json`.
