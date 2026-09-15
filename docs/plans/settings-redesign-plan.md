# Ferryx Settings redesign: one Remote domain

Status: proposed implementation plan; source-reviewed 2026-09-15. No application changes are included in this document.

## 1. Decision and boundaries

Replace the separate **Remote Access** and **SSH Machines** Settings destinations with **Remote**. Within Remote, distinguish **Machines** (outbound connections), **Access to This Machine** (inbound access), and **Connection Details** (diagnostics and compatibility). Unify navigation and presentation, not credentials, authorization, project identity, or transport implementations.

New paired-machine connections require only a PIN and always use `https://relay.checka.cc`. Do not ask users for a relay address, machine ID, token, SSH credentials, or a mandatory name. SSH remains an explicit alternative with its existing connection fields and import capabilities. Disabling access to this machine must not disable outbound SSH or paired-machine connections.

Keep all non-remote tabs, their order, settings, storage, and behavior intact. The request's “Alerts” corresponds to the actual **Notifications** tab and its **Alerts & Sounds** group; do not introduce an extra Alerts tab or rename Notifications as part of this work.

This plan is based on source inspection, not runtime verification. No builds or tests were run. Proposed commands and events below are explicitly identified as new; their existence must not be inferred from a wireframe.

## 2. Verified current-state inventory

### 2.1 Settings shell and every existing tab

`ui/src/components/SettingsDialog.tsx` renders the following ten navigation entries in this order. `ui/src/components/settings/types.ts` defines section IDs. Settings is a dialog, not a route-backed page; it receives `initialSection`, closes via its owner, and currently exposes only an `onOpenSshProject` project-navigation callback.

In the table, component paths are relative to `ui/src/components/settings/`. Supporting UI library paths are relative to `ui/src/lib/`. “No UDS operation” means the settings action is not implemented through the local daemon socket; it does not imply that later workspace activity is daemon-free.

