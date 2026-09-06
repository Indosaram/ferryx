# Discovery Report: Project-Add & SSH Settings UI Contracts

**Generated:** 2026-09-06  
**Status:** Discovery and contract mapping complete; production code untouched.  
**Deliverable:** `docs/evidence/project-location/discovery-ui.md`  
**Repository Scope:** `ui/**` (with verification against `src-tauri/**` IPC command handlers)  
**Execution Context:** Ferryx Desktop (Tauri v2 + React 18 + Tailwind CSS + Zustand)

---

## Executive Summary & Core Findings

1. **Local Selection Gate Missing in `AddProjectDialog`**:
   In `ui/src/components/ProjectDialogs.tsx:79-104`, mounting `AddProjectDialog` under Tauri runtime unconditionally invokes `@tauri-apps/plugin-dialog`'s `open({ directory: true, multiple: false, title: "Add Project" })`. While waiting, lines 180–222 mount a placeholder dialog (`aria-busy="true"`, "Waiting for the folder picker."). There is currently no pre-picker choice between Local and Remote.
2. **Prior Memory `sshHosts.ts` Does Not Exist**:
   `ui/src/lib/sshHosts.ts` (from historical commit `1c37cb0`) is **absent** from the current tree. The single source of truth for SSH host persistence is the backend JSON store `ssh_hosts.json` managed via typed Tauri IPC commands in `src-tauri/src/ipc/ssh.rs`.
3. **Where Actual SSH Settings UI Now Lives**:
   - In `ui/src/components/SettingsDialog.tsx:44-54`, `VALID_SECTIONS` contains `"general"`, `"appearance"`, `"terminal"`, `"shortcuts"`, `"agents"`, `"browser"`, `"notifications"`, `"remote"`, and `"permissions"`.
   - Nav button `remote` (`SettingsDialog.tsx:151`) exclusively renders `RemoteAccessSection` (`ui/src/components/settings/RemoteAccessSection.tsx:55`), which manages **inbound Axum gateway pairing** (PIN code, QR code, paired mobile/web devices). It does **not** manage outbound SSH machines.
   - An SSH host editor (`HostSettings`), run target selector (`RunOn`), and reconnect button (`RemoteReconnect`) were implemented in `ui/src/features/ferryx/ssh/components.tsx:9-65` (introduced in commit `831ae2c`), but they are currently **unmounted and unreferenced** anywhere in production UI.
4. **Required Minimal UX & QA Verification Tiering**:
   - Sidebar `+` button (`ui/src/components/Sidebar.tsx:324`) opens `AddProjectDialog`.
   - Initial dialog view displays a **Local vs Remote location selection** (design system card/choice group) **without opening the OS folder picker**.
   - Choosing **Local** opens the native folder picker (`open({ directory: true, ... })`) in Tauri, or displays the manual path form in non-Tauri web fallback.
   - Choosing **Remote** switches to a remote project configuration view powered by the SSH inventory (`cmd_ssh_list_hosts`).
   - If no SSH hosts are configured, an empty state provides direct navigation to Settings (`handleOpenSettings("ssh")`) and disables project submission.
   - **QA Constraint & Strict Evidence Tiering**: Runtime discovery revealed that the Orca runtime exits immediately after open on certain host configurations, and macOS System Events UI elements report `enabled=false`. The Lead handles the final native desktop gate; UI discovery is strictly bounded and does not widen into debugging OS tooling. Browser-component integration evidence (Vitest mocks/DOM) is kept strictly decoupled from actual native picker proof, and browser tests must never be claimed to satisfy desktop acceptance.

---

## 1. Verified Local Project Entry Flow

### 1.1 Trigger Seams

| Component | Location | Code Pattern | Role |
|---|---|---|---|
| `Sidebar` | `ui/src/components/Sidebar.tsx:324` | `<IconButton label="Add project" className="no-drag" size="sm" onClick={onAddProject}>` | Top titlebar action in sidebar |
| `SidebarProps` | `ui/src/components/Sidebar.tsx:86` | `onAddProject?: () => void;` | Prop interface |
| `App` Empty State | `ui/src/App.tsx:1926-1930` | `<button type="button" onClick={handleOpenAddProject} ...><span>Add Project</span></button>` | Main pane CTA when `projects.length === 0` |
| `App` State | `ui/src/App.tsx:1050` | `const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);` | Latches dialog visibility |
| `App` Handler | `ui/src/App.tsx:1630` | `const handleOpenAddProject = useCallback(() => setIsAddProjectOpen(true), []);` | Setter callback |
| `App` Modal Mount | `ui/src/App.tsx:2029-2035` | `<AddProjectDialog projects={projects} onClose={handleCloseAddProject} onRegistered={handleRegisteredProject} />` | Renders dialog |

