# Task 1 Revision 2 Source Review Addendum

- **Task:** Task 1 — Preserve prior work and compose one source-bound starting candidate
- **Plan:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
- **Candidate Worktree (C):** `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`
- **Revision 2 Manifest:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest-r2.json`
- **Prior Review:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/task-1-gate-review.md` (PRESERVED; REJECTED)
- **Reviewer:** OmO senpi-task child (`omo-native-gate-reviewer`)
- **Date:** 2026-10-03
- **Review Scope:** Independent source review of Revision 2 queue additions, explicit retraction of prior false closure conclusion, method/symbol dependency analysis, telemetry privacy check, test determinism audit. (Zero builds/tests/remote commands executed).

---

## 1. Executive Summary & Verdict

- **Overall Task 1 Confirmation:** **PENDING** (Withheld pending execution of remote verification gates on `maho-win` and receipt of disposable-fixture check).
- **Revision 2 Source Review Verdict:** **PASS (SOURCE FIDELITY & CLOSURE VERIFIED)**.
  - The missing dependency `ui/src/lib/nativeTerminalInputQueue.ts` and its test `ui/src/lib/nativeTerminalInputQueue.test.ts` have been promoted to `selectedFiles` in `manifest-r2.json` and applied to candidate `C`.
  - The 2 queue files match declared hashes bit-for-bit and match salvage worktree `W` identically (`diff -u` is empty).
  - TypeScript dependency closure across `paneLiveness.ts` -> `nativeTerminalInputQueue.ts` is fully restored.
  - Telemetry additions are verified strictly payload-free (zero keystroke or buffer leakage).
  - New queue test assertions are verified 100% deterministic (controlled fake timers and deferred promises).
- **Prior Review Status:** Preserved as historical record; earlier "complete closure" statement is formally retracted below.

---

## 2. Formal Retraction of Initial "Complete Closure" Conclusion

In the initial gate review (`task-1-gate-review.md`), the review concluded that candidate `C` had "complete untracked closure" and complete scope exclusion based primarily on:
1. Comparing untracked files on physical disk against the entries listed in `manifest.json`.
2. Confirming that all 16 overlay files in `W` were untouched relative to base `d82b35e4`.

### 2.1 The Inspection Gap
That review failed to perform cross-module method call inspection on the newly introduced candidate files. Specifically:
- Candidate file `ui/src/lib/paneLiveness.ts` (lines 176 and 216) called `terminalInputQueue.getQueuedHeadAgeMs(sessionId)`.
- Associated tests in `ui/src/lib/paneLiveness.test.ts` and `paneDebugInfo.ts` similarly invoked `getQueuedHeadAgeMs`.
- However, `ui/src/lib/nativeTerminalInputQueue.ts` was categorized under `excludedFiles` in `manifest.json` as an "Uncompiled input queue extension draft. Belongs to Task 6" and was left at base `d82b35e4`.
- In base commit `d82b35e4`, `NativeTerminalInputQueueManager` did not implement `getQueuedHeadAgeMs` or `getHeadQueuedRequestId`.

### 2.2 Remote Verifier Disproof
Remote verification on `maho-win` immediately exposed this closure failure:
- `bun run --cwd ui build` failed (`exit 2`, TypeScript error `TS2339: Property 'getQueuedHeadAgeMs' does not exist on type 'NativeTerminalInputQueueManager'` at `paneLiveness.ts:176,216`).
- Scoped Vitest failed 5 of 25 tests (`exit 1`, `TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function`).
- Rust test compilation subsequently panicked because `ui/dist` was never built (`tauri build failed: resource path '..\ui\dist' doesn't exist`).

**Retraction:** The previous claim of "complete dependency and source closure" in `task-1-gate-review.md` is hereby explicitly retracted. The source composition was deficient until `nativeTerminalInputQueue.ts` was included.

---

## 3. Independent Source Review of Revision 2 Additions

### 3.1 Hash & Byte Verification
The two newly included files were independently inspected and measured in candidate `C`:

| File Path | Status | Pre-Commit SHA-256 (`d82b35e4`) | Post-SHA-256 in C | Bytes | Matches `manifest-r2.json` | Matches Worktree W |
|---|---|---|---|---:|:---:|:---:|
| `ui/src/lib/nativeTerminalInputQueue.ts` | modified | `34c082b1d53d14dec5b905313cd14626e5bc683a0dee774f099cb6705aca2969` | `4445db58a7474b7ae0fb9256fa7adca8fd7869e0c22006afb8eff8d99469e10c` | 16,601 | YES (100%) | YES (`diff -u` empty) |
| `ui/src/lib/nativeTerminalInputQueue.test.ts` | modified | `9d62c3253080b3ad1beb99b18b1cd1fca3394483b6314cb861ef7740cc22be0e` | `7488c2d1311e4313af1d9d5b3b7b3e51f47a1cd0a5fff0bf98d72af0c4636554` | 21,783 | YES (100%) | YES (`diff -u` empty) |

- **Total Selected Files in Revision 2:** 21 files (12 modified, 9 untracked).
- **Working Tree Cleanliness in C:** `git status --porcelain=v1 -uall` shows exactly the 12 modified files and 9 untracked files. Zero foreign or unrecorded files exist.
- **Excluded Files in Revision 2:** Reduced from 16 to 14 files (the 2 queue files were promoted). The remaining 14 files (`client.rs`, `handover.rs`, `protocol.rs`, `server.rs`, `session_service.rs`, `terminal.rs`, `NativeTerminalPane.tsx`, `TerminalSplitView.tsx`, `nativeTerminalLifecycle.ts`, etc.) remain clean and unmodified at `d82b35e4`.