| Existing tab / section | Source and configuration | Actual backing operations |
| --- | --- | --- |
| General / behavior | `GeneralSection.tsx`; confirm tab closure; sidebar visible at startup. `generalSettings.ts` | Local-storage preferences and frontend change notification; no dedicated Tauri command or UDS operation. |
| General / Software Update | `GeneralSection.tsx` (`SoftwareUpdateCard`); current version, check, download/install, relaunch. `updater.ts` | Tauri app `getVersion`, updater plugin `check` and `downloadAndInstall`, process plugin `relaunch`; `cmd_updater_managed_externally` checks external management. Frontend update-status subscribers and plugin progress callbacks. No Settings UDS operation. |
| General / Ferryx CLI | `GeneralSection.tsx` (`CliLauncherCard`); launcher status and installation | `cmd_cli_launcher_status`, `cmd_cli_launcher_install` via `tauri.ts`; native implementation `src-tauri/src/ipc/cli_install.rs`. |
| Appearance / Display & Styling | `AppearanceSection.tsx`; theme, accent, interface density, reset; `appearanceSettings.ts` | Local storage and frontend settings event; no dedicated Settings IPC/UDS. |
| Terminal / imported preferences and overrides | `TerminalSection.tsx`, props wired by `SettingsDialog.tsx`; font family/size, macOS Option-as-Alt, shell selection/custom path, scrollback, use imported/reset. Source information explains Ghostty inheritance. `terminalSettings.ts` | `cmd_terminal_preferences`, `cmd_terminal_apply_overrides` via `tauri.ts`; `src-tauri/src/ipc/preferences.rs`. Local overrides are saved in local storage and synchronized to native preferences. These are not remote-machine preference controls. |
| Keyboard Shortcuts / reference | `ShortcutsSection.tsx`; searchable, filterable reference for Tabs, Workspaces, Terminal Panes, Global, View; `shortcuts.ts` | Static shortcut definitions and component-local filter/search state. No editing command, Tauri IPC, or UDS operation. |
| Agents / Default Agent, Installed, Custom Agents | `AgentsSection.tsx`; default choice, detection/refresh, enablement, command/arguments overrides, add/edit/remove custom agents; `agentsSettings.ts` | `cmd_agents_detect` through `tauri.ts`, implemented by `src-tauri/src/ipc/agents.rs`; overrides/custom definitions use local storage and frontend settings events. This is not SSH helper installation. |
| Browser / Web & Navigation | `BrowserSection.tsx`; home page, search engine, default zoom, restore tabs, remember/clear history; `browserSettings.ts`, `browserHistory.ts` | Preferences/history local storage. Applying zoom uses `cmd_browser_list` and `cmd_browser_set_zoom`. Workspace restoration is a downstream consumer, not a separate Settings save command. |
| Browser / Link Routing | Same component; built-in link default, Shift override, terminal link actions, localhost worktree labels | Local preferences; no dedicated Settings IPC/UDS for these switches. |
| Browser / Session & Cookies | Same component; browser profile selection/management, profile names and cookie import, with platform-supported profile constraints | Profiles in `browserSettings.ts`; dialog plugin file picker and `cmd_browser_import_cookies` via `browserTauri.ts`. |
| Browser / Active Browser Tabs | Same component; list and focus current native browser tabs | `cmd_browser_list`, `cmd_browser_focus`; native browser commands in `src-tauri/src/ipc/browser.rs`. |
| Notifications / system authorization and delivery diagnostics | `NotificationsSection.tsx`; permission status, request permission, OS settings, test delivery | `cmd_notification_get_permission_status`, `cmd_notification_request_permission`, `cmd_notification_open_system_settings`, `cmd_notification_probe_delivery`. Native implementation `src-tauri/src/ipc/notifications.rs`; focus refresh is a DOM event, not a daemon event. |
| Notifications / Alerts & Sounds | Same component; notification enablement, agent completion, pane attention border, terminal bell, sound/custom file, volume, preview/test, reset; `notificationSettings.ts` | Preferences in local storage; `cmd_notification_pick_audio`, `cmd_notification_play_sound`, and delivery probe above. `ferryx:notifications:settings-changed` updates frontend consumers. |
| Permissions / OS permissions | `PermissionsSection.tsx`; Full Disk Access, Accessibility, Desktop Notifications, refresh/system settings and onboarding entry | `cmd_permissions_get_status`, `cmd_permissions_open_settings`, `cmd_permissions_request_accessibility`, `cmd_notification_request_permission`; `src-tauri/src/ipc/permissions.rs`. `permissionsOnboarding.ts` supplies dismissal reset and `OPEN_PERMISSIONS_ONBOARDING_EVENT`. Platform-specific applicability remains intact. |
| Remote Access / gateway and relay | `RemoteAccessSection.tsx`; enable switch, editable Relay / Signaling Server URL (blank means local-network mode), status | `cmd_remote_status` -> `RemoteGetStatus`; `cmd_remote_enable` -> `RemoteGetStatus` then `RemoteConfigure`; `cmd_remote_disable` -> the same read/configure path with mode Off. Native implementation `src-tauri/src/ipc/remote.rs` also supports an in-process `RemoteGatewayState` branch, without UDS. Gateway port is fixed by native code, not a Settings control. |
| Remote Access / Pairing QR Code | Same component; generate/regenerate control pairing, 60-second countdown, PIN copy, QR and link copy | `cmd_remote_pairing_create` -> `RemoteCreatePairingCode`; QR generation is local `qrcode` rendering. The current Settings API takes permission, not machine-access scope. It must not be presented as issuing a machine grant. |
| Remote Access / Paired Devices | Same component; inbound authorized devices, permissions, last activity, revoke confirmation | `cmd_remote_devices` -> `RemoteListDevices`; `cmd_remote_device_revoke` -> `RemoteRevokeDevice`. In-process equivalents call the auth manager. This list is not the outbound machine inventory. |
| Remote Access / Paired machines | `PairedMachinesSection.tsx`; outbound PIN pairing, advanced relay/name overrides, refresh, capability check, re-pair, forget credentials | `pairedHostInventory.ts`, `remoteHostStore.ts`, `pairedDaemonProject.ts`; `paired_host_list`, `paired_host_capabilities`, `paired_host_pair`, `paired_host_forget`, `paired_host_operation`. Exact UDS map below. Rendered only in Tauri. Its Add Project action is disabled here because `RemoteAccessSection` supplies no `onOpenProject`. |
| SSH Machines / config source and imports | `SshSection.tsx`; system `~/.ssh/config` or chosen config, reload, raw preview, import one/all, paste config | `sshHosts.ts`: `cmd_ssh_read_system_config`, `cmd_ssh_import_config`, `cmd_ssh_update_host` for single import; Tauri dialog picker. Custom source path is local storage. Native `src-tauri/src/ipc/ssh.rs` reads files/parses config; no SSH-inventory UDS operation. |
| SSH Machines / add/edit and Configured Machines | Same component; label, hostname/IP, username, port, agent/key authentication, identity file, jump host; enable/disable, delete, test, prepare agent integration, open project | `cmd_ssh_list_hosts`, `cmd_ssh_update_host`, `cmd_ssh_delete_host`, `cmd_ssh_test_connection`, `cmd_ssh_prepare_integration`. Hosts persist in native `ssh_hosts.json` (app-data/dev or `FERRYX_DATA_DIR`), including import tombstones. Test calls `ssh::runtime::detect`; prepare installs the agent-state extension via SSH. Open Project is an App callback, not itself an IPC command. |

### 2.2 Remote surfaces outside Settings

- `ui/src/components/ProjectDialogs.tsx`: Add Project initially offers **Local Project**, **Remote (SSH)**, and **Paired Daemon**. The SSH flow uses `useSshHosts`, refreshes authoritative hosts before registration, rejects changed/disabled hosts, and uses `RemoteDirectoryPicker`.
- `ui/src/components/PairedDaemonProjectForm.tsx`: a separate paired-machine modal selects a host, checks native readiness/grant/online status, negotiates capabilities, browses folders and registers a project. Changing host/generation remounts its inner form and invalidates listings and pending adoption. It currently selects the first host by default, not a host passed from Settings.
- `ui/src/components/RemoteDirectoryPicker.tsx` and `ui/src/lib/remoteDirectories.ts`: shared folder UI with a `DirectorySource` abstraction; SSH uses `cmd_ssh_list_directories`, paired daemons provide an adapter-backed source. Reuse this; do not create another remote file picker.
- `ui/src/App.tsx`: `handleOpenSshProject` and the Settings callback bridge SSH settings to Add Project. Extend this bridge to typed machine targets rather than creating an unrelated paired-machine navigation system.
- `ui/src/lib/remoteProject.ts`, `ui/src/state/workspaceStore.ts`, `workspaceRestore.ts`, `workspaceRuntime.ts`, `inactiveProjectWorktrees.ts`: remote project/worktree/session ownership and restoration live in the workspace domain. Settings should launch those flows, not become a second workspace manager.
- `ui/src/lib/pairedDaemonProduct.ts` does **not** exist in this checkout. The implementation inspected is `ui/src/lib/pairedDaemonProject.ts`, with runtime schemas in `pairedDaemonContracts.ts`.

