# Recovery Final Local Determinism (2026-10-14)

## Executive Summary

Diagnosed and resolved non-deterministic scheduling flushes in `ui/src/state/workspaceStore.localReconcile.test.ts` in candidate worktree `/Volumes/T9-Mac/project/ferryx-wt/recovery-final-20261014`.

- **Root Source Cause (Lines 285–370)**: Two test cases post-probe used three sequential `await Promise.resolve()` scheduling flush guesses to wait for asynchronous recovery admission side effects:
  1. `preserves concurrent other pane updates while describeTerminal probe is held` (3 flushes)
  2. `rejects late probe resolution if owner token changed while probe was held` (3 flushes)
- **Coherent Same-File Audit (Lines 180–265)**: Three additional test cases in the same describe block used two sequential `await Promise.resolve()` guesses:
  3. `manual retry with revalidate unblocks only after affirmative original identity proof` (2 flushes)
  4. `manual retry does not unblock on changed incarnation and retains original identity` (2 flushes)
  5. `manual retry stays blocked when authority reports conflict or non-running` (2 flushes)
- **Deterministic Repair**: Replaced all 12 arbitrary `Promise.resolve()` guesses across all 5 test cases with deterministic exact signals subscribed before probe resolution / dispatch, using:
  - Existing snapshot subscriptions via `subscribeWorkspaceSnapshots` for store/snapshot state transitions (`waitForSnapshotUpdate`).
  - Existing admission outcome callbacks via `requestLocalRecoveryAdmission` (`onResult`) for terminal delivery signals (`settled` / `cancelled`).
  - Bounded native timeouts (`5_000ms`) with explicit `clearTimeout` teardown to eliminate hanging timers and test run deadlocks.
- **Invariants & Discipline**:
  - Zero test weakening: all existing assertions preserved verbatim.
  - Zero sleeps, polling loops, or fake-clock deadlines (`vi.useFakeTimers` prohibited).
  - Test execution status: strictly `UNEXECUTED` per instructions; root sole verifier runs the suite post-freeze.
  - Zero product edits, zero builds, zero LSP, zero child processes, zero commits.
  - All edits applied exclusively via `/Users/indo/.local/bin/apply_patch` with stdin patch format.

---

## File Identification & Exact SHA-256 Hashes

### Candidate Worktree (`/Volumes/T9-Mac/project/ferryx-wt/recovery-final-20261014`)
- **File**: `ui/src/state/workspaceStore.localReconcile.test.ts`
- **SHA-256**: `5e3c5580f606088a20352e088b77cce3519b182b74b3683ed59b4463fff37fc5`
- **Line Count**: 563 lines
- **Worktree Base Revision (HEAD)**: `7fdfa3e4ed7c69ff65c5b1297667f48174c9112d`

---

## Exact Case Counts

| Metric | Count | Details |
|---|---|---|
| Total `it(...)` Test Cases | **15** | All 15 cases verified intact |
| Unaffected Synchronous Cases | **10** | 8 reducer cases + 1 passive blocked case + 1 hook stability case |
| Repaired Microtask Flush Cases | **5** | All 5 cases converted to exact subscribed signals |
| Arbitrary `Promise.resolve()` Guesses Removed | **12** | 6 in root lines 285–370 + 6 in coherent lines 180–265 |
| Sleeps / Polling Loops / Fake Clock Deadlines | **0** | Strictly eliminated per prompt policy |

### Detailed Inventory of Test Cases
1. `clears reconnecting on a live local session and adopts the live epoch` — Synchronous reducer test.
2. `keeps the replay cursor when the live epoch is unchanged` — Synchronous reducer test.
3. `preserves a missing local session identity as retryable uncertainty` — Synchronous reducer test.
4. `preserves nonrunning inventory without exit authority as retryable uncertainty` — Synchronous reducer test.
5. `retains retryable uncertainty on query failure` — Synchronous reducer test.
6. `settles a local session as exited when authoritative inventory proves affirmative exit` — Synchronous reducer test.
7. `leaves ssh and paired sessions untouched` — Synchronous reducer test.
8. `ignores local sessions that are not reconnecting` — Synchronous reducer test.
9. `passive admission without revalidate returns blocked on identity conflict without probing` — Synchronous probe-free check.
10. `manual retry with revalidate unblocks only after affirmative original identity proof` — **Repaired**: replaced 2 `Promise.resolve()` flushes with `waitForSnapshotUpdate` signal.
11. `manual retry does not unblock on changed incarnation and retains original identity` — **Repaired**: replaced 2 `Promise.resolve()` flushes with `settled` admission callback signal.
12. `manual retry stays blocked when authority reports conflict or non-running` — **Repaired**: replaced 2 `Promise.resolve()` flushes with `settled` admission callback signal.
13. `preserves concurrent other pane updates while describeTerminal probe is held` — **Repaired (Root lines 285–330)**: replaced 3 `Promise.resolve()` flushes with `waitForSnapshotUpdate` subscribed before `resolveProbe`.
14. `rejects late probe resolution if owner token changed while probe was held` — **Repaired (Root lines 332–375)**: replaced 3 `Promise.resolve()` flushes with `cancelled` callback signal subscribed before `resolveProbe`.
15. `retains recovery owner across status updates without resubscribing and disposes on close/rebind` — Synchronous `act()` hook stability test (only mock return values retain `ready: Promise.resolve()`).

---

## Source Cause & Signal Mechanism Analysis