### 1.2 The Existing Local Picker Defect

In `ui/src/components/ProjectDialogs.tsx`:
```tsx
// Lines 79-104:
useEffect(() => {
  if (!isTauri || pickerOpenedRef.current) return;
  pickerOpenedRef.current = true;
  void open({
    directory: true,
    multiple: false,
    title: "Add Project",
  })
    .then((selected) => {
      if (typeof selected === "string" && selected.length > 0) {
        setSelectedPath(selected);
      } else {
        onCloseRef.current();
      }
    })
    .catch((cause) => {
      console.error(cause);
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message || "Could not open folder picker.");
    });
}, [isTauri]);
```
- **Observed Behavior**: The hook fires as soon as `AddProjectDialog` mounts. In Tauri mode, `open()` spawns the OS folder picker immediately.
- **Lines 180–222**: While `open()` is in flight, `AddProjectDialog` renders:
  ```tsx
  <div role="dialog" aria-label="Add Project" aria-busy="true" className="...">
    ...
    <span>Waiting for the folder picker.</span>
    ...
  </div>
  ```
- **Native Surface Yielding**: This `[role="dialog"]` surface is required because `ui/src/lib/nativeTerminalVisibility.tsx:11` defines:
  `const YIELDING_SURFACE_SELECTOR = '[role="dialog"], [role="search"]';`
  When a modal with `role="dialog"` mounts, WGPU native terminal panes mask/hide their child views so the webview dialog is not obscured.
- **Lines 106–128**: Once a path is returned, `handleConfirm` derives a workspace ID via `deriveWorkspaceId(selectedPath, projectsRef.current)` (`ui/src/components/ProjectDialogs.tsx:38-54`) and registers it via `registerProject({ workspaceId: derivedId, repoPath: selectedPath })`.
- **Registration Command**: `ui/src/lib/tauri.ts:140-142` invokes Tauri command `cmd_project_register`.

---

## 2. SSH Inventory Source: Reality vs Prior Memory

### 2.1 Absence of `sshHosts.ts`
- Prior memory cited `ui/src/lib/sshHosts.ts`.
- Inspection of the current repository tree confirms `ui/src/lib/sshHosts.ts` **does not exist**.
- Searching for `sshHosts` across git log confirms it was authored in commit `1c37cb0` (2026-09-01) and was subsequently replaced by the scoped contracts architecture (`docs/evidence/ferryx-scope/contracts.md` / `ui/src/lib/scopedContracts.ts`).

### 2.2 Backend IPC Commands & Storage Contract (`src-tauri`)

The authoritative host inventory resides on disk at `{app_data_dir}/ssh_hosts.json` (or `{app_data_dir}/dev/ssh_hosts.json` under dev runtime, per `src-tauri/src/ipc/ssh.rs:32-44`).

Registered Tauri command handlers in `src-tauri/src/lib.rs:787-794` & `src-tauri/src/ipc/ssh.rs`:

| Command | Signature | Source Line | Description |
|---|---|---|---|
| `cmd_ssh_list_hosts` | `() -> Result<Vec<SshHost>, IpcError>` | `src-tauri/src/ipc/ssh.rs:64-68` | Returns full list of stored SSH hosts |
| `cmd_ssh_import_config` | `(config_text: String) -> Result<Vec<SshHost>, IpcError>` | `src-tauri/src/ipc/ssh.rs:70-98` | Parses OpenSSH config and imports aliases |
| `cmd_ssh_update_host` | `(host: SshHost) -> Result<Vec<SshHost>, IpcError>` | `src-tauri/src/ipc/ssh.rs:100-120` | Upserts host by `host.id` and saves store |
| `cmd_ssh_delete_host` | `(id: String) -> Result<Vec<SshHost>, IpcError>` | `src-tauri/src/ipc/ssh.rs:122-143` | Removes host; adds config-source hosts to tombstones |
| `cmd_ssh_test_connection` | `(host: SshHost) -> Result<SshTargetSummary, IpcError>` | `src-tauri/src/ipc/ssh.rs:145-167` | Executes probe command; returns reachable & last error |
| `cmd_ssh_list_remote_worktrees`| `(host: SshHost) -> Result<Vec<RemoteWorktree>, IpcError>` | `src-tauri/src/ipc/ssh.rs:169-188` | Lists remote git worktrees via ssh |
| `cmd_ssh_create_remote_worktree`| `(host: SshHost, path, ws_id, slug, base_ref) -> Result<(), IpcError>` | `src-tauri/src/ipc/ssh.rs:190-213` | Creates worktree on remote host |
| `cmd_ssh_delete_remote_worktree`| `(host: SshHost, path) -> Result<(), IpcError>` | `src-tauri/src/ipc/ssh.rs:215-234` | Deletes worktree on remote host |