### 2.3 SSH helper reality, not a promised feature

Three different operations must be distinguished:

1. **Test connection**: `cmd_ssh_test_connection` probes platform/executor/home/temp/Git through `src-tauri/src/ssh/runtime.rs`. Reachability is not proof of terminal helper readiness.
2. **Prepare agent integration**: `cmd_ssh_prepare_integration` -> `ssh/direct.rs::ensure_remote_extension_installed` -> platform-aware `ssh/operations.rs` extension installation. This is agent-state reporting setup, not installation of the terminal helper binary.
3. **Terminal helper**: `cmd_ssh_install_project_helper` exists in `src-tauri/src/ipc/ssh.rs`, accepts a registered workspace and a user-selected local binary, and calls `helper_setup::{default_location, install}`. It is not exposed by the current Settings section. `ssh/bridge.rs` calls `helper_setup::ensure_started` when connecting. `ssh/helper_assets.rs` contains manifest/checksum/target-resolution support, but its presence alone is not evidence that automatic provisioning is wired into the Settings flow. Do not label the current Prepare action “Install helper” or promise automatic downloads.

## 3. Problems with the current information architecture

1. **One “remote” label conflates direction and purpose.** Remote Access describes phone mirrors but embeds project-owning paired daemons. SSH Machines is outbound-only. Users cannot tell whether the Remote Access switch controls all remote work.
2. **“Paired Devices” and “Paired machines” look interchangeable but are different authorities.** The former can access this machine; the latter are machines this desktop can access. Revoking an inbound device and forgetting an outbound credential are not equivalent actions.
3. **Transport terminology dominates entry points.** Remote Access, SSH Machines, SSH hosts, Remote (SSH), Paired Daemon, Relay / Signaling Server URL, and machine grants are exposed without a consistent hierarchy. A relay is infrastructure, not a user-owned machine.
4. **Settings is not a consistent starting point for work.** SSH Open Project is wired; paired Add Project is visibly disabled with instructions to find another modal. Both flows make users select machine context again in separate UI.
5. **PIN-only is only a default today.** Paired-machine Advanced allows arbitrary relay/name inputs. The inbound gateway starts with an empty relay draft, even though the shared product default is fixed. A placeholder is not an applied value.
6. **Status claims mix incompatible evidence.** Gateway listener readiness, relay/control-channel readiness, cached host online state, capability admission, SSH probe success, and extension preparation are not interchangeable “connected” states.
7. **SSH import configuration occupies prime space ahead of the inventory.** Raw config and source controls should be subordinate to adding or managing a machine.
8. **Failures can resemble valid empty state.** Remote status/device refresh currently catches failures as null/empty, and native paired actions expose deliberately sanitized generic errors. The redesign needs explicit unavailable states without leaking credentials or inventing unsupported error specificity.
9. **Ephemeral checks can appear more authoritative than they are.** Paired checks are generation-bound presentation; SSH preparation is component-local state invalidated by connection changes. Neither is durable health telemetry.

## 4. Proposed Settings IA

### 4.1 Exact top-level navigation

Preserve order: **General**, **Appearance**, **Terminal**, **Keyboard Shortcuts**, **Agents**, **Browser**, **Notifications**, **Permissions**, **Remote**.

Remove the separate SSH Machines entry. Keep `remote` as the stable top-level section ID; accept legacy `ssh` navigation requests and normalize them to Remote > Machines with the SSH filter selected. Do not migrate unrelated preference keys.

### 4.2 Remote hierarchy and section order

Remote opens **Machines** by default. Use three in-page subnavigation destinations, each with an addressable internal selection (not a new OS window):

1. **Machines**
   1. Intro: “Connect to another machine.”
   2. Inventory toolbar: Search; All / Paired / SSH filters; **Add Machine**; Refresh.
   3. Inventory, including saved offline/disabled/needs-repair rows.
   4. Selected machine details: Connection; Readiness; Projects; Management, in that order.
   5. Add Machine chooser opens **Pair with PIN** or **Connect with SSH**. SSH source/import controls live inside the SSH branch and its **Import SSH Config** action, not above the inventory.
2. **Access to This Machine**
   1. Intro and **Allow remote access** switch; direction is explicit.
   2. Connection summary: listener and relay readiness separately; fixed relay shown read-only for the standard path.
   3. **Pair a Device**: generate/copy PIN and expiration. Scope is explicitly device/mirror access in the first release; machine-grant issuance is a separate gated addition.
   4. **Authorized Devices**: inbound permissions, activity, and Revoke Access.
3. **Connection Details**
   1. Local daemon/native compatibility and inventory readiness.
   2. Gateway diagnostics: effective relay origin, configured mode, local endpoint/port and relay/control connection state.
   3. Selected-machine diagnostics link/summary: stable identity, connection type, paired generation/grant/capability results or SSH environment/error stage.
   4. Legacy configuration notice, when relevant; no arbitrary relay editor in the new pairing flow.

“Details” is diagnostic information, not a bucket of mandatory setup. Per-machine diagnostics are also accessible directly from the selected row so errors do not require navigation hunting.

### 4.3 Movement map

