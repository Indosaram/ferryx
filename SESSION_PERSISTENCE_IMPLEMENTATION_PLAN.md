# rorca Session Persistence & Updater Safe-Restart Implementation Plan

## 1. Objective

Implement durable **Session Save & Restore / State Persistence** for rorca so that a normal quit, application crash, OS restart, or automatic-update restart does not discard the user's workspace/session topology.

The target behavior is:

- registered workspaces survive process restart;
- the last active workspace/worktree survives restart;
- terminal tabs and their labels survive restart;
- split layout and active primary/secondary tabs survive restart;
- terminal session metadata survives restart;
- a bounded terminal replay buffer/scrollback survives restart;
- terminals that were alive when the application stopped are recreated as **new PTYs** in the safest known CWD;
- stale/deleted workspaces and worktrees degrade gracefully instead of causing startup failure;
- an automatic update cannot restart the application until the latest session checkpoint has been durably committed;
- normal window close goes through the same persistence barrier;
- an updater restart can bypass user-interaction close prompts where appropriate, but it must **never bypass the persistence checkpoint**.

This document is an implementation plan only. It does not implement the feature.

---

## 2. Scope and Non-Goals

### In scope

1. Persistent workspace registration and active-workspace state.
2. Persistent worktree snapshot/reconciliation metadata.
3. Persistent terminal logical sessions.
4. Persistent terminal tabs and split layout.
5. Bounded terminal output replay/scrollback.
6. Safe CWD restoration with validation/fallback.
7. Restore lifecycle and degraded recovery UX.
8. Native/local session store architecture.
9. Periodic and event-driven checkpointing.
10. Window-close guard and updater-safe restart barrier.
11. Tauri updater integration points.
12. Unit, integration, crash-recovery, and E2E verification.

### Explicit non-goals for the first implementation

- Reattaching to the exact same OS PTY master after the rorca process has died.
- Resuming the exact foreground child process that was running inside the old shell.
- Re-executing arbitrary foreground commands after restart.
- Persisting arbitrary environment variables, shell secrets, command history, or process argv.
- Treating a persisted worktree snapshot as more authoritative than the current Git repository.
- Unlimited terminal history.
- Cloud/session synchronization.

True process/PTY continuation would require a persistent multiplexer or daemon such as tmux/zellij/a dedicated PTY broker. That is a separate architecture and should not be mixed into the first persistence implementation.

---

## 3. Current rorca State Verified

## 3.1 Backend

### `src-tauri/src/lib.rs`

Current startup creates only in-memory runtime services:

- `PtyManager`;
- `TerminalOutputHub`;
- `TerminalService`;
- `RemoteGatewayState` / `RemoteGatewayManager`;
- `WorkspaceRegistry`.

`WorkspaceRegistry` attempts to register `"default"` against the process current directory and is then registered as Tauri managed state.

There is currently:

- no session persistence service;
- no database;
- no app-data-dir initialization;
- no close coordinator;
- no updater plugin initialization;
- no updater IPC.

### `src-tauri/src/worktree/registry.rs`

`WorkspaceRegistry` is an `Arc<RwLock<HashMap<String, WorktreeManager>>>`.

Important behavior:

- all workspace registration is in memory;
- registration validates workspace IDs;
- `register(...)` canonicalizes/validates the repository through `WorktreeManager::try_new(...)`;
- duplicate IDs pointing at different repositories are rejected;
- terminal targets are resolved through registered workspaces/worktrees rather than trusting arbitrary frontend paths.

This validation must remain authoritative after persistence is added.

### `src-tauri/src/ipc/project.rs`

`cmd_project_register` accepts:

- `workspaceId`;
- `repoPath`.

It returns the canonical `repoRoot` resolved from the registry.

Therefore a persistent workspace record must contain at least:

- stable workspace ID;
- canonical repository root.

### `src-tauri/src/worktree/model.rs`

Current durable-looking domain values already exist for:

- `Worktree`;
- `WorktreeIdentity { ws_id, slug }`;
- branch/head/path state.

Persisted worktrees should be treated as a **last-known snapshot**, not as the source of truth. On every restore, current `git worktree list` state must win.

### `src-tauri/src/terminal/pty.rs`

`PtyManager` owns an in-memory map:

```text
backend session id -> Arc<PtySession>
```

A new UUID is generated for each PTY spawn.

The PTY manager currently stores live handles only. Once the application process exits, those handles are gone. Even if a child process temporarily survives a crash, rorca cannot safely reopen the old portable-pty master from the new process.

### `src-tauri/src/terminal/session.rs`

`PtySession` currently contains:

- generated runtime `id`;
- PTY master/writer/reader;
- child handle;
- lifecycle state;
- cols/rows;
- writer lease;
- reader task.

It exposes useful runtime state such as:

- `pid()`;
- `writer_worktree()`;
- `writer_owner_id()`;
- `get_size()`;
- lifecycle state.

The generated PTY ID and PID are **ephemeral runtime identifiers** and must not be used as the durable session identity.

### `src-tauri/src/terminal/output_hub.rs`

`TerminalOutputHub` already maintains a bounded in-memory buffer per backend session.

Current limits/behavior:

- default buffer capacity: **512 KiB**;
- output is stored as raw `Vec<u8>` chunks;
- `subscribe(...)` can return a snapshot plus a live broadcast receiver;
- the buffer disappears when the process exits.

This existing ring-buffer behavior is the best source for the first persistent terminal replay buffer. The persistence implementation should reuse the same bound instead of introducing a second unrelated history policy.

### `src-tauri/src/terminal/service.rs`

`TerminalService` currently:

- spawns a PTY in a validated worktree;
- registers the PTY in `TerminalOutputHub`;
- pumps PTY output into the hub;
- attaches to existing live sessions;
- writes/resizes/signals/closes sessions.

It currently has no durable metadata map linking a runtime backend PTY ID to a stable logical session ID.

### `src-tauri/src/ipc/terminal.rs`

`cmd_terminal_spawn` currently:

1. validates workspace/worktree through `WorkspaceRegistry`;
2. builds the default shell;
3. sets its CWD to the validated target;
4. spawns a PTY;
5. returns only `{ sessionId }`;
6. emits `terminal_lifecycle` and base64 `terminal_output` events.

`cmd_terminal_list` currently reports only:

- runtime session ID;
- writer worktree path.

The restore implementation needs richer logical metadata while preserving the existing path-validation boundary.

### Tauri/updater configuration

Verified current state:

- `src-tauri/Cargo.toml` has no `tauri-plugin-updater` dependency;
- `src-tauri/src/lib.rs` initializes no updater plugin;
- `src-tauri/tauri.conf.json` has no updater endpoint/public-key configuration;
- `src-tauri/capabilities/default.json` contains only core/window-drag permissions.

Therefore updater integration is new work in rorca rather than a small wrapper around an already initialized plugin.

---

## 3.2 Frontend

### `ui/src/state/workspaceStore.ts`

`WorkspaceState` currently contains:

```ts
{
  worktrees,
  activeWorktreePath,
  sessions,
  layout,
}
```

`TerminalSession` objects are created in memory after `spawnTerminal(...)` returns.

The store currently generates two different IDs:

- frontend session ID (`session:<random>`);
- backend PTY session ID (`backendSessionId`).

The frontend session ID is already close to the stable identity needed for persistence, but it is currently created **after** backend spawn and is not passed to Rust.

### `ui/src/state/layout.ts`

The layout model already captures the persistent topology that must survive restart:

- tabs;
- primary tab;
- secondary tab;
- split orientation;
- nested split metadata.

`normalizeLayout(...)` already provides useful repair behavior for invalid tab references and should also be applied to restored state.

### `ui/src/state/workspaceRuntime.ts`

Current refresh behavior is important for restore ordering:

1. list worktrees;
2. sync them into store;
3. select the preferred worktree;
4. call `ensureTabForWorktree(...)`.

If this runs before hydration, it can create a new terminal/tab before the persisted tabs are restored. A startup hydration gate is therefore required.

### `ui/src/App.tsx`

Current browser storage persists only a subset of application state:

- project list (`rorca.projects`);
- active project (`rorca.active-project`);
- sidebar visibility.

The actual terminal/workspace state remains in-memory.

Project switching is currently synchronous from the UI's perspective. The persistence implementation must guarantee that the old workspace is checkpointed before or as part of switching to another workspace.

### `ui/src/lib/terminalEvents.ts`

