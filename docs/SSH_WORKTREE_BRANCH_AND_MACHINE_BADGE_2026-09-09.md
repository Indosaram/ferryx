# SSH Worktree Branch Resolution and Machine Badge Report

- **Date:** 2026-09-09
- **Author:** OMO / Senpi
- **Consultant / Reviewer:** `(opencodex) gpt-6-astra:high`
- **Scope:** Remote worktree branch name resolution instead of folder names, removal of hardcoded `SSH` badge, truncated machine nickname/connection badge display on the right with full hover tooltip.

---

## 1. Problem Statement

1. **Folder names displayed instead of Git branch names:**
   In remote worktrees / workspaces, the UI displayed folder basenames instead of actual Git branch names (e.g. `repo` or directory path instead of `main`, `feature/xyz`).
2. **Hardcoded `SSH` label:**
   Remote worktrees showed a generic `<span ...>SSH</span>` badge. The requirement was to remove `SSH`, show branch/worktree name on the left, and show a machine nickname or connection summary on the right.
3. **Machine address truncation:**
   Full machine addresses / hostnames must not overflow or push UI elements; they must be truncated at an appropriate width (`max-w-[88px] truncate`) while providing full details on hover via `title`.

---

## 2. Architecture & Implementation

### A. Backend Remote Probe (`src-tauri`)
- `src-tauri/src/ssh/operations.rs`:
  - Extended probe script from 4 fields to 6 fields:
    1. `repo_root`
    2. `git_root`
    3. `git_remote`
    4. `git_common_dir`
    5. `git_branch` (`git branch --show-current`, falling back to `git symbolic-ref --quiet --short HEAD`)
    6. `git_head` (`git rev-parse --verify HEAD`)
  - Implemented both POSIX shell and Windows PowerShell (`Invoke-FerryxGit`) variants with carriage return stripping (`\r\n`).
- `src-tauri/src/ssh/projects.rs`:
  - Extended `RemoteProject` with `git_branch: Option<String>` and `git_head: Option<String>`.
- `src-tauri/src/ipc/project_remote.rs`:
  - Extended `RegisteredRemoteProject` to serialize `git_branch` and `git_head`.
  - Updated `register_remote_project` command handler to propagate probe metadata.

### B. Frontend Metadata Propagation (`ui/src/lib`)
- `ui/src/lib/types.ts` & `ui/src/lib/tauri.ts`:
  - Extended `RegisteredProject`, `Worktree`, and `PersistedWorkspace` interfaces with `gitBranch?: string | null`, `gitHead?: string | null`, and `hostLabel?: string | null`.
- `ui/src/lib/remoteProject.ts`:
  - `toRegisteredProject()` preserves `remote.hostLabel` and normalizes absent branch/head to explicit `null` to ensure detached transitions are authoritative.
- `ui/src/lib/projectIdentity.ts`:
  - `projectRootWorktree()` synthesizes root worktree rows with `branch`, `head`, and `detached` (`detached: Boolean(!project.gitBranch && project.gitHead)`).
- `ui/src/lib/branchFilter.ts`:
  - Scoped legacy folder name hiding (`ferryx`, `rorca`, `orca-lite`) strictly to local worktrees, while remote repositories preserve their genuine branch names.
  - Added Windows slash/backslash path splitting and explicit `detached HEAD` handling.
- `ui/src/App.tsx`:
  - Registration metadata equality includes `gitBranch`, `gitHead`, and `hostLabel` to trigger state updates when branch changes remotely.

### C. UI Rendering & Badge Layout (`ui/src/components`)
- `ui/src/components/WorktreeList.tsx`:
  - Replaced hardcoded `SSH` badge with machine nickname badge on the right.
  - Badge styled with `max-w-[88px] shrink-0 truncate rounded px-1.5 py-0.5 text-[10px] font-medium bg-muted/60 text-muted-foreground`.
  - Added full tooltip via `title` attribute displaying machine label and remote connection details.
  - Left-aligned branch/worktree name using `displayWorkspaceTitle(worktree)`.
- `ui/src/components/Sidebar.tsx`:
  - Standalone remote workspace button displays branch name on the left and machine badge on the right.
  - Prioritized live host inventory nickname (`hosts.find`), then persisted `hostLabel`, then `hostId`.
  - Subscribed to `useSshHosts()` to ensure immediate reactivity on host renames.

---

## 3. Code Review with `(opencodex) gpt-6-astra:high`

### Iteration 1: `CHANGES_REQUESTED`
1. **[P1] Missing branch/HEAD values could not clear cached metadata:**
   - *Finding:* Omitting `gitBranch` or leaving it `undefined` preserved stale branches when detaching remotely.
   - *Resolution:* Made `toRegisteredProject()` emit explicit `null`, and updated sidebar reconciliation and serialization to treat `null` as cleared.
2. **[P2] Machine nickname discarded and fallback preferred host IDs:**
   - *Finding:* `toRegisteredProject()` omitted `remote.hostLabel`, and fallback order in Sidebar used `hostId` before `project.hostLabel`.
   - *Resolution:* Copied `remote.hostLabel` in adapter and standardized fallback order: `inventory.label ?? project.hostLabel ?? hostId`.
3. **[P2] Valid branch names displayed as `main`:**
   - *Finding:* `displayWorkspaceTitle` replaced branches named `ferryx`, `rorca`, or `orca-lite` with `main`.
   - *Resolution:* Scoped legacy folder hiding strictly to local worktrees (`!worktree.workspaceId?.startsWith("ssh:")`), preserving remote branches verbatim.

### Final Verdict: `APPROVED`
> "The remaining blocker is resolved for the remote-worktree feature... All three findings are closed:
> 1. Authoritative null propagation clears stale branch/HEAD metadata.
> 2. Machine nicknames survive registration and are used when live inventory is unavailable.
> 3. Remote branch names no longer undergo legacy substitutions."

---

## 4. Verification

- **Frontend Vitest Suites:**
  - `src/components/Sidebar.remote.test.tsx` (10 tests)
  - `src/components/WorktreeList.test.tsx` (17 tests)
  - `src/components/Sidebar.projectIdentity.test.tsx` (2 tests)
  - `src/lib/sessionPersistence.test.ts` (28 tests)
  - `src/lib/projectGrouping.test.ts` (19 tests)
  - `src/lib/remoteProject.test.ts` (7 tests)
  - `src/lib/branchFilter.test.ts` (20 tests)
  - **Result:** 103 passed / 103 tests in 7 test files.
- **Frontend Production Build:**
  - `bun run --cwd ui build` passed in 2.07s without warnings or errors.
- **Rust Backend Checks:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`: Clean.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib ssh::`: 62 passed.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib project_remote`: 4 passed.
