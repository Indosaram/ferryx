# Remote project runtime integration

Task: `st_01a0777f`
Date: 2026-09-06
Status: Implemented; focused verification passes. Desktop SSH proof remains the lead's separate QA gate.

## Delivered behavior

- App preserves `RegisteredProject.target` when loading WebView storage, recovering the native session catalog, canonicalizing projects, merging registration results, and saving sessions. Missing targets retain legacy local behavior. Malformed targets and reserved `ssh:` records without a valid SSH target are rejected, not converted into local projects.
- Active SSH projects call the existing `registerRemoteProject` adapter on startup/selection and on focus retry after registration failure. App adopts the server-returned workspace ID, canonical remote root, Git root, and host ID. No local `registerProject` call receives the remote project.
- Direct SSH workspaces expose one root row, including Git-backed remote directories. Active refresh bypasses local Git listing; inactive refresh retains the full target and renders metadata only. Selecting an inactive remote root revalidates it via active registration before opening a terminal.
- Remote root rows carry an explicit workspace owner, preventing collisions with identical paths on the local machine or a second SSH host. Sidebar labels use folder plus configured host label, falling back to host ID rather than displaying the opaque workspace hash.
- Sidebar Git-worktree and local-reveal actions are disabled with an explanation for SSH projects. Disabled native-menu callbacks are guarded too; the remote root does not use the local worktree-row reveal surface. App also rejects remote worktree creation and excludes remote roots/sessions from local DAG filesystem watchers.
- New terminal tabs, split panes, queued root selection, and restored sessions retain the backend remote workspace ID. Restoration completion is observable via `useSyncExternalStore`: queued SSH-root selection waits for restoration instead of losing the request while the store refuses to create a tab. Failed backend recovery now propagates errors to App while still releasing its in-flight retry guard.
- The actual chooser's Settings CTA calls the stable App callback for `handleOpenSettings("ssh")`, not inbound Remote Access.

## Backend contract consumed

Read the actual `ui/src/lib/remoteProject.ts`, `ui/src/lib/tauri.ts`, chooser evidence, plan, and evolving `src-tauri/src/ipc/project_remote.rs` / `src-tauri/src/ssh/projects.rs`.

The adapter invokes:

```text
cmd_project_register_remote
{ request: { workspaceId, hostId, repoPath } }
-> { workspaceId, repoRoot, gitRoot, hostId, hostLabel }
```

The backend owns canonical host/path identity and returns reserved opaque `ssh:<sha256>` workspace IDs. No placeholder directory or installed remote helper is assumed. The backend owner corrected remote re-registration to accept returned reserved IDs; this was confirmed by re-reading `project_remote.rs` after the correction. No backend files were edited by this lane.

No `hostLabel` field was added to `RegisteredProject`: Sidebar consumes the authoritative SSH-host hook. Additions to persistence are optional `target` and `gitRoot` on `PersistedWorkspace`; the Rust session model's existing flattened extra fields preserve them. `Worktree.workspaceId` is optional explicit ownership metadata for synthesized/restored remote roots.

## Behavioral RED evidence

Before production edits, the existing seams failed with actual local registration calls, not missing imports or source-text assertions:

```text
App: loads a stored remote target
  registerProject({ workspaceId: "ssh:remote", repoPath: "/srv/repo" })
App: handles the chooser's registered remote target
  registerProject({ workspaceId: "ssh:remote", repoPath: "/srv/repo" })
Inactive refresh:
  registerProject({ workspaceId: "ssh:build", repoPath: "/srv/repo" })
Test Files: 2 failed
Tests: 3 failed
```

Full capture: `runtime-red.log`.

## Verification

Final focused command (one complete passing run):

```sh
bun run --cwd ui test \
  src/App.remote.test.tsx \
  src/state/inactiveProjectWorktrees.remote.test.tsx \
  src/components/Sidebar.remote.test.tsx \
  src/lib/projectIdentity.test.ts \
  src/state/inactiveProjectWorktrees.test.tsx \
  src/state/workspaceRuntime.test.tsx \
  src/state/workspaceRestore.test.tsx \
  src/state/workspaceStore.test.tsx \
  src/lib/sessionPersistence.test.ts \
  src/lib/worktreeOwnership.test.ts \
  src/components/Sidebar.test.tsx \
  src/components/Sidebar.dnd.test.tsx \
  src/components/Sidebar.activity.test.tsx
```

Result: **13 files, 170 tests passed**. Capture: `runtime-green.log`.

The new App suite mounts the real App, chooser, sidebar, workspace store/runtime, and restoration coordinator. It exercises stored targets, actual chooser submission, Settings routing, canonical ID/path adoption, new tabs, split panes, a deferred restoration/switch race with identical local/remote paths, failed-host focus retry, stale registration completion, malformed records, and native-startup restored-session/save routing. Platform IPC and terminal rendering are test boundaries, not replacements for the lifecycle under test.

New remote tests use controlled promises, awaited signals, and React `act`, without polling or sleeps. Existing inactive-project tests were converted from polling/a fixed sleep to awaiting the exact service promises. The backend-recovery test now asserts rejection propagation and deterministic spawn-start signals while retaining retry coverage.

Fresh LSP diagnostics returned **no diagnostics** on all 17 changed TypeScript/TSX files, including the changed existing App and store tests.

### Separate existing App suite failure

```sh
bun run --cwd ui test src/App.test.tsx
```

Result: **88 passed, 1 failed**. The remaining failure is:

```text
checks for a signed update when the native app starts
expected updater.checkForUpdate to have been called once
```

Capture: `runtime-app-related.log`. This is independently reproduced using the HEAD versions of App and App.test in temporary, subsequently removed files: `runtime-updater-baseline.log`. The test partially mocks the exported `checkForUpdate`, but `startUpdatePolling` calls its original module-local function. No updater production/test changes were made and the failing test was not removed or skipped in the full App run.

The App local chooser integration test was updated for the new explicit Local card and dialog role; its branch-dropdown flow passes.

## Boundaries and remaining QA

- No full build was run, as requested while other lanes edit. No desktop or live SSH execution is claimed here; the lead owns real-VM/desktop proof.
- Direct SSH Git worktree operations, local file-manager reveal, and local DAG watching for remote directories are intentionally unsupported/disabled. This is root-only SSH terminal operation, not remote Git/helper integration.
- At the last read, the chooser owner's `RemoveProjectDialog` still used `project.workspaceId` in its confirmation sentence. The lead was notified to use folder/host display there without replacing the actual backend identity. Sidebar labels are already corrected.
- No chooser, adapter, SSH-host-store, Settings, or backend writes; no commits. Foreign dirty files were preserved.