There is a second 512 KiB-equivalent frontend backlog:

- decoded text per backend session ID;
- bounded to 512 KiB characters;
- replayed to newly mounted terminal panes.

This backlog also disappears when the renderer restarts.

The restore design should seed persisted history into this event bus before the restored `TerminalPane` subscribes.

### `ui/src/components/TerminalPane.tsx`

`TerminalPane` creates a fresh xterm instance whenever the frontend logical `session.id` changes.

It subscribes to `terminalEventBus` by current `backendSessionId` and writes replay/live output to xterm.

This means the cleanest restore path is:

1. keep the durable logical session ID stable;
2. obtain a **new** backend PTY ID;
3. seed old replay history under that new backend ID;
4. mount the same logical terminal/tab against the new backend ID.

### `ui/src/lib/tauri.ts`

This file is already rorca's typed native bridge.

New persistence/updater operations should continue to use this facade rather than spreading raw `invoke(...)` calls throughout React components.

---

## 3.3 Original Orca behavior verified in `ui/original-dist`

The bundled original Orca code provides useful behavioral guidance.

### `updater-beforeunload-*.js`

The original bundle contains:

- `registerWindowCloseGuard(...)`;
- `dispatchWindowCloseRequest(...)`;
- `setWindowCloseRequestHandler(...)`;
- `registerUpdaterBeforeUnloadBypass(...)`;
- `isIntentionalAppRestartInProgress()`.

It tracks updater/restart intent separately from ordinary window close.

### `shutdown-checkpoint-guard-*.js`

The original bundle has a once-per-shutdown checkpoint guard:

- persist once;
- veto unload if persistence fails;
- reset the checkpoint if unload/restart is aborted.

### `App-*.js`

Before unload, original Orca:

- captures current session state;
- stages a native checkpoint;
- installs a `beforeunload` handler;
- resets on updater/restart abort;
- handles native close requests through a close dispatcher.

### `lazy-with-retry-*.js`

The original restart flow explicitly dispatches a cancelable `beforeunload` event and refuses the restart when the renderer checkpoint did not complete.

The relevant event family includes:

- updater quit/install started;
- updater quit/install aborted;
- app restart started;
- app restart aborted;
- renderer unload prevented.

### Important interpretation for rorca

The useful parity rule is **not** “skip beforeunload during updater restart.”

The correct rule is:

> Intentional restart may bypass user-decision prompts, but it must still cross the durable session checkpoint barrier.

rorca should keep those two responsibilities separate.

---

# 4. Core Architecture Decision

## 4.1 Recommended storage: SQLite Local State DB

Use SQLite as the durable local state store, with:

- small relational tables for app/workspace metadata;
- a versioned JSON snapshot per workspace for the evolving UI/session graph;
- a separate bounded BLOB per logical terminal session for scrollback/replay data.

Recommended database file name:

```text
session-state.sqlite3
```

The path must be resolved dynamically through Tauri's application data directory API, not hard-coded.

Conceptually:

```text
app.path().app_data_dir()/session-state.sqlite3
```

The exact platform path is intentionally left to Tauri.

## 4.2 Why SQLite over one JSON file

### JSON-only advantages

- very simple initial implementation;
- easy manual inspection;
- no DB dependency.

### JSON-only disadvantages for this feature

- a 512 KiB buffer per terminal makes whole-file rewrites expensive;
- terminal output and UI-state saves arrive at different rates;
- crash-safe write requires temp file + fsync + atomic rename discipline;
- concurrent save requests need a serialization layer anyway;
- schema migration gets harder as state grows;
- cleanup of orphan buffers requires rewriting a potentially large document;
- detecting partial/corrupt state is less structured.

### SQLite advantages

- atomic transactions;
- WAL journaling;
- one serialized writer connection;
- cheap metadata updates;
- independent BLOB updates;
- schema versioning/migrations;
- structured corruption checks;
- easy stale-buffer cleanup;
- durable checkpoint receipts/revisions.

### Decision

Use **SQLite as the source of truth**.

A JSON export can be added later for diagnostics or support bundles, but it must not be the primary runtime store.

## 4.3 Why not use browser localStorage as the source of truth

localStorage is useful for small UI preferences but is the wrong authority for this feature because:

- Rust services need session/workspace state before/while the renderer is hydrating;
- terminal replay bytes originate in Rust;
- native close/update coordination needs a native durability barrier;
- a renderer reset should not be able to lose the native session registry.

Existing project/sidebar localStorage can remain temporarily and be migrated gradually.

---

# 5. Persistent Identity Model

## 5.1 Separate logical session identity from runtime PTY identity

This is the most important model change.

### Durable ID

Add a stable:

```text
logical_session_id
```

Example frontend representation:

```text
session:<uuid>
```

This ID survives application restarts.

### Runtime-only ID

Keep the backend PTY ID as:

```text
backend_session_id
```

This ID is regenerated every time a PTY is spawned/restored.

### Never persist as authoritative

Do not persist the following as resumable identifiers:

- `backendSessionId`;
- OS PID;
- PTY master handle;
- child process handle;
- writer lease instance.

They are meaningful only for the current process lifetime.

## 5.2 New-spawn flow

Today the frontend generates its logical session ID after backend spawn.

Change the flow to:

1. frontend allocates `logicalSessionId`;
2. frontend calls `cmd_terminal_spawn` with that logical ID;
3. backend validates uniqueness;
4. backend spawns a new PTY and maps logical ID -> runtime backend ID;
5. backend returns both IDs where useful;
6. UI stores the stable logical ID and current backend ID separately.

This establishes one durable identity for tabs, session metadata, and scrollback.

---

# 6. Persistent Data Model

## 6.1 Database tables

Use a deliberately small schema.

### `schema_meta`

Purpose: database migration/version information.

Suggested fields:

```text
schema_version INTEGER NOT NULL
created_at_ms INTEGER NOT NULL
migrated_at_ms INTEGER NOT NULL
```

### `app_state`

One-row application persistence metadata.

Suggested fields:

```text
id INTEGER PRIMARY KEY CHECK (id = 1)
active_workspace_id TEXT NULL
checkpoint_revision INTEGER NOT NULL
last_checkpoint_reason TEXT NULL
last_checkpoint_at_ms INTEGER NULL
last_clean_shutdown INTEGER NOT NULL DEFAULT 0
last_shutdown_reason TEXT NULL
```

`checkpoint_revision` must monotonically increase after every successful full checkpoint.

### `workspaces`

Persistent workspace registry.

Suggested fields:

```text
workspace_id TEXT PRIMARY KEY
repo_root TEXT NOT NULL
last_opened_at_ms INTEGER NOT NULL
last_active_at_ms INTEGER NOT NULL
updated_at_ms INTEGER NOT NULL
```

`repo_root` stores the canonical path returned by the current registry logic.

### `workspace_snapshots`

One versioned JSON snapshot per workspace.

Suggested fields:

```text
workspace_id TEXT PRIMARY KEY
snapshot_schema_version INTEGER NOT NULL
snapshot_revision INTEGER NOT NULL
payload_json TEXT NOT NULL
updated_at_ms INTEGER NOT NULL
```

A full workspace snapshot is intentionally stored as a versioned JSON payload because layout/session graph fields will evolve faster than core native registry metadata.

### `terminal_scrollback`

One bounded replay buffer per stable logical session.

Suggested fields:

```text
logical_session_id TEXT PRIMARY KEY
workspace_id TEXT NOT NULL
buffer BLOB NOT NULL
byte_len INTEGER NOT NULL
truncated INTEGER NOT NULL
updated_at_ms INTEGER NOT NULL
```

The first implementation should keep the current backend capacity of **512 KiB per logical session**.

Optional later field:

```text
encoding_version INTEGER
```

This can distinguish raw PTY byte replay from a future serialized xterm representation.

---

## 6.2 Workspace snapshot JSON

Recommended conceptual TypeScript shape:

```ts
type PersistedWorkspaceSnapshotV1 = {
  schemaVersion: 1;
  workspace: {
    workspaceId: string;
    activeWorktreePath: string | null;
    savedAtMs: number;
  };
  worktrees: PersistedWorktreeSnapshot[];
  sessions: PersistedTerminalSessionV1[];
  tabs: PersistedTerminalTabV1[];
  layout: PersistedLayoutV1;
};
```

The Rust equivalent should use Serde with an explicit schema version and strict validation where practical.

---

