# Task 1 Source Defect Report: Missing Dependency Closure (NativeTerminalInputQueueManager)

**Reported By:** Sole Remote Verifier  
**Target:** Source Owner / Parent Orchestrator  
**Date:** 2026-10-03  
**Status:** BLOCKING SOURCE DEFECT (Stops Task 1 Remote UI Verification)  

---

## 1. Exact Defect Description

During remote verification on `maho-win`, the remote UI build (`bun run --cwd ui build`) failed at the TypeScript compilation phase (`tsc`) with exit code **2**:

```text
$ tsc && vite build
src/lib/paneLiveness.ts(176,46): error TS2339: Property 'getQueuedHeadAgeMs' does not exist on type 'NativeTerminalInputQueueManager'.
src/lib/paneLiveness.ts(216,46): error TS2339: Property 'getQueuedHeadAgeMs' does not exist on type 'NativeTerminalInputQueueManager'.
```

Subsequently, running the scoped vitest suite (`bun run --cwd ui test src/lib/switchDebug.test.ts src/lib/paneDebugInfo.test.ts src/lib/paneLiveness.test.ts`) produced **5 test failures** with exit code **1**:

```text
FAIL src/lib/paneDebugInfo.test.ts > formatPaneDebugInfo > emits one JSON line carrying the identity triad and daemon binding
TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function
 ❯ observePaneLiveness src/lib/paneLiveness.ts:176:46

FAIL src/lib/paneDebugInfo.test.ts > formatPaneDebugInfo > formats pane debug info asynchronously awaiting native observation
TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function
 ❯ observePaneLivenessAsync src/lib/paneLiveness.ts:216:46

FAIL src/lib/paneLiveness.test.ts > classifyPaneLiveness > observePaneLiveness queries real input queue state
TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function
 ❯ observePaneLiveness src/lib/paneLiveness.ts:176:46

FAIL src/lib/paneLiveness.test.ts > classifyPaneLiveness > observePaneLiveness tracks real in-flight execution barrier
TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function
 ❯ observePaneLiveness src/lib/paneLiveness.ts:176:46

FAIL src/lib/paneLiveness.test.ts > classifyPaneLiveness > prioritizes held execution over queue backlog and asserts explicit ages with controlled clock
TypeError: terminalInputQueue.getQueuedHeadAgeMs is not a function
 ❯ src/lib/paneLiveness.test.ts:206:48
```

---

## 2. Root Cause Analysis

1. In `manifest.json`, `ui/src/lib/nativeTerminalInputQueue.ts` was explicitly listed under `excludedFiles`:
   ```json
   {
     "path": "ui/src/lib/nativeTerminalInputQueue.ts",
     "linesInW": "+61 / -0",
     "reason": "Uncompiled input queue extension draft. Belongs to Task 6."
   }
   ```
2. In base commit `d82b35e4`, `NativeTerminalInputQueueManager` does **not** implement `getQueuedHeadAgeMs` or `getHeadQueuedRequestId`.
3. However, candidate file #12 (`ui/src/lib/paneLiveness.ts`), candidate file #7 (`ui/src/lib/paneDebugInfo.ts`), and their associated unit tests (`ui/src/lib/paneLiveness.test.ts`) **directly invoke** `terminalInputQueue.getQueuedHeadAgeMs(sessionId)`.
4. Therefore, omitting `ui/src/lib/nativeTerminalInputQueue.ts` broke the TypeScript dependency closure.

---

## 3. Scoped Remediation Required from Source Owner

1. Re-include `ui/src/lib/nativeTerminalInputQueue.ts` (from worktree `W`) into candidate worktree `C`.
   - Specifically, `getQueuedHeadAgeMs(sessionId: string): number | null` and `getHeadQueuedRequestId(sessionId: string): string | null`, plus the `InputIdentityMeta` interface and `switchDebug` telemetry calls.
2. Update `manifest.json` to promote `ui/src/lib/nativeTerminalInputQueue.ts` from `excludedFiles` to `selectedFiles` (bringing the total composed candidate files from 19 to 20).
3. Update `ownership.json` and `source-inventory.md` to reflect the closure addition.
4. Notify the Remote Verifier to re-stage and re-verify.

---

## 4. Verifier Action

Per strict project instructions:
- "Stop on source defects and return exact error for scoped source-owner repair rather than patching yourself."
- "No product edits or commits."

The remote verifier will NOT edit `ui/src/lib/nativeTerminalInputQueue.ts` or `ui/src/lib/paneLiveness.ts`. This defect report and the raw exit/log evidence are saved under `E/task-1/`.
