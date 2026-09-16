# Ferryx Native Process Suspend & Memory Reclamation — Implementation Plan

**Date:** 2026-09-16  
**Status:** Implementation-ready design  
**Scope:** Ferryx local terminal lifecycle, daemon protocol, frontend session state, native terminal rendering/input, tab/split controls, restart semantics, and cold-start restore policy  
**Primary objective:** Replace destructive “hibernate = kill process, later respawn” behavior with true process suspension that preserves the complete live PTY/process tree while allowing the operating system to reclaim resident memory.

---

## 1. Executive Summary

Ferryx currently conflates two fundamentally different lifecycle concepts:

1. **Cold standby** — no backend process exists yet (appropriate after application launch when restoring persisted workspace state lazily).
2. **Runtime suspension** — a live terminal process already exists and should remain exactly the same process, PTY, shell/REPL state, descendants, job-control state, and file descriptors while consuming no CPU and allowing inactive memory to leave the working set.

The existing runtime “hibernate” flow is destructive. `ui/src/lib/sessionLifecycle.ts::hibernateRegisteredSession()` captures scrollback, invokes `cmd_terminal_hibernate`, then clears `backendSessionId`, marks `processState: "hibernated"`, and marks the lifecycle exited. The daemon-side hibernate request ultimately removes the running backend process. “Resume” therefore requires spawning/reconnecting a replacement and cannot provide process identity or in-memory state fidelity.

This plan replaces runtime hibernation with **native suspend/resume**:

- **macOS/Linux:** stop the entire terminal foreground/process group with `SIGSTOP`; resume with `SIGCONT`. The kernel retains process identity and state while normal VM pressure mechanisms reclaim physical pages.
- **Windows:** suspend the process with `NtSuspendProcess`, immediately trim its working set with `EmptyWorkingSet`/`K32EmptyWorkingSet`, and resume with `NtResumeProcess`.
- **Cold app startup:** continue using Lazy / Active-Only restore for sessions whose processes do not exist. This remains a spawn-on-demand path and is explicitly separate from runtime suspend/resume.
- **Frontend:** model the distinction directly with `SessionProcessState = "running" | "suspended" | "standby"`. Sleeping/suspended state remains reactive and does not erase backend identity.
- **Native surface:** suspended panes immediately set native visibility false, stop native surface work, and suppress PTY keyboard/mouse input until resume.
- **Restart Session:** becomes a deliberate terminate-then-spawn-new operation rather than an alias of hibernate.

The implementation must preserve current remote/paired-daemon safety boundaries: the first implementation applies native suspend/resume only to locally owned PTY sessions unless/ until the paired/remote protocol explicitly gains equivalent capabilities.

---

## 2. Background and Problem Analysis

### 2.1 Current destructive lifecycle

Repository inspection shows the present frontend runtime lifecycle in `ui/src/lib/sessionLifecycle.ts`:

- `useSleepingSessionIds()` exposes an external-store set of “sleeping” local frontend session IDs.
- `hibernateRegisteredSession()`:
  - rejects remote/paired workspaces,
  - captures recent scrollback,
  - sets the sleeping bit,
  - calls `hibernateTerminal(backendSessionId)`,
  - then rewrites the registered session to `backendSessionId: null`, `processState: "hibernated"`, and `lifecycle: "exited"`.
- `requestSessionLifecycleAction("restart", ...)` currently enters the same hibernate path as manual hibernate; only the manual hold bookkeeping differs.

`ui/src/lib/tauri.ts` maps this operation to `cmd_terminal_hibernate`. `src-tauri/src/ipc/terminal.rs` forwards it to `DaemonClient::hibernate_terminal()`, which sends `DaemonRequest::Hibernate`. The daemon returns `HibernateOk` after destructive handling.

This is not suspension. It is process termination plus later replacement.

### 2.2 Why kill-and-respawn is architecturally wrong for runtime “hibernate”

Destroying the process forfeits all state that is not externally persisted, including:

- shell process identity and PID;
- in-memory REPL variables, interpreters, debuggers, database consoles, editor state, language servers, and ad-hoc scripts;
- child processes and process-tree/job-control state;
- open anonymous pipes and inherited descriptors;
- PTY line discipline state and foreground process-group relationships;
- process-local caches and unsaved runtime state;
- terminal applications that do not have a provider-specific reconnect mechanism.

Capturing scrollback before termination preserves only presentation history, not the process itself. Respawning with CLI flags or provider reconnect behavior is therefore neither general nor fidelity-preserving.

### 2.3 Required semantic split

Ferryx must use different operations for different lifecycle states:

| Concept | Process exists? | Backend ID retained? | Resume action | Fidelity |
|---|---:|---:|---|---|
| Running | Yes | Yes | N/A | Full |
| Suspended | Yes | Yes | OS native resume | Same process / PTY / children |
| Standby | No | Frontend standby identity or null | Spawn/reconnect on demand | Cold restore, not same process |
| Exited | No | May retain historical metadata only | Explicit restart/new shell | New process |

“Suspended” must never be represented as “exited,” and suspension must never clear the backend session ID.

---

## 3. Architectural Principles and Invariants

### 3.1 Core invariants

1. **Suspend is non-destructive.** A successful suspend must leave the same daemon PTY session, child process, PTY master, output hub, and backend session identifier registered.
2. **Resume is identity-preserving.** Resume targets the already-registered `PtySession`; it never spawns a replacement.
3. **Standby remains process-absent.** Standby is used for cold restore and other explicitly process-absent states only.
4. **Restart is destructive by definition.** Restart terminates the current process/session, waits for teardown/daemon acknowledgement, and creates a fresh backend session while rebinding the existing logical frontend pane/session.
5. **Lifecycle state is authoritative and race-safe.** Daemon and frontend transitions are idempotent where practical and reject impossible transitions with explicit typed errors.
6. **Input cannot leak into a suspended PTY.** Both frontend and backend enforce this. Frontend gating is UX; backend gating is correctness.
7. **Suspension must not be undone by incidental rerenders.** Resume occurs only from explicit user focus/click/activation transitions or an explicit lifecycle call, not merely because a suspended pane is still the active tab.
8. **Native graphics resources follow process visibility.** A suspended terminal has no visible native surface and performs no ongoing WGPU presentation work.
9. **User restore policy governs app launch.** Runtime suspension semantics must not force eager cold-start spawning.
10. **Remote ownership boundaries are respected.** A local frontend cannot signal an unknown remote process group through local OS primitives.

