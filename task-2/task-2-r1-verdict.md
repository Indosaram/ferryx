# Task 2 Independent Verification Verdict: REJECT

**Worktree**: `/Volumes/T9-Mac/project/ferryx-wt/consistent-session-recovery-20261013`  
**Base Commit**: `7fdfa3e4ed7c69ff65c5b1297667f48174c9112d`  
**Verdict**: **REJECT** (Gates RED + Empirical Failures + Zero Product Callers + Reducer Fail-Open Seams)  
**Verifier**: `st_01a1352a`  
**Date**: 2026-10-13  

---

## 1. Monitored Gate Execution Receipts

### UI Gate (Command U + Independent Test Suite)
- **Command**: `bun run --cwd ui test src/lib/nativeTerminalAttachPolicy.test.ts src/lib/sessionPersistence.test.ts src/state/workspaceStore.terminalExit.test.ts src/state/workspaceStore.sshReattach.test.ts src/lib/sessionAuthority.independent.test.ts`
- **Monitor Handle**: `mon_MQ5HZ83VF7K23PKW` (`bash_id`: `bash_14`)
- **Raw Log Path**: `/Volumes/T9-Mac/project/ferryx-wt/consistent-session-recovery-20261013/task-2/task-2-authority-ui-r1.log`
- **Exit Code**: `1`
- **Executed Counts**: 5 test files, **18 failed | 113 passed** (131 total tests).
- **Test Failures by Suite**:
  1. `src/state/workspaceStore.terminalExit.test.ts` (2 failed):
     - `marks remote workspace session disconnected with a failure`: expected `disconnected`, received `reconnecting`.
     - `is a no-op when bindingKey does not match the current session binding epoch`: state mutated instead of no-op.
  2. `src/lib/nativeTerminalAttachPolicy.test.ts` (5 failed):
     - Author's own tests (`special characters`, `quotes`, `long session ID`, `prefer typed details`, `report reason`) expect `confirmed-missing` but receive `unverified-missing` because author tests pass bare errors omitting required `expectedIncarnation` context.
  3. `src/lib/sessionAuthority.independent.test.ts` (2 failed):
     - `classifies structured SESSION_NOT_FOUND with unclear origin as unverified-missing`: expected `unverified-missing`, received `operational-error`.
     - `rejects SESSION_BACKEND_UNAVAILABLE attempt outcome when attemptKey or attemptToken is stale, mismatched, or absent`: state mutated because reducer does not validate keys when current session lacks `recoveryKey`.
  4. `src/lib/sessionPersistence.test.ts` (9 failed):
     - Regression tests expect uninventoried sessions to settle to `exited` / `null`, but author changed persistence to preserve `working`, breaking existing recovery contracts.
     - `TypeError: Cannot read properties of undefined (reading 'backendSessionId')` on line 1295 in paired proxy test.

### Rust Gate (Command R)
- **Command**: `cargo test --manifest-path src-tauri/Cargo.toml --lib recovery_consistency -- --test-threads=1`
- **Monitor Handle**: `mon_8D6HPYAB4BN1C14G` (timed out after 300s initial compile) -> resumed via `mon_11J5QSE7V2EEFNTJ` (`bash_id`: `bash_23`)
- **Raw Log Path**: `/Volumes/T9-Mac/project/ferryx-wt/consistent-session-recovery-20261013/task-2/task-2-authority-rust-r1.log`
- **Exit Code**: `101`
- **Failure Cause**: `resource path '../ui/dist' doesn't exist`. Build script failed during `ferryx` compilation because `ui/dist` was never built in the worktree. Zero tests executed.

---

## 2. Concrete Source Integration & Coverage Findings

### Finding 1: Zero Product Callers for Describe-to-Exit Consumer (Freeze Claim False)
- Candidate freeze claim 4 states: *"The actual routed describe-to-exit consumer reconcileSessionFromDescribe and markSessionAffirmativeExit are implemented and wired in workspaceStore.ts"*.
- **Source Discovery**: `grep -rnE 'reconcileSessionFromDescribe|markSessionAffirmativeExit' ui/src` yields only:
  - Definition in `ui/src/state/workspaceStore.ts:2013, 2050`
  - Direct unit test calls in `ui/src/state/workspaceStore.terminalExit.test.ts:3, 262, 278, 311, 328`
  - **Zero actual product callers in `ui/src`** (unwired to scheduler, lifecycle bus, or UI components).