## 6.3 Persisted Workspace

Persist:

- `workspaceId`;
- canonical `repoRoot` in the `workspaces` table;
- active worktree path;
- last active/open timestamps.

Do not assume the path still exists at restore time.

On restore:

1. canonicalize/validate again;
2. verify it is still a Git repository accepted by `WorktreeManager`;
3. register it through `WorkspaceRegistry`;
4. if invalid, mark the workspace unavailable and continue restoring other workspaces.

---

## 6.4 Persisted Worktree

Persist the last-known `Worktree` fields:

- path;
- head;
- branch;
- bare;
- detached;
- locked;
- prunable;
- derived `WorktreeIdentity` where present.

Purpose:

- restore tab/worktree associations;
- show meaningful degraded-state UI when a worktree disappeared;
- reconcile persisted paths with current Git state.

Do **not** automatically recreate a missing worktree during startup.

Current Git state remains authoritative.

Reconciliation should produce something like:

```ts
type RestoredWorktreeStatus =
  | { state: "valid"; current: Worktree }
  | { state: "missing"; persisted: PersistedWorktreeSnapshot }
  | { state: "moved-or-changed"; persisted: PersistedWorktreeSnapshot; current: Worktree };
```

---

## 6.5 Persisted Terminal Session Metadata

Suggested model:

```ts
type PersistedTerminalSessionV1 = {
  logicalSessionId: string;
  workspaceId: string;
  worktree: WorktreeIdentity | null;

  spawnCwd: string;
  lastKnownCwd: string | null;
  cwdSource: "spawn-target" | "osc7";

  cols: number;
  rows: number;

  terminalTitle: string | null;
  lifecycleAtCheckpoint: "starting" | "working" | "waiting" | "exited" | "failed";
  restorePolicy: "respawn" | "do-not-respawn";

  createdAtMs: number;
  lastActiveAtMs: number;
};
```

### Must not be persisted in this payload

- runtime `backendSessionId`;
- PID;
- writer lease object;
- arbitrary environment variables;
- arbitrary shell/process argv.

### Restore policy

At checkpoint time:

- `starting` / `working` / `waiting` -> `respawn`;
- `exited` / `failed` -> normally `do-not-respawn`.

This avoids resurrecting terminals that had already intentionally exited.

The UI can still offer `Start new shell` for an exited/failed restored tab.

---

## 6.6 CWD semantics

CWD requires an explicit precision rule.

### What rorca can guarantee today

Current `cmd_terminal_spawn` knows the initial validated CWD because it sets `CommandBuilder::cwd(...)` before spawn.

Therefore `spawnCwd` is always known.

### Dynamic CWD after `cd`

After the shell runs, the current code does not have a portable authoritative way to query the foreground shell's CWD.

Do not inject `pwd` into an interactive PTY at shutdown. That can corrupt an active command or foreground process.

Recommended approach:

1. store `spawnCwd` unconditionally;
2. add `lastKnownCwd` as optional metadata;
3. update `lastKnownCwd` only from a trusted terminal-shell signal such as OSC 7 (`file://...`) when available;
4. record the source (`cwdSource`);
5. on restore, validate/canonicalize the candidate path before using it;
6. if absent/invalid/outside the allowed workspace policy, fall back to `spawnCwd`;
7. if that also fails, fall back to the validated worktree root or mark the terminal unavailable.

This gives safe deterministic behavior without pretending rorca can always know the shell's latest directory.

### Security rule

Persisted CWD is untrusted input on next launch.

Never pass it directly to `CommandBuilder::cwd(...)` without canonicalization and workspace/worktree validation.

---

## 6.7 Persisted Tabs

Persist:

```ts
type PersistedTerminalTabV1 = {
  id: string;
  label: string;
  logicalSessionId: string;
  order: number;
};
```

Multiple tabs may reference one logical terminal session because current rorca permits shared-session tab behavior.

Do not duplicate terminal buffers per tab.

---

## 6.8 Persisted Split Layout

Persist the current `LayoutState` semantics:

- ordered tabs;
- `primaryTabId`;
- `secondaryTabId`;
- `split` (`none` / `horizontal` / `vertical`);
- `nestedSplit`.

On hydration, run the equivalent of current `normalizeLayout(...)` so invalid IDs do not crash rendering.

Rules:

- missing primary -> first valid tab;
- missing secondary -> another valid tab or disable split;
- missing nested split tab -> remove nested split;
- tabs whose session record is missing -> remove or surface as explicit failed restore entries, but never leave dangling IDs.

---

## 6.9 Terminal Buffer / Scrollback

### First implementation

Persist the same bounded raw PTY replay buffer already maintained by `TerminalOutputHub`.

Recommended limit:

```text
512 KiB per logical session
```

Add a configurable aggregate safety cap later if required, for example 16-32 MiB across all sessions.

### Why backend buffer is the first source of truth

- it captures hidden/unmounted terminal panes;
- it survives React component remounts while the process is alive;
- it does not depend on xterm being mounted;
- it already has bounded memory semantics;
- it can be checkpointed during renderer problems.

### Restore rendering

The old buffer is a **visual replay**, not process state.

Recommended UX:

1. replay persisted bytes into the new xterm instance;
2. display a small separator such as `Session restored — shell restarted`;
3. append output from the new PTY.

Do not send the replay bytes into the new PTY stdin.

### Fidelity limitation

Raw ANSI replay can be imperfect if the retained window starts in the middle of a terminal control-state sequence or alternate-screen session.

If fidelity becomes a product requirement, Phase 2+ can evaluate `@xterm/addon-serialize` and persist a versioned serialized xterm snapshot. The raw backend replay buffer should remain the crash-safe baseline.

---

# 7. Backend Storage Architecture

## 7.1 New module layout

Recommended additions:

```text
src-tauri/src/session/
  mod.rs
  model.rs
  store.rs
  writer.rs
  restore.rs
  migration.rs
  tests.rs

src-tauri/src/ipc/session.rs
src-tauri/src/ipc/updater.rs
src-tauri/src/app_close.rs
```

Names can be adjusted to current module conventions, but persistence, restore orchestration, and IPC should not be embedded directly into `lib.rs`.

---

## 7.2 `SessionStore`

Responsibilities:

- open/create SQLite DB;
- run schema migrations;
- load registered workspaces;
- load one/all workspace snapshots;
- store workspace metadata;
- store full snapshot transactionally;
- store/delete terminal scrollback;
- maintain checkpoint revision;
- mark clean/unclean shutdown;
- run integrity checks/quarantine handling.

Avoid letting arbitrary React actions issue ad-hoc SQL.

---

## 7.3 Dedicated writer actor

`rusqlite`-style synchronous database work should not block Tauri async command execution.

Recommended pattern:

```text
SessionPersistence
  -> bounded Tokio mpsc queue
  -> one dedicated DB writer task/thread
  -> one owned SQLite connection
```

Conceptual messages:

```rust
enum PersistenceCommand {
    SaveWorkspaceSnapshot { snapshot, reply },
    SaveWorkspaceRegistry { workspace, reply },
    SaveScrollback { logical_session_id, bytes, truncated },
    DeleteScrollback { logical_session_id },
    Flush { reason, reply },
    FinalizeShutdown { reason, reply },
}
```

Benefits:

- all writes are serialized;
- no `Connection` sharing complexity;
- a `Flush` message becomes a natural ordering barrier;
- shutdown can wait for every earlier queued write;
- debounce/coalescing can happen before messages are enqueued.

---

## 7.4 SQLite durability configuration

Recommended baseline:

```text
journal_mode = WAL
foreign_keys = ON
busy_timeout = bounded non-zero value
```

Choose `synchronous` deliberately during implementation and benchmark it.

For this small local-state DB, prefer correctness over micro-optimizing a few metadata commits.

The durability contract required by this feature is:

> When `prepareShutdownCheckpoint` resolves successfully, the DB transaction containing the corresponding snapshot revision and terminal buffer flush has committed successfully.

If implementation uses WAL, the shutdown barrier may also perform an appropriate WAL checkpoint so the persistence boundary is explicit and easy to test.

---

## 7.5 Atomic write semantics

SQLite transactions are the primary atomicity mechanism.

A full checkpoint should transactionally update at least:

1. workspace registry metadata affected by the snapshot;
2. workspace snapshot JSON;
3. checkpoint revision/timestamp/reason;
4. orphan scrollback cleanup decisions;
5. clean-shutdown/shutdown-intent metadata where applicable.