### 2.3 Backend Data Types (`src-tauri/src/ssh/mod.rs:9-24`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub label: String,
    pub hostname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_host: Option<String>,
    pub source: SshHostSource,       // "config" | "manual"
    pub auth_method: SshAuthMethod,   // "agent" | "key"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}
```

### 2.4 Existing Frontend UI Contracts (`ui/src/features/ferryx/ssh/`)

1. **`HostConfig` Interface** (`ui/src/features/ferryx/ssh/model.ts:2-5`):
   ```ts
   export interface HostConfig {
     id: string;
     name: string;
     hostname: string;
     user: string;
     port: number;
     identityFile: string | null;
     proxyJump: string | null;
     knownHostsFile: string;
   }
   ```
2. **Validation & Resolution** (`ui/src/features/ferryx/ssh/model.ts:6-17`):
   - `validateHost(host: HostConfig): string | null`: Validates port (1..65535), hostname/user/proxyJump formatting, non-empty id, name, and knownHostsFile.
   - `resolveRunTarget(target: RunTarget | undefined, hosts: readonly HostConfig[]): RunTarget`: Rejects removed SSH targets (`throws "HOST_UNAVAILABLE"`), preventing silent fallback to local.
3. **`RunOn` Selector Component** (`ui/src/features/ferryx/ssh/components.tsx:9-21`):
   - Props: `{ value?: RunTarget; hosts: readonly HostConfig[]; immutable?: boolean; onChange: (target: RunTarget) => void }`
   - Elements: `SelectTrigger` with `data-testid="run-on"`, `data-host-id`.
   - Options: `<SelectItem value="local">Local</SelectItem>` + mapped `hosts.map(...)`.
4. **`HostSettings` Component** (`ui/src/features/ferryx/ssh/components.tsx:25-60`):
   - Props:
     ```ts
     export interface HostSettingsProps {
       initial: HostConfig;
       save: (host: HostConfig) => Promise<void>;
       test: (host: HostConfig) => Promise<{ hostId: string; protocol: number }>;
       remove?: (hostId: string) => Promise<void>;
     }
     ```
   - Surface: `<section data-testid="ssh-host" aria-labelledby={`${id}-heading`}>`
   - Form inputs: `name`, `hostname`, `user`, `identityFile`, `proxyJump`, `knownHostsFile`, `port`.
   - Actions: `Save host` (`type="submit"`), `Test connection` (`data-testid="ssh-test"`), `Remove host` (`variant="ghost"`).
5. **RunTarget Contract** (`ui/src/lib/scopedContracts.ts:10`):
   `export type RunTarget = { readonly kind: "local" } | { readonly kind: "ssh"; readonly hostId: string };`

---

## 3. Where Actual SSH Settings UI Now Lives

### 3.1 Status in `SettingsDialog.tsx`
- In `ui/src/components/SettingsDialog.tsx:44-54`:
  ```ts
  const VALID_SECTIONS: readonly SectionId[] = [
    "general",
    "appearance",
    "terminal",
    "shortcuts",
    "agents",
    "browser",
    "notifications",
    "remote",
    "permissions",
  ];
  ```
- In `ui/src/components/settings/types.ts:1-10`:
  `SectionId` contains the same 9 section strings. There is **no `"ssh"` or `"machines"` section**.
- In `ui/src/components/SettingsDialog.tsx:151, 178`:
  Nav button `"remote"` renders `<RemoteAccessSection />`.
- In `ui/src/components/settings/RemoteAccessSection.tsx`:
  This section controls Ferryx's inbound Axum gateway:
  - `createPairingCode("control")` (`tauri.ts:588`)
  - QR Code generation (`RemoteAccessSection.tsx:109`)
  - `listRemoteDevices()` and `revokeRemoteDevice(deviceId)` (`tauri.ts:592, 596`)
  - It does **not** provide any SSH host or machine management.

### 3.2 Orphaned State of `HostSettings`
- `ui/src/features/ferryx/ssh/components.tsx` is completely isolated:
  An LSP reference check for `HostSettings` (`components.tsx:29`) shows **zero** imports across `ui/src/` outside of its own module.
- There are no typed wrappers in `ui/src/lib/tauri.ts` for `cmd_ssh_list_hosts`, `cmd_ssh_update_host`, `cmd_ssh_delete_host`, or `cmd_ssh_test_connection`.
- **Verdict**: The SSH Settings UI was designed and componentized in `ui/src/features/ferryx/ssh/components.tsx`, but **never mounted** into `SettingsDialog.tsx` or wired to backend IPC.

---

## 4. Remote Project Entry Contract & Flow

### 4.1 Project Addition Flow Architecture

```
                       [ Sidebar "+" Click / Empty View ]
                                      │
                                      ▼
                        ┌───────────────────────────┐
                        │   AddProjectDialog Mount  │
                        │    (No native picker!)    │
                        └─────────────┬─────────────┘
                                      │
                     Choose Project Location: Local vs Remote
                                      │
            ┌─────────────────────────┴─────────────────────────┐
            │                                                   │
            ▼                                                   ▼
     [ Local Project ]                                   [ Remote Project ]
            │                                                   │
   Tauri? ──┴────────┐                                  Fetch SSH Hosts
   Yes               No                                         │
    │                 │                                  Hosts available?
    ▼                 ▼                                   ┌─────┴─────┐
