# WORKSPACE_NOT_FOUND: Workspace 'default' is not registered — Root Cause & Resolution

## Problem Summary
When starting Ferryx or pairing with a machine, the application presented a blank main view with:
```
Workspace unavailable (WORKSPACE_NOT_FOUND)
```
and the backend error:
```json
{
  "code": "WORKSPACE_NOT_FOUND",
  "details": { "workspaceId": "default" },
  "message": "Workspace 'default' is not registered"
}
```
In the sidebar, an un-openable phantom project `default` with worktree `primary` appeared.

## Root Cause Analysis

1. **Launch from Root (`/`)**:
   - When macOS launches a GUI application bundle (`Ferryx.app`) via Finder, Dock, Spotlight, or `open`, macOS LaunchServices sets the process working directory (`cwd`) to the filesystem root `/`.
   - In `src-tauri/src/ipc/project.rs`, `initial_project(&registry)` checked `canonical.parent().is_none()` on `cwd`.
   - Because `/` has no parent, `initial_project` returned an error (`"filesystem root cannot be registered as a startup workspace"`).
   - `cmd_project_initial` failed as a result.

2. **Frontend Fallback to Legacy Phantom `'default'` Project**:
   - In `ui/src/App.tsx`, when `cmd_project_initial` failed, `.catch` merged `storedBootstrap` with `DEFAULT_PROJECT` (`{ workspaceId: "default", repoRoot: ".", gitRoot: null }`).
   - If the user had no existing projects stored in `localStorage`, the frontend adopted `{ workspaceId: "default", repoRoot: "." }` as the active project.
   - The sidebar rendered `v default` -> `. primary`.
   - Any tab opening or worktree click dispatched `cmd_terminal_spawn` with `workspaceId: "default"`.

3. **Backend Strictly Disallows `'default'`**:
   - The backend intentionally never registers `"default"` (it was deprecated as a hardcoded startup alias).
   - When the frontend sent `cmd_terminal_spawn` for `"default"`, the backend threw `WorktreeError::WorkspaceNotFound { workspace_id: "default" }`.
   - `WorkspaceApp` caught this error and rendered `Workspace unavailable (WORKSPACE_NOT_FOUND)`.

4. **Headless Daemon / Remote CLI Pairing**:
   - When running standalone headless daemon or `ferryx-cli pair generate`, `DaemonServer` did not register an initial project into its `WorkspaceRegistry`, leaving it empty.

## Changes Implemented

1. **Root Directory Fallback to User Home Directory** (`src-tauri/src/ipc/project.rs`):
   - Added `initial_project_from_path`.
   - When `canonical.parent().is_none()` (launched from `/`), it now resolves the user's home directory (`$HOME` / `%USERPROFILE%`).
   - Registers the home directory as a valid startup workspace (e.g. `/Users/<username>` with ID `<username>`).
   - GUI launches from Finder/Dock now open seamlessly in the home directory, matching the behavior of standard terminal applications.

2. **Headless Daemon Startup Registration** (`src-tauri/src/cli.rs`):
   - In `run_daemon_headless`, the daemon now registers its initial workspace upon startup so remote clients can spawn sessions in a registered workspace.

3. **Frontend Genuine Empty State on Bootstrap Rejection** (`ui/src/App.tsx`):
   - In `App.tsx`'s `.catch`, if `cmd_project_initial` rejects and there are no real user projects in `localStorage` or recovered session, the app boots into a genuine empty state (`projects: []`, `activeProjectId: ""`).
   - Shows `no-projects-view` ("No projects - Add a project to open a terminal workspace.") and "No projects registered yet." in the sidebar instead of a broken phantom `"default"` workspace.
   - Safe guards prevent dispatching terminal spawn operations when `projects.length === 0`.

4. **Regression Verification**:
   - Rust test `initial_project_from_root_falls_back_to_home_dir` in `tests/rorca_native_contract.rs`.
   - Frontend Vitest test `renders genuine empty state and does not create phantom default workspace when getInitialProject rejects without stored projects` in `ui/src/App.test.tsx`.