Terminal BLOB updates may be committed independently during periodic operation, but a shutdown checkpoint must flush the latest available terminal buffers before returning its receipt.

### If a future JSON diagnostic export is added

Use:

1. write temp file in same directory;
2. flush/fsync;
3. atomic rename over target;
4. fsync directory where supported.

Do not use that as the primary persistence algorithm.

---

## 7.6 Save scheduling

Use both event-driven and periodic persistence.

### UI/session graph debounce

Recommended starting point:

```text
250-500 ms debounce
```

Events that schedule it:

- tab added/closed/renamed;
- active tab changed;
- split enabled/disabled/rotated;
- active worktree changed;
- terminal title metadata changed;
- terminal restore status changed;
- project/workspace switched.

Persist a full compact workspace snapshot rather than trying to mirror every reducer action as a DB operation.

### Native workspace registry

Persist successful `cmd_project_register` immediately because losing project registration breaks all later restore.

### Terminal scrollback

Do **not** commit on every PTY chunk.

Recommended strategy:

- `TerminalOutputHub.publish(...)` marks a session buffer dirty;
- coalesce dirty sessions;
- snapshot dirty buffers every ~1-2 seconds;
- also flush all dirty buffers during shutdown/update barriers.

### Safety periodic checkpoint

Even with debounce, run a low-frequency safety checkpoint while a workspace is active, for example every **5-10 seconds**.

The purpose is crash recovery, not normal-save latency.

### Event priority

Immediate flush triggers:

- project switch;
- application close;
- updater install/restart;
- explicit app relaunch;
- renderer reload controlled by rorca;
- destructive workspace/worktree operation that invalidates persisted state.

---

# 8. Runtime Terminal Metadata Architecture

## 8.1 Extend `TerminalService`

Add a runtime metadata registry distinct from `PtyManager`.

Conceptual structure:

```rust
struct TerminalRuntimeMetadata {
    logical_session_id: String,
    backend_session_id: String,
    workspace_id: String,
    worktree: Option<WorktreeIdentity>,
    spawn_cwd: PathBuf,
    last_known_cwd: Option<PathBuf>,
    cols: u16,
    rows: u16,
    restore_origin: RestoreOrigin,
}
```

Maintain mappings needed for:

- backend ID -> logical metadata;
- logical ID -> active backend ID.

This avoids trying to turn `PtySession` itself into a persistence record containing unrelated UI state.

---

## 8.2 Extend spawn IPC

Evolve `SpawnTerminalRequest` to include:

```text
logicalSessionId
```

Potential response:

```ts
{
  logicalSessionId: string;
  sessionId: string; // runtime backend PTY id
}
```

Validate that one logical session cannot accidentally own two active backend PTYs unless an explicit future feature allows that.

---

## 8.3 Add restore spawn IPC

Recommended command:

```text
cmd_terminal_restore
```

Input:

- stable logical session ID;
- workspace ID;
- worktree identity;
- saved CWD metadata;
- saved cols/rows.

Backend responsibilities:

1. validate workspace still exists;
2. validate worktree still exists;
3. select safe restore CWD;
4. acquire the normal writer lease;
5. spawn a **new** default shell PTY;
6. create runtime metadata mapping;
7. return new backend PTY ID;
8. return or expose the persisted replay buffer for that logical session.

Do not bypass existing writer/worktree safety checks during restore.

---

# 9. PTY Restore Strategy

## 9.1 Default: respawn, not reconnect

On app restart:

```text
old logical session A
  old backend PTY id = X   [gone]

restore
  logical session A        [same]
  new backend PTY id = Y   [new]
```

Tabs continue referencing logical session A.

`backendSessionId` is patched to Y only after restore succeeds.

## 9.2 Do not rerun foreground commands

The restored PTY should launch the same **default shell policy** that current `cmd_terminal_spawn` uses.

Do not attempt to infer and re-run the previous foreground process.

Reasons:

- a previous command may have side effects;
- it may have already completed immediately before the crash;
- argv/environment may contain secrets;
- process reconstruction is not PTY continuation.

## 9.3 Restore dimensions

Persist cols/rows from runtime terminal metadata.

On respawn:

- initialize PTY using saved cols/rows;
- after `TerminalPane` mounts/fits, its existing resize path sends current real dimensions and becomes authoritative.

## 9.4 Restore ordering for terminal output

Race to avoid:

- a newly spawned shell can print its prompt before React has mapped the new backend session ID.

Recommended ordering strategy:

1. call `ensureTerminalEvents()` before any restore spawn;
2. the global event bus begins capturing output for all backend IDs;
3. restore spawn returns the new backend ID plus the **old persisted replay bytes only**;
4. add `terminalEventBus.prependPersistedReplay(backendSessionId, replay)`;
5. any live prompt bytes already captured remain after the persisted replay;
6. hydrate the frontend session with the new backend ID;
7. mount `TerminalPane`;
8. subscription replays old history + early new-shell output in order.

Do not overwrite live backlog when seeding persisted history.

Add explicit tests for this race.

## 9.5 Restore separator

After persisted history and before/around new-shell output, surface a non-interactive visual marker indicating that the process was restarted.

This avoids misleading the user into believing the old shell process continued running.

---

# 10. Workspace / Worktree Restore Lifecycle

## 10.1 Native startup

Recommended native bootstrap:

1. resolve Tauri app data directory;
2. initialize `SessionStore`;
3. migrate schema transactionally;
4. read prior `last_clean_shutdown` value for diagnostics;
5. mark the current boot as not-yet-clean;
6. load persisted workspace registrations;
7. validate/register each valid workspace in `WorkspaceRegistry`;
8. collect invalid workspace recovery records;
9. only after persisted registration, apply a current-directory `default` fallback if appropriate;
10. expose restore plan to the renderer.

### Important change to current `lib.rs`

Do not register the current directory as `"default"` before persisted workspaces are restored.

Otherwise a persisted `default` workspace pointing somewhere else can conflict with the current process directory and prevent correct restoration.

## 10.2 Store initialization timing

The app-data path is available from the Tauri application handle/setup lifecycle.

Recommended structure:

- create a lazily initialized `SessionPersistence` handle;
- initialize its concrete DB path inside Tauri `.setup(...)`;
- hydrate the registry there;
- keep test constructors that accept an explicit temp path.

Do not hard-wire production app-data lookups into low-level store unit tests.

---

# 11. Restore Plan IPC

Add a command such as:

```text
cmd_session_restore_plan
```

Suggested response:

```ts
type SessionRestorePlan = {
  checkpointRevision: number | null;
  previousShutdownWasClean: boolean;
  activeWorkspaceId: string | null;
  workspaces: RestoredWorkspaceDescriptor[];
  warnings: RestoreWarning[];
};
```

Each workspace descriptor can include:

- validated current workspace metadata;
- parsed persisted workspace snapshot;
- current worktree reconciliation result;
- persisted terminal scrollback indexed by logical session ID or retrievable lazily.

For large buffers, prefer lazy terminal-buffer retrieval during terminal restore rather than returning every BLOB in one giant bootstrap payload.

---

# 12. Frontend Hydration Architecture

## 12.1 Add explicit restore state

Recommended app bootstrap state machine:

```text
idle
  -> loading-native-state
  -> reconciling-workspaces
  -> hydrating-ui
  -> restoring-terminals
  -> ready

error paths:
  -> degraded-ready
  -> fatal-bootstrap-error (only for truly unrecoverable native startup failure)
```

Most restore errors should lead to `degraded-ready`, not a blank/fatal application.

## 12.2 Prevent current auto-tab race

`workspaceRuntime.refreshWorktrees()` currently auto-calls `ensureTabForWorktree(...)`.

During restoration it must not do this before persisted state is hydrated.

Recommended change:

- add a hydration/restore-ready gate to `useWorkspaceRuntime`, or
- split worktree refresh from “ensure default tab” behavior.

Rules:

- if a valid persisted snapshot exists: hydrate it first;
- only create a default first tab when there is **no usable persisted tab/session state**.

## 12.3 Reducer hydration action

Add an explicit one-shot action such as:

```ts
{ type: "HYDRATE_SESSION"; snapshot: HydratedWorkspaceState }
```

It should:

- replace stale workspace state rather than merge blindly;
- preserve stable logical session IDs;
- set every runtime backend session ID to `null` initially;
- normalize layout;
- mark sessions as restore-pending where applicable.