| Current surface | New destination |
| --- | --- |
| Remote Access switch and relay controls | Access to This Machine; effective configuration details in Connection Details |
| QR/PIN/link card | Pair a Device, PIN-first; final desktop PIN-only treatment removes QR/link as required inputs |
| Paired Devices | Authorized Devices under Access to This Machine |
| Paired machines form/list | Machines inventory and Add Machine > Pair with PIN |
| SSH Config source/raw/import UI | Add Machine > Connect with SSH > Import SSH Config; source management accessible from that action |
| SSH configured list/test/edit/enable/prepare | Machines inventory and SSH machine detail |
| Separate Settings-to-SSH project launch | Shared typed machine-to-project launch for Paired and SSH |
| Add Project transport chooser | Local / Remote, then shared machine inventory selector with type badges; existing type-specific adapters remain underneath |

### 4.4 Wireframes

```text
Settings                       Remote
  General                      [Machines] [Access to This Machine] [Connection Details]
  Appearance
  Terminal                     Connect to another machine
  Keyboard Shortcuts           [Search machines...] [All v] [Refresh] [+ Add Machine]
  Agents
  Browser                      Name          Type      State              Action
  Notifications                build-linux   Paired    Ready              [Open Projects]
  Permissions                  lab-box       SSH       Not checked        [Test Connection]
> Remote                       travel-mac    Paired    Offline            [Details]
                               old-server    SSH       Disabled           [Details]

                               build-linux / Paired
                               Connection  relay.checka.cc (read-only)
                               Readiness   Projects ready; terminals not checked
                               Projects    [Open Projects] [Add Project]
                               Management  [Re-pair] [Forget Credentials]
```

```text
Add Machine
  [Pair with PIN] [Connect with SSH]

  On the other machine, obtain a machine-access PIN.
  Relay: https://relay.checka.cc
  PIN [______]                 [Cancel] [Pair Machine]

  No URL / machine ID / required label / token / SSH password fields.
```

```text
Access to This Machine
  Allow remote access                                  [Off / On]
  Controls incoming connections only. Outbound machines are unaffected.
  Listener: Running     Relay: Connecting / Ready / Unavailable
  Relay: https://relay.checka.cc

  Pair a Device
  [Generate PIN]   [123456] [Copy PIN]   Expires in 42s
  Device access is not a machine-project grant.

  Authorized Devices
  Phone browser        Control       Last active ...    [Revoke Access]
```

Use text and icons as well as color for statuses; real labels, keyboard-operable controls, visible focus, field-level errors and a non-disruptive status live region. Restore focus to the initiating row after closing a detail/form. At narrow widths, use stacked machine cards and a detail subview rather than horizontal overflow. Do not nest a second active modal inside Settings; hand off to the workspace dialog with a return context.

## 5. Interaction flows

### 5.1 Pair a machine with only a PIN

1. Open Remote > Machines > Add Machine > Pair with PIN. Explain that the owner runs `ferryx-cli --daemon` and obtains `ferryx-cli pair generate --access machine` on the other machine. Those are the current instructions; desktop machine-PIN issuance is not already implemented.
2. Accept/paste the PIN, preserve leading zeros, and submit exactly once. The present UI says six-digit PIN, while native validation also accepts alphanumeric/hyphen tokens; confirm the issued machine-PIN contract before enforcing a digits-only validator. Never require a second connection datum.
3. Construct the request internally with `relayOrigin: "https://relay.checka.cc"` and the existing fallback `displayLabel: "Machine"`. Clear the PIN draft on submission, cancellation, success and failure; never persist it or log raw IPC failures.
4. Native `paired_host_pair` sends the exchange to `/api/v1/pair/exchange` on that fixed origin, obtains machine identity/token, authenticates the returned scope, and persists credentials in daemon-owned inventory. UI receives only `HostView`.
5. Show the returned identity/type and immediately negotiate capabilities against the captured host ID and generation. A mirror grant is **Needs machine access**, not successful project readiness. Offer fresh machine-PIN re-pairing; never elevate a mirror grant in the renderer.
6. Successful admission focuses the new inventory row and offers Open Projects/Add Project. With the existing DTO, distinguish generic names using a short machine-ID suffix rather than falsely claiming an authenticated remote display name. Friendly rename requires an additive native metadata API if approved.
7. Errors preserve other machines and workspace references. Show “Could not pair” plus retry/compatibility guidance for the current sanitized generic code. Only show “expired PIN” or “rate limited” when native provides a corresponding safe typed code. A failed PIN should not visually imply that every saved machine was deleted.
8. Re-pair captures the expected selected machine identity. A PIN resolving to a different machine must not silently replace the selected row; disclose the mismatch and require explicit add-new confirmation. This needs a native expected-identity check before credential adoption, not merely a post-success UI comparison.

### 5.2 Enable inbound access and issue a device PIN

- For a new/default installation, enabling uses explicit relay mode and `https://relay.checka.cc`; display actual returned status, not optimistic success. A bound local listener alone must not enable the claim “Ready for PIN pairing through relay.”
- Generate a device PIN only with the relevant readiness satisfied. Use the returned lifetime with the current safety cap; invalidate old display generations on regenerate/disable/unmount. PIN-only means the user need not scan a QR or transmit a URL/token. A phone can be directed to the fixed relay site and enter the PIN.
- Keep access scope visible. Existing `cmd_remote_pairing_create` does not issue the machine-access variant. If product wants desktop-to-desktop issuance here, add an explicit scope-aware native entry point backed by `RemoteCreateMachinePairingCode`; old daemons must reject unsupported issuance rather than silently return a mirror PIN.
- Disabling only stops the inbound listener. Explain that outbound credentials and remote workspace sessions are separate. Do not erase paired inventory or saved layouts.
- Revoke Access confirms the named inbound device and permission. Handle the existing boolean revoke result: `false` is not a successful revoke. Refresh authorized devices and report failure independently of a list-loading failure.

