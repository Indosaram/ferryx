# Ferryx Implementation Plan: Direct SSH Remote Project Integration & Location Chooser

**Date:** 2026-09-06  
**Document:** `docs/PROJECT_LOCATION_PLAN_2026-09-06.md`  
**Status:** Implemented and code-reviewed; native desktop acceptance pending  
**Inputs Reviewed:** `docs/evidence/project-location/discovery-ui.md`, `docs/evidence/project-location/discovery-ssh.md`, `AGENTS.md`  

## Execution corrections from lead review

- The direct SSH design is accepted; the previous metadata-only draft is rejected.
- Final implementation reconciliation: `ipc/project_remote.rs` owns registration;
  `ssh/projects.rs` persists host-qualified `ssh:<hash>` records, and the daemon
  uses typed `RemoteSsh { host_store_path }` routing. There are no local placeholder
  directories, arbitrary-program startup fields, or remote helper installation.
  These implemented choices supersede the older illustrative sketches below.
  Current evidence and limitations are in `PROJECT_LOCATION_REPORT_2026-09-06.md`.
- Examples in this plan are interface sketches, not permission to introduce
  arbitrary-program startup or use nonexistent error variants. Use a typed SSH
  startup payload and existing structured error codes, validating at IPC boundaries.
- A placeholder local directory is internal bookkeeping only. Remote identity
  must include host ID and remote canonical path, deduplicate without colliding
  with local workspaces, persist across app restart, and never become a local
  terminal or filesystem fallback. All remote operations must resolve the enabled
  persisted host, including new/split/restored terminals. Local directory helpers,
  status, branch deletion, reveal and worktree mutations must fail closed for SSH.
- Remote registration must obtain canonical remote directory via the remote
  shell, not merely trust the input path. Registration failure must leave no
  project. Host/path quoting, options and bounded subprocess termination are
  backend acceptance requirements. No helper/agent installation is allowed.
- Tests must fail on an existing behavioral seam before production changes:
  current SSH argv builders for the backend and current AddProjectDialog for the
  chooser. Missing imports or unimplemented symbols do not constitute RED.
- The backend lane owns Rust integration and tests; a later UI lane owns both
  remoteProject.ts and chooser tests, avoiding a code-only adapter lane. Runtime
  App/state integration is separate with disjoint files. Each phase ends with an
  independent verifier; a final single code reviewer follows integrated QA.
- Desktop proof remains an open required gate, not covered by a mock or build.

---

## 1. Scope Boundaries & Foundation Integration

### 1.1 Foundation Lane Boundary (External DAG `dag_6082b919-1a5d-4145-9489-015358f731fa`)
An independent accepted foundation lane owns and implements:
- `ui/src/lib/sshHosts.ts`:
  - Authoritative `SshHost` TypeScript DTO matching backend Rust `src-tauri/src/ssh/mod.rs:9-24`.
  - Typed IPC bridges: `listSshHosts`, `importSshConfig`, `updateSshHost`, `deleteSshHost`, `testSshConnection`.
  - Shared reactive subscription hook: `useSshHosts()`.
- `ui/src/components/settings/types.ts`: Adds `"ssh"` to `SectionId`.
- `ui/src/components/SettingsDialog.tsx`: Adds navigation button `"ssh"` labeled **"SSH Machines"**.
- `ui/src/components/settings/SshSection.tsx`: Complete SSH machine management UI (host list, reachability status, CRUD, and OpenSSH config import).

**Boundary Rule**: This plan **consumes** `ui/src/lib/sshHosts.ts` and the `"ssh"` Settings section. It does **not** duplicate or modify their host store, rewrite their types, or re-implement Settings machine management.

### 1.2 This Plan's Assigned Execution Scope
This plan specifies the remaining backend remote registration and frontend chooser/runtime integration:
1. **Backend Remote Registration (`src-tauri/src/ipc/project.rs`)**:
   - Implements typed command `cmd_project_register_remote` (registered in `src-tauri/src/lib.rs`).
   - Resolves persisted enabled host server-side from `ssh_hosts.json`.
   - Safely quotes remote path (POSIX single-quote escaping); never canonicalizes remote path locally.
   - Executes remote SSH directory probe (`test -d '<quoted_path>'`) honoring configured `port`, `identity_file`, `jump_host`.
   - Binds local placeholder workspace identity in `WorkspaceRegistry` and Tokio daemon.
