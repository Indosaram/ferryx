# Workspace and Worktree Deletion Fix Report

**Date:** 2026-09-04  
**Status:** Completed and Verified  
**Scope:** UI Context Menu & Trash Icon Deletion (Workspace & Worktree), App.tsx Workspace Routing, Dirty Worktree Force Deletion Backend & Dialog

---

## 1. Executive Summary

Users experienced issues where:
1. Workspaces (Projects) in the sidebar could not be deleted/removed via context menu (right-click) or trash icon.
2. Worktrees could not be deleted when right-clicking, when attempting deletion across multi-project setups, or when worktrees had dirty/uncommitted files.

We diagnosed all root causes and implemented fixes across frontend (`Sidebar.tsx`, `WorktreeList.tsx`, `ProjectDialogs.tsx`, `WorktreeDeleteDialog.tsx`, `App.tsx`, `tauri.ts`) and backend (`manager.rs`, `ipc/mod.rs`), backed by 152 passing frontend tests and 26 passing Rust backend tests.

---

## 2. Root Causes Identified

### A. Workspace (Project) Removal
- **No Context Menu or Trash Icon on Workspace Row**:
  - `ProjectHeader` in `ui/src/components/Sidebar.tsx` lacked right-click context menu handling (`onContextMenu`) and had no trash icon button on hover.
  - `Sidebar` had no `onRemoveProject` prop passed from `App.tsx`, leaving project unregistration unhandled.
  - No project removal confirmation dialog existed, and the user had no way to remove unwanted projects from the sidebar.

### B. Worktree Deletion in Multi-Project Environments
- **`workspaceId` Mismatch in `App.tsx`**:
  - In `App.tsx`, `WorktreeDeleteDialog` was mounted with `workspaceId={activeProject.workspaceId}`.
  - When the user attempted to delete a worktree belonging to an inactive project, `WorktreeDeleteDialog` queried deletion preview using the active project's ID.
  - The backend could not find the worktree under the active project's path and returned `WORKTREE_IDENTITY_NOT_FOUND`.
  - Consequently, deletion preview failed (`preview === null`), leaving the confirmation button disabled.

### C. Worktree Deletion on Dirty / Untracked Changes
- **Backend Rejection Even with Destructive Deletion**:
  - In `src-tauri/src/worktree/manager.rs`, `delete_worktree_and_branch_inner` passed `allow_dirty = false` to `remove_worktree_locked`, ignoring `destructive = true`.
  - `remove_worktree_locked` checked `if dirty_state.is_dirty` unconditionally and threw `WorktreeError::DirtyWorktree`, preventing `git worktree remove --force` from ever running.
- **Frontend Dialog Lacked Dirty Worktree Destructive Mode**:
  - `WorktreeDeleteDialog.tsx` only offered destructive deletion for `UNMERGED_BRANCH`, treating `DIRTY_WORKTREE` as an unrecoverable failure.

---

## 3. Implementation Details

### A. Workspace Removal (`Sidebar.tsx`, `ProjectDialogs.tsx`, `App.tsx`, `tauri.ts`)
1. **`ProjectHeader` UI Enhancements**:
   - Added right-click context menu to `ProjectHeader` with:
     - `Add Worktree` (disabled if `gitRoot === null`)
     - `Reveal in Finder / Show in File Explorer / Open in File Manager`
     - `Copy Project Path`
     - Separator
     - `Remove Project` (`Trash2` icon, destructive)
   - Added hover trash icon button (`IconButton`) to `ProjectHeader` next to `FolderPlus`.
2. **`RemoveProjectDialog`**:
   - Created modal in `ProjectDialogs.tsx` displaying the project ID and path, clarifying that repository files on disk will not be deleted.
3. **`App.tsx` Integration**:
   - Added `handleConfirmRemoveProject` which:
     - Removes project from `projects` state and updates `PROJECTS_STORAGE_KEY`.
     - Switches active project to the next available project if the active project was removed.
     - Clears snapshot cache and HMR state via `clearWorkspaceSnapshot` and `clearHmrWorkspaceState`.
     - Calls `unregisterProject` IPC command to clean up backend state.

### B. Worktree Deletion Routing & Fallback (`App.tsx`, `WorktreeList.tsx`)
1. **Dynamic `workspaceId` Resolution**:
   - Changed `WorktreeDeleteDialog` in `App.tsx` to pass:
     `workspaceId={resolveWorktreeOwnerId(deleteTarget, projects, activeProject.workspaceId) ?? activeProject.workspaceId}`
2. **Active Worktree Fallback**:
   - When deleting the currently active worktree, automatically falls back to selecting the project's primary worktree.
3. **Worktree Context Menu**:
   - Added context menu to `WorktreeRow` in `WorktreeList.tsx` with:
     - `Reveal in Finder / File Explorer`
     - `Copy Worktree Path`
     - Separator
     - `Delete Worktree` (disabled for primary/root worktrees).

### C. Dirty Worktree Destructive Deletion (`manager.rs`, `WorktreeDeleteDialog.tsx`)
1. **Rust Worktree Engine**:
   - In `remove_worktree_locked`: changed dirty check to `if !force && dirty_state.is_dirty`. When `force` is true, allows git worktree force removal.
   - In `delete_worktree_and_branch_inner`: passed `destructive` to `remove_worktree_locked(worktree_path, destructive)`.
   - In `remove_worktree`: kept standard safe guard rejecting dirty worktrees.
2. **Dialog UI**:
   - In `WorktreeDeleteDialog.tsx`: enabled `destructiveRequired` when `error.code === "DIRTY_WORKTREE"`.
   - Displays clear warning that uncommitted/untracked files will be discarded permanently, with explicit destructive delete button.

---

## 4. Verification

1. **Frontend Unit & Integration Tests**:
   - `ui/src/components/Sidebar.test.tsx` (30 passed)
   - `ui/src/components/WorktreeList.test.tsx` (14 passed)
   - `ui/src/components/ProjectDialogs.test.tsx` (16 passed)
   - `ui/src/components/WorktreeDeleteDialog.test.tsx` (6 passed)
   - `ui/src/App.test.tsx` (86 passed)
   - Total: 152 passed in 16.29s.

2. **Backend Unit & Integration Tests**:
   - `cargo test --lib --manifest-path src-tauri/Cargo.toml worktree` (26 passed in 1.63s).
   - Confirmed safe deletion rejects dirty worktrees, and destructive deletion successfully deletes dirty worktrees and branches.