### 5.3 Add, edit, import and test SSH

1. Add Machine > Connect with SSH exposes label, hostname/IP, optional username, port default 22, Agent/Key authentication, key identity path, and jump host. Keep identity-file/jump-host details collapsible but discoverable. No invented password storage or new SSH trust bypass.
2. Import SSH Config retains default/custom file selection, raw preview on demand, one/all import and pasted config. Preview before committing; preserve native IDs and tombstone semantics. Treat a duplicate-label hint as a hint, not a new identity rule.
3. Save with `cmd_ssh_update_host`; keep editing ID unchanged. Validate port and required fields. Refresh the shared host cache, invalidate any results captured for the prior connection configuration, and select the saved row.
4. Test Connection calls the existing platform-aware probe. Show Testing -> Reachable or Failed, with returned checked time, platform, executor, Git availability and safe diagnostic stage. Reachable is not “helper installed” or continuous online status.
5. Agent Integration is a separate explicit write to the remote machine, with path/purpose explanation and the existing prepare command. Show a completed preparation result as “Prepared in this check,” not as a durable guarantee after future remote changes.
6. Terminal Helper is independently shown as Not checked / Needed / Ready / Incompatible when supported evidence exists. Initially expose the existing workspace-scoped explicit-binary installer after a project is registered; require artifact selection and confirmation, and never build/download implicitly. Host-level helper inspection/installation is a proposed additive API, not a prerequisite to shipping the unified navigation.
7. Disable/delete affects the configured endpoint, not remote files. Confirm removal, list affected saved project references when available, and explain reconnect implications. Do not close active remote sessions or delete remote worktrees as a side effect of deleting a machine row.

### 5.4 Inventory and project/worktree/session browsing

Use stable discriminated references, conceptually `{ kind: "pairedDaemon", hostId, generation }` and `{ kind: "ssh", hostId }`, never row index, label, or global active host. Both types may legitimately point at the same physical computer; show separate connection rows unless the product later defines an explicit association model.

| Behavior | Paired machine | SSH machine |
| --- | --- | --- |
| Inventory authority | Native paired inventory mirrored into `remoteHostStore` | Native `ssh_hosts.json` mirrored by `sshHosts.ts` |
| Connection summary | Auth/grant, online snapshot, generation-bound capabilities | Enabled state, endpoint, timestamped probe result; no implied live presence |
| Projects | Adapter `projects()` discovers the remote daemon's registered projects; include unavailable IDs and saved local references | Show projects registered in this desktop's workspace for the host; filesystem browsing is not an inventory of all remote repositories |
| Add Project | Negotiate, browse via adapter `directories`, register via `registerProject` with request ID | Refresh host, browse via `cmd_ssh_list_directories`, register via `cmd_project_register_remote` |
| Worktrees | `worktrees`, `worktreeStatus`; mutation only when `managedWorktreesV1` is present | `cmd_ssh_list_remote_worktrees`; create/delete use existing SSH worktree commands and path guards |
| Sessions | `sessions`, `session`, create/close subject to `terminalCreateV1`; stream requires `terminalStreamV1` | Existing workspace terminal list/attach/spawn/retry flows and SSH helper bridge; do not imply discovery of arbitrary OS shells |

Open Projects hands off to a workspace/project browser with the selected machine fixed. Existing paired projects can be adopted from the native-mapped project DTO without registering a guessed local ID. Add Project opens the same picker from Settings and the workspace. Back/cancel restores machine context. Machine/generation/SSH configuration changes invalidate selected paths and in-flight adoption.

Do not move destructive worktree/session actions into the Settings inventory. Open a selected project/worktree in its existing workspace surface; maintain backend checks there. For paired mutations, preserve request IDs, expected revisions and daemon epochs; resolve ambiguous outcomes with `operation(requestId)` rather than blindly repeating a create/delete. Never route a paired project through a local filesystem fallback.

Offline and incompatible rows remain visible. Keep saved layouts/projects available as references, disable only actions needing the absent capability, and explain why. Opening Settings, changing its selected row or switching filters must not switch the active workspace or tear down local terminals.

## 6. State, IPC, socket and event impact map

### 6.1 State ownership (including the Zustand question)

There is no Zustand import in the inspected `ui/src` source. In particular, `remoteHostStore.ts` is a custom `getState/setState/subscribe` store consumed through `useSyncExternalStore`; `sshHosts.ts` is a module-level cache/listener set plus React hooks; `workspaceStore.ts` uses React `useReducer`. Do not plan migrations against nonexistent Zustand stores.