## 12.4 Restore result action

Add actions such as:

```text
SESSION_RESTORE_STARTED
SESSION_RESTORE_SUCCEEDED
SESSION_RESTORE_FAILED
```

Success patches:

- new runtime backend ID;
- working lifecycle;
- current runtime CWD if backend adjusted it.

Failure keeps the logical session/tab so the user can understand what failed.

## 12.5 Session restore UX

A failed restored tab should not silently disappear.

Provide a compact state with:

- `Session could not be restored`;
- reason;
- `Retry`;
- `Start new shell` where safe;
- `Close tab`.

Examples:

- missing worktree;
- deleted repository;
- CWD no longer exists;
- writer lease conflict;
- PTY spawn failure.

## 12.6 Loading UI

Avoid showing `No workspace available` while restore is still in progress.

Add a dedicated bootstrap view, e.g.:

```text
Restoring workspace…
```

If terminal restore takes noticeably longer, render tabs/layout first with per-terminal `Restoring…` placeholders rather than blocking the whole app.

---

# 13. Multi-Workspace / Project Switching

The persistence design must cover switching between registered projects, not just process startup.

Recommended flow:

1. request checkpoint of current workspace;
2. wait for checkpoint acknowledgment;
3. update active workspace ID in native app state;
4. load target workspace snapshot;
5. reconcile its worktrees;
6. hydrate target UI state;
7. restore/spawn terminals as needed;
8. mark target workspace active.

Do not let `workspaceId` prop changes accidentally retain sessions from the previous workspace.

Implementation options:

- a reducer action that fully resets/hydrates on workspace change; or
- a keyed workspace shell component that remounts state per workspace.

Whichever is chosen, add a regression test proving no session crosses workspace boundaries.

---

# 14. Persistence Scheduler in the UI

## 14.1 Central snapshot builder

Do not scatter persistence serialization through UI components.

Add something like:

```text
ui/src/state/sessionPersistence.ts
```

Responsibilities:

- build a persistence-safe snapshot from current store state;
- strip runtime-only fields;
- debounce writes;
- track dirty/clean revision;
- expose `flush(reason)`;
- freeze/reset around shutdown attempts.

## 14.2 Never persist runtime backend IDs

The snapshot builder should explicitly map `TerminalSession` into a persistence DTO and omit:

```text
backendSessionId
```

Tests should assert this.

## 14.3 Save-after-reducer pattern

`useWorkspaceStore` currently maintains `stateRef` as the freshest synchronous snapshot.

Use that to avoid stale React closure data during shutdown:

- reducer action updates `stateRef`;
- persistence scheduler reads `stateRef.current`;
- close/update checkpoint captures current state synchronously into a DTO before awaiting native I/O.

---

# 15. Shutdown Checkpoint Contract

## 15.1 Required receipt

Native checkpoint call should return a receipt similar to:

```ts
type ShutdownCheckpointReceipt = {
  checkpointRevision: number;
  persistedAtMs: number;
  workspaceId: string;
  reason: "window-close" | "update-restart" | "app-relaunch" | "project-switch";
};
```

The receipt is the proof that the requested snapshot crossed the durable boundary.

## 15.2 Idempotence

A repeated request for the exact same shutdown attempt should be safe.

Use a shutdown-attempt/token ID generated in the frontend/native coordinator.

The backend can return the already committed receipt for duplicate requests rather than writing divergent state twice.

## 15.3 Freeze semantics

After the final renderer snapshot has been captured for an intentional shutdown:

- freeze UI snapshot revision advancement for that shutdown attempt;
- keep the native PTY output buffer able to collect/flush a final tail;
- reject/disable new terminal creation and input once final shutdown is authorized.

If shutdown/update is aborted:

- reset the checkpoint guard;
- unfreeze persistence;
- resume normal interaction.

This mirrors the useful reset behavior in original Orca.

---

# 16. Window Close Guard Design

## 16.1 Frontend coordinator

Add a source module modeled on the behavior found in original Orca, for example:

```text
ui/src/lib/windowCloseGuard.ts
```

Suggested public API:

```ts
registerWindowCloseGuard(guard)
setWindowCloseRequestHandler(handler)
dispatchWindowCloseRequest(request)
registerUpdaterBeforeUnloadBypass()
isIntentionalAppRestartInProgress()
```

## 16.2 Two guard classes

Do not treat all guards the same.

### A. Decision guards

Examples:

- unsaved settings form;
- future dirty editor files;
- destructive operation confirmation.

These may return `false` and block an ordinary close.

During an intentional updater/app restart, selected decision guards may return `true` automatically.

### B. Mandatory persistence checkpoint

This is not a user prompt.

It must run for:

- ordinary close;
- updater restart;
- explicit app relaunch.

It is **never bypassed** simply because the restart is intentional.

This separation prevents the common bug where `isIntentionalAppRestartInProgress()` accidentally skips session saving.

## 16.3 Close serialization

Keep a `closeInFlight`/state-machine lock.

A second close click while the first async checkpoint is running should not:

- execute guards twice;
- create two checkpoint revisions;
- call updater install twice;
- race native window close.

---

# 17. Native Window-Close Coordination

A browser `beforeunload` callback alone is not a sufficient durability primitive because it cannot reliably await arbitrary async Tauri IPC.

Recommended design:

1. native window receives close request;
2. native side prevents immediate destruction;
3. emit a typed `window_close_requested` event to renderer;
4. frontend dispatches close guards;
5. frontend calls `prepareShutdownCheckpoint(...)` and awaits the receipt;
6. frontend calls a native `confirmWindowClose(checkpointRevision)`;
7. native verifies the checkpoint revision is current/acceptable;
8. native performs final terminal-buffer flush;
9. native authorizes/destroys the window.

## 17.1 Fail-safe

If the renderer is already dead/unresponsive, normal close must not leave an immortal process.

Native close coordination should have a bounded fail-safe timeout.

On timeout:

- log the failure;
- rely on the most recent periodic checkpoint;
- allow process exit.

This fail-safe applies to a user trying to quit a broken application.

It should **not** silently permit an updater install to proceed without its required checkpoint.

---

# 18. `beforeunload` Role

Keep `beforeunload` as a defensive checkpoint/veto signal, not the primary async database API.

Recommended behavior:

- register a small synchronous handler;
- if a shutdown checkpoint has already been durably prepared, allow unload;
- if unload occurs unexpectedly with no prepared checkpoint, prevent where the WebView permits and signal/reset the coordinator;
- controlled restart/reload paths must explicitly await `prepareShutdownCheckpoint` before triggering unload.

This is analogous to original Orca's `createShutdownCheckpointBeforeUnloadHandler(...)` model while respecting Tauri's async IPC reality.

---

# 19. Updater Integration

## 19.1 Add updater plugin

Add Tauri v2 updater support in the implementation phase:

- Rust updater plugin dependency;
- plugin initialization in `src-tauri/src/lib.rs`;
- updater endpoints/public key/artifact configuration in `tauri.conf.json`;
- minimum required capability changes only if updater calls are made directly from guest JavaScript.

Preferred architecture: expose a narrow rorca-owned updater facade/commands rather than letting React components call plugin primitives throughout the tree.

## 19.2 Frontend updater facade

Add typed APIs in/behind `ui/src/lib/tauri.ts`, for example:

```text
checkForUpdate()
downloadUpdate()
prepareUpdateRestart(snapshot)
installDownloadedUpdate(checkpointRevision)
```

If a higher-level `ui/src/lib/updater.ts` is added, it should still use the typed Tauri facade.

## 19.3 Intentional restart events/state

Keep explicit restart lifecycle state analogous to original Orca:

```text
update-restart-started
update-restart-aborted
app-restart-started
app-restart-aborted
renderer-unload-prevented
```

Names do not have to match original Orca exactly, but semantics should.

## 19.4 Safe updater sequence

Required sequence:

```text
Update downloaded
  -> mark intentional restart in progress
  -> run decision guards with updater-aware bypass policy
  -> capture latest UI snapshot
  -> prepareShutdownCheckpoint(reason = update-restart)
  -> await durable receipt
  -> backend performs final dirty terminal-buffer flush
  -> verify receipt/revision
  -> install update
  -> relaunch/exit
```

At no point should updater installation be allowed to initiate the process restart before the persistence receipt exists.

## 19.5 Update failure/abort

If any step fails before process exit:

1. do not restart;
2. dispatch/reset updater-aborted state;
3. unfreeze the persistence scheduler;
4. re-enable UI interaction;
5. surface an update error;
6. keep the just-written checkpoint as a valid ordinary crash-recovery point.

## 19.6 Token/revision verification

The native updater install command should accept the checkpoint revision/token produced by the prepare step.

Before install/relaunch it should verify that:

- the checkpoint exists;
- it belongs to the current shutdown attempt where applicable;
- required terminal buffers have been flushed.

This prevents an old successful checkpoint from accidentally authorizing a new update restart after more state changes.

---

# 20. App Relaunch Integration

Any future/manual `Restart app` action should use exactly the same safe-restart coordinator as updater restart:

```text
intentional restart
  -> durable checkpoint
  -> final native flush
  -> relaunch
```

Do not create a separate unchecked relaunch path.

---

# 21. Restore Error Handling

## 21.1 Missing workspace root

Behavior:

- do not fail entire startup;
- keep a recovery descriptor;
- show project as unavailable;
- allow user to locate/re-register the repository or remove the stale entry.

Do not spawn its terminals.

## 21.2 Missing worktree

Behavior:

- reconcile against current `git worktree list`;
- keep associated tabs in failed/unavailable restore state;
- do not silently spawn them at workspace root, because that changes the user's worktree isolation context.

Offer explicit recovery.

## 21.3 Invalid saved CWD

Fallback chain:

1. valid `lastKnownCwd`;
2. valid `spawnCwd`;
3. validated current worktree path;
4. validated workspace root only for sessions whose target semantics allow root;
5. otherwise fail that terminal restore.

## 21.4 PTY spawn failure

Keep tab/session metadata and show retry UI.

Do not drop the snapshot automatically.

## 21.5 Partial session restore

One broken terminal must not roll back successful terminals in the same workspace.

Restore sessions independently after the workspace/layout graph has been hydrated.

## 21.6 Newer unsupported snapshot schema

If persisted snapshot schema is newer than this binary understands:

- do not destructively overwrite it immediately;
- surface a compatibility warning;
- fall back to a fresh runtime session if necessary;
- preserve/quarantine the old snapshot for recovery.

---

# 22. Database Corruption Recovery

On startup:

1. open DB;
2. validate schema version;
3. perform a lightweight integrity check appropriate for startup;
4. parse every requested workspace snapshot with version validation.

If SQLite itself is corrupt:

- close the DB;
- quarantine the DB and associated WAL/SHM files with a timestamped suffix where safe;
- create a new empty DB;
- start rorca in degraded recovery mode;
- surface a non-fatal warning.

If only one JSON snapshot payload is malformed:

- quarantine/ignore that workspace snapshot;
- keep other workspaces usable;
- do not discard the entire DB.

---

# 23. Clean vs Crash Shutdown Metadata

Use `last_clean_shutdown` only as diagnostic/recovery context, not as permission to discard the session.

Suggested behavior:

### Startup

- read previous value;
- retain it in memory as `previousShutdownWasClean`;
- set current DB marker to unclean/not-finished.

### Normal close/update restart

- final checkpoint succeeds;
- final native flush succeeds;
- mark clean shutdown + reason;
- then exit/restart.

### Crash

The marker remains unclean.

On next launch:

- restore the latest committed periodic/debounced checkpoint;
- optionally show a subtle `Recovered after unexpected shutdown` notice;
- do not require a modal prompt just because the prior shutdown was unclean.

---

# 24. Persistence Cleanup / Retention

## 24.1 Closed terminal

When the final tab referencing a logical session is intentionally closed:

- remove session from next workspace snapshot;
- close runtime PTY;
- delete its persisted scrollback after the snapshot commit establishes that it is no longer referenced.

## 24.2 Orphan cleanup

At startup and after snapshot commit:

- collect all logical session IDs referenced by current workspace snapshots;
- delete `terminal_scrollback` rows with no referenced logical session after a conservative grace policy if needed.

## 24.3 Bounded storage

Initial limits:

- 512 KiB per terminal replay buffer;
- no unbounded append log;
- optionally add aggregate DB size/LRU protection later.

---

# 25. Security and Privacy Requirements

Terminal persistence can contain secrets visible in terminal output. Treat it accordingly.

## 25.1 Store location/permissions

- use private app-data directory;
- best-effort restrictive file permissions on Unix;
- rely on user-profile ACL isolation on Windows;
- never store session DB in the Git repository/workspace.

## 25.2 Never persist unnecessary secrets

Do not persist:

- full process environment;
- tokens from environment variables;
- arbitrary command argv;
- shell input history separately.

The scrollback itself can still contain sensitive output; document this behavior and provide a future `Persist terminal history` setting if product policy requires opt-out.

## 25.3 Path validation

Every restored path must be treated as untrusted persisted input and revalidated through the same canonical workspace/worktree safety model used for fresh commands.

---

# 26. Concrete File-Level Changes

## 26.1 Rust

### `src-tauri/Cargo.toml`

Add during implementation:

- SQLite dependency, preferably a small synchronous crate suitable for a dedicated writer thread;
- Tauri updater plugin;
- any updater restart/process support actually required by the selected native path.

Avoid adding async SQL abstraction unless the project genuinely benefits from it; the persistence workload is local, small, and serialized.

### `src-tauri/src/lib.rs`

Refactor startup to:

- create/lazily initialize `SessionPersistence`;
- initialize DB in `.setup(...)` with app-data path;
- hydrate workspace registry before current-directory fallback;
- manage close/updater coordinators;
- register session/updater commands;
- initialize updater plugin.

### `src-tauri/src/worktree/registry.rs`

Add only the minimal APIs required for persistence/bootstrap, for example:

- snapshot/list registered workspaces if needed;
- safe re-registration behavior.

Do not move SQL responsibilities into the registry.

### `src-tauri/src/terminal/output_hub.rs`

Add APIs such as:

- snapshot buffer by runtime backend session ID;
- dirty-buffer notification/sink integration;
- optional buffer seed utility if later useful.

Keep the existing bound.

### `src-tauri/src/terminal/service.rs`

Add:

- stable logical-to-runtime metadata registry;
- spawn with logical ID;
- restore spawn;
- lookup by logical ID;
- latest terminal dimensions/CWD metadata;
- final buffer collection for shutdown.

### `src-tauri/src/ipc/terminal.rs`

Evolve spawn/list DTOs and add restore APIs.

Preserve current structured `IpcError` behavior.

### `src-tauri/src/ipc/session.rs`

Add commands such as:

```text
cmd_session_restore_plan
cmd_session_save_workspace
cmd_session_prepare_shutdown
cmd_session_confirm_shutdown
cmd_session_clear_workspace (optional explicit user action)
```

### `src-tauri/src/ipc/updater.rs`

Add narrow updater commands/coordinator integration.

### `src-tauri/src/app_close.rs`

Own native close authorization state so a renderer guard can delay destruction safely.

---

## 26.2 Frontend

### `ui/src/lib/types.ts`

Add:

- persisted DTO types;
- restore state types;
- logical session ID semantics;
- checkpoint receipt types.

Consider renaming fields/types to make runtime ID vs logical ID impossible to confuse.

### `ui/src/lib/tauri.ts`

Add typed wrappers for:

- session restore plan;
- session snapshot save;
- shutdown checkpoint;
- restore terminal;
- native close confirm;
- updater operations.

### `ui/src/lib/terminalEvents.ts`

Add safe persisted-history seeding/prepending for a newly restored backend session ID.

Must preserve early live output already captured during PTY startup.

### `ui/src/state/workspaceStore.ts`

Add:

- create logical session ID before spawn;
- hydrate action;
- restore success/failure actions;
- persistence snapshot builder integration;
- workspace reset isolation.

### `ui/src/state/workspaceRuntime.ts`

Add hydration gating so it does not auto-create a terminal during restore.

### `ui/src/state/sessionPersistence.ts`

New centralized UI persistence scheduler/snapshot builder.

### `ui/src/lib/windowCloseGuard.ts`

New close/updater guard coordinator based on the proven original-Orca pattern.

### `ui/src/components/TerminalPane.tsx`

Support:

- restored session placeholder states;
- replay divider where appropriate;
- optional OSC 7 CWD metadata capture if implemented at the renderer layer.

### `ui/src/App.tsx`

Add:

- app bootstrap/restore state machine;
- restore loading/degraded UI;
- native close request registration;
- updater before-unload/checkpoint registration;
- safe project-switch checkpointing.