open() native   Manual Path form                          │           │
folder picker   (repoPath + workspaceId)                 Yes          No
    │                 │                                   │           │
    ▼                 │                                   ▼           ▼
Confirm Dialog        │                          Select SSH Host   Empty State:
(derived ID, path)    │                          Remote Repo Path  "Configure SSH Hosts
    │                 │                          Derived ID         in Settings"
    └────────┬────────┘                                   │        (Settings CTA link)
             │                                            │
             ▼                                            ▼
   registerProject(...)                         registerRemoteProject(...)
(Local canonical git root)                  (target: {kind: "ssh", hostId})
```

### 4.2 Data Model: `RegisteredProject` Extension

Currently, in `ui/src/lib/tauri.ts:36-40`:
```ts
export type RegisteredProject = {
  workspaceId: string;
  repoRoot: string;
  gitRoot?: string | null;
};
```
To represent remote projects accurately across the UI, layout persistence, and terminal spawning:
```ts
export type RegisteredProject = {
  workspaceId: string;
  repoRoot: string;
  gitRoot?: string | null;
  target?: RunTarget;  // { kind: "local" } | { kind: "ssh", hostId: string }
};
```
- Local projects omit `target` or set `target: { kind: "local" }`.
- Remote projects set `target: { kind: "ssh", hostId: "<id>" }` and `repoRoot: "/remote/path"`.
- `App.tsx:1158-1166` (`handleRegisteredProject`) persists the project array directly into `localStorage[PROJECTS_STORAGE_KEY]` (`ui/src/lib/storageKeys.ts:1`).

### 4.3 SSH Inventory Consumption Seam

To drive both `SettingsDialog` and `AddProjectDialog` from the same single source of truth without drift:
1. **IPC Bridge** in `ui/src/lib/tauri.ts`:
   ```ts
   export async function listSshHosts(): Promise<SshHost[]> {
     if (!isTauri()) return [];
     return invokeCommand<SshHost[]>("cmd_ssh_list_hosts");
   }
   export async function updateSshHost(host: SshHost): Promise<SshHost[]> {
     return invokeCommand<SshHost[]>("cmd_ssh_update_host", { host });
   }
   export async function deleteSshHost(id: string): Promise<SshHost[]> {
     return invokeCommand<SshHost[]>("cmd_ssh_delete_host", { id });
   }
   export async function testSshConnection(host: SshHost): Promise<SshTargetSummary> {
     return invokeCommand<SshTargetSummary>("cmd_ssh_test_connection", { host });
   }
   ```
2. **Reactive Inventory Hook**: A lightweight React hook `useSshHosts()` that calls `listSshHosts()` on mount and provides a refresh callback.

---

## 5. Recommended Minimal UX & Design System Specification

### 5.1 Design System Alignment (Design System Workflow Mandate)

All proposed UI adheres strictly to the tokens defined in `ui/src/index.css` and `ui/tailwind.config.js`:
- **Colors**:
  - Modal backdrop: `bg-black/45`
  - Modal card background: `bg-card` (`#171717`, `--card-rgb: 23 23 23`)
  - Borders: `border-border` (`--border: #ffffff12`)
  - Primary text: `text-foreground` (`#fafafa`)
  - Secondary text / hints: `text-muted-foreground` (`#a1a1a1`)
  - Danger / Error messages: `text-destructive` (`#ff6568`)
  - Interactive choices (unselected): `border-border/70 bg-muted/20 hover:border-ring hover:bg-accent/40`
  - Interactive choices (selected): `border-primary bg-accent/60`
  - Primary button: `bg-primary text-primary-foreground shadow hover:bg-primary/90`
  - Secondary button: `border border-border text-muted-foreground hover:bg-accent hover:text-foreground`
