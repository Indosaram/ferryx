# Session Restore Duplicate Terminal Tab Root Cause & Fix

**Date:** 2026-09-05  
**Component:** `ui/src/state/workspaceStore.ts`, `ui/src/state/workspaceRuntime.ts`  
**Test Coverage:** `ui/src/state/workspaceRestoreDuplicateTab.test.tsx`

---

## 1. Problem Summary

When restoring an existing workspace session on startup or switching projects, an unintended new empty terminal tab (typically labeled `main (2)`) was created alongside the restored tabs.

Inspection of `session_state.json` confirmed this symptom across multiple projects:
- `mahoquot`: Tab 1 (`main`, OmO agent session), Tab 2 (`main (2)`, empty shell session)
- `orca-lite-release-verify-13768`: Tab 1 (`main`, 3-pane split), Tab 2 (`main (2)`, empty single pane)
- `maho-workspace`: Tab 1 (`main (2)`, after `main` was closed)

---

## 2. Root Cause Analysis

The issue was caused by a timing race condition between asynchronous session restoration (`useWorkspaceRestore`) and background worktree runtime initialization (`useWorkspaceRuntime`), coupled with a missing `allowCreate` check in `ensureTabForWorktree`.

### Root Cause A: Race Condition on Startup and Project Switch
1. In `App.tsx`, project registration finishes and sets `registeredProjectId = activeProject.workspaceId`.
2. This simultaneously triggers two `useEffect` hooks:
   - `useWorkspaceRestore`: Begins asynchronous session restoration via `runRestore()`, which calls `loadSession()` (disk I/O) and `listTerminalSessions()` (daemon IPC) before invoking `restoreWorkspace(restoredState)`. While this is in flight, `state.layout.tabs` in the store remains empty (`[]`).
   - `useWorkspaceRuntime`: Its `initialize()` function sets up listeners and unconditionally called `await refreshWorktreesRef.current()`, without specifying `{ allowCreate: false }`. Thus, `allowCreate` defaulted to `true`.
3. Inside `refreshWorktrees`, `ensureTabForWorktree(preferred)` was called with `allowCreate: true`.
4. Because `runRestore()` was still awaiting disk/daemon queries, `ensureTabForWorktree` observed `state.layout.tabs.length === 0`.
5. Believing the workspace had zero open tabs, `ensureTabForWorktree` invoked `openTab(worktree)` to create a new tab.
6. `openTab` began spawning a backend PTY process in the daemon (`spawnTerminal`).
7. While `spawnTerminal` was awaiting, `runRestore()` completed, calling `restoreWorkspace(restoredState)` and populating `state.layout.tabs` with the user's restored tab (e.g. `main`).
8. `spawnTerminal` completed, and `openTab` resumed. It calculated the new tab's label using `nextTabLabel(...)`. Finding an existing `main` tab in the layout, it labeled the newly spawned tab `main (2)` and dispatched `ADD_TAB_WITH_SESSION`.
9. The newly spawned empty terminal tab was appended to the layout next to the restored tab.

### Root Cause B: Missing `allowCreate` Guard in `ensureTabForWorktree`
In `ui/src/state/workspaceStore.ts`, `ensureTabForWorktree` checks if a tab exists for the target worktree:
- When `snapshot.activeWorktreePath === worktree.path`, it guarded creation with `if (!allowCreate) return null;`.
- However, when `snapshot.activeWorktreePath !== worktree.path` (e.g. on fresh store mount where `activeWorktreePath` is still `null`, or across secondary worktrees), line 710 unconditionally called `return openTab(worktree);` without checking `allowCreate`.
- As a result, any call to `ensureTabForWorktree(preferred, { allowCreate: false })` still spawned a tab if the active path differed.

---

## 3. Implemented Fix

### 1. Guard `ensureTabForWorktree` against in-flight session restoration (`workspaceStore.ts`)
- Imported `getWorkspaceRestoreStatus` from `./workspaceRestore`.
- In `ensureTabForWorktree`, computed `isRestoring = getWorkspaceRestoreStatus(workspaceId) === "loading"`.
- Defined `effectiveAllowCreate = allowCreate && !isRestoring`.
- While disk restoration is in flight, tab creation is suppressed. If an active tab already exists in memory, it is activated; otherwise, the call safely returns `null` without spawning a duplicate tab.
- Added the missing `if (!effectiveAllowCreate) return null;` guard to the `snapshot.activeWorktreePath !== worktree.path` branch before calling `openTab(worktree)`.

### 2. Disallow tab creation during `initialize()` in `useWorkspaceRuntime` (`workspaceRuntime.ts`)
- During runtime initialization (`initialize()`), listeners are registered and worktrees synced. When `registeredWorkspaceId !== undefined` (standard app lifecycle), `initialize()` now explicitly passes `{ allowCreate: false }` to `refreshWorktreesRef.current(...)`.
- Preserved `allowCreate: true` when `registeredWorkspaceId === undefined` to maintain backward compatibility for standalone unit tests.

---

## 4. Verification

1. **Reproduction Test (`ui/src/state/workspaceRestoreDuplicateTab.test.tsx`)**:
   - Recreated the asynchronous disk restore and worktree initialization timing.
   - Verified that on unpatched code, the test failed with `AssertionError: expected [ Array(2) ] to have a length of 1 but got 2` and `services.spawnTerminal` was called.
   - Verified that with the fix applied, the test passed: exactly 1 restored tab, 0 calls to `spawnTerminal`.
2. **State Subsystem Test Suite**:
   - Ran `bun run --cwd ui test src/state`: All 26 test files (228 tests) passed.
3. **App Integration Test Suite**:
   - Ran `bun run --cwd ui test src/App.test.tsx`: All 87 tests passed.
4. **UI Build & Typecheck**:
   - Ran `bun run --cwd ui build`: TypeScript and Vite production bundle compiled cleanly in 1.86s.
5. **Rust Backend Check**:
   - Ran `cargo check --manifest-path src-tauri/Cargo.toml`: 0 errors.