### 3.2 Terminology migration

Use **Suspend / Resume** for live-process runtime lifecycle.

Retain **Standby** for process-absent cold restore.

The old “Hibernate Session” wording should be replaced in user-facing UI with **Suspend Session** once the backend primitive is implemented. Internal compatibility aliases may exist temporarily during migration, but the end-state API should not call a live suspend operation “hibernate.”

---

## 4. Backend Daemon PTY Lifecycle Design

### 4.1 Extend `PtySessionState`

Current `src-tauri/src/terminal/session.rs` defines:

- `Starting`
- `Running`
- `Closing`
- `Exited`
- `Failed`

Add an explicit suspended state:

```rust
pub enum PtySessionState {
    Starting,
    Running,
    Suspended,
    Closing,
    Exited { code: Option<i32> },
    Failed { reason: String },
}
```

Update all state matches in `PtySession` so:

- `begin_closing()` accepts both `Running` and `Suspended` as closeable live states;
- input/write functions reject `Suspended` with a stable lifecycle error rather than writing to the PTY;
- resize may remain allowed while suspended if it only updates PTY winsize, but no resize operation may accidentally resume the process;
- describe/list APIs report “live but suspended” distinctly from “not running.”

### 4.2 Add `PtySession::suspend()` and `PtySession::resume()`

Implement platform-specific helpers behind a shared interface in `src-tauri/src/terminal/session.rs` or small dedicated sibling modules such as:

- `src-tauri/src/terminal/process_suspend_unix.rs`
- `src-tauri/src/terminal/process_suspend_windows.rs`

Recommended public methods:

```rust
pub fn suspend(&self) -> Result<(), PtyError>;
pub fn resume(&self) -> Result<(), PtyError>;
```

Behavior:

- `Running -> Suspended` on successful OS operation.
- `Suspended -> Running` on successful resume.
- Suspend called while already suspended is idempotent success.
- Resume called while already running is idempotent success.
- `Starting`, `Closing`, `Exited`, or `Failed` return an explicit lifecycle error unless a specific idempotent case is intentionally supported.
- State mutation happens only after OS success; if the syscall fails, state remains unchanged.

### 4.3 Unix implementation: macOS and Linux

#### Suspend

Use the live PTY child/process-group identity and send `SIGSTOP` to the terminal process group:

```rust
unsafe { libc::kill(-(pgid as i32), libc::SIGSTOP) }
```

The required target is the terminal process group, not merely the shell process, so child processes are frozen together.

The implementation should prefer the process group established for the PTY session. Current `PtySession::foreground_process_group()` uses `tcgetpgrp()` from the PTY master. The implementation must carefully distinguish:

- the PTY foreground process group, which can change when an interactive child takes foreground control; and
- the session/shell process group that should define the entire terminal-owned process tree.

**Decision:** Suspend the terminal-owned process group that Ferryx created for the PTY, not a transient single foreground child. If current `portable_pty::Child` metadata exposes only PID, establish/store the initial PGID at spawn time. Do not discover the target only at suspend time if that could freeze only the current foreground job and leave background children running.

If existing spawn semantics guarantee `pgid == child pid`, persist that PGID explicitly in `PtySessionConfig`/`PtySession` and document the guarantee with tests.

#### Resume

Send:

```rust
unsafe { libc::kill(-(pgid as i32), libc::SIGCONT) }
```

A successful `SIGCONT` resumes the same process group in place. No daemon attachment, PTY, output hub, or backend ID is recreated.

#### Signal errors

Normalize OS failures:

- `ESRCH`: process group no longer exists; reconcile the session to exited rather than pretending it remains suspended.
- `EPERM`: return lifecycle/permission failure and leave frontend state running unless daemon observation proves otherwise.
- other errno: return a typed daemon error with context.

### 4.4 macOS memory behavior and guarantee language

`SIGSTOP` immediately makes threads non-runnable, reducing process CPU use to effectively zero while stopped.

Do **not** claim that SIGSTOP itself “compresses RAM” synchronously. Correct guarantee:

- the process remains live and its virtual address space is preserved;
- stopped pages become inactive candidates;
- XNU can move inactive anonymous pages through VM compression and/or swap under memory pressure;
- memory reclamation amount/timing is kernel policy, not an application-level deterministic byte guarantee.

Plan verification should measure resident footprint under controlled pressure, but tests must not require an exact reclaimed byte count.

### 4.5 Linux memory behavior and guarantee language

On Linux, SIGSTOP freezes execution but does not itself evict resident pages.

Correct guarantee:

- anonymous pages remain part of the process virtual address space;
- under memory pressure, reclaim/kswapd may move eligible pages to configured swap or zram;
- systems without swap/zram may reclaim only file-backed pages and may retain anonymous RSS;
- therefore “memory reclamation” is best-effort and kernel/configuration-dependent on Linux.

Do not add unsafe process-memory dumping or destructive checkpoint/restore to simulate swap.

### 4.6 Windows implementation

#### Native handles

Windows suspend requires a process `HANDLE`. The implementation must obtain/retain a handle for the process represented by `PtySession`, with rights sufficient for:

- `NtSuspendProcess`
- `NtResumeProcess`
- working-set trimming (`PROCESS_SET_QUOTA` / `PROCESS_QUERY_INFORMATION` as required by the selected Win32 API)

Prefer storing a durable owned handle at spawn time instead of reopening by PID for every transition, avoiding PID reuse races.

If the PTY backend exposes only PID, open a handle with minimal required rights and validate it before use.

#### Suspend

Call `NtSuspendProcess(hProcess)` from `ntdll` and check the returned NTSTATUS.

Only after successful suspension call `EmptyWorkingSet(hProcess)` or the equivalent `K32EmptyWorkingSet` function exposed by the Windows API binding.

Sequence is intentional:

1. freeze execution;
2. trim the process working set;
3. mark daemon state `Suspended`.

If working-set trimming fails after suspension succeeds:

- retain `Suspended` state because the correctness requirement (frozen process) succeeded;
- return/log a distinct reclamation warning or partial-success diagnostic rather than resuming or killing the process;
- do not report that memory was reclaimed when it was not.