- **Typography Scale**:
  - Header title: `text-[13px] font-medium` (`ProjectDialogs.tsx:135`)
  - Description / hint: `text-xs text-muted-foreground` (`ProjectDialogs.tsx:138`)
  - Card title: `text-xs font-medium text-foreground`
  - Card subtitle: `text-[11px] text-muted-foreground`
  - Path monospace: `font-mono text-xs text-muted-foreground break-all`
- **Spacing Grid**:
  - Dialog container: `w-full max-w-[420px] rounded-lg border border-border bg-card shadow-2xl`
  - Header: `h-9 px-3 border-b border-border`
  - Body: `p-3 space-y-3`
  - Footer: `px-3 py-2 border-t border-border flex justify-end gap-2`
  - Inputs: `h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none focus:border-ring` (`fieldClass`, line 15)

### 5.2 AddProjectDialog State Machine & Minimal View Spec

```ts
type AddProjectStep =
  | "choose-location"  // Initial view: Local vs Remote choice cards
  | "local-pending"    // Waiting for OS folder picker (Tauri only)
  | "local-confirm"    // Folder selected, confirms derived workspace ID
  | "local-manual"     // Non-Tauri fallback text inputs
  | "remote-form";     // SSH host selector + remote repo path
```

#### View 1: Location Choice (`choose-location`)
- **Title**: "Add Project"
- **Subtitle**: "Select where the repository is located."
- **Cards**:
  1. **Local Card** (`data-testid="project-type-local"`, `role="button"`):
     - Icon: `FolderGit2` (`size-4 text-muted-foreground`)
     - Label: "Local Project"
     - Description: "Choose a folder on this machine using the folder picker."
     - Click: In Tauri, transitions to `"local-pending"` and calls `open()`. In non-Tauri, transitions to `"local-manual"`.
  2. **Remote Card** (`data-testid="project-type-remote"`, `role="button"`):
     - Icon: `Radio` or `TerminalSquare` (`size-4 text-muted-foreground`)
     - Label: "Remote (SSH)"
     - Description: "Connect to a repository hosted on an SSH machine."
     - Click: Transitions to `"remote-form"`.
- **Footer**: `Cancel` button calling `onClose()`.

#### View 2: Remote Configuration (`remote-form`)
- **Header**: Back button (`<ArrowLeft />`), title "Add Remote Project".
- **Content**:
  - **Case A (No SSH hosts configured or all disabled)**:
    - Alert/notice: "No SSH hosts configured. Add an SSH host in Settings before registering a remote project."
    - CTA Button: `Configure SSH Hosts in Settings` (`data-testid="configure-ssh-settings"`).
      Click invokes `onOpenSettings?.("ssh")` and calls `onClose()`.
    - Submit button disabled (`disabled={true}`).
  - **Case B (SSH hosts available)**:
    - Host Dropdown (`data-testid="remote-host-select"`):
      Lists configured active hosts: `<SelectItem value={host.id}>{host.label} ({host.username ? `${host.username}@` : ""}{host.hostname})</SelectItem>`.
    - Remote Repository Path Input (`data-testid="remote-repo-path-input"`):
      `<input aria-label="Remote repository path" className={fieldClass} placeholder="/home/user/project" ... />`
    - Workspace ID Input (`data-testid="remote-workspace-id-input"`):
      Auto-slugified from remote path via `deriveWorkspaceId`, user editable.
    - Submit Button (`data-testid="remote-submit-button"`):
      `Add Remote Project`, disabled when path is empty or host is unselected.