### 1. Root Source Cause (SOURCE 285–370)
In lines 285–370, two tests simulated asynchronous daemon probe resolution using a manually held promise (`heldProbe` / `resolveProbe`):
- In `preserves concurrent other pane updates while describeTerminal probe is held`, resolving the probe triggers `describeTerminal` resolution in `workspaceStore.ts`, which asynchronously validates affirmative identity and calls `setWorkspaceSnapshot` to update `s1` without clobbering concurrent updates on `s2`. The test relied on three sequential `await Promise.resolve()` calls to yield control across multiple async stages.
- In `rejects late probe resolution if owner token changed while probe was held`, resolving the probe detects that `s1.recoveryToken` was changed to 42 while the probe was held, triggering `finish({ kind: "cancelled" })`. The test relied on three sequential `await Promise.resolve()` calls to wait for the callback.

Under varying runtime microtask scheduling, arbitrary `Promise.resolve()` turns fail if additional microtask queues (e.g. promise wrappers, vitest mocks, or scheduler ticks) are introduced.

### 2. Exact Replacement Signals
1. **Snapshot Update Signal (`waitForSnapshotUpdate`)**:
   - Uses existing exported `subscribeWorkspaceSnapshots` from `workspaceSnapshotCache.ts`.
   - Subscribed BEFORE `resolveProbe` (or before `requestLocalRecoveryAdmission`).
   - Checks predicate upon subscription and on each snapshot notification from `setWorkspaceSnapshot`.
   - Bounded by a 5,000ms native `setTimeout` with clean `clearTimeout` and listener unsubscription.
2. **Admission Callback Signal (`settled` / `cancelled`)**:
   - Uses the existing `onResult` callback accepted by `requestLocalRecoveryAdmission`.
   - Subscribes a listener promise before `resolveProbe` (or during admission request).
   - Resolves immediately when `onResult` receives the expected non-pending outcome (`blocked` or `cancelled`).
   - Bounded by a 5,000ms native `setTimeout` with clean `clearTimeout` teardown.

---

## Exact Gap Analysis: Background-Loaded-Session >30s Retry Case

### Instruction
> "Add background-loaded-session >30s retry case only if existing safe injectedclock/persistence seam already present and acceptance currently absent, otherwise state exactgap not guessedtest. No sleeps/polling/fakeclockdeadlines/no tests weakening. DO NOTproductedit/no builds/tests/LSP/children/GUI/commits."

### Audit of Seams in Product Source
1. **Clock Interface Availability**:
   `ui/src/lib/sessionRecovery.ts` declares the `RecoveryClock` interface:
   ```ts
   export interface RecoveryClock {
     now(): number;
     setTimeout(callback: () => void, ms: number): unknown;
     clearTimeout(handle: unknown): void;
   }
   ```
   and `RecoverySchedulerOptions<T>` accepts `clock?: RecoveryClock`.
2. **Missing Clock Injection Seam in Store**:
   In `ui/src/state/workspaceStore.ts`, `reconcileLoadedLocalRecovery` instantiates `RecoveryScheduler` directly:
   ```ts
   owner.scheduler = new RecoveryScheduler({
     identity,
     attemptAction: async (attempt) => { ... },
     onOutcomeChange: (outcome) => { ... },
   });
   ```
   No `clock` option is passed, causing `RecoveryScheduler` to fall back to `defaultRecoveryClock` (`globalThis.setTimeout`).
3. **Absence of Store Seam**:
   While `workspaceStore.ts` exposes `setLocalRecoveryPersistence` to inject persistence logic, it exposes NO `setLocalRecoveryClock` or clock injection mechanism.
4. **Exponential Backoff Contract**:
   `calculateBackoffDelay` in `sessionRecovery.ts` computes retry intervals up to `maxDelayMs = 30_000` (30 seconds).
5. **Determinism Obstacle**:
   Without an injected clock seam:
   - A real wall-clock wait of >30s is strictly forbidden by policy and impractical for test runners.
   - Using `vi.useFakeTimers()` is strictly forbidden by the prompt policy ("no fakeclockdeadlines").
   - Modifying product code to introduce `setLocalRecoveryClock` is strictly forbidden by the prompt policy ("DO NOTproductedit").
6. **Formally Stated Exact Gap**:
   Because no safe injected clock seam is currently present in `workspaceStore.ts` for local recovery schedulers, adding a background-loaded-session >30s retry test without product edits would require either a real 30s delay or a fake clock deadline, both of which are forbidden. Per instructions, this test case is NOT guessed with synthetic workarounds, and the exact gap is formally documented here.
7. **Recommended Minimal Product Seam (for future work)**:
   ```ts
   let localRecoveryClock: RecoveryClock | undefined;
   export function setLocalRecoveryClock(clock?: RecoveryClock): void {
     localRecoveryClock = clock;
   }
   ```
   and in `reconcileLoadedLocalRecovery`:
   ```ts
   owner.scheduler = new RecoveryScheduler({
     identity,
     clock: localRecoveryClock,
     attemptAction: async (attempt) => { ... },
     ...
   });
   ```

---

## Scope & Execution Discipline

1. **Target File Isolation**: `ui/src/state/workspaceStore.localReconcile.test.ts` was the ONLY code file modified.
2. **Tooling Discipline**: All changes applied strictly via `/Users/indo/.local/bin/apply_patch` with stdin patch format. No `edit` or `write` fallbacks were used.
3. **Execution Status**: Strictly `UNEXECUTED` (`testUNEXECUTED`). No test runners, cargo, vitest, or LSP tools were executed.
4. **Root Sole Verifier Role**: The root sole verifier will run the same file after freeze.