| Redesigned section | State consumed / changed | Required impact |
| --- | --- | --- |
| Remote shell and subnavigation | Settings local state; App navigation handoff | Add Remote subpage/filter/selected-machine context and legacy `ssh` alias. No persisted authority or new global active-host routing. |
| Machines inventory | `remoteHostStore`, `pairedHostInventory`, `useSshHosts` | Derived union view only. Preserve native readiness, migration status, generation fencing, cache epochs and offline rows. Per-machine loading/error state instead of one list-wide success label. |
| Pair with PIN / re-pair / forget | `pairedHostInventory` mutation results; native credential authority | Fixed-origin request construction. Preserve migration/readback and expected-generation forget. Proposed expected-identity re-pair support; do not place tokens in a presentation store. |
| SSH detail and import | Existing `sshHosts.ts` cache; component-local forms/test/preparation state; source-path storage | Keep cache write epochs and changed-connection invalidation. Optional future safe helper summary is separate from probe success. |
| Access to This Machine | Currently component-local gateway, device, PIN/countdown state | Extract a reusable section controller only if needed by subviews; explicit loading/error/stale states. Gateway writes remain independent of outbound store. |
| Connection Details | Read-only native status and per-machine summaries | No arbitrary invoke inspector, raw credentials, PIN-bearing URL or unrestricted remote request facility. |
| Project handoff | App project-dialog context, workspace reducer, workspace restore/runtime | Add paired preselection and typed callback; preserve host-scoped project IDs and active workspace until user opens a project. |

### 6.2 Existing Tauri -> local daemon UDS operations

Names on the right are actual `DaemonRequest` variants in `src-tauri/src/daemon/protocol.rs`, consumed by `daemon/client.rs` and `daemon/server.rs`; serde uses camel-case wire request tags. This avoids confusing Rust variants, Tauri command names and HTTP routes.

| Tauri command | UDS request / downstream |
| --- | --- |
| `cmd_remote_status` | `RemoteGetStatus` |
| `cmd_remote_enable`, `cmd_remote_disable` | `RemoteGetStatus`, `RemoteConfigure`; native gateway-state branch is also supported |
| `cmd_remote_pairing_create` | `RemoteCreatePairingCode` (device permission; not machine issuance) |
| `cmd_remote_devices` | `RemoteListDevices` |
| `cmd_remote_device_revoke` | `RemoteRevokeDevice` |
| `paired_host_list` | `PairedHostList` |
| `paired_host_capabilities` | `GetCapabilities` through native compatibility checks |
| `paired_host_pair` | `PairedHostPair`; native HTTPS PIN exchange and scope authentication |
| `paired_host_forget` | `PairedHostForget` with expected generation; native cancellation/credential removal |
| `paired_host_migrate_legacy` / `paired_host_read` | `PairedHostMigrateLegacy` / `PairedHostRead`; receipt-verified legacy migration |
| `paired_host_operation` | `PairedHostOperation` with captured host/generation and typed operation |
| Proposed desktop machine-PIN command | Existing UDS `RemoteCreateMachinePairingCode`; requires new renderer-facing IPC and capability admission, not changing the meaning of `RemoteCreatePairingCode` |

`paired_host_operation` downstream uses native machine HTTP routes relative to the selected host's `/api/v1/`: `capabilities`, `fs/directories`, `workspace/projects`, `workspace/worktrees`, `workspace/worktrees/status`, `sessions`, and `workspace/operations/{requestId}`. Register/create use POST; unregister/delete/close use DELETE; reads use GET, with IDs/query/body constrained by the typed operation. Terminal stream attachment uses the native-authenticated `terminal/{sessionId}` WebSocket with epoch/sequence checks. No renderer token transport or arbitrary URL is needed.

SSH host CRUD/config/probe/extension preparation run in native IPC and SSH subprocess code, **not** a `PairedHostOperation` or an invented SSH CRUD UDS method. `cmd_project_register_remote` first pings the local daemon, then probes/persists the canonical host/path project in native code. Worktree commands call `ssh/worktree.rs`; directories call `ssh/browse.rs`.

Remote terminal execution remains in the workspace path: `cmd_terminal_spawn`, `cmd_terminal_attach`, `cmd_terminal_list`, `cmd_terminal_remote_status`, `cmd_terminal_remote_retry`, and generation-aware remote write/resize commands in `tauri.ts`. `daemon/session_service.rs`, `terminal/remote.rs`, `ssh/bridge.rs` and `ssh/helper_setup.rs` own SSH session/helper orchestration. The helper bridge has its own RPCs (`project.register`, `worktree.create`, `pty.spawn`, `pty.describe`, `pty.read`, `pty.write`, `pty.resize`, `pty.stop`, `pty.list`); those are not Settings Tauri commands. Preserve its owner/epoch identity rather than translating it into paired-daemon generations.

### 6.3 Events and refresh policy

- Existing Settings paired inventory uses adapter refresh/mutation completion and store subscriptions. Existing SSH inventory uses cache listeners. Neither is currently driven by a dedicated native machine-inventory Tauri event. Initial rollout should refresh on entry, explicit Refresh, completed mutation and relevant window focus; indicate stale snapshots rather than claiming continuous presence.
- `worktree_changed` is emitted by SSH worktree create/delete and consumed through `tauri.ts`. Refresh only the owning workspace/project's worktrees.
- `terminal_remote_status`, `terminal_output`, `terminal_lifecycle` and native terminal metadata events already serve workspace session/recovery UI. Settings can consume a derived summary if necessary; it must not start a second terminal stream or own reconnection.
- `SubscribeRemoteEvents` / `remote_selection_requested` and `cmd_remote_set_active_selection` serve inbound desktop-selection control. They are not generic inventory or gateway-health events and must remain separate.
- Paired capability `machineEventsV1` exists, and native machine event handling lives in `src-tauri/src/remote/machine_events.rs`. The inspected Settings adapter does not subscribe to such events. A later native event bridge would be additive work, with host ID/generation/sequence checks, snapshot recovery and unsubscription; do not present it as an available renderer event today.
- If continuous gateway/inventory status is needed, propose a sanitized `remote_inventory_changed` / `remote_gateway_status_changed` event contract in a later phase. These names are proposals, not existing commands/events. Subscribe before requesting the initial snapshot and ignore stale responses. Do not use timer-based guessed readiness.