### 3.2 Method & Interface Dependency Closure
Inspection of all method calls from dependent files confirmed that Revision 2 fully resolves all missing symbols:

1. **`getQueuedHeadAgeMs(sessionId: string): number | null`**:
   - Implemented at line 175 of `nativeTerminalInputQueue.ts`.
   - Computes `Math.max(0, Date.now() - s.items[0].queuedAt)` when items exist; returns `null` when empty.
   - Satisfies `paneLiveness.ts:176` and `paneLiveness.ts:216`.
2. **`getHeadQueuedRequestId(sessionId: string): string | null`**:
   - Implemented at line 181 of `nativeTerminalInputQueue.ts`.
   - Returns `s.items[0].requestId` or `null`.
3. **`InputIdentityMeta`**:
   - Exported interface defined at line 76:
     ```typescript
     export interface InputIdentityMeta {
       readonly paneIdentity?: string | null;
       readonly bindingKey?: string | null;
       readonly attemptGeneration?: number | null;
       readonly daemonEpoch?: string | null;
       readonly incarnation?: string | null;
     }
     ```
   - Accepted as an optional parameter in `enqueue` (line 296) and `enqueuePreedit` (line 359).
4. **Existing Queue Methods (`getRunningAgeMs`, `getInFlightRequestId`, `getQueuedCount`, `enqueue`)**:
   - All called by `paneLiveness.ts` and confirmed present and typed.

### 3.3 Telemetry & Privacy Audit (Payload-Free Verification)
Inspection of `ui/src/lib/nativeTerminalInputQueue.ts` diff shows two telemetry emissions via `switchDebug`:
1. **`terminal.surface.input.accepted`** (lines 337-347, 436-447):
   - Fields logged:
     - `operationId`: string (UUID nonce)
     - `backendSessionId`: string
     - `paneIdentity`: string | null
     - `bindingKey`: string | null
     - `attemptGeneration`: number
     - `daemonEpoch`: string | null
     - `incarnation`: null
     - `payloadBytes`: number (integer byte count only)
     - `queuedAt`: number (timestamp)
2. **`terminal.surface.input.dispatch`** (lines 473-482):
   - Fields logged:
     - `operationId`: string
     - `backendSessionId`: string
     - `paneIdentity`: string | null
     - `bindingKey`: string | null
     - `attemptGeneration`: number
     - `daemonEpoch`: string | null
     - `incarnation`: null
     - `dispatchedAt`: number (timestamp)

**Privacy Verdict:** ZERO raw keystrokes, typed text characters, or byte buffers are passed to `switchDebug`. Only metadata identifiers, timestamps, and bounded integer byte sizes (`payloadBytes`) are emitted. This strictly complies with project privacy rules.

### 3.4 Test Determinism Audit
The added test in `ui/src/lib/nativeTerminalInputQueue.test.ts` ("distinguishes in-flight execution age from waiting queued head age", lines 595-645) was reviewed for flakiness and timing hazards:
- **Clock Control:** Uses `vi.useFakeTimers()` wrapped in a `try / finally { vi.useRealTimers(); }` block.
- **Async Synchronization:** Uses explicit deferred promises (`createDeferred<string>()`) to coordinate execution steps.
- **Time Advancement:** Uses deterministic `vi.advanceTimersByTime(20)` and `vi.advanceTimersByTime(50)` instead of real `setTimeout` or sleep loops.
- **Assertions:** Validates exact numerical ages (`70` ms running age, `50` ms queued head age) against the virtual clock and verifies reset to `null` upon resolution.
- **Determinism Verdict:** 100% deterministic; zero wall-clock races.

---

## 4. Manifest R2 Digest & Provenance

- **Manifest Path:** `C/.omo/evidence/local-pane-liveness-completion-replan/task-1/manifest-r2.json`
- **Measured SHA-256:** `63f7f3deb55b57b7fe1b6c769e3cae7328ec6eef809d39e9fe41f15e80fc1cc7`
- **Measured Byte Length:** 20,949 bytes
- **Composition Summary:**
  - Base Commit: `d82b35e43f208b53adf6310f4e3c89cbde8814f4`
  - Total Files: 21 (12 modified tracked, 9 untracked)
  - Excluded Files: 14 uncompiled dirty overlay files from `W`

---

## 5. Prerequisites for Final Gate Approval

Overall gate confirmation remains **PENDING** until the following immutable evidence arrives from the remote verifier on `maho-win`:
1. Remote `bun run --cwd ui build` succeeds with exit code 0 and populates `ui/dist`.
2. Remote scoped Vitest (`switchDebug.test.ts`, `paneDebugInfo.test.ts`, `paneLiveness.test.ts`, `nativeTerminalInputQueue.test.ts`) passes 100% (26+ tests).
3. Remote scoped Rust diagnostics (`pane_liveness_diagnostics`) passes (9 tests).
4. Remote QA barrier suite (`qa_barrier`) passes (10 tests).
5. Remote exact presentation barrier test passes (1 test).
6. Remote runner Vitest (`pane-liveness.test.mjs`) passes (19 tests).
7. Remote headless smoke (`diagnostic-classifier --headless`) passes with `DEFERRED-NATIVE`.
8. QA failure mismatch check reproduced against an isolated disposable fixture file.