---

# 27. Phase-by-Phase Implementation Roadmap

## Phase 0 — Contracts and test seams

### Goal

Lock down identities and native interfaces before adding storage.

### Work

1. Introduce `logicalSessionId` terminology in DTOs/types.
2. Change fresh spawn to allocate logical ID before native spawn.
3. Add runtime metadata mapping in `TerminalService`.
4. Extend terminal list/debug APIs to report logical + backend IDs.
5. Add tests proving:
   - logical ID stays stable for a frontend session;
   - backend ID is runtime-only;
   - duplicate logical active sessions are rejected.

### Exit criteria

No persistence yet, but runtime architecture no longer conflates durable and ephemeral session identity.

---

## Phase 1 — SQLite local state foundation

### Goal

Create reliable versioned local persistence independent of React.

### Work

1. Add SQLite dependency.
2. Add `session/` module.
3. Implement DB path injection for tests and Tauri app-data path for production.
4. Create schema/migration v1.
5. Implement:
   - workspace registry persistence;
   - app state;
   - workspace snapshot JSON;
   - terminal BLOB table.
6. Add writer actor + flush barrier.
7. Add checkpoint revision receipts.
8. Add DB corruption/version handling.

### Exit criteria

Rust unit/integration tests can write a snapshot, destroy all in-memory services, reopen the DB, and load the same logical state.

---

## Phase 2 — Workspace/UI state checkpointing

### Goal

Persist workspaces, worktrees, tabs, and layout before attempting PTY restore.

### Work

1. Add persistence DTOs to Rust/TS.
2. Add `cmd_session_save_workspace`.
3. Add UI snapshot builder.
4. Add 250-500 ms debounce.
5. Persist project registration immediately.
6. Persist active workspace/worktree.
7. Persist tabs/layout.
8. Persist terminal metadata excluding backend IDs.
9. Add project-switch flush.

### Exit criteria

Restarting the UI/native app restores the exact tab labels/order/split topology using placeholder terminal sessions, even before terminal respawn is implemented.

---

## Phase 3 — Native restore bootstrap and reconciliation

### Goal

Restore registered workspaces safely and hydrate UI without duplicate auto-created tabs.

### Work

1. Initialize DB from Tauri setup.
2. Restore workspace registrations before default-CWD fallback.
3. Add `cmd_session_restore_plan`.
4. Reconcile persisted worktrees with current Git worktrees.
5. Add frontend bootstrap state machine.
6. Gate `workspaceRuntime` auto-tab creation.
7. Add `HYDRATE_SESSION` + normalization.
8. Add degraded workspace/worktree states.

### Exit criteria

With all terminals still placeholders, a process restart restores workspace/tab/layout state and does not create duplicate tabs.

---

## Phase 4 — Terminal respawn + CWD + replay buffer

### Goal

Restore usable terminal sessions.

### Work

1. Add `cmd_terminal_restore`.
2. Respawn new PTY for sessions with `restorePolicy=respawn`.
3. Preserve logical session ID; generate new backend ID.
4. Validate restore CWD with fallback chain.
5. Track cols/rows.
6. Add `TerminalOutputHub` snapshot API.
7. Add dirty scrollback scheduling.
8. Persist bounded 512 KiB buffer.
9. Add frontend persisted-history prepend.
10. Add restore separator.
11. Add restore failure/retry UX.
12. Add optional OSC 7 last-known-CWD tracking.

### Exit criteria

After restart, previously active terminal tabs show prior bounded history and then a newly spawned shell in a safe restored CWD.

---

## Phase 5 — Crash-resilient periodic checkpointing

### Goal

Make state useful after SIGKILL/app crash, not only clean quit.

### Work

1. Add 5-10 second safety checkpoint.
2. Flush dirty terminal buffers every ~1-2 seconds.
3. Track clean/unclean shutdown marker.
4. Add startup recovered-after-crash status.
5. Add stale scrollback cleanup.
6. Add DB size/retention instrumentation.

### Exit criteria

Force-killing the app loses at most the documented debounce/periodic window and the next launch restores the latest committed state.

---

## Phase 6 — Native close guard

### Goal

Make ordinary close wait for a durable checkpoint.

### Work

1. Add frontend `windowCloseGuard.ts`.
2. Add native close coordinator.
3. Prevent immediate window destruction.
4. Run decision guards.
5. Capture latest state through `stateRef`.
6. Await `prepareShutdownCheckpoint`.
7. Flush native terminal buffers.
8. authorize close only with valid receipt.
9. add bounded native fail-safe for dead renderer.
10. reset/unfreeze when close is vetoed.

### Exit criteria

Closing the window immediately after a tab/split change still restores that exact change on next launch.

---

## Phase 7 — Updater integration and before-unload guard

### Goal

Guarantee update installation cannot race session persistence.

### Work

1. Add/configure `tauri-plugin-updater`.
2. Add typed updater facade.
3. Add intentional-restart lifecycle state/events.
4. Register updater-aware decision-guard bypass.
5. Keep session checkpoint mandatory.
6. Add `beforeunload` defensive handler.
7. Require checkpoint receipt/token before install/relaunch.
8. Perform final native buffer flush immediately before restart.
9. reset/unfreeze if update install/relaunch fails before exit.

### Exit criteria

An update restart restores the same state as a normal protected close, and a forced checkpoint failure prevents the update restart.

---

## Phase 8 — Hardening, migration, and observability

### Goal

Make persistence safe to ship and debuggable.

### Work

1. migrate/merge existing localStorage project metadata as needed;
2. add schema migration tests;
3. add corruption quarantine tests;
4. add restore warning telemetry/logging without terminal contents;
5. add DB size metrics/logging;
6. add explicit session-reset action;
7. document local terminal-history persistence/privacy;
8. run packaged updater tests on supported desktop OSes.

### Exit criteria

Feature is resilient across version upgrades, missing repositories, corrupt state, and updater failures.

---

# 28. Verification Plan

## 28.1 Rust unit tests

### Store/schema

- creates schema v1 in empty temp directory;
- reopens existing DB;
- migration is idempotent;
- unsupported future schema is non-destructive;
- malformed snapshot JSON affects only that workspace;
- transaction rollback leaves previous snapshot intact;
- checkpoint revision increments exactly once per committed checkpoint;
- flush barrier observes all earlier queued writes.

### Workspace persistence

- registered workspace reloads with canonical repo root;
- missing repo becomes unavailable rather than panicking;
- conflicting/default workspace registration order is handled;
- persisted worktree state is reconciled with fresh Git state.

### Terminal persistence

- logical ID is stable;
- backend ID is not serialized into persisted DTO;
- restored PTY receives a new backend ID;
- saved cols/rows are used initially;
- invalid CWD falls back safely;
- missing worktree refuses unsafe respawn;
- exited/failed session respects `do-not-respawn`;
- active session respects `respawn`;
- writer-lease rules are still enforced.

### Scrollback

- buffer is capped to 512 KiB;
- newest output is retained;
- dirty sessions are coalesced;
- final shutdown flush contains latest available buffer snapshot;
- deleted logical sessions are garbage-collected.

### Shutdown/updater coordinator

- stale checkpoint token cannot authorize restart;
- current checkpoint token can authorize restart;
- failed DB write returns failure and never authorizes updater restart;
- duplicated shutdown request is idempotent;
- aborted restart resets shutdown state.

---

## 28.2 Frontend unit tests (Vitest)

### Reducer/hydration

- `HYDRATE_SESSION` reproduces tabs/order/layout;
- invalid primary/secondary IDs are normalized;
- dangling sessions/tabs are repaired deterministically;
- backend IDs begin null after hydration;
- restore success patches only the correct logical session;
- restore failure preserves tab and exposes error state.

### Runtime startup

- worktree refresh during hydration does not auto-create a duplicate tab;
- no snapshot -> current default-tab behavior still works;
- valid snapshot -> no extra default terminal;
- project switch never carries old project's sessions into new workspace.

### Persistence scheduler

- many rapid reducer changes coalesce into one save;
- `flush()` writes latest state, not stale closure state;
- runtime backend IDs are stripped;
- project switch forces a flush;
- aborted shutdown unfreezes writes.

### Terminal event bus

- persisted history is prepended to early live output;
- early new-shell prompt is not lost;
- history is not duplicated;
- per-session cap remains enforced.

### Close/updater guards