#### Resume

Call `NtResumeProcess(hProcess)`. Demand-paged memory will fault back in as touched. Mark state `Running` only after successful resume.

#### Process-tree caveat and decision

`NtSuspendProcess` suspends one process, not an arbitrary descendant process tree. Ferryx must define Windows terminal ownership accurately.

**Decision for implementation plan:** suspend the root ConPTY process using `NtSuspendProcess`, and audit how descendant terminal processes are launched/contained. If Ferryx already owns a Windows Job Object for the terminal tree, enumerate/suspend all job members or adopt a job-backed tree-suspension mechanism. If not, Phase 1 must add a process-tree ownership primitive before claiming child-process fidelity on Windows.

The publication/UX guarantee “child processes frozen” is not complete until this is verified. A root-only implementation may be shipped only with explicitly narrower documented semantics and a follow-up tree fix; the preferred implementation is tree-complete from the first release.

### 4.7 Reader/output behavior while suspended

The PTY reader task remains allocated because the session remains live. A fully stopped process group should produce no new application output, but there may be bytes already buffered in the PTY/kernel/output channel.

Rules:

- do not destroy the reader task on suspend;
- allow already-buffered output to drain before/while the UI detaches;
- preserve output sequence/history bookkeeping;
- resume uses the same reader, master PTY, writer, and output hub;
- do not perform an attach/reset sequence solely due to suspend/resume.

### 4.8 Backend input safety

All backend write entry points must check the lifecycle state. While `Suspended`, return a stable error such as `TERMINAL_SUSPENDED` rather than accepting bytes that will unexpectedly execute after resume.

Apply equivalent checks to:

- direct input writes;
- cancellable async writes;
- bracketed paste paths if separate;
- synthetic key/mouse paths if backend-mediated.

Frontend gating is not sufficient because races, stale UI state, IPC replay, or future callers could otherwise queue input.

---

## 5. Daemon Protocol and IPC

### 5.1 Replace destructive hibernate protocol for runtime use

Add explicit request/response pairs in `src-tauri/src/daemon/protocol.rs`:

```rust
DaemonRequest::Suspend { session_id }
DaemonRequest::Resume { session_id }

DaemonResponse::SuspendOk
DaemonResponse::ResumeOk
```

Do not overload `Hibernate` because old semantics are destructive and persisted clients/tests may depend on that distinction during migration.

### 5.2 `DaemonClient`

Add:

```rust
pub async fn suspend_terminal(&self, session_id: &str) -> Result<(), IpcError>;
pub async fn resume_terminal(&self, session_id: &str) -> Result<(), IpcError>;
```

Map response validation exactly and return a typed error on unexpected responses.

Update request classification/telemetry helpers that currently enumerate `DaemonRequest::Hibernate` so suspend/resume are assigned stable operation names.

### 5.3 Server routing

In `src-tauri/src/daemon/server.rs` add handlers:

- local session + Suspend -> `terminal_service` / `PtySession::suspend()`;
- local session + Resume -> `PtySession::resume()`;
- absent local session -> explicit not-found/stale-session error rather than claiming a live process is suspended;
- remote/legacy peer sessions -> unsupported unless peer protocol is extended in the same change.

Unlike close/old hibernate, a missing session is **not** a successful idempotent suspend. The frontend needs to know that the process no longer exists and transition to Standby/Exited recovery behavior.

### 5.4 Tauri commands

Add in `src-tauri/src/ipc/terminal.rs`:

```rust
#[tauri::command]
pub async fn cmd_terminal_suspend(...)

#[tauri::command]
pub async fn cmd_terminal_resume(...)
```

Register both commands in `src-tauri/src/lib.rs`.

Add frontend wrappers in `ui/src/lib/tauri.ts`:

```ts
suspendTerminal(sessionId: string): Promise<void>
resumeTerminal(sessionId: string): Promise<void>
```

Do not invalidate cached CWD on suspend/resume because the process and PTY remain alive. Destructive close/restart may continue invalidating appropriate caches.

### 5.5 Session description/list protocol

Extend live-session summaries/descriptions so frontend reconciliation can distinguish running from suspended sessions after UI reload/HMR or daemon reconnect.

Recommended wire value:

```ts
processState: "running" | "suspended"
```

Do not overload boolean `running` with “not suspended”; a suspended process is still alive.

Compatibility strategy:

- older responses without state default to running when a live backend session exists;
- new responses persist/restore suspended status during frontend reconnection where the daemon process itself survived.

---

## 6. Frontend State Machine

### 6.1 Replace `hibernated` runtime state

Current type in `ui/src/lib/types.ts` is:

```ts
export type SessionProcessState = "standby" | "running" | "hibernated";
```

Migrate to:

```ts
export type SessionProcessState = "standby" | "running" | "suspended";
```

Persisted data migration should accept legacy `"hibernated"` and interpret it as **standby**, not suspended, because historical hibernated sessions have no live process.

This migration is critical: a persisted old hibernated record must never cause Ferryx to issue Resume against a nonexistent backend ID.

### 6.2 State definitions

#### Running

- live daemon backend ID exists;
- daemon process state is Running;
- native terminal surface may be visible;
- PTY input permitted.

#### Suspended

- live daemon backend ID exists and is retained;
- daemon process state is Suspended;
- `sleepingSessionIds` contains the logical frontend session ID;
- native terminal surface is detached/hidden;
- keyboard/mouse/paste input is blocked;
- click/focus transition can invoke native Resume.

#### Standby

- no live process is assumed;
- used by cold startup Lazy / Active-Only restore;
- backend identity is null or a `standby:` frontend-only sentinel during migration;
- activation triggers normal spawn/reconnect logic, not `cmd_terminal_resume`.

### 6.3 `getSessionProcessState()` rules

Rework `ui/src/lib/sessionLifecycle.ts::getSessionProcessState()`:

1. Explicit suspended/sleeping + real backend ID -> `suspended`.
2. Real backend ID and not suspended -> `running`.
3. Standby sentinel/null -> `standby`.
4. Legacy persisted `hibernated` with no live backend -> `standby`.

Never infer “running” solely from `backendSessionId` before checking reactive suspended state.

### 6.4 Runtime suspend operation

Replace `hibernateRegisteredSession()` with an operation such as:

```ts
suspendRegisteredSession(sessionId: string): Promise<void>
```

Flow:

1. Look up registered session.
2. Reject unsupported remote/paired ownership explicitly.
3. Require a real backend session ID.
4. Optionally capture scrollback only as a UX snapshot optimization; this is no longer necessary for process recovery.
5. Set a transition flag (`suspending`) if needed to prevent duplicate calls.
6. Call `suspendTerminal(backendSessionId)`.
7. On success:
   - retain `backendSessionId`;
   - set `processState: "suspended"`;
   - keep process lifecycle live (do not mark `exited`);
   - set `sleepingSessionIds`.
8. On failure:
   - clear transition/sleeping optimistic state;
   - leave process state running unless daemon reconciliation reports exit.

Prefer marking UI suspended only after daemon success, or use a separate `suspending` transient state so UI never claims a stopped process before acknowledgement.

### 6.5 Runtime resume operation

Add:

```ts
resumeRegisteredSession(sessionId: string): Promise<void>
```

Flow:

1. Resolve registered session and real backend ID.
2. If `standby`, dispatch to existing cold-start spawn/reconnect path instead; do not call native Resume.
3. If `suspended`, deduplicate concurrent resume promises per session.
4. Call `resumeTerminal(backendSessionId)`.
5. On success:
   - set `processState: "running"`;
   - clear sleeping state;
   - retain the same backend ID;
   - restore native surface visibility on the next reactive render.
6. On not-found/process-exited response:
   - reconcile the session to process-absent state;
   - allow existing recovery/new-shell UX to take over.

### 6.6 Auto-idle suspension

Existing idle sweep can remain conceptually similar but must call native suspend rather than destructive hibernate.

Selection criteria remain:

- inactive pane/session;
- idle long enough per setting;
- real local backend exists;
- not already suspended;
- not remote/paired until protocol support exists.

Rename settings/UI text from “Auto-hibernate idle sessions” to “Auto-suspend idle sessions” and update description to explain that the process remains alive and resumes instantly, while OS memory reclamation is best-effort/platform-dependent.

### 6.7 Auto-resume loop prevention

Current `manualHibernateHoldIds` exists because a manually hibernated already-active pane can otherwise immediately wake from “active” state.

Preserve the intent, but rename and tighten it, e.g. `manualSuspendHoldIds`.

Rules:

- Manual suspend of an already active pane sets the hold.
- The current `active=true` level alone must not resume it.
- A **new user interaction edge** resumes it:
  - pointer click on the suspended overlay/pane;
  - inactive -> active pane focus transition;
  - explicit Resume command.
- Programmatic rerender, state reconciliation, session registration, resize, or tab metadata update must not resume it.
- Once the user leaves and returns to the pane, clear the hold and resume.

Resume promise deduplication prevents double `cmd_terminal_resume` from simultaneous tab activation and overlay click.

---

## 7. Native Terminal Surface Detachment and Input Blocking

### 7.1 `NativeTerminalPane.tsx`

Current code derives a `visible` value from native visibility/interactivity and exit state and uses it throughout attach, focus, IME, pointer, clipboard, and surface behavior.

Add:

```ts
const sleepingSessionIds = useSleepingSessionIds();
const suspended = sleepingSessionIds.has(session.id);
const visible = interactive && !isExited && !suspended;
```

The exact backend/logical ID lookup must use the logical `TerminalSession.id`, not backend ID, because `sleepingSessionIds` is frontend logical-session keyed.

Effects of `visible=false` while suspended:

- native WGPU surface receives/honors `visible = false` immediately;
- native attachment/presentation loop stops or detaches according to existing visibility implementation;
- focus/IME updates stop;
- mouse/keyboard/paste callbacks return before any PTY IPC;
- terminal geometry may remain cached but should not trigger visible presentation;
- resume restores the same target backend ID, so no cold attach/spawn occurs.

If the native renderer’s “visible false” implementation merely occludes but continues frame production, extend the native surface API so suspended means no frame scheduling/present work.

### 7.2 `TerminalPane.tsx`

Subscribe reactively:

```ts
const sleepingSessionIds = useSleepingSessionIds();
const suspended = sleepingSessionIds.has(session.id);
```

When suspended render a clear overlay above the terminal host:

- text: **“Suspended”**
- affordance: **“Click to resume”**
- paused/sleep icon;
- accessible role/button semantics;
- visible focus ring and keyboard activation (Enter/Space) if appropriate.

The overlay click must call the dedicated resume operation, not generic reconnect/spawn logic.

The terminal remains visually identifiable underneath or via a placeholder, but the native surface itself is detached/hidden to guarantee GPU resource reclamation and prevent accidental interaction.

### 7.3 Input correctness

Block input in two layers:

1. **Frontend:** native/web terminal handlers check `suspended` before writes, paste, mouse protocol, IME, or synthetic input.
2. **Backend:** `PtySession` rejects writes while `PtySessionState::Suspended`.

This prevents a race where a user types during the suspend transition and bytes execute unexpectedly after resume.

---

## 8. Tab Bar and Split-Pane Controls

### 8.1 Active tab indicator fix

Current `TabBar.tsx` computes:

```ts
const sleeping = tab.kind !== "browser" && !active && sleepingSessionIds.has(tab.sessionId);
```

Remove `!active`:

```ts
const sleeping = tab.kind !== "browser" && sleepingSessionIds.has(tab.sessionId);
```

A manually suspended active tab must immediately display its pause/sleep indicator.

Update tests to cover:

- inactive suspended tab;
- active manually suspended tab;
- immediate indicator removal after successful resume.

### 8.2 Context-menu naming

Replace “Hibernate Session” with “Suspend Session” for running sessions.

For suspended sessions, expose “Resume Session” instead of duplicate Suspend.

Keep “Restart Session” as a distinct destructive action.

### 8.3 Split pane targeting

A tab can contain multiple terminal sessions via `layoutsByTabId.sessionIdsByLeafId`. Tab-level actions alone are insufficient.

Add pane-level lifecycle controls in the split-pane UI/context menu, targeting the **leaf’s logical session ID**.

Required actions:

- Suspend Pane / Session
- Resume Pane / Session when suspended
- Restart Pane / Session if restart is exposed at pane level

Rules:

