# Code Review: Local Worktree Immediate Rendering Fix

## Review Date
2026-09-09

## Verdict
**APPROVE** (Score: 100%, No blocking issues)

---

## 1. Overview & Objective
- **Problem**: When opening or switching to a project folder containing local worktrees, the local worktrees did not appear immediately in the sidebar, resulting in a blank list or noticeable delay until an asynchronous backend refresh finished.
- **Root Causes**:
  - `projectRootWorktree` in `ui/src/lib/projectIdentity.ts` discarded `gitBranch` and `gitHead` for local projects (only preserving them for SSH remote).
  - `useInactiveProjectWorktrees` in `ui/src/state/inactiveProjectWorktrees.ts` initialized with an empty state for non-active projects, ignoring stored snapshots (`getWorkspaceSnapshot`).
  - `groupWorktreesByProject` in `ui/src/components/Sidebar.tsx` unconditionally discarded cached worktrees when `member.workspaceId === activeProjectId` even when the active store had not yet populated `worktrees`, and `activeBucket` recovery lacked recovery logic for local projects in unified project groups.
  - `App.tsx` had an overly strict identity guard `if (state.workspaceId !== pendingRemoteSlug.workspaceId) return;` which failed in environments where `state.workspaceId` was not yet populated.

---

## 2. File-by-File Review

### 1) `ui/src/lib/projectIdentity.ts`
- **Change**: Removed `isRemote &&` restriction from `branch` and `head` assignments in `projectRootWorktree`.
- **Review**:
  - `project.gitBranch` and `project.gitHead` are populated during local backend registration.
  - Preserving them for local projects enables the root worktree to display its active branch immediately on render instead of waiting for `listWorktrees`.
  - Type safety and edge case handling (`detached` calculation) remain solid.
- **Verdict**: PASS

### 2) `ui/src/state/inactiveProjectWorktrees.ts`
- **Change**: Pre-seeded `worktreesByProject` initial state from `getWorkspaceSnapshot(project.workspaceId)` for both active (if empty) and inactive projects.
- **Review**:
  - Leverages existing persistent cache (`getWorkspaceSnapshot`).
  - Eliminates the cold-start empty array problem on initial render and during project switches.
  - When backend `listWorktrees` returns fresh results asynchronously, the state is cleanly updated.
- **Verdict**: PASS

### 3) `ui/src/components/Sidebar.tsx`
- **Change**:
  - Guarded `member.workspaceId === activeProjectId` with `hasActiveWorktrees`.
  - Added targeted recovery in `activeBucket` when the active project is local and lacks a local active worktree row in the bucket (`!hasLocalActiveWorktree`).
- **Review**:
  - Prevents the sidebar from dropping cached worktree rows during project switch transitions when `state.worktrees` is temporarily `[]`.
  - Handles unified multi-target project groups (local + SSH) correctly without duplicating rows.
- **Verdict**: PASS

### 4) `ui/src/App.tsx`
- **Change**: Relaxed `state.workspaceId` equality guard: `if (state.workspaceId && state.workspaceId !== pendingRemoteSlug.workspaceId) return;`.
- **Review**:
  - Prevents premature rejection of pending remote slugs in environments where `state.workspaceId` is optional or asynchronously bound, while still protecting against cross-project race conditions when defined.
- **Verdict**: PASS

### 5) `ui/src/state/workspaceStore.ts`
- **Change**: Exported `createInitialState`.
- **Review**:
  - Clean export allowing callers and tests to create typed initial states consistently.
- **Verdict**: PASS

---

## 3. Test & Build Verification
- `ui/src/lib/projectIdentity.test.ts`: 4/4 passing
- `ui/src/state/inactiveProjectWorktrees.test.tsx`: 9/9 passing
- `ui/src/components/Sidebar.test.tsx`: 37/37 passing
- `ui/src/components/Sidebar.remote.test.tsx`: 11/11 passing
- `ui/src/App.test.tsx`: 91/92 passing (the sole failing test is the documented pre-existing failure `checks for a signed update when the native app starts`)
- `bun run --cwd ui build`: exit code 0
- `cargo check --manifest-path src-tauri/Cargo.toml`: exit code 0