- **Footer**: `Back` button (returns to `"choose-location"`), `Cancel` button, `Add Remote Project` button.

### 5.3 SSH Settings Integration Spec

1. **Add Section to `SettingsDialog`**:
   - In `ui/src/components/settings/types.ts`:
     Add `"ssh"` to `SectionId`:
     ```ts
     export type SectionId =
       | "general"
       | "appearance"
       | "terminal"
       | "shortcuts"
       | "agents"
       | "browser"
       | "notifications"
       | "remote"
       | "permissions"
       | "ssh";
     ```
   - In `ui/src/components/SettingsDialog.tsx`:
     - Add `"ssh"` to `VALID_SECTIONS`.
     - Add navigation item in `SettingsDialogBody`:
       `<NavButton active={section === "ssh"} icon={<Radio />} label="SSH Hosts" onClick={() => setSection("ssh")} />`
     - Render `SshSection` in main pane:
       `{section === "ssh" ? <SshSection /> : null}`
2. **`SshSection` Component** (`ui/src/components/settings/SshSection.tsx`):
   - Uses existing primitives: `SettingsHeading`, `SettingsGroup`, `Card`, `Button`, `Input`, `Badge`.
   - List of configured hosts with `StatusDot`, `host.label`, `target(host)`, and edit/delete/test actions.
   - "Add Host" card / modal wiring into `HostSettings` (`ui/src/features/ferryx/ssh/components.tsx:25`).
   - "Import from ~/.ssh/config" action invoking `cmd_ssh_import_config`.

---

## 6. Test Seams & Accessible QA Selectors

### 6.1 Test Seams in `ui/src/components/ProjectDialogs.test.tsx`

The existing test file mocks `@tauri-apps/plugin-dialog` and `../lib/tauri` (`lines 5-17`).

Key regression test specifications:

| Test Case | Purpose & Assertion | Verification Seam |
|---|---|---|
| **Mount does not invoke picker** | Render `<AddProjectDialog />` under `isTauriRuntime: true`. Assert `dialog.open` has NOT been called. | `expect(dialog.open).not.toHaveBeenCalled()` |
| **Choice renders Local and Remote** | Verify both choice buttons/cards appear on initial render. | `screen.getByTestId("project-type-local")`, `screen.getByTestId("project-type-remote")` |
| **Local invokes native picker** | Click `project-type-local`. Assert `dialog.open` is called exactly once with `{ directory: true, multiple: false, title: "Add Project" }`. | `fireEvent.click(screen.getByTestId("project-type-local"))` -> `expect(dialog.open).toHaveBeenCalledTimes(1)` |
| **Local cancellation recovery** | Resolving `dialog.open` with `null` closes dialog or returns cleanly without error. | `expect(onClose).toHaveBeenCalledOnce()` |
| **Remote view without hosts** | When no SSH hosts exist, clicking Remote renders notice and Settings link; submit button is disabled. | `expect(screen.getByTestId("configure-ssh-settings")).toBeInTheDocument()`, `expect(submitBtn).toBeDisabled()` |
| **Remote view with hosts** | With mock hosts, selecting a host, filling remote repo path `/srv/repo`, and clicking submit registers remote project without calling `dialog.open`. | `expect(dialog.open).not.toHaveBeenCalled()`, `expect(onRegistered).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: "repo", repoRoot: "/srv/repo" }))` |
| **Remote cancellation** | Clicking Cancel in remote form invokes `onClose` and does not call `dialog.open` or register anything. | `expect(onClose).toHaveBeenCalledOnce()`, `expect(onRegistered).not.toHaveBeenCalled()` |

### 6.2 Accessible QA Selectors