- suspending one split leaf must not suspend sibling leaf sessions;
- tab indicator may show suspended if the tab’s primary session is suspended; optionally evolve to aggregate “some panes suspended” state, but do not block pane-level correctness on aggregate icon design;
- focus transitions use leaf identity and must not auto-resume unrelated sleeping siblings;
- moving/detaching a pane to a new tab preserves its suspended state and backend session identity.

### 8.4 Tab-level suspend semantics with splits

Make the behavior explicit before implementation:

- **Decision:** existing tab context “Suspend Session” targets the tab’s primary `tab.sessionId` only, matching current lifecycle action behavior.
- Pane-level controls are the canonical way to suspend a non-primary split session.
- A future “Suspend All Panes in Tab” may be added separately; do not silently change the single-session command into a multi-session command.

---

## 9. Clean Restart Session Orchestration

### 9.1 Current defect

`requestSessionLifecycleAction("restart", sessionId)` currently falls through to `hibernateRegisteredSession(sessionId)`. This makes Restart Session semantically identical to destructive hibernate and relies on subsequent focus/resume behavior to create another process.

Restart must be explicit and deterministic.

### 9.2 Required restart transaction

Implement a dedicated operation, for example:

```ts
restartRegisteredSession(sessionId: string): Promise<void>
```

Transaction:

1. Acquire a per-session lifecycle operation lock/dedupe token.
2. Read the current logical session and its backend ID.
3. If live and suspended, do **not** resume merely to terminate; daemon close must be able to terminate a suspended process.
4. Call normal close/terminate on the existing backend session.
5. Await daemon acknowledgement that ownership/PTY resources are torn down.
6. Clear sleeping/suspended frontend state and old backend binding.
7. Spawn a fresh terminal using the same intended shell/profile/cwd/worktree/session metadata.
8. Rebind the **existing logical frontend session ID** to the newly returned backend session ID using the established workspace reducer/rebind path.
9. Persist the new binding before reporting restart success where existing strict persistence rules require it.
10. Focus the replacement pane only according to explicit Restart UX; do not rely on focus as the mechanism that causes the spawn.

### 9.3 Failure handling

- Close succeeds, spawn fails: frontend logical session becomes process-absent/standby or explicit failed state with “Open new shell / Retry” affordance. Never reattach the old backend ID.
- Close fails because process already exited: continue with fresh spawn if restart intent is still valid.
- Spawn succeeds, persistence fails: keep in-memory live binding but surface persistence/runtime error according to existing app policy; do not kill the successfully restarted process solely to restore old metadata.
- Concurrent suspend/resume/restart: serialize per logical session; restart has destructive precedence once accepted.

### 9.4 TabBar integration

`TabBar.tsx` should dispatch a dedicated restart callback/action. It may activate the tab for UX, but activation must not be what performs the restart.

Tests must assert Restart invokes close + fresh spawn/rebind and never invokes suspend/hibernate.

---

## 10. Cold Start Restore Policy

### 10.1 Preserve standby at app launch

Cold application launch is different because historical terminal processes normally do not exist. Lazy / Active-Only restore remains correct here.

`ui/src/state/workspaceRestore.ts::prepareDiskRestoredState()` already evaluates `loadGeneralSettings().sessionRestorePolicy` and assigns standby identities for missing local sessions under:

- `lazy`: all applicable missing local sessions sleep/standby;
- `activeOnly`: missing sessions outside the active restore set sleep/standby.

This logic should continue to describe **Standby**, not runtime Suspended.

### 10.2 Legacy hibernated migration

Current restore code preserves persisted `processState === "hibernated"` for standby records. During migration:

- deserialize legacy `hibernated` as `standby` because no live backend is guaranteed;
- persist only the new vocabulary going forward;
- do not add legacy hibernated session IDs to “suspended live process” state unless daemon reconciliation proves the backend is alive and suspended.

### 10.3 Fix `App.tsx` restore policy bypass

Current startup auto-resume call contains:

```ts
scheduleAgentAutoResume({
  ...,
  ignorePolicy: true,
  ...
})
```

Remove `ignorePolicy: true` from the normal app-start path.

`agentAutoResume.ts` already honors policy when `ignorePolicy` is false:

- Lazy -> no eager auto-resume.
- Active-Only -> auto-resume only eligible active restore targets.
- Eager policy, if supported -> resume according to existing policy semantics.

The `ignorePolicy` escape hatch may remain for narrowly scoped tests or explicit non-startup product behavior, but app boot must not use it.

### 10.4 Reconciliation ordering

Startup ordering should be:

1. load persisted workspace/session state;
2. query daemon live sessions;
3. reconcile which backend IDs genuinely still exist;
4. for missing local sessions, apply cold restore policy -> Standby where applicable;
5. for live daemon sessions, preserve actual daemon process state (Running/Suspended);
6. schedule agent auto-resume with user policy intact;
7. spawn/reconnect only the sessions allowed by policy.

This prevents a persisted record from forcing either an eager spawn or a bogus resume call.

---

## 11. Phase-by-Phase Implementation Roadmap

## Phase 1 — Backend daemon cross-platform suspend/resume primitives

**Primary paths:**

- `src-tauri/src/terminal/session.rs`
- `src-tauri/src/terminal/` platform helper modules as needed
- PTY spawn/config code that can capture Unix PGID or Windows process/job handles

**Tasks:**

1. Add `PtySessionState::Suspended`.
2. Audit PTY spawn ownership metadata.
3. Persist Unix terminal-owned PGID in `PtySession`.
4. Persist/open Windows process handle and confirm descendant process containment strategy.
5. Implement Unix SIGSTOP/SIGCONT group operations.
6. Implement Windows NtSuspendProcess/NtResumeProcess.
7. Implement Windows working-set trim after successful suspend.
8. Make close/kill work from Suspended.
9. Block PTY writes while Suspended.
10. Add typed errors and state reconciliation for process disappearance.

**Acceptance criteria:**

- PID/backend ID are unchanged across suspend/resume.
- interactive child/REPL state survives.
- CPU usage of a stopped Unix process group falls to zero runnable work.
- Windows target(s) are actually suspended and can resume.
- write calls while suspended are rejected.
- close of suspended session succeeds without first exposing/resuming it to user input.

---

## Phase 2 — Daemon protocol and IPC commands

**Primary paths:**