### Finding 2: Consumer Semantic Inversions (`workspaceStore.ts:2050-2100`)
1. **Paired Proxy Absence Returns Active**: Line 2062 returns `"active"` when `details.isPairedProxy` is true, even when `running: false` / `owner_unavailable`. This conflates absent proxy with an active PTY.
2. **Legacy Unproved Liveness Returns Active**: If `details.running === false` but `details.incarnation` or `expectedIncarnation` is missing, the function falls through to line 2095 and returns `"active"`.
3. **Unvalidated Session ID**: `details.sessionId` is never validated against the requested `backendSessionId`.
4. **False Exited Return on Reducer Rejection**: Dispatches `SESSION_AFFIRMATIVE_EXIT` with `token: token ?? null` and `daemonEpoch: authority?.daemonEpoch ?? null`. When the reducer rejects these due to missing/mismatched token or epoch, `reconcileSessionFromDescribe` still returns `"exited"`.

### Finding 3: Reducer Fail-Open Vulnerabilities (`workspaceStore.ts:3037+`)
1. **`SESSION_LIFECYCLE`**: Checks only `action.incarnation === session.incarnation`; lacks daemon epoch and owner context fences.
2. **`SESSION_AFFIRMATIVE_EXIT`**: Daemon epoch check (`if (action.daemonEpoch && session.daemonEpoch ...)`) only rejects if *both* are non-null and differ. If action epoch is null/omitted, the fence fails open. Token check only applies if `session.recoveryToken` is non-null.
3. **`SESSION_BACKEND_UNAVAILABLE`**: Attempt key and attempt token are only compared if the session already has `session.recoveryKey` and `session.recoveryToken`. Sessions lacking prior keys mutate state unconditionally without fence validation.

### Finding 4: Backend Test Seam Gap (`src-tauri/src/daemon/recovery_consistency_tests.rs`)
- Tests 1–5 in `recovery_consistency_tests.rs` only test fabricated structs and helper methods (`matches_incarnation`, `is_paired_proxy_actor`) plus self-serialized error JSON.
- Zero tests exercise `SessionService::describe_session`, server socket handlers, or actual routed absence paths.

---

## 3. Required Bounded Task 2 Producer Repairs

1. **Build Pre-requisite**: Ensure `ui/dist` is built or accessible so `cargo test --manifest-path src-tauri/Cargo.toml --lib recovery_consistency` compiles cleanly without exit 101.
2. **Wire Describe Consumer**: Connect `reconcileSessionFromDescribe` to the recovery scheduler / terminal lifecycle flow where recovery queries are evaluated.
3. **Fix Consumer Inversions**:
   - Return `"uncertain"` (not `"active"`) when paired proxy actor is absent/unavailable.
   - Return `"uncertain"` (not `"active"`) when unproved session lacks affirmative exit proof.
   - Verify `details.sessionId === backendSessionId`.
   - Only return `"exited"` if affirmative proof carries valid required token and epoch matching the session.
4. **Harden Reducer Fences**:
   - Require matching `daemonEpoch` and owner context on `SESSION_LIFECYCLE` exit events.
   - Require non-null matching `daemonEpoch` on `SESSION_AFFIRMATIVE_EXIT`.
   - In `SESSION_BACKEND_UNAVAILABLE`, require valid `attemptKey` and `attemptToken` matching current session owner context; reject when current owner context is absent or mismatched.
5. **Fix Broken Test Suites**:
   - Update author tests in `nativeTerminalAttachPolicy.test.ts` to pass matching `expectedIncarnation` context where `confirmed-missing` is expected.
   - Fix `TypeError` in `sessionPersistence.test.ts:1295` and scope persistence uncertainty preservation so existing regression contracts are satisfied.
6. **Backend Routed Integration Coverage**:
   - Add integration tests exercising `describe_session_identity` through actual server/service seams rather than fabricated helper structs.