| Element | Selector / Role | Test ID | Description |
|---|---|---|---|
| Dialog Surface | `[role="dialog"][aria-label="Add Project"]` | `data-testid="add-project-dialog"` | Top-level modal container (triggers terminal mask yield) |
| Local Choice Card | `button[aria-label="Local Project"]` | `data-testid="project-type-local"` | Card to choose local folder |
| Remote Choice Card | `button[aria-label="Remote Project"]` | `data-testid="project-type-remote"` | Card to choose remote SSH machine |
| Host Selector | `button[aria-label="Select SSH Host"]` / `[role="combobox"]` | `data-testid="remote-host-select"` | Dropdown to pick SSH host |
| Remote Path Input | `input[aria-label="Remote repository path"]` | `data-testid="remote-repo-path-input"` | Text input for remote file path |
| Workspace ID Input | `input[aria-label="Workspace id"]` | `data-testid="remote-workspace-id-input"` | Text input for project workspace ID |
| Settings Navigation Link | `button[aria-label="Configure SSH Hosts in Settings"]` | `data-testid="configure-ssh-settings"` | Action link to jump to Settings SSH section |
| Submit Button | `button[type="submit"]` | `data-testid="add-project-confirm"` | Final confirmation CTA |
| Cancel Button | `button[aria-label="Close Add Project"]` / text "Cancel" | `data-testid="add-project-cancel"` | Closes dialog |

### 6.3 QA Constraints & Strict Evidence Tiering (Lead Native Gate vs Component Proof)

During workstation discovery, a known native QA constraint was established:
1. **OS Environment Flaw**: The Orca native runtime process can exit immediately after launching without an active desktop display context or under certain background runner conditions. Additionally, macOS System Events accessibility scripting reports UI elements `enabled=false`, blocking reliable headless GUI driver automation on this machine.
2. **Scope Boundary**: Discovery is strictly bounded to UI contracts (`ui/**`) and does **not** widen into debugging macOS window server accessibility permissions, AppleScript tooling, or OS automation fixtures. The Lead engineer owns the final native desktop acceptance gate.
3. **Decoupled Verification Tiers**:
   - **Tier A — Component-Level Integration Evidence (In-Scope for Frontend CI/Vitest)**:
     - Target: `bun run --cwd ui test src/components/ProjectDialogs.test.tsx`
     - Validates:
       - No unconditional `dialog.open()` invocation on mount.
       - Deterministic transition to `"local-pending"` / `"remote-form"`.
       - Empty SSH inventory renders guidance, disables submission, and links to Settings.
       - Remote submission fires `onRegistered` with `{ workspaceId, repoRoot, target: { kind: "ssh", hostId } }`.
       - Full DOM accessibility, keyboard events, and ARIA roles (`role="dialog"`, `role="button"`, `role="combobox"`).
     - **Boundary Rule**: Component tests assert frontend contracts against hoisted mocks; they do **not** constitute proof of native OS Cocoa picker display.
   - **Tier B — Native Desktop Runtime Verification Gate (Owned by Lead)**:
     - Target: `bun tauri dev` on active interactive macOS desktop.
     - Validates:
       - Physical clicking of Sidebar `+` button opens the modal webview.
       - Native WGPU terminal surfaces yield child window visibility to `[role="dialog"]`.
       - Clicking "Local" triggers the real Cocoa `NSOpenPanel` directory sheet.
       - Clicking "Remote" remains within the webview dialog and invokes backend `cmd_ssh_list_hosts`.
       - Cancellation leaves terminal state uncorrupted.
     - **Explicit Constraint**: Tier A browser/Vitest test receipts MUST NOT be presented as Tier B native desktop proof.

---

## 7. Open Decisions & Technical Trade-offs

### Decision 1: Host Data Model Alignment (`SshHost` vs `HostConfig`)
- **Context**:
  - Backend Rust `SshHost` (`src-tauri/src/ssh/mod.rs:9-24`): fields `id`, `label`, `hostname`, `username`, `port`, `identity_file`, `jump_host`, `source` (`"config"` | `"manual"`), `auth_method` (`"agent"` | `"key"`), `disabled: Option<bool>`.
  - UI `HostConfig` (`ui/src/features/ferryx/ssh/model.ts:2-5`): fields `id`, `name`, `hostname`, `user`, `port`, `identityFile`, `proxyJump`, `knownHostsFile`.
- **Options**:
  - *Option A (Direct wire contract)*: Standardize TypeScript types to camelCase `SshHost` matching Tauri IPC directly (`label`, `username`, `identityFile`, `jumpHost`, `disabled`).
  - *Option B (Adapter layer)*: Maintain `HostConfig` in `ui/src/features/ferryx/ssh/model.ts` and write two-way translation functions (`hostConfigToSshHost` / `sshHostToHostConfig`).
- **Recommendation**: Option A. Direct wire contract eliminates mapping boilerplate and ensures `disabled` status and `source` (`config` vs `manual`) are preserved faithfully.

