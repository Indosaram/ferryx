# macOS TCC Documents Folder Access Alert: Root Cause Analysis & Fix

**Date:** 2026-09-04  
**Trigger:** "“Ferryx” would like to access files in your Documents folder."  
**TCC Service:** `kTCCServiceSystemPolicyDocumentsFolder`

---

## 1. Executive Summary

When Ferryx was running, macOS presented a TCC security dialog requesting access to the user's `Documents` folder (`kTCCServiceSystemPolicyDocumentsFolder`), followed by prompts for other protected user folders (Downloads, Photos, Desktop).

Through dissection of the macOS Unified Log (`tccd`, `kernel`), Ferryx state databases, and agent session transcripts, two interrelated root causes were identified:
1. An agent sub-process ran `find /` inside a terminal session spawned by Ferryx, which systematically traversed the root filesystem starting from `/` and entering `/Users/indo/Documents`.
2. Ferryx itself had an invalid workspace registered with `repoRoot: "/"` and `workspaceId: "project"`, causing its background DAG watcher to attach an FSEvents recursive file watcher (`watcher.watch("/", RecursiveMode::Recursive)`) to the entire macOS root filesystem and scan the root directory.

Both the active state and all code paths allowing filesystem root registration and recursive root watching have been hardened and tested.

---

## 2. Root Cause Investigation

### 2.1 Unified Log Trace
Querying macOS Unified Log at the time of the alert (14:13:36 to 15:44:21 KST) revealed:
- Process `/usr/bin/find` (PID 19605) ran for 1 hour 30 minutes, responsible process attributed by `tccd` to `com.ferryx.app` (PID 36197).
- Traversal order walked directories alphabetically: `/Library` -> `/System` -> `/Users/indo/Library` -> `/Users/indo/Documents` -> `/Users/indo/Downloads`.
- As soon as `find` crossed into `/Users/indo/Documents`, `tccd` intercepted the `getattrlistbulk` system call and triggered `kTCCServiceSystemPolicyDocumentsFolder`.

### 2.2 Agent Session Transcript Trace
Inspection of `~/.omo/sessions/` transcripts revealed that an AI coding agent operating inside a terminal under Ferryx was caught in a tool-denial retry loop and issued `find /` in a blind attempt to locate files across the system.

### 2.3 Ferryx Workspace and DAG Watcher Root Cause
In Ferryx:
- `~/Library/Application Support/com.ferryx.app/session_state.json` and WebKit LocalStorage (`localstorage.sqlite3`) contained an entry:
  ```json
  {"workspaceId": "project", "repoRoot": "/", "activeWorktreePath": "/"}
  ```
- In `ui/src/App.tsx`, Ferryx bootstrapped all registered projects and initiated DAG run tracking for each project root:
  ```typescript
  void watchDagProject(project.repoRoot); // Invoked watchDagProject("/")
  ```
- In `src-tauri/src/dag/watcher.rs`:
  ```rust
  let watch_target = if root.join(".omo/senpi-task/dag").exists() {
      root.join(".omo/senpi-task/dag")
  } else if ... || root.exists() {
      root.clone() // Resolved to "/"!
  } ...
  watcher.watch(&watch_target, RecursiveMode::Recursive); // Watched entire "/" filesystem!
  ```
  And `scan_and_emit` attempted to read the root directory:
  ```rust
  let target_dir = if runs_dir.is_dir() { &runs_dir } else { root };
  std::fs::read_dir(target_dir); // Read entries of "/"
  ```
- In `src-tauri/src/worktree/manager.rs`, `WorktreeManager::try_new` did not reject `canonical.parent().is_none()`, permitting `"/"` to be treated as a valid folder workspace.
- In `ui/src/components/ProjectDialogs.tsx`, `deriveWorkspaceId("/")` stripped slashes, yielded an empty string, and defaulted to `"project"`.

---

## 3. Fixes Implemented

### 3.1 Backend: DAG Watcher Scoping & Root Rejection (`src-tauri/src/dag/watcher.rs`)
- `run_watcher_loop` immediately exits if `root.parent().is_none()` (filesystem root).
- `watch_target` is strictly restricted to `runs_dir` (`NonRecursive`), `.omo/senpi-task/dag` (`Recursive`), or `.omo` (`Recursive`). It **never** watches the workspace root recursively.
- `scan_and_emit` only scans `runs_dir` if it is a directory; it no longer falls back to reading `root`.
- Tests updated to canonicalize temporary paths to ensure macOS FSEvents compatibility.

### 3.2 Backend: IPC DAG Root Guard (`src-tauri/src/ipc/dag.rs`)
- `dag_watch_project` checks `Path::new(&canonical).parent().is_none()` and immediately returns `IpcErrorCode::InvalidRepoRoot`.
- Added unit test `test_dag_watch_project_rejects_filesystem_root`.

### 3.3 Backend: Worktree Manager Root Guard (`src-tauri/src/worktree/manager.rs`)
- `WorktreeManager::try_new` validates `!canonical.is_dir() || canonical.parent().is_none()`, returning `WorktreeError::InvalidRepoRoot`.
- Added unit test `worktree::tests::filesystem_root_is_rejected`.

### 3.4 Frontend: Filesystem Root Validation (`ui/src/components/ProjectDialogs.tsx`)
- Exported `isFilesystemRoot(folderPath: string)` checking for `/` and Windows drive roots (`C:`, `C:\`).
- `AddProjectDialog` validates both native picker selection and manual input against `isFilesystemRoot` and displays an error message without submitting.
- Added unit tests in `ProjectDialogs.test.tsx`.

### 3.5 Frontend: Project Bootstrap Filtering (`ui/src/App.tsx`)
- `loadProjects()` filters out entries where `isFilesystemRoot(project.repoRoot)` is true.
- `recoverProjectBootstrap()` ignores workspace entries where `isFilesystemRoot(workspace.repoRoot)` is true.
- `dagWatchedPathsRef` loop explicitly skips any path satisfying `isFilesystemRoot(path)`.
- Added test in `App.test.tsx` verifying `"/"` is filtered out during app bootstrap.

### 3.6 Active State Remediation
- Removed `"project": "/"` from `/Users/indo/Library/Application Support/com.ferryx.app/session_state.json`.
- Removed `"project": "/"` from WebKit LocalStorage (`ItemTable.ferryx.projects` in `localstorage.sqlite3`).

---

## 4. Verification Results

- `cargo test --manifest-path src-tauri/Cargo.toml --lib dag`: 12 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib worktree`: 27 passed, 0 failed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: Clean compilation (0 errors).
- `bun run --cwd ui test src/components/ProjectDialogs.test.tsx src/App.test.tsx`: 102 passed, 0 failed.
- `bun run --cwd ui build`: TypeScript typecheck and Vite build succeeded with 0 errors.
