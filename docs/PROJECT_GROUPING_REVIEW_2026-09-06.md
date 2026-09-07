# Project Grouping & Remote Worktree Unification — Gate Review Report

**Date:** 2026-09-06  
**Status:** APPROVED  
**Reviewer:** `omo-senpi-gate-reviewer` (mahoquot/gemini-3.8-flash-high)  
**Confidence:** HIGH  

---

## 1. Executive Summary

Remote SSH repositories matching an existing project (by Git remote origin URL or folder name) are unified as a remote worktree within that project rather than creating a separate top-level project section in the sidebar.

An initial gate review identified 4 blocking issues related to active state persistence, worktree coexistence, host label visibility, and path collision highlighting. All 4 blockers have been remediated with focused code updates and verified with targeted regression tests. The final gate review confirmed full resolution with zero remaining blockers.

---

## 2. Gate Review Verification

### Blocker 1: Git Remote URL Persistence Across Restarts
- **Issue:** `loadProjects()` in `ui/src/App.tsx` dropped `gitRemote` when hydrating stored projects from `localStorage`, causing URL-based grouping to break on application refresh or restart.
- **Resolution:** Preserved `gitRemote` in `loadProjects()` and added `candidate.gitRemote === registered.gitRemote` check in `App.tsx:827`.
- **Status:** RESOLVED

### Blocker 2: Local Worktree Preservation When Remote Worktree is Active
- **Issue:** `isProjectGroupActive` previously caused `inactiveProjectWorktrees[primaryId]` to be emptied when a remote member became active, making local worktrees vanish from the sidebar. Additionally, initial collapse seeding collapsed the active project group.
- **Resolution:** Updated `Sidebar.tsx:697-703` so `listed` takes `inactiveProjectWorktrees?.[primaryId]` whenever `activeProjectId !== primaryId`, and updated `seedCollapsedProjects` to evaluate collapse state per project group (`isProjectGroupActive`).
- **Status:** RESOLVED

### Blocker 3: Host Label Visibility on Active Remote Worktree
- **Issue:** `App.tsx` constructed `plainRootWorktree` without `hostLabel`, causing `WorktreeList.tsx` to lose `[SSH: hostLabel]` and fall back to the folder name or "main" while active.
- **Resolution:** Attached the resolved `hostLabel` from `getCachedSshHosts` to `plainRootWorktree` and `listVisibleWorktrees` in `App.tsx`, and ensured `Sidebar.tsx` backfills `hostLabel` on remote worktree rows.
- **Status:** RESOLVED

### Blocker 4: Accurate Active Row Highlighting Without Path Collisions
- **Issue:** `WorktreeList.tsx` matched active rows using `worktree.path === activePath` without verifying `workspaceId`, causing dual-active highlights when local and remote worktrees shared the same filesystem path.
- **Resolution:** Passed `activeWorkspaceId={activeProjectId}` to `WorktreeList` and resolved `effectiveWorkspaceId = worktree.workspaceId ?? sortableWorkspaceId`. Qualified active row matching with `(!activeWorkspaceId || !effectiveWorkspaceId || effectiveWorkspaceId === activeWorkspaceId)`, and deduplicated rows by both path and workspace ID.
- **Status:** RESOLVED

---

## 3. Verification Evidence

- **Frontend Unit & Component Tests:** 111 tests passed across 7 test files (`projectGrouping.test.ts`, `Sidebar.remote.test.tsx`, `App.remote.test.tsx`, `Sidebar.test.tsx`, `Sidebar.dnd.test.tsx`, `WorktreeList.test.tsx`, `ProjectDialogs.test.tsx`).
- **Backend Rust Tests:** 36 tests passed (`ssh::direct_tests` and `ipc::project_remote`).
- **Frontend Production Build:** `bun run --cwd ui build` succeeded in 2.12s.
- **Backend Compilation:** `cargo check --manifest-path src-tauri/Cargo.toml` passed with 0 errors.