- `src-tauri/src/daemon/protocol.rs`
- `src-tauri/src/daemon/client.rs`
- `src-tauri/src/daemon/server.rs`
- `src-tauri/src/ipc/terminal.rs`
- `src-tauri/src/lib.rs`
- `ui/src/lib/tauri.ts`

**Tasks:**

1. Add Suspend/Resume daemon request/response variants.
2. Add daemon client methods.
3. Add server request routing and local ownership validation.
4. Add `cmd_terminal_suspend` and `cmd_terminal_resume`.
5. Register Tauri commands.
6. Add TS wrappers.
7. Extend describe/list wire data with live process state.
8. Add protocol compatibility tests.

**Acceptance criteria:**

- IPC suspend/resume are idempotent for already-target-state live sessions.
- missing backend is reported as missing, not successful suspend.
- CWD/output history/session attachment identity remain unchanged.
- remote/paired unsupported cases fail safely and explicitly.

---

## Phase 3 — Frontend state and reactive lifecycle

**Primary paths:**

- `ui/src/lib/types.ts`
- `ui/src/lib/sessionLifecycle.ts`
- `ui/src/lib/sessionPersistence.ts`
- lifecycle/persistence tests

**Tasks:**

1. Migrate `SessionProcessState` to Running/Suspended/Standby vocabulary.
2. Add legacy persisted `hibernated -> standby` migration.
3. Replace destructive runtime hibernate function with suspend.
4. Add resume operation with in-flight deduplication.
5. Keep backend ID on suspend.
6. Rework idle sweep to native suspend.
7. Rename manual hold bookkeeping and make it edge-triggered.
8. Ensure register/reconciliation does not erase suspended state.
9. Implement dedicated restart transaction separate from suspend.
10. Update external-store snapshots/tests.

**Acceptance criteria:**

- suspended logical session remains bound to the same backend ID;
- no `lifecycle: "exited"` is assigned for successful suspend;
- repeated renders cannot auto-resume a manually suspended active pane;
- one explicit user focus/click resumes exactly once;
- cold standby invokes spawn/reconnect, not native resume.

---

## Phase 4 — NativeTerminalPane surface detachment and TerminalPane overlay

**Primary paths:**

- `ui/src/components/NativeTerminalPane.tsx`
- `ui/src/components/TerminalPane.tsx`
- native terminal renderer/surface command implementation if visibility false currently still presents

**Tasks:**

1. Subscribe both components to `useSleepingSessionIds()`.
2. Derive `suspended` from logical session ID.
3. Include `!suspended` in native surface visibility/interactivity.
4. Verify `visible=false` detaches/stops WGPU work.
5. Gate all keyboard/mouse/paste/IME sends.
6. Add “Suspended (Click to resume)” overlay.
7. Add accessible pointer/keyboard resume interaction.
8. Ensure the same native backend attachment is restored after resume rather than spawning a new process.

**Acceptance criteria:**

- surface disappears immediately after suspend acknowledgement;
- no PTY input can be generated while suspended;
- overlay is clearly visible on active and inactive suspended panes;
- clicking overlay resumes the exact same process and surface.

---

## Phase 5 — TabBar, split-pane controls, and Restart Session fix

**Primary paths:**

- `ui/src/components/TabBar.tsx`
- `ui/src/components/tab-dnd/SortableTab.tsx`
- split pane container/menu components
- `ui/src/lib/sessionLifecycle.ts`
- App/reducer orchestration used for close/spawn/rebind

**Tasks:**

1. Remove `!active` blocker from sleeping indicator computation.
2. Rename Hibernate -> Suspend in menus/tests.
3. Show Resume action for suspended target.
4. Add pane-level Suspend/Resume controls using split leaf session IDs.
5. Implement clean restart: close old backend -> spawn new -> rebind same logical session.
6. Ensure restart never calls suspend/hibernate.
7. Preserve split topology and pane identity during restart.

**Acceptance criteria:**

- active manually suspended tab immediately shows indicator;
- individual split leaf can suspend without siblings;
- Restart changes backend/PID and preserves logical pane/tab placement;
- Suspend/Resume preserves backend/PID.

---

## Phase 6 — App startup restore policy repair

**Primary paths:**

- `ui/src/App.tsx`
- `ui/src/state/workspaceRestore.ts`
- `ui/src/lib/agentAutoResume.ts`
- related startup/restore tests

**Tasks:**

1. Remove `ignorePolicy: true` from normal startup `scheduleAgentAutoResume()`.
2. Preserve Lazy and Active-Only semantics in disk restore.
3. Treat legacy persisted hibernated process state as process-absent standby.
4. Reconcile live daemon Suspended sessions separately from cold standby.
5. Verify active session set includes split leaves as already gathered by `activeRestoreSessionIds()`.
6. Prevent startup code from spawning sessions policy says should remain standby.

**Acceptance criteria:**

- Lazy policy does not eagerly spawn restored missing sessions;
- Active-Only spawns/resumes only active restore targets according to existing policy contract;
- live daemon suspended session is not mistaken for missing/cold standby;
- no normal startup path forces `ignorePolicy`.

---

## Phase 7 — Verification and Test Plan

### 11.1 Rust unit tests

Add tests for `PtySession` lifecycle transitions:

- Running -> Suspended -> Running.
- duplicate Suspend and duplicate Resume idempotence.
- writes rejected while Suspended.
- Suspended -> Closing -> Exited.
- failed suspend does not mutate state.
- failed resume remains Suspended.

Platform-gated tests:

- Unix target PGID calculation and negative-PGID signaling helper.
- Windows NTSTATUS mapping and working-set trim partial failure behavior (mock/wrapper boundary where direct OS integration is impractical).

### 11.2 Daemon protocol tests

Cover:

- request serialization/deserialization for Suspend/Resume;
- success response mapping;
- missing session errors;
- suspended state in describe/list;
- close while suspended;
- remote/paired unsupported behavior.

### 11.3 Frontend unit tests

`sessionLifecycle.test.ts`:

- live backend + suspended set -> `suspended`;
- standby sentinel -> `standby`;
- legacy `hibernated` persisted record -> standby migration;
- manual active-pane suspend holds until actual interaction edge;
- successful resume clears sleeping state but preserves backend ID;
- native resume not used for standby;
- idle sweep calls suspend, not hibernate/close;
- restart calls clean restart orchestration only.