- close guards run sequentially;
- one veto stops ordinary close;
- reentrant close requests do not run twice;
- intentional restart bypasses configured decision prompts;
- intentional restart does **not** bypass persistence checkpoint;
- checkpoint failure blocks updater restart;
- abort event resets intentional-restart state.

---

## 28.3 Rust/Tauri integration tests

Use temp repositories and temp persistence DB paths.

### Scenario A — full save/reopen

1. register repo;
2. create worktree;
3. create logical terminal session;
4. write terminal output;
5. save tabs/layout;
6. flush checkpoint;
7. tear down services;
8. create fresh registry/store/service;
9. load restore plan;
10. verify logical topology matches.

### Scenario B — PTY respawn

1. spawn shell in worktree;
2. capture output;
3. checkpoint;
4. close first runtime service;
5. construct fresh runtime service;
6. restore logical session;
7. assert new backend ID;
8. assert same logical ID;
9. assert saved replay bytes remain available.

### Scenario C — missing worktree

Persist a worktree session, delete the worktree between runs, then verify restore is degraded and does not silently spawn at the wrong path.

### Scenario D — DB failure

Inject write failure/read-only path and verify checkpoint receipt is not produced.

### Scenario E — close authorization

Verify native close is not authorized until the current revision has a durable checkpoint.

### Scenario F — updater adapter

Use a mocked updater transport/trait so tests can prove `install/restart` is not invoked when checkpoint preparation fails.

---

# 29. Manual / E2E Acceptance Matrix

## 29.1 Normal quit

1. open two worktrees;
2. create multiple terminal tabs;
3. change tab labels through terminal title;
4. enable split;
5. select non-first active tab;
6. generate terminal output exceeding several screens;
7. quit immediately after a final interaction;
8. relaunch.

Expected:

- same active project/worktree;
- same tabs/order/labels;
- same split orientation and active panes;
- prior bounded scrollback visible;
- live terminals have new backend IDs;
- new shell starts in safe restored CWD;
- clear restored-session marker is visible.

## 29.2 Crash / SIGKILL

1. create state;
2. wait for at least one periodic checkpoint;
3. change state again;
4. force-kill process without close event;
5. relaunch.

Expected:

- latest committed checkpoint restores;
- no DB corruption;
- user may lose only changes inside documented debounce/checkpoint window;
- app indicates unexpected-shutdown recovery if UX chooses to surface it.

## 29.3 Update restart

Using a test/staged updater channel:

1. build state with several terminals;
2. download update;
3. trigger install;
4. verify checkpoint receipt is recorded before process exit;
5. allow updater relaunch;
6. verify restored state.

Expected:

- no extra prompt solely because restart is intentional;
- persistence still occurs;
- session restores after new version launches.

## 29.4 Forced checkpoint failure during update

Inject DB write failure immediately before update install.

Expected:

- updater restart is aborted;
- application stays running;
- restart-in-progress state resets;
- user receives an actionable error;
- no stale checkpoint token authorizes install.

## 29.5 Missing repository

1. persist workspace;
2. move/delete repository outside rorca;
3. launch.

Expected:

- app starts;
- workspace is marked unavailable;
- other workspaces remain usable;
- no terminal spawns into an unintended directory.

## 29.6 Missing worktree only

Expected:

- workspace remains usable;
- affected tab shows restore failure/recovery option;
- other terminal sessions restore.

## 29.7 Multiple tabs sharing one logical session

Expected:

- one restored PTY;
- both tabs point to same logical session/new backend runtime session;
- one persisted scrollback row;
- closing one shared tab does not kill/delete the session while another still references it.

## 29.8 Large scrollback

Generate >512 KiB output.

Expected:

- DB contains no more than configured per-session bound;
- newest history is retained;
- restore remains responsive.

## 29.9 Repeated rapid close/update clicks

Expected:

- one close/update operation in flight;
- one valid checkpoint barrier;
- no duplicate install/relaunch.

## 29.10 Platform paths

Verify packaged app on each supported desktop OS:

- DB created in correct app-data location;
- Unicode paths work;
- repository paths with spaces work;
- path canonicalization remains safe.

---

# 30. Performance Targets

Initial targets, to be measured rather than assumed:

- UI state save debounce should be imperceptible to interaction;
- normal metadata checkpoint should complete in well under the close-guard timeout on local disk;
- terminal output must never synchronously write SQLite on the PTY reader path;
- scrollback persistence should be coalesced and off the hot output path;
- startup should render workspace/layout shell quickly and restore terminals progressively;
- large/multiple terminal buffers should be loaded lazily enough to avoid one huge bootstrap payload.

Add timing logs around:

- DB open/migrate;
- restore-plan load;
- workspace reconciliation;
- per-terminal respawn;
- shutdown checkpoint;
- final updater checkpoint.

Do not log terminal contents.

---

# 31. Observability / Diagnostics

Use structured tracing fields such as:

```text
checkpoint_revision
checkpoint_reason
workspace_id
logical_session_id
restore_result
restore_failure_code
scrollback_bytes
previous_shutdown_clean
```

Never log:

- terminal replay contents;
- shell input;
- environment variables;
- authentication tokens.

Useful restore warning codes:

```text
WORKSPACE_MISSING
WORKTREE_MISSING
CWD_INVALID_FALLBACK_USED
PTY_RESTORE_FAILED
SNAPSHOT_SCHEMA_UNSUPPORTED
SNAPSHOT_JSON_INVALID
SESSION_DB_CORRUPT
CHECKPOINT_WRITE_FAILED
UPDATER_CHECKPOINT_REJECTED
```

---

# 32. Migration Strategy for Existing Users

Current users have no native session DB.

First-run migration behavior:

1. DB absent -> create schema v1;
2. existing browser localStorage projects continue to load;
3. each successful `registerProject(...)` immediately populates native `workspaces` table;
4. active project can be copied to native `app_state` after first successful bootstrap;
5. after native state is proven stable, localStorage project metadata can become compatibility fallback rather than primary state.

No destructive migration is needed because there is no prior native session format.

---

# 33. Suggested Acceptance Criteria

The feature is complete when all of the following are true:

1. A registered workspace survives app restart without relying solely on current process CWD.
2. Persisted worktree associations are reconciled against current Git state.
3. Tab IDs/order/labels survive restart.
4. Split layout survives restart.
5. Stable logical session IDs survive restart.
6. Runtime backend PTY IDs do not survive restart and are regenerated.
7. Active terminal sessions are respawned as new default shells, not falsely “reattached.”
8. Saved CWD is validated and restored with a documented fallback chain.
9. Up to the configured bounded terminal scrollback survives restart.
10. UI does not create duplicate default tabs during hydration.
11. Missing workspaces/worktrees produce degraded recovery UI instead of startup failure.
12. Periodic saves provide useful recovery after hard crash.
13. Normal window close waits for a successful durable checkpoint when renderer is responsive.
14. Updater restart cannot proceed without a current successful checkpoint receipt.
15. Updater abort/failure resets intentional-restart/guard state.
16. An intentional updater restart may bypass appropriate user-decision guards but never bypass persistence.
17. Database corruption is quarantined/recovered without making the application permanently unlaunchable.
18. All unit/integration tests and packaged E2E update/restore tests pass.

---

# 34. Recommended Implementation Order Summary

The implementation should be executed in this dependency order:

```text
Stable logical session IDs
  -> SQLite store + migrations + writer actor
  -> Workspace/tab/layout checkpointing
  -> Native workspace restore bootstrap
  -> UI hydration gate
  -> PTY respawn
  -> Scrollback persistence/replay
  -> Crash-periodic checkpointing
  -> Native close coordinator
  -> Updater safe-restart barrier
  -> Corruption/migration/platform hardening
```

Do not start by adding updater calls before the checkpoint contract exists. The updater must consume a proven persistence barrier, not define one ad hoc.

---

# 35. Final Design Principle

The implementation should maintain one clear invariant:

> **A durable logical session describes what rorca should reconstruct; a runtime PTY describes only what exists in the current process.**

Everything in the persistence and updater design follows from that separation:

- workspace/worktree/tab/layout state is durable;
- terminal replay history is durable but bounded;
- logical terminal identity is durable;
- PID/PTy handles/backend session IDs are ephemeral;
- restart means safe reconstruction, not pretending a dead PTY can be reattached;
- updater and close paths are allowed to terminate the process only after the durable reconstruction description has been committed.