### Decision 2: Backend Remote Project Registration
- **Context**:
  `cmd_project_register` (`src-tauri/src/ipc/project.rs:156-180`) invokes `register_canonical_project`, which calls `WorktreeManager::try_new(repo_path)`. This strictly requires `repo_path` to exist on the local filesystem.
- **Options**:
  - *Option A (Backend-native remote registration)*: Add `cmd_project_register_remote(workspaceId, hostId, repoPath)` in `src-tauri/src/ipc/project.rs` to register the project in `WorkspaceRegistry` with remote marker.
  - *Option B (Client-side project record + Scoped RunTarget)*: Store remote projects directly in frontend `RegisteredProject` state (`localStorage[PROJECTS_STORAGE_KEY]`) with `target: { kind: "ssh", hostId }`. When spawning terminal tabs, inspect `project.target` and dispatch via the remote helper rather than local PTY spawn.
- **Recommendation**: Option B for UI increment; coordinate with backend owner for Option A daemon workspace registry sync.

### Decision 3: Settings Section Hierarchy for SSH
- **Context**:
  Settings dialog currently has `"remote"` for Axum gateway pairing.
- **Options**:
  - *Option A (Separate top-level section)*: Add `"ssh"` ("SSH Hosts" / "Machines") as a 10th top-level section in `SettingsDialog.tsx`.
  - *Option B (Nested under "Remote Access")*: Subdivide `RemoteAccessSection` into tabs: "Inbound Gateway" (mobile/browser pairing) and "Outbound Machines" (SSH hosts).
- **Recommendation**: Option A. Outbound SSH connections for remote development and inbound Axum pairing for web/mobile devices serve fundamentally different purposes. A dedicated "SSH Hosts" section in Settings is cleaner, more discoverable, and matches desktop developer mental models.

---

## 8. Verification & File:Line Citation Index

All cited symbols and line numbers were verified via language server protocol (`lsp_symbols`, `lsp_diagnostics`, `lsp_find_references`) and direct file reads in the repository:

- `ui/src/components/Sidebar.tsx:86, 111, 324` (`SidebarProps.onAddProject`, `IconButton "Add project"`)
- `ui/src/App.tsx:1050, 1630-1631, 1642-1658, 1893, 1926, 2029-2035` (`isAddProjectOpen`, `handleOpenAddProject`, `handleOpenSettings`, `Sidebar` mount, `AddProjectDialog` mount)
- `ui/src/components/ProjectDialogs.tsx:38-54, 56-60, 62, 76-104, 106-128, 180-222` (`deriveWorkspaceId`, `AddProjectDialogProps`, `AddProjectDialog`, `open` in `useEffect`, `handleConfirm`, pending dialog)
- `ui/src/lib/nativeTerminalVisibility.tsx:11` (`YIELDING_SURFACE_SELECTOR = '[role="dialog"], [role="search"]'`)
- `ui/src/lib/storageKeys.ts:1` (`PROJECTS_STORAGE_KEY = "ferryx.projects"`)
- `ui/src/lib/tauri.ts:36-40, 140-142` (`RegisteredProject`, `registerProject`)
- `ui/src/components/SettingsDialog.tsx:28, 44-54, 63, 151, 178` (`SettingsDialogProps`, `VALID_SECTIONS`, `SettingsDialogBody`, nav button `"remote"`, `RemoteAccessSection` mount)
- `ui/src/components/settings/types.ts:1-10` (`SectionId`)
- `ui/src/components/settings/RemoteAccessSection.tsx:55, 97-123, 174-178` (`RemoteAccessSection`, pairing code/QR generation)
- `ui/src/features/ferryx/ssh/model.ts:2-5, 6-13, 14-17` (`HostConfig`, `validateHost`, `resolveRunTarget`)
- `ui/src/features/ferryx/ssh/components.tsx:9-21, 22-27, 29-60, 62-65` (`RunOn`, `HostSettingsProps`, `HostSettings`, `RemoteReconnect`)
- `ui/src/lib/scopedContracts.ts:10` (`RunTarget`)
- `src-tauri/src/lib.rs:787-794` (8 registered `cmd_ssh_*` Tauri commands)
- `src-tauri/src/ipc/ssh.rs:12-21, 23-30, 32-44, 64-167, 169-234` (`SshTargetSummary`, `SshHostStore`, `get_ssh_store_path`, `cmd_ssh_*` handlers)
- `src-tauri/src/ssh/mod.rs:9-24` (`SshHost` struct)
