# Local Worktree Immediate Rendering Fix

## Issue Summary
When opening or switching to a project folder containing local worktrees, the local worktrees did not appear immediately in the sidebar. Instead, there was a noticeable delay or missing render until an asynchronous backend refresh completed.

## Root Cause Analysis
1. **`projectRootWorktree` branch/head erasure**:
   In `ui/src/lib/projectIdentity.ts`, `projectRootWorktree` only preserved `gitBranch` and `gitHead` for SSH remote projects (`isRemote && ...`). For local projects, `branch` and `head` were unconditionally discarded as `null` and `""`. This caused local root worktrees to appear as branch-less / uninitialized before backend IPC returned.
2. **`useInactiveProjectWorktrees` cold start empty state**:
   In `ui/src/state/inactiveProjectWorktrees.ts`, the hook initialized `worktreesByProject` with only the `activeProjectId`'s current worktrees, ignoring all stored workspace snapshots (`getWorkspaceSnapshot`). Projects other than the active project started with an empty array `[]` until async `listWorktrees` returned.
3. **`groupWorktreesByProject` filtering and recovery gap**:
   In `ui/src/components/Sidebar.tsx`, `groupWorktreesByProject` had two issues:
   - When checking `memberProjects` in a unified project group, if `member.workspaceId === activeProjectId`, it unconditionally returned `[]`, discarding any cached worktrees if `worktrees` in the active store was momentarily empty (`hasActiveWorktrees === false`).
   - In `activeBucket` recovery, if an active group had remote worktrees (e.g. SSH member) but no local worktree row yet, the previous check `activeBucket.length === 0` was not met, preventing fallback to the cached local worktrees.

## Solution Implemented
1. **`ui/src/lib/projectIdentity.ts`**:
   Updated `projectRootWorktree` so that `branch` and `head` are preserved from `project.gitBranch` and `project.gitHead` for local projects as well as remote projects.
2. **`ui/src/state/inactiveProjectWorktrees.ts`**:
   Pre-seeded `worktreesByProject` initial state from `getWorkspaceSnapshot(project.workspaceId)` on mount. Now when projects are rendered or switched, cached worktree lists are available synchronously from snapshot storage without waiting for network/IPC roundtrips.
3. **`ui/src/components/Sidebar.tsx`**:
   - In `groupWorktreesByProject`, guarded `member.workspaceId === activeProjectId` with `hasActiveWorktrees`. If the active workspace has not yet loaded active worktrees, cached worktrees are preserved.
   - Enhanced `activeBucket` recovery to check specifically if a local worktree is missing (`!hasLocalActiveWorktree`) when the active project is local (`isLocalActive`), pulling from `inactiveProjectWorktrees?.[activeProjectId]`.
4. **`ui/src/App.tsx`**:
   Maintained clean separation by not injecting unnecessary synchronous store overrides into `App.tsx`, and secured the `pendingRemoteSlug` cross-project race guard with `if (state.workspaceId && state.workspaceId !== pendingRemoteSlug.workspaceId) return;` to support environments where `workspaceId` may be hydrated asynchronously.

## Verification
- `ui/src/lib/projectIdentity.test.ts`: 4/4 passing (including new test verifying local root worktree preserves branch and head).
- `ui/src/state/inactiveProjectWorktrees.test.tsx`: 9/9 passing (including new test verifying pre-seeding from `getWorkspaceSnapshot`).
- `ui/src/components/Sidebar.test.tsx`: 37/37 passing (including new test verifying inactive local project worktrees render immediately).
- `ui/src/components/Sidebar.remote.test.tsx`: 11/11 passing (including new test verifying local worktrees preserved in unified project group).
- `ui/src/App.test.tsx`: 91/92 passing (the sole failing test is the documented pre-existing failure `checks for a signed update when the native app starts`).
- `bun run --cwd ui build`: exits with code 0.