2. **Backend Direct SSH Terminal Spawning (`src-tauri/src/ipc/terminal.rs`, `src-tauri/src/daemon/protocol.rs`, `src-tauri/src/terminal/shell.rs`)**:
   - Per user decision D5: **No remote helper or agent installation**. Direct interactive SSH shell only (`ssh -tt`).
   - Extends `TerminalStartup::RemoteSsh { program: String, args: Vec<String> }` in daemon protocol.
   - In `cmd_terminal_spawn`: intercepts remote workspaces, resolves host options, constructs direct SSH command plan with remote `cd '<quoted_path>' && exec $SHELL -l`, and spawns inside local `portable-pty` daemon.
3. **Frontend Project Location Chooser (`ui/src/components/ProjectDialogs.tsx`)**:
   - Initial mount renders **Location Chooser** (Local vs Remote choice cards). OS folder picker is **NOT called on mount**.
   - Local card triggers folder picker via existing `registerProject`.
   - Remote card consumes `useSshHosts()` from `../lib/sshHosts`.
     - 0 active hosts: renders guidance, disabled submit, and CTA button "Configure SSH Machines in Settings" (`data-testid="configure-ssh-settings"`) calling `onOpenSettings?.("ssh")` and `onClose()`.
     - Populated hosts: host selector dropdown, remote path input, auto-derived workspace ID, and submit button calling `registerRemoteProject`.
4. **Frontend Runtime & Target Routing (`ui/src/App.tsx`, `ui/src/state/inactiveProjectWorktrees.ts`)**:
   - Preserves `target?: RunTarget` in `App.tsx:loadProjects()`.
   - Guards `App.tsx` active project registration effect against remote paths.
   - Guards `inactiveProjectWorktrees.ts` against local `registerProject` for remote projects; yields `plainRootWorktree(project)`.
   - Unsupported operations in v1 (Git worktree create/delete on remote projects) explicitly return structured `UNSUPPORTED` errors.
5. **Foreign Dirty Working-Tree Preservation**:
   - Unrelated permission edits in `ui/src/lib/tauri.ts` and `ui/src/lib/types.ts` (`canOpenSettings`, `PermissionStatus`) are preserved untouched; narrow additions are layered cleanly.
6. **Desktop QA Status**:
   - Desktop automation remains **unavailable** (due to macOS Accessibility / System Events permissions and Orca background lifecycle). It is **not approved** as verified acceptance evidence; component integration tests and backend contract tests form the authoritative verification baseline.

---

## 2. Direct SSH Remote Architecture & Data Flow

```
[ Frontend: Sidebar "+" / Empty Workspace ]
                     │
                     ▼
           [ AddProjectDialog ] ◄── (role="dialog" masks WGPU terminal)
                     │
         Choose Location View
           ├─ Local  ──► [ local-pending ] ──► Native open() ──► cmd_project_register (Local)
           │
           └─ Remote ──► Query useSshHosts() from ui/src/lib/sshHosts.ts
                              │
                    Configured Active Hosts?
                      ├─ No  ──► Empty state + CTA link to Settings "SSH Machines"
                      └─ Yes ──► Select Host, enter Remote Repo Path, derived ID
                                       │
                                       ▼
                       [ cmd_project_register_remote ] (Rust Backend)
                                       │
                    ┌──────────────────┴──────────────────┐
                    ▼                                     ▼
           Lookup host in ssh_hosts.json          Remote SSH Probe
           (Validate ID & enabled)                (test -d '<quoted_path>')
                    │                                     │
                    └──────────────────┬──────────────────┘
                                       │
                                       ▼
                   Create Local Placeholder Dir in App Data
                   Register with WorkspaceRegistry & Daemon
                   Return RegisteredRemoteProject DTO
                                       │
                                       ▼
                     Frontend Persists to ferryx.projects
                   { workspaceId, repoRoot, target: { kind: "ssh", hostId } }
                                       │
                     ┌─────────────────┴─────────────────┐
                     ▼                                   ▼
             [ Active Workspace ]              [ Inactive Worktrees ]
             (App.tsx:763)                     (inactiveProjectWorktrees.ts)
                     │                                   │
             Target === "ssh"?                 Target === "ssh"?
             ├─ Skip local registerProject     ├─ Skip local registerProject
             └─ Spawn Remote Terminal          └─ Yield plainRootWorktree(project)
                     │
                     ▼
          [ cmd_terminal_spawn ] (Rust Backend)
                     │
          Target is Remote SSH?
          ├─ Resolve host credentials from ssh_hosts.json
          ├─ Build Command: ssh -tt [-p port] [-i key] [-J jump] [user@]host "cd '<path>' && exec $SHELL -l"
          ├─ Set TerminalStartup::RemoteSsh { program, args }
          └─ Daemon PTY Manager spawns ssh in local portable-pty
                     │
                     ▼
          Interactive Remote Shell Connected!
          (PTY output -> Ring buffer -> 20-byte framing -> xterm.js / Ghostty)
```