### 6.4 Invariants and migrations

- Preserve paired `hostId = relay origin + machine ID`, native generation fences and forgotten-host tombstones. A fixed default does not permit rewriting old host IDs to the new relay.
- Desktop `remoteHostStore` must remain token-free. Browser mirror persistence and direct-path hints are a different context; do not migrate them into desktop machine authority.
- Preserve `pairedHostInventory.migrateLegacy`: read credentials only in the migration adapter, migrate to native, verify receipt through readback, and remove only the matching legacy copy. Pending migration remains visible and retryable.
- Preserve SSH IDs, disabled flags, config import tombstones and native canonical host/path project IDs. No cross-transport auto-merge.
- Existing custom-relay/local-only gateway configurations and paired hosts must not silently reconnect elsewhere or gain public exposure. Preserve their current effective connection, mark them Legacy in details, and offer an explicit migration requiring a new PIN on the fixed relay. New pairing always uses the fixed relay. Whether legacy connectivity is eventually removed is a product decision, not an automatic data rewrite.
- Keep label rename out of the first implementation unless native metadata update is added. Re-pair is credential replacement, not a rename workaround.

## 7. Independently shippable implementation phases

Touch lists are estimates of implementation files; listed new files are proposals. Each implementation phase should add/update behavioral tests in the corresponding existing test families, but no tests are added for this prose document.

### Phase 1: Unified navigation, honest direction labels (about 5-8 implementation files)

Deliver Remote with Machines / Access to This Machine / Connection Details, initially reusing current forms and operations. Separate inbound devices from outbound inventory. Preserve all non-remote tabs. Route legacy `ssh` opens to Machines/SSH. Existing functionality remains reachable even before later polish.

Estimated touches: `SettingsDialog.tsx`, `settings/types.ts`, `settings/RemoteAccessSection.tsx`, `settings/PairedMachinesSection.tsx`, `settings/SshSection.tsx`, new `settings/RemoteSection.tsx`, `App.tsx` if needed for typed section context. Keep compatibility exports used by other consumers.

Acceptance: every existing tab/section is reachable; old SSH entry requests resolve correctly; enabling/disabling inbound access leaves outbound inventory unchanged; no credentials or storage formats change. Rollback is navigation-only.

### Phase 2: Fixed-relay PIN-only pairing and typed project handoff (about 7-10 files)

Deliver the standard fixed-origin PIN form, generation-bound automatic capability checking, safe generic failures, and working paired-machine Open/Add Project from Settings. Reuse the SSH callback path with a discriminated target; add preselected host support to the paired form. Make the gateway's new/default relay selection explicit without silently migrating old configurations.

Estimated touches: `settings/RemoteSection.tsx`, `settings/PairedMachinesSection.tsx`, `settings/RemoteAccessSection.tsx`, `lib/pairedHostInventory.ts`, `components/PairedDaemonProjectForm.tsx`, `components/ProjectDialogs.tsx`, `components/SettingsDialog.tsx`, `App.tsx`, optionally a small shared typed navigation definition.

Acceptance: pairing submission needs only PIN, uses the exact fixed origin, preserves leading zeros, clears secrets, handles mirror-grant rejection, and opens a project on the selected host without reselection. Capability failure/forget/re-pair invalidates stale work. Initially keep existing re-pair semantics only where identity can be safely enforced; do not ship silent host replacement.

### Phase 3: Shared inventory/detail and remote project browsing (about 8-12 files)

Deliver the derived machine inventory, filters/search, consistent readiness/action layout, imported SSH source management inside Add Machine, and Local/Remote Add Project entry with shared machine selection. Add paired existing-project browsing and workspace-owned worktree navigation without moving destructive workspace controls into Settings.

Estimated touches: new `settings/RemoteMachinesSection.tsx`, new machine-detail component and derived presentation module; `settings/SshSection.tsx`, `settings/PairedMachinesSection.tsx`, `ProjectDialogs.tsx`, `PairedDaemonProjectForm.tsx`, `RemoteDirectoryPicker.tsx` only if its interface needs contextual presentation, `lib/pairedDaemonProject.ts` only if a UI adapter seam is needed, `App.tsx`, `state/workspaceStore.ts` / `inactiveProjectWorktrees.ts` only for the project handoff/read model.

Acceptance: offline/disabled machines persist visibly; matching labels never conflate identities; host/generation changes discard stale folder selections; both types open the correct project/worktree; existing local terminals remain running. Failure of one inventory source does not hide the other.

### Phase 4: Explicit SSH readiness and helper setup (about 6-10 files)

Deliver separate Connection, Agent Integration and Terminal Helper evidence, first using existing probe/prepare commands and the existing project-scoped explicit-binary installer. Show installation write effects before confirmation. Missing helper errors lead to an actionable setup path, not another misleading connection test. Do not block earlier phases on automatic provisioning.

Estimated touches: SSH machine detail / `settings/SshSection.tsx`, `lib/sshHosts.ts`, `lib/remoteProject.ts`, `src-tauri/src/ipc/ssh.rs` if a safe inspection summary is added, `src-tauri/src/ssh/helper_setup.rs`, `src-tauri/src/ssh/helper_assets.rs` only for approved artifact integration, and `src-tauri/src/lib.rs` only for new IPC registration. Project handoff may need a helper-setup context.