`TabBar` tests:

- active suspended tab indicator visible;
- context menu switches Suspend/Resume labels;
- Restart action is independent.

`TerminalPane` / `NativeTerminalPane` tests:

- suspended state renders overlay;
- visibility false is sent/derived;
- keyboard/mouse/paste handlers do not invoke PTY write while suspended;
- click invokes resume once.

`workspaceRestore` / `App` tests:

- Lazy stays lazy;
- Active-Only respects active split leaf collection;
- startup call no longer passes policy bypass;
- daemon live suspended session reconciles without respawn.

### 11.4 Unix integration tests

Spawn a test PTY process that:

1. holds mutable in-memory state;
2. forks/launches a child that periodically updates a counter or writes a heartbeat;
3. exposes PID/PGID.

Assert:

- after Suspend, root and child remain present;
- heartbeats stop;
- process IDs do not change;
- after Resume, heartbeat resumes;
- in-memory value remains unchanged;
- PTY interaction continues on same session.

Avoid timing assertion “resume <1 ms” as a hard CI contract. Product goal can be sub-millisecond syscall overhead, but end-to-end scheduling/wake time is host-dependent. Use a generous functional timeout and separately benchmark latency.

### 11.5 Windows integration tests

Spawn a ConPTY-backed process that allocates/touches memory and has child activity.

Assert:

- root/tree activity freezes according to the implemented ownership model;
- working-set size drops materially after trim in a controlled environment where API supports it;
- process IDs survive suspend/resume;
- state resumes correctly;
- demand paging does not corrupt terminal state.

Treat exact working-set byte amount as non-deterministic.

### 11.6 Manual E2E validation matrix

Run on:

- macOS current supported release;
- Linux with swap;
- Linux with zram if supported configuration is relevant;
- Linux with no swap to confirm truthful UX/docs despite limited anonymous reclamation;
- Windows current supported release.

Scenarios:

1. shell with exported variable;
2. Python/Node REPL with in-memory object;
3. long-running child process;
4. foreground TUI;
5. split tab with two independent shells;
6. manual suspend of currently active pane;
7. suspend inactive pane then select it;
8. repeated rapid click/focus transitions;
9. restart a running session;
10. restart a suspended session;
11. close a suspended session;
12. app cold launch under Lazy;
13. app cold launch under Active-Only;
14. daemon survives UI HMR/reload while a process is suspended.

For each scenario verify process identity, CPU behavior, UI indicator, surface detachment, input suppression, and correct wake path.

---

## 12. Concurrency and Race Handling

### 12.1 Per-session lifecycle serialization

Introduce a single in-flight lifecycle operation per logical/frontend session and corresponding daemon-side state transition protection.

Forbidden races to handle:

- Suspend vs Resume.
- Suspend vs Restart.
- Resume vs Close.
- Restart vs auto-idle sweep.
- pane activation vs manual suspend acknowledgement.

Recommended policy:

- operations are serialized;
- duplicate same-state operations coalesce;
- destructive Close/Restart prevents new Suspend/Resume once closing starts;
- stale completion handlers verify the current backend binding before updating frontend state.

### 12.2 Backend-ID generation safety

If a logical frontend session is rebound during restart, any pending suspend/resume promise must carry the backend ID it targeted. On completion, mutate state only if the logical session is still bound to that same backend ID.

This avoids an old Resume response marking a newly restarted process as the result of the previous lifecycle operation.

### 12.3 Daemon crash/reconnect

After daemon reconnect/restart:

- query live session list/state;
- if the expected backend session no longer exists, clear `Suspended` and reconcile to Standby/Exited recovery semantics;
- never spawn solely because the previous frontend set still says sleeping;
- never call Resume against a `standby:` sentinel.

---

## 13. Memory Behavior: Product Guarantees vs Best Effort

### macOS

Guaranteed by Ferryx:

- process group is stopped;
- CPU execution is paused;
- process identity/state is preserved until external termination or system failure.

Kernel-dependent:

- when/how much inactive memory XNU compresses or swaps.

### Linux

Guaranteed by Ferryx:

- process group is stopped;
- CPU execution is paused;
- virtual memory/process state is preserved.

Kernel/configuration-dependent:

- anonymous memory reclaim requires swap/zram availability and pressure;
- exact RSS reduction is not guaranteed.

### Windows

Guaranteed by Ferryx when APIs succeed:

- target process/tree is suspended according to the implemented ownership model;
- explicit working-set trim is requested immediately;
- process identity remains intact.

OS-dependent:

- exact resident-memory reduction and page-in latency.

### UX/documentation wording

Use “Suspend” rather than promises like “free all RAM.” A suitable explanation:

> Suspended sessions keep the same process and terminal state while using no active CPU. Ferryx asks the operating system to reclaim inactive memory where supported; actual RAM savings depend on OS memory pressure and configuration.

---

## 14. Migration and Compatibility

1. Keep old daemon `Hibernate` protocol temporarily only if compatibility with older clients requires it; new UI must not use it for runtime suspension.
2. Migrate persisted `processState: "hibernated"` to Standby on load.
3. Do not persist `Suspended` across full app/daemon shutdown unless a live daemon session is independently known to survive and can be reconciled. Persistence alone cannot prove a process still exists.
4. HMR/UI reload may recover Suspended if daemon session listing reports it live and suspended.
5. Update settings labels and tests that use “hibernate” terminology once migration is complete.
6. Remove obsolete destructive hibernate code only after no callers remain and compatibility window is closed.

---

## 15. Observability and Diagnostics

Add structured lifecycle telemetry/logging for:

- suspend requested / succeeded / failed;
- resume requested / succeeded / failed;
- backend ID and logical session ID (non-sensitive identifiers only);
- platform;
- duration of OS suspend/resume calls;
- Windows working-set trim success/failure separately from process suspend;
- auto-idle vs manual reason;
- restart close/spawn/rebind stages;
- reconciliation from Suspended -> Standby/Exited after missing backend.

Do not log terminal content, environment variables, command input, or memory contents.

Recommended counters:

- active running local sessions;
- suspended local sessions;
- standby restored sessions;
- suspend failures by normalized reason;
- resume failures by normalized reason;
- unexpected writes rejected while suspended.

---

## 16. Security and Safety Considerations