---

## 3. Backend Implementation Contracts (`src-tauri`)

### 3.1 Remote Project Registration Command (`src-tauri/src/ipc/project.rs`)

Add Tauri IPC command `cmd_project_register_remote` to `src-tauri/src/ipc/project.rs` and register it in `src-tauri/src/lib.rs:848`.

#### Request & Response Models
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRemoteProjectRequest {
    pub workspace_id: String,
    pub host_id: String,
    pub repo_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRemoteProject {
    pub workspace_id: String,
    pub repo_root: String,
    pub git_root: Option<String>,
    pub host_id: String,
    pub host_label: String,
}
```

#### Server-Side Validation & Execution Sequence
1. **Server-Side Host Resolution**:
   - Resolves `host_id` from `{app_data_dir}/ssh_hosts.json` using `get_ssh_store_path(&app)` and `load_store`.
   - If `host_id` does not exist: returns `IpcError::new(IpcErrorCode::NotFound, format!("SSH host '{host_id}' not found"))`.
   - If `host.disabled == Some(true)`: returns `IpcError::new(IpcErrorCode::Forbidden, format!("SSH host '{}' is disabled", host.label))`.
2. **Safe Remote Path Quoting**:
   - Escapes the remote path using POSIX single-quote escaping:
     ```rust
     pub fn quote_posix_path(path: &str) -> String {
         format!("'{}'", path.replace('\'', "'\\''"))
     }
     ```
   - **Never** calls `fs::canonicalize` or checks the remote path on the local host filesystem.
3. **Remote SSH Directory & Git Probe**:
   - Builds probe arguments using `crate::ssh::exec::interactive_argv(&host)` with probe command:
     `format!("test -d {quoted} && (git -C {quoted} rev-parse --show-toplevel 2>/dev/null || echo '__PLAIN__')")`
   - Executes via `run_blocking` with a bounded timeout (8.0 seconds).
   - If connection fails or directory does not exist: returns `IpcError::new(IpcErrorCode::InvalidPath, format!("Remote path validation failed: {}", stderr_line))`.
   - If output is a path: sets `git_root = Some(output.trim().to_string())`. If output is `__PLAIN__`: sets `git_root = None`.
4. **Workspace Identity Binding**:
   - Creates a deterministic local placeholder directory: `{app_data_dir}/remote_workspaces/{workspace_id}`.
   - Registers directory in `WorkspaceRegistry` via `registry.register(&workspace_id, &placeholder_dir)`.
   - Registers workspace with Tokio daemon via `daemon_client.register_workspace(&workspace_id, &placeholder_dir.to_string_lossy())`.
   - Records mapping in `RemoteWorkspaceStore`: `{ workspace_id, host_id, remote_repo_path, git_root }`.
   - Returns `RegisteredRemoteProject`.

### 3.2 Direct SSH Terminal Spawning (`src-tauri`)

#### 1. Protocol Extension (`src-tauri/src/daemon/protocol.rs:48`)
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalStartup {
    #[serde(rename_all = "camelCase")]
    AgentResume {
        agent_type: String,
        provider_session: AgentProviderSession,
    },
    #[serde(rename_all = "camelCase")]
    RemoteSsh {
        program: String,
        args: Vec<String>,
    },
}
```

#### 2. Shell Plan Mapping (`src-tauri/src/terminal/shell.rs:388`)
In `resolve_startup_command_pure`:
```rust
match startup {
    Some(TerminalStartup::AgentResume { agent_type, provider_session }) => {
        resolve_agent_resume_plan(agent_type, provider_session)
    }
    Some(TerminalStartup::RemoteSsh { program, args }) => {
        Ok(ShellCommandPlan {
            program: program.clone(),
            args: args.clone(),
        })
    }
    None => Ok(resolve_shell_command_pure(
        preference,
        platform,
        is_executable_on_path,
        get_env,
    )),
}
```

#### 3. Terminal Spawn Remote Routing (`src-tauri/src/ipc/terminal.rs:596-670`)
In `cmd_terminal_spawn`:
- Inspects whether `request.workspace_id` is registered in `RemoteWorkspaceStore`.
- If remote:
  1. Resolves `host` from `ssh_hosts.json`. If host is missing or disabled: returns `IpcError::new(IpcErrorCode::NotFound, "Configured SSH host is unavailable")`.
  2. Builds direct SSH command:
     - Program: `"ssh"` (resolved on system PATH or `/usr/bin/ssh`).
     - Args: `interactive_argv(&host)` with remote command appended:
       `format!("cd {} && exec ${{SHELL:-/bin/sh}} -l", quote_posix_path(&remote_path))`
  3. Injects `request.startup = Some(TerminalStartup::RemoteSsh { program, args })`.
  4. Calls `daemon_client.spawn_terminal_with_startup(...)`.
  5. The daemon's `portable-pty` spawns local `ssh`. Ring buffer, 20-byte framing, xterm.js / Ghostty renderer, resize, and keyboard inputs operate identically to a local shell.
- If local:
  - Operates existing local path without alteration.

### 3.3 Unsupported Operations in v1
- **Git Worktrees**: `cmd_worktree_create` and `cmd_worktree_delete` require local Git repositories. In v1, invoking worktree operations on a remote workspace returns `IpcError::new(IpcErrorCode::Unsupported, "Worktree operations on remote SSH projects are not supported in v1")`.
- **Terminal Restore & Split**: Uses the remote workspace ID to spawn another SSH session into the remote host and directory. CWD inheritance across splits on remote machines defaults to the project `repo_root`.

---

## 4. Frontend Implementation Contracts (`ui`)

### 4.1 Remote Project IPC Client Module (`ui/src/lib/remoteProject.ts`)

Create a dedicated module `ui/src/lib/remoteProject.ts` to keep `ui/src/lib/tauri.ts` and `ui/src/lib/types.ts` untouched:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { RunTarget } from "./scopedContracts";

export interface RegisterRemoteProjectRequest {
  workspaceId: string;
  hostId: string;
  repoPath: string;
}

export interface RegisteredRemoteProject {
  workspaceId: string;
  repoRoot: string;
  gitRoot: string | null;
  hostId: string;
  hostLabel: string;
}

export async function registerRemoteProject(
  request: RegisterRemoteProjectRequest,
): Promise<RegisteredRemoteProject> {
  return invoke<RegisteredRemoteProject>("cmd_project_register_remote", { request });
}
```

### 4.2 Extended Project Model Layering
In `ui/src/lib/scopedContracts.ts:10` (existing):
`export type RunTarget = { readonly kind: "local" } | { readonly kind: "ssh"; readonly hostId: string };`

In `ui/src/lib/tauri.ts:36-40`:
Narrowly add `target?: RunTarget` to `RegisteredProject` without altering foreign permission code:
```ts
export type RegisteredProject = {
  workspaceId: string;
  repoRoot: string;
  gitRoot?: string | null;
  target?: RunTarget;
};
```

---

## 5. UI Component Specifications & State Machine

### 5.1 `AddProjectDialog` State Machine (`ui/src/components/ProjectDialogs.tsx`)

```ts
type AddProjectStep =
  | "choose-location"  // Initial view: Local vs Remote choice cards (NO folder picker!)
  | "local-pending"    // Waiting for OS folder picker (Tauri only)
  | "local-confirm"    // Local folder selected; confirms derived ID & registers
  | "local-manual"     // Web fallback manual path form
  | "remote-form";     // SSH host select + remote repo path
```

#### Step 1: `choose-location` (Initial Mount)
- Dialog container retains `[role="dialog"] aria-label="Add Project"` throughout to ensure native WGPU terminal visibility yields.
- Header: "Add Project", close button.
- Body:
  - **Local Project Card** (`data-testid="project-type-local"`):
    - Title: "Local Project"
    - Subtitle: "Choose a folder on this machine using the folder picker."
    - Icon: `FolderGit2`
    - Click: If Tauri, transitions to `"local-pending"` and calls `open({ directory: true, ... })`. If web, transitions to `"local-manual"`.
  - **Remote (SSH) Card** (`data-testid="project-type-remote"`):
    - Title: "Remote (SSH)"
    - Subtitle: "Connect to a repository hosted on an SSH machine."
    - Icon: `Radio`
    - Click: Transitions to `"remote-form"`.
- Footer: Cancel button (`data-testid="add-project-cancel"`).

#### Step 2: `local-pending` & `local-confirm`
- Operates existing local flow:
  - Picker cancelled (`null`): invokes `onClose()`.
  - Path selected: displays confirmation with derived ID (`deriveWorkspaceId`), calls `registerProject({ workspaceId, repoPath })` (`cmd_project_register`), fires `onRegistered`, closes dialog.

#### Step 3: `remote-form`
- Header: Back button (`data-testid="add-project-back"`, returns to `"choose-location"`), title "Add Remote Project".
- Body: Consumes `useSshHosts()` from `../lib/sshHosts`.
  - **Case A: 0 configured hosts or all hosts disabled**:
    - Message: "No SSH machines configured. Add an SSH machine in Settings before registering a remote project."
    - CTA button: "Configure SSH Machines in Settings" (`data-testid="configure-ssh-settings"`).
      Click invokes `onOpenSettings?.("ssh")` and `onClose()`.
    - Submit button disabled (`disabled={true}`).
  - **Case B: Configured active hosts available**:
    - Host dropdown (`data-testid="remote-host-select"`): Lists active hosts (`hosts.filter(h => !h.disabled)`).
    - Remote path input (`data-testid="remote-repo-path-input"`): Monospace text input for remote directory path (e.g. `/home/user/repo`).
    - Workspace ID input (`data-testid="remote-workspace-id-input"`): Auto-derived via `deriveWorkspaceId(remotePath)` as user types, editable.
    - Submit button (`data-testid="add-project-confirm-remote"`):
      Submits form, calls `registerRemoteProject({ workspaceId, hostId, repoPath })`.
      On success: calls `onRegistered({ workspaceId, repoRoot: result.repoRoot, gitRoot: result.gitRoot, target: { kind: "ssh", hostId } })` and closes dialog.
      On error: displays structured IPC error in dialog without dismissing.

### 5.2 Downstream Project Routing & Guarding

#### 1. Project Persistence (`ui/src/App.tsx:2159-2194`)
In `loadProjects()`:
- Parse and preserve `target?: RunTarget`.
- Missing target resolves to `{ kind: "local" }`.

#### 2. Active Project Daemon Registration Guard (`ui/src/App.tsx:763-868`)
- In `WorkspaceApp` active project registration effect:
  ```ts
  if (activeProject.target?.kind === "ssh") {
    // Remote projects are registered on the backend via cmd_project_register_remote;
    // bypass local filesystem canonicalization.
    return;
  }
  ```

#### 3. Inactive Worktree Refresh Guard (`ui/src/state/inactiveProjectWorktrees.ts:144`)
- In `useInactiveProjectWorktrees`:
  ```ts
  if (project.target?.kind === "ssh") {
    // Remote projects do not run local Git canonicalization; yield plain root worktree.
    return [project.workspaceId, [plainRootWorktree(project)]] as const;
  }
  ```

---

## 6. RED-First Test Specifications & Commands

### 6.1 Backend Rust Contract Tests (`src-tauri`)

| Test Target | Test Identifier | Verification Condition |
|---|---|---|
| `src-tauri/src/ipc/project.rs` | `red_remote_register_rejects_missing_host` | Invoking `cmd_project_register_remote` with non-existent `host_id` returns `IpcErrorCode::NotFound`. |
| `src-tauri/src/ipc/project.rs` | `red_remote_register_rejects_disabled_host` | Invoking with `disabled: true` host returns `IpcErrorCode::Forbidden`. |
| `src-tauri/src/ipc/project.rs` | `red_quote_posix_path_handles_quotes` | Verifies POSIX single-quote escaping (`/path/with 'quote'` -> `'/path/with '\''quote'\''`). |
| `src-tauri/src/terminal/shell.rs` | `red_terminal_startup_remote_ssh_plan` | `resolve_startup_command_pure` with `TerminalStartup::RemoteSsh { program: "ssh", args: [...] }` yields exact command plan. |

### 6.2 Frontend Vitest Tests (`ui`)

| Test Target | Test ID | Description & Assertion |
|---|---|---|
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-01` | Mount under Tauri: assert `dialog.open` NOT called; assert `project-type-local` and `project-type-remote` rendered. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-02` | Click Local card: assert `dialog.open` called with `{ directory: true, multiple: false, title: "Add Project" }`. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-03` | Local picker cancels: assert `onClose` called once, `onRegistered` not called. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-04` | Local picker selects path: assert confirmation view displays path; confirm click calls `registerProject`. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-05` | Click Remote with 0 hosts: assert empty message, disabled submit, and `configure-ssh-settings` CTA button. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-06` | Click `configure-ssh-settings`: assert `onOpenSettings("ssh")` invoked and `onClose` called. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-07` | Click Remote with mock hosts: select host, enter `/srv/repo`, submit: assert `registerRemoteProject` called; assert `onRegistered` receives project with `target: { kind: "ssh", hostId }`; assert `dialog.open` NOT called. |
| `ui/src/components/ProjectDialogs.test.tsx` | `T-PRJ-08` | Click "Back" button in remote form: assert view returns to location choice cards. |
| `ui/src/state/inactiveProjectWorktrees.test.tsx` | `T-INACT-01`| Inactive project with `target: { kind: "ssh", hostId: "h1" }`: asserts `registerProject` NOT called; yields plain root worktree. |

### 6.3 Executable Test Commands

```bash
# Frontend Vitest test suites
bun run --cwd ui test src/components/ProjectDialogs.test.tsx
bun run --cwd ui test src/state/inactiveProjectWorktrees.test.tsx

# Backend cargo check & tests
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::project
cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::shell
```

---

## 7. Disjoint Implementation Lanes & Execution DAG

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Lane 1: Backend Remote Registration & Terminal Startup (Rust)               │
│ [Category: Backend IPC / Terminal Engine]                                   │
│ Files: src-tauri/src/daemon/protocol.rs, src-tauri/src/terminal/shell.rs,   │
│        src-tauri/src/ipc/project.rs, src-tauri/src/ipc/terminal.rs,         │
│        src-tauri/src/lib.rs                                                 │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Lane 2: Frontend Remote Project Client & Model Layering                      │
│ [Category: Frontend Core]                                                   │
│ Files: ui/src/lib/remoteProject.ts, ui/src/lib/tauri.ts (target field only) │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
         ┌─────────────────────────────┴─────────────────────────────┐
         ▼                                                           ▼
┌───────────────────────────────────────────┐ ┌───────────────────────────────────────────┐
│ Lane 3: Project Location Dialog State     │ │ Lane 4: App Integration & Target Routing  │
│ [Category: Visual Engineering / Dialogs]  │ │ [Category: State / Storage]               │
│ Files: ui/src/components/ProjectDialogs.tx│ │ Files: ui/src/App.tsx,                    │
│        ui/src/components/ProjectDialogs.. │ │        ui/src/state/inactiveProjectWorktr.│
└─────────────────────┬─────────────────────┘ └─────────────────────┬─────────────────────┘
                      │                                             │
                      └──────────────────────┬──────────────────────┘
                                             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Lane 5: Integration Verifier (cargo test + vitest)                          │
│ [Category: Verification]                                                    │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ Lane 6: Code Reviewer & Safety Teardown                                     │
│ [Category: Review / Safety]                                                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Lane Responsibilities
- **Lane 1 (Backend IPC / Terminal Engine)**:
  - Implement `RegisterRemoteProjectRequest`, `RegisteredRemoteProject`, and `cmd_project_register_remote` in `src-tauri/src/ipc/project.rs`.
  - Add `TerminalStartup::RemoteSsh { program, args }` in `src-tauri/src/daemon/protocol.rs`.
  - Map `TerminalStartup::RemoteSsh` to `ShellCommandPlan` in `src-tauri/src/terminal/shell.rs`.
  - Route remote workspace spawning to direct SSH command plan in `src-tauri/src/ipc/terminal.rs`.
  - Register command in `src-tauri/src/lib.rs`.
  - Run backend unit tests.
- **Lane 2 (Frontend Remote Client)**:
  - Create `ui/src/lib/remoteProject.ts` with typed `registerRemoteProject`.
  - Layer narrow `target?: RunTarget` field onto `RegisteredProject` in `ui/src/lib/tauri.ts`.
- **Lane 3 (Project Location Dialogs)**:
  - Eliminate eager `open()` on mount in `AddProjectDialog`.
  - Implement `choose-location` step with Local and Remote choice cards.
  - Implement `remote-form` step consuming `useSshHosts()` from `../lib/sshHosts`.
  - Connect empty state to Settings "SSH Machines" via `onOpenSettings?.("ssh")`.
  - Implement remote submission calling `registerRemoteProject`.
  - Add unit tests in `ProjectDialogs.test.tsx` (T-PRJ-01..T-PRJ-08).
- **Lane 4 (App & Inactive Worktree Target Routing)**:
  - Preserve `target` in `App.tsx:loadProjects()`.
  - Pass `onOpenSettings={handleOpenSettings}` to `<AddProjectDialog />`.
  - Guard `App.tsx` active registration effect against remote projects.
  - Guard `inactiveProjectWorktrees.ts` against calling local `registerProject` for remote projects.
  - Add test in `inactiveProjectWorktrees.test.tsx` (T-INACT-01).
- **Lane 5 (Integration Verifier)**:
  - Execute full Vitest suite and Cargo test targets. Verify zero regressions.
- **Lane 6 (Code Reviewer & Teardown)**:
  - Audit against foreign dirty file list.
  - Confirm acceptance criteria satisfied.

---

## 8. Working Tree Safety & Teardown Protocol

### 8.1 Foreign Dirty Files Catalog (Strict Read-Only)
The following files belong to concurrent active sessions and MUST remain untouched:
- `docs/FERRYX_MEMORY_BASELINE_2026-09-05.md`
- `docs/FERRYX_METAL_OWNERSHIP_INVESTIGATION_2026-09-05.md`
- `src-tauri/Cargo.toml`
- `src-tauri/src/native_terminal/platform/macos.rs`
- `src-tauri/src/permissions/mod.rs`
- `ui/src/components/settings/PermissionsSection.test.tsx`
- `ui/src/components/settings/PermissionsSection.tsx`
- `ui/src/lib/tauri.ts` (preserve concurrent `canOpenSettings` edits; layer narrow `target?: RunTarget` addition only)
- `ui/src/lib/types.ts` (preserve concurrent `PermissionStatus` / `canOpenSettings` edits)

### 8.2 Foundation Lane Files (Consumer-Only)
The following files belong to foundation DAG `dag_6082b919-1a5d-4145-9489-015358f731fa` and must be consumed, not modified:
- `ui/src/lib/sshHosts.ts`
- `ui/src/components/settings/types.ts`
- `ui/src/components/SettingsDialog.tsx`
- `ui/src/components/settings/SshSection.tsx`

---

## 9. Verification & Acceptance Checklist

- [ ] **Criterion 1 (Local Gate & Eager Picker Elimination)**:
  - `AddProjectDialog` mounting under Tauri runtime does NOT call `dialog.open`.
  - Choice cards for Local and Remote render on initial mount.
  - Clicking Local triggers `dialog.open({ directory: true, multiple: false, title: "Add Project" })`.
  - Cancelling native picker closes dialog cleanly without error.
  - Selecting local directory confirms derived ID and invokes `cmd_project_register`.
- [ ] **Criterion 2 (Remote Project Direct SSH Registration & Spawning)**:
  - Selecting Remote in `AddProjectDialog` consumes `useSshHosts()`.
  - When 0 active hosts exist, displays notice, disabled submit, and CTA button navigating to Settings "SSH Machines".
  - When hosts exist, user selects host, enters remote path, and submits.
  - Backend executes `cmd_project_register_remote`, validates host server-side from `ssh_hosts.json`, safely quotes remote path, executes remote SSH probe, and binds placeholder workspace identity.
  - Project persists to `ferryx.projects` with `target: { kind: "ssh", hostId }`.
  - When remote project is active, `cmd_terminal_spawn` builds direct `ssh -tt` command plan using `TerminalStartup::RemoteSsh` and spawns interactive remote shell in local daemon PTY.
  - Native OS folder picker is NEVER invoked during remote addition.
- [ ] **Criterion 3 (Downstream Safety & Zero Regressions)**:
  - `inactiveProjectWorktrees.ts` skips local `registerProject` for remote projects and yields plain root worktree.
  - `App.tsx:loadProjects()` parses and preserves `target`.
  - Unsupported operations (Git worktree create/delete on remote projects) return structured `UNSUPPORTED` error in v1.
  - All test suites (`ProjectDialogs.test.tsx`, `inactiveProjectWorktrees.test.tsx`, Rust tests) pass cleanly.
  - Foreign dirty working-tree files remain pristine.