Acceptance: a reachable host with missing helper is not marked terminal-ready; preparation does not claim helper installation; explicit install failures retain actionable stage/errors; configured-host changes invalidate stale setup results. Automatic provisioning is a separate decision and release, not hidden inside Test Connection.

### Phase 5: Optional additive authority and live status contracts (roughly 10-16 files; split releases by contract)

Ship independently: (a) expected-identity re-pair and safe typed pairing errors, (b) desktop machine-PIN issuance if approved, (c) sanitized live status bridge if approved. Keep snapshot refresh working with old daemons; unsupported operations fail closed.

Estimated touches: `lib/pairedHostInventory.ts`, `lib/tauri.ts`, `state/remoteHostStore.ts` only for new non-secret view fields, remote sections; `src-tauri/src/ipc/paired_host.rs`, `ipc/remote.rs`, `paired_host/service.rs`, `paired_host/inventory.rs`, `daemon/protocol.rs`, `daemon/client.rs`, `daemon/server.rs`, `remote/machine_events.rs` if event bridging is selected, and `src-tauri/src/lib.rs` for command registration. Do not modify the transport protocol solely for a visual rename.

Acceptance: old daemon rejects machine issuance instead of producing a mirror grant; expected-machine mismatch cannot replace credentials; no token/PIN/raw credential error appears in native events; out-of-order snapshots/events cannot resurrect forgotten generations. Feature availability derives from native capabilities, with explanatory disabled states.

### Rollout and future verification

Order: navigation compatibility -> fixed-relay new pairing and handoff -> inventory/browsing parity -> SSH helper clarity -> optional additive daemon contracts. Run existing credential migration before inventory hydration, as bootstrap already does. Keep legacy identity records untouched through all UI phases. Never couple navigation rollout to a destructive host-store conversion.

For the future implementation, use existing tests near `settings/SshSection.test.tsx`, `lib/sshHosts*.test.*`, `lib/pairedDaemonProject.test.ts`, `lib/pairedDaemonRollout.test.ts`, `lib/pairedActionBoundaries.test.tsx`, `state/workspaceRestore.test.tsx`, `state/workspaceStore.sshReattach.test.ts`, and native `paired_host`, SSH helper/bridge/project test families. Add behavior assertions for typed routing, fixed-origin request values, secret clearing, stale-result rejection, generation/identity checks and preserved workspaces; do not pin marketing prose.

Async tests must subscribe to the exact state/event before triggering an operation and use bounded completion signals; no fixed sleeps or polling-to-green. PIN expiry tests may use a controlled clock because time is the behavior under test. For each code phase, perform changed-file diagnostics, relevant deterministic tests, build as appropriate to its scope, and real desktop exercises of paired machine and SSH flows (including offline, wrong scope, removed/edited host, unavailable helper and incompatible daemon). Those validations are future implementation gates, not claimed results of this document-only review.

## 8. Product decisions requiring resolution

1. **Legacy relay/local-only support:** recommendation: fixed relay for every new paired machine, preserve existing legacy connectivity read-only, require explicit re-pair for migration. Decide support lifetime and whether legacy gateway mode remains configurable outside the standard Settings path. Never silently broaden local-only access.
2. **PIN-only reach:** recommendation: PIN-only desktop machine setup and PIN-first inbound device setup; remove QR/link from the primary desktop card. Decide whether optional QR convenience remains for phone clients elsewhere. It must never be required or restore a custom-relay input to the new pairing form.
3. **Desktop machine-grant issuance:** recommendation: add an explicit “Pair another desktop” scope only after native capability/issuance support is exposed. Until then, retain owner CLI issuance instructions and label existing Settings-generated codes as device access. Is GUI issuance required for the first release?
4. **Machine naming:** current PIN exchange inventory uses a caller-supplied display label, not a verified remote friendly name. Recommendation: use Machine plus short identity initially; add a separate native rename-metadata operation later. Decide whether naming is essential for launch without becoming a pairing prerequisite.
5. **SSH provisioning ownership:** recommendation: explicit helper setup first; distinguish agent extension from terminal helper. Decide whether automatic helper distribution/download, supported platform/architecture matrix, updates and installation consent belong to this release. Asset utilities alone are not a shipped provisioning contract.
6. **SSH key trust/authentication UX:** recommendation: retain agent/key and native SSH behavior; report trust/authentication failure clearly without disabling host-key checks. Decide whether in-app first-use host-key approval or interactive authentication is desired; neither is implied by this redesign.
7. **Multiple connections to one physical machine:** recommendation: retain separate Paired and SSH rows with explicit type badges. Decide whether a later user-controlled association is useful; do not auto-merge by label, hostname or path.
8. **PIN syntax and failure taxonomy:** confirm whether owner-issued machine PINs are strictly six decimal digits. Recommendation: preserve the native-supported input contract until confirmed, and add safe typed expired/rate-limit/wrong-scope errors before making those specific UI claims.
9. **Scope of Settings project discovery:** recommendation: show remote daemon registered projects for Paired and this desktop's registered projects for SSH, then hand off to workspace browsing. Decide whether organization-wide or arbitrary remote repository discovery is wanted; that is additional backend/product work, not a Settings layout change.

The first three phases do not require replacing stores, rewriting transports, deleting saved references, or redesigning non-remote Settings. The core outcome is a single understandable place to connect to machines and separately control who can connect to this machine.
