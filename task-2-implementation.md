# Task 2 Implementation Report: Bounded Causal Diagnostics (Revision 3)

Date: 2026-10-03  
Worktree: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`  
Parent Commit: `01c20797335ae482d0a08c3a8987d66e206ef66d`  
Branch: `fix/local-pane-liveness-w1`  
Master Plan: `.omo/plans/local-pane-liveness-root-remediation.md` (Task 2)  
Status: Revision 3 Implementation & Test Specifications Ready for Remote Verification (Unrun Status: No local builds/tests executed per project boundary)  

---

## 1. Executive Summary & Honest Unrun Status

Per strict project policy ("No local builds/tests/dev server, no remote execution by you, no GUI/production probes, no commits. Remote verifier is separate"), **zero local builds or tests have been executed in this session**.

A prior remote execution attempt failed at `build.rs:122:39` due to a missing `..\ui\dist` directory. This is an environment build-order prerequisite (Tauri desktop requires frontend assets to exist prior to cargo build/test compilation), **not a product code defect**. The remote test procedure has been updated to explicitly run candidate frontend compilation (`bun run --cwd ui build`) before invoking `cargo test`.

Revision 3 addresses all findings from `task-2-source-review.md` and `task-2-source-review-r2.md`, strictly following the orchestrator's architectural directives rather than flawed review recipes:
- **Private State Encapsulation**: Private `state.sessions` is not exposed; `NativeTerminalSurfaceHostState` exposes a narrow observation method `session_liveness_observation(session_id)`.
- **Honest Generation Identity**: Undeclared generation is never fabricated as `0`. In `NativeTerminalPresentationReceipt` and `NativeTerminalPane.tsx`, missing generation remains optional `null`/`undefined`.
- **Prearmed Injected Dependency Barriers**: Rather than dummy tasks with synthetic snapshot literals, the actual write production function (`send_native_terminal_input_with_stage_logging`) and queue production manager (`terminalInputQueue.enqueue`) are exercised against injected barriers that hold execution, proving that removing stage updates causes test assertions to fail.
- **Strict Error Code Allowlist**: Error normalization uses an explicit enum allowlist of known structured error codes (`TIMEOUT`, `SESSION_NOT_FOUND`, `IO_ERROR`, etc., fallback `"UNKNOWN_ERROR"`), eliminating arbitrary string/secret leakage.
- **Real Production Observation Path**: Exposes `cmd_native_terminal_pane_liveness` (registered in `src-tauri/src/lib.rs` and `ipc/native_terminal.rs`) and `observePaneLiveness` (in `ui/src/lib/paneLiveness.ts`), querying real producer states while retaining `UNKNOWN` when evidence is missing.
- **Session Identity Fencing**: Fences same-epoch sequence comparison (`daemon_epoch == vt_epoch && hub_session == vt_session`) to prevent cross-session sequence mixing.

---

## 2. Producer, Caller & Test Wiring Mapping

| Requirement / Review Finding | Production Producer | Consumer / Caller Surface | Verification Test |
| :--- | :--- | :--- | :--- |
| **Accepted Stage Record** | `terminalInputQueue.enqueue` in `nativeTerminalInputQueue.ts:340` | Dispatched by `NativeTerminalPane.tsx:1270` | `nativeTerminalInputQueue.test.ts` & `switchDebug.test.ts` |
| **Dispatch Stage Record** | `terminalInputQueue.pump` in `nativeTerminalInputQueue.ts:470` | Dispatched upon item dequeue | `nativeTerminalInputQueue.test.ts` |
| **Backend Write Start & Result** | `send_native_terminal_input_with_stage_logging` in `native_terminal.rs:1497` | Invoked by `cmd_native_terminal_send_input` in `native_terminal.rs:1560` | `pane_liveness_diagnostics_prearmed_producer_write_barrier` in `native_terminal.rs:2450` |
| **VT Consumed Sequence** | `surface_host.rs:2465` upon VT grid feed | Native pump update loop | `pane_liveness_diagnostics_prearmed_held_presentation_barrier` in `debug.rs` |
| **Presentation Proof Gating** | `NativeTerminalPane.tsx:2490` (`receipt?.presented === true`) | Lifecycle presentation emitter `nativeTerminalLifecycle.ts:426` | `paneLiveness.test.ts` |
| **Bounds Acknowledged** | `NativeTerminalPane.tsx:2465` (before early return) | Bounds dispatch completion | `switchDebug.test.ts` allowlist |
| **Narrow Surface Host Observation** | `session_liveness_observation` in `surface_host.rs:3145` | `collect_pane_liveness` in `debug.rs` & `cmd_native_terminal_pane_liveness` in `native_terminal.rs` | `pane_liveness_diagnostics_prearmed_held_presentation_barrier` |
| **Real Frontend Observation** | `observePaneLiveness` in `paneLiveness.ts:125` | Diagnostic queries from UI and Task 3 harness | `observePaneLiveness tracks real in-flight execution barrier` in `paneLiveness.test.ts:165` |
| **Error Code Allowlist** | `normalize_error_code` in `debug.rs:115` & `NativeTerminalPane.tsx:2545` | Error logging paths | `pane_liveness_diagnostics_sanitization_removes_sensitive_payloads` in `debug.rs` |
| **Wire Codec Compatibility** | `DaemonSessionDetails` & `DaemonResponse::DescribeSessionOk` | Daemon UDS protocol v5 | `pane_liveness_diagnostics_full_wire_envelope_codec` in `debug.rs` |

---

## 3. Exact Files & SHA-256 Hash Manifest

| File Path | SHA-256 Checksum | Bytes | Status |
| :--- | :--- | :--- | :--- |
| `src-tauri/src/daemon/protocol.rs` | `8bc25c3db517d3df6c43a77986e26d5a82a08a6e337256515159395f476ea342` | 73,433 | Modified |
| `src-tauri/src/daemon/session_service.rs` | `7b287ad18b2dcb4d9c55f50a116e93b600dcddeb543a9d1c138efb043442c55c` | 102,693 | Modified |
| `src-tauri/src/ipc/debug.rs` | `80606be16705641a0899b7f8f0811200ac1adff8033b936ac9395539da7b4704` | 31,808 | Modified |
| `src-tauri/src/ipc/file_link_tests.rs` | `dfc35c15f3fde2db508ca052d4a7291af5fd5fb06b068c0bb9397d0a493f87f8` | 20,647 | Modified |
| `src-tauri/src/ipc/native_terminal.rs` | `c535860454aff59d8fca057c6fd41325a764be071a01d6bff6e4495bce58d1fc` | 128,947 | Modified |
| `src-tauri/src/ipc/native_terminal_disabled.rs` | `0ecffa8c6a6a906c5195930834a6d7136a5af463193cec5ff82164ebd553f3e9` | 4,573 | Modified |
| `src-tauri/src/ipc/terminal.rs` | `9284c5539b8c970aa902bc0c0c3b35f9e4b5e8c66871a6bf0a7188acbf7c5ab6` | 130,639 | Modified |
| `src-tauri/src/ipc/tests.rs` | `0b0a2b1196dabb27feba7a4027d0091dd1c8fa073c32d57396d3ae04c58f830d` | 156,334 | Modified |
| `src-tauri/src/lib.rs` | `e85b4395262c879dfc464da8b8d8d1274ec390717f52a75b3917033c8af2f27f` | 88,476 | Modified |
| `src-tauri/src/native_terminal/surface_host.rs` | `2ce0f52598fdb525c88bc711b731bd92daa190831d829665a585c3cab23cd022` | 347,705 | Modified |
| `src-tauri/src/remote/tests.rs` | `37dc4477f5578982d09512156f49e9658a404be97c1d73d24853ef72c8ef7dfa` | 160,290 | Modified |
| `ui/src/components/NativeTerminalPane.tsx` | `2355cbaa3c27531dfef0824cd6123f447c0765421392e9042a3bcfd12c8fad6d` | 128,835 | Modified |
| `ui/src/lib/nativeTerminalInputQueue.test.ts` | `c913d2dca9b366c63a7afee402fbf5468512bb6aecd33e696754731e65706dc1` | 21,673 | Modified |
| `ui/src/lib/nativeTerminalInputQueue.ts` | `4ceae4075665642d855a032129997a81b90646dbeedb3c4f0b204f92a182eb3a` | 16,601 | Modified |
| `ui/src/lib/nativeTerminalLifecycle.ts` | `2301fcdbf611afe893c35a98777d55c84da25120e223c9d6af00ca97a305cb58` | 17,762 | Modified |
| `ui/src/lib/paneLiveness.test.ts` | `79c8733b345f68ab6c60a6614d618b2205254958a2e692607a065e50ab9a0ce3` | 5,835 | Created |
| `ui/src/lib/paneLiveness.ts` | `a755635da9993b0a4c4a3faaaacc8a8f33f27698eb736d28928831165132f9e1` | 6,720 | Created |
| `ui/src/lib/switchDebug.test.ts` | `6cd6cc7ab3348866984aa4417ffb6d8b84545d0180e49a6c30dcfad8f4ec9a53` | 3,719 | Modified |
| `ui/src/lib/switchDebug.ts` | `e9e1f742cd0a6552df6245e5a73594141093ec3610d68fb826ebb7eda27e36e1` | 3,747 | Modified |

---

## 4. Proposed Remote Test Commands (With Required Build Staging)

To avoid the `build.rs:122:39` failure on the remote host, **step 1 must compile candidate UI assets before step 2 executes cargo tests**:

```bash
# Step 1: Build candidate UI dist (mandatory prerequisite for src-tauri/build.rs)
bun run --cwd ui build