- Never signal arbitrary PIDs supplied directly by frontend. Resolve session ID through daemon-owned `PtySession` and use stored ownership metadata.
- Persist handles/PGIDs from trusted spawn state to avoid PID reuse races.
- On Unix, validate PGID is positive before negating it for `kill(-pgid, signal)`; never allow PGID 0 semantics to signal the daemon’s own process group.
- On Windows, minimize requested process-handle rights.
- Ensure suspension cannot target the Ferryx daemon itself.
- Paired/remote sessions require an authenticated remote-daemon protocol operation rather than local signaling.
- Resume/Restart commands must honor current session ownership/generation checks.

---

## 17. Definition of Done

The feature is complete only when all of the following are true:

1. Manual Suspend preserves PID/backend ID and in-memory REPL state on supported local platforms.
2. Resume restores the same process with no spawn/reconnect path involved.
3. Unix suspends the intended terminal-owned process group, not only a transient child.
4. Windows process/tree suspension semantics are verified and working-set trimming occurs after suspend.
5. Backend rejects input while suspended.
6. `TerminalPane` and `NativeTerminalPane` reactively subscribe to sleeping/suspended state.
7. Native WGPU surface is hidden/detached and no input is forwarded while suspended.
8. Suspended overlay clearly says “Suspended (Click to resume)” or equivalent.
9. Active suspended tab shows the paused/sleep indicator; no `!active` blocker remains.
10. Individual split panes can be suspended/resumed independently.
11. Manual suspend of an active pane does not immediately self-resume.
12. Restart Session performs close + fresh spawn + rebind and never calls suspend/hibernate.
13. Cold startup Lazy / Active-Only policy is preserved.
14. `App.tsx` normal startup no longer passes `ignorePolicy: true`.
15. Legacy persisted hibernated records migrate safely to Standby.
16. Unit, protocol, integration, frontend, and platform E2E tests pass.
17. Documentation accurately describes OS-dependent memory reclamation without promising deterministic byte-level RAM release.

---

## 18. Expected File-Level Change Map

### Backend

- `src-tauri/src/terminal/session.rs`
  - add Suspended state;
  - add suspend/resume;
  - input guard;
  - close-from-suspended handling;
  - store platform ownership metadata.
- `src-tauri/src/terminal/*unix*` / `*windows*` helper modules as appropriate
  - OS primitives and error normalization.
- PTY spawn builder/service files
  - capture PGID / durable Windows handle or job ownership.
- `src-tauri/src/daemon/protocol.rs`
  - Suspend/Resume wire variants and live-state reporting.
- `src-tauri/src/daemon/client.rs`
  - client methods and operation classification.
- `src-tauri/src/daemon/server.rs`
  - route local suspend/resume and return typed errors.
- `src-tauri/src/ipc/terminal.rs`
  - `cmd_terminal_suspend` / `cmd_terminal_resume`.
- `src-tauri/src/lib.rs`
  - command registration.

### Frontend lifecycle/state

- `ui/src/lib/types.ts`
  - SessionProcessState migration.
- `ui/src/lib/sessionLifecycle.ts`
  - suspend/resume/restart state machine and auto-idle behavior.
- `ui/src/lib/sessionPersistence.ts`
  - legacy migration and new state serialization.
- `ui/src/lib/tauri.ts`
  - suspend/resume IPC wrappers.

### Frontend UI

- `ui/src/components/TerminalPane.tsx`
  - reactive suspended overlay / resume click.
- `ui/src/components/NativeTerminalPane.tsx`
  - reactive surface visibility and input gating.
- `ui/src/components/TabBar.tsx`
  - active indicator fix; Suspend/Resume/Restart commands.
- `ui/src/components/tab-dnd/SortableTab.tsx`
  - wording/accessibility updates if needed.
- split-pane component(s)
  - pane-level lifecycle menu/action wiring.

### Startup/restore

- `ui/src/App.tsx`
  - remove startup `ignorePolicy: true`; route restart/rebind cleanly as needed.
- `ui/src/state/workspaceRestore.ts`
  - distinguish cold Standby from live Suspended and migrate legacy hibernated data.
- `ui/src/lib/agentAutoResume.ts`
  - preserve policy contract; likely test updates rather than semantic rewrite.

### Tests

- `src-tauri/src/terminal/*tests*`
- `src-tauri/src/daemon/*tests*`
- `src-tauri/src/ipc/tests.rs`
- `ui/src/lib/sessionLifecycle.test.ts`
- `ui/src/lib/sessionPersistence.test.ts`
- `ui/src/state/workspaceRestore*.test.ts*`
- `ui/src/components/TabBar*.test.tsx`
- `ui/src/components/TerminalPane*.test.tsx`
- `ui/src/components/NativeTerminalPane*.test.tsx`
- `ui/src/App*.test.tsx`

---

## 19. Recommended Implementation Order Within Pull Requests

To keep each change reviewable and avoid a half-migrated runtime:

1. **Backend capability PR:** PTY state + OS primitives + daemon protocol + tests, not yet exposed in UI.
2. **Frontend lifecycle PR:** new state vocabulary, suspend/resume API, legacy persisted migration, tests.
3. **UI integration PR:** native surface/input behavior, overlay, indicators, split controls.
4. **Restart semantics PR:** dedicated close/spawn/rebind path and tests.
5. **Startup policy PR:** remove policy bypass and strengthen cold restore reconciliation/tests.
6. **Terminology cleanup PR:** remove obsolete hibernate runtime code/protocol after compatibility requirements are satisfied.

If delivered as one feature branch, keep commits in that dependency order so bisection never exposes a frontend suspend action before the daemon supports it.

---

## 20. Final Architectural Decision

Ferryx will treat **Suspend** and **Standby** as separate first-class lifecycle states.

- **Suspend** is a live-process operation implemented with native OS primitives. It preserves process/PTY identity and allows the OS to reclaim physical memory without destroying runtime state.
- **Standby** is a cold-restore state used when no process exists, governed by the user’s startup restore policy.
- **Restart** is a third, explicitly destructive operation that closes the old process and creates a fresh one.

This separation removes the architectural contradiction in the old “hibernate then respawn” behavior and gives Ferryx predictable semantics: suspend means the same process comes back; standby means no process has been started yet; restart means intentionally create a new process.