# Step 2: Run Rust in-crate diagnostics test suite (including barrier & wire tests)
cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_diagnostics -- --nocapture --test-threads=1

# Step 3: Run Frontend Vitest suites
bun run --cwd ui test src/lib/switchDebug.test.ts
bun run --cwd ui test src/lib/nativeTerminalInputQueue.test.ts
bun run --cwd ui test src/lib/paneLiveness.test.ts

# Step 4: Cargo check validation across all targets
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
```

---

## 5. Load-Bearing Mutation Recipes

1. **Mutation 1 (F-01: Absent evidence evaluates to IDLE)**:
   - File: `src-tauri/src/ipc/debug.rs` (in `classify_pane_liveness`)
   - Change: Replace `let is_not_stopped = snapshot.suspended == Some(false) && ...` with `let is_not_stopped = snapshot.suspended != Some(true) && ...`
   - Expected Failure: `pane_liveness_diagnostics_prearmed_held_writer_barrier` or missing-evidence tests fail.
2. **Mutation 2 (Prearmed Producer Barrier Test)**:
   - File: `src-tauri/src/ipc/native_terminal.rs` (in `send_native_terminal_input_with_stage_logging`)
   - Change: Comment out the `terminal.surface.input.stage.backend_write_start` logging call.
   - Expected Failure: `pane_liveness_diagnostics_prearmed_producer_write_barrier` fails asserting start receipt.
3. **Mutation 3 (Strict Error Code Allowlist)**:
   - File: `src-tauri/src/ipc/debug.rs` (in `normalize_error_code`)
   - Change: Return `raw` directly instead of matching allowlist.
   - Expected Failure: `pane_liveness_diagnostics_sanitization_removes_sensitive_payloads` fails asserting `"UNKNOWN_ERROR"`.
4. **Mutation 4 (Session Identity Fencing)**:
   - File: `ui/src/lib/paneLiveness.ts` (in `classifyPaneLiveness`)
   - Change: Remove `hubSession === vtSession` check.
   - Expected Failure: `paneLiveness.test.ts` fails `"fences sequence comparison by session identity in addition to epoch"`.
5. **Mutation 5 (Real Queue Producer Barrier)**:
   - File: `ui/src/lib/paneLiveness.ts` (in `observePaneLiveness`)
   - Change: Replace `queuedHeadAgeMs = terminalInputQueue.getQueuedHeadAgeMs(sessionId)` with `null`.
   - Expected Failure: `paneLiveness.test.ts` fails `"observePaneLiveness tracks real in-flight execution barrier"`.

---

## 6. Exact Remaining Gaps

1. **Task 3 Prerequisite (QA Harness Runner)**:
   Task 2 provides the stage telemetry, queue metrics, observation APIs, and classifier. Task 3 will implement `scripts/qa/pane-liveness.mjs` consuming these surfaces for automated desktop/headless verification.
2. **Task 4 Prerequisite (Stable Nonce & Incarnation Identities)**:
   `incarnation` currently emits as `null`. Task 4 will implement cross-handover persistent incarnation tracking.
3. **Task 5 Prerequisite (Verified Actuation Receipts & Auto-Resume)**:
   `BLOCKED_IN_ATTRIBUTED_SUSPENSION` remains unreachable because verified Ferryx actuation receipts do not yet exist; process suspension attribution remains `unknown`. Task 5 will implement verified actuation receipts and auto-resume.

---

## 7. Cleanup Receipt

- Zero background processes, test harnesses, dev servers, or daemons spawned.
- Zero git commits created.
- All modifications are strictly isolated within `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1`.
