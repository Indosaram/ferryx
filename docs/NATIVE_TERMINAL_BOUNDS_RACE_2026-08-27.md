# Native Terminal Bounds Race + Agent Activity Verification — 2026-08-27

## Result

`Failed to update native terminal bounds` appeared in the terminal area during rapid tab switching.
The visible banner was the smallest of three defects on that path: the same race also let a
backgrounded pane rebuild a GPU surface, and — as a consequence of an earlier, uncommitted change to
detach semantics — let a backgrounded pane accept keystrokes, pastes, mouse events, and selections.

All three are fixed and covered by tests. Verification criteria C1–C5 are recorded below; C1–C4 are
verified at the code/test level here, and the parts that need a running desktop build are listed as
manual steps rather than claimed as observed.

| Gate | Result |
| :--- | :--- |
| `bun run --cwd ui test` | 101 files / 876 tests passed, 0 failed |
| `bun run --cwd ui build` (runs `tsc`) | exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features native-terminal` | 315 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml --features native-terminal` (integration) | all suites pass except the pre-existing parallelism constraint below |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --features native-terminal` | no new warnings |

**Pre-existing, not caused by this work:** `tests/daemon_persistence_contract.rs` fails when run in
parallel. Clean `HEAD` (d4315cb) fails 4 of 9 the same way; this tree fails 3. It passes fully with
the invocation `src-tauri/AGENTS.md` documents:

```
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1
# 9 passed, 0 failed
```

## Root cause

`detach_session` was previously "release the surface **and** drop the session". Uncommitted work
changed it to "release the GPU surface, **keep** the session and its daemon pump", so a backgrounded
agent keeps reporting title/bell/agent-state instead of freezing on its last observed value. That is
the right behavior, but three call sites still used *session existence* as a proxy for *a pane owns
this surface*, and that proxy silently stopped meaning anything.

1. **Bounds banner.** A pane's `ResizeObserver` callback and layout pass are asynchronous. Switching
   tabs quickly dispatches `cmd_native_terminal_set_bounds` for a session already detached.
   `render` → `prepare_session_layout` would *create* a session on demand and then build a host for
   it, or fail with `NoValue`, which `NativeTerminalPane` rendered as a fatal error banner.
2. **Surface resurrection.** Because `prepare_session_layout` creates on demand, a late geometry
   update could rebuild a compositor surface for a pane nobody can see — an invisible GPU host leak,
   worse than the banner it produced.
3. **Input reaching backgrounded panes.** `encode_attached_native_input` and its siblings in
   `src-tauri/src/ipc/native_terminal.rs` gated on `snapshot_for_session(...).is_none()`. Once detach
   stopped removing the session, that guard passed for every detached pane. Keystrokes, pastes, mouse
   events, selection, copy, search, scroll, and scrollbar reads all became reachable on a pane with
   no surface. The integration suite caught this as 4 failures the moment the proxy was replaced.

## The fix

The session now carries an explicit predicate instead of an inferred one.

| Change | File |
| :--- | :--- |
| `NativeTerminalSession.surface_attached: bool` — whether a frontend pane currently owns a compositor surface | `src-tauri/src/native_terminal/surface_host.rs` |
| `ensure_surface_attached(session_id)` — the single predicate; `Err(SessionDetached)` for both a detached and an unknown session | `src-tauri/src/native_terminal/surface_host.rs` |
| `render` calls `ensure_surface_attached` **before** any layout or host work, so a late update can no longer resurrect a surface | `src-tauri/src/native_terminal/surface_host.rs` |
| `detach_session` clears the flag; `attach_daemon_attachment` and `attach_daemon_attachment_with_bounds` re-arm it | `src-tauri/src/native_terminal/surface_host.rs` |
| `NativeTerminalError::SessionDetached(String)` — a benign, distinguishable outcome, not a generic `NoValue` | `src-tauri/src/native_terminal/error.rs` |
| `From<NativeTerminalError> for IpcError` maps `SessionDetached` → `IpcErrorCode::SessionNotFound` (feature-gated on `native-terminal`) | `src-tauri/src/ipc/error.rs` |
| `require_attached_surface` gates all eight production input/selection/scroll boundaries | `src-tauri/src/ipc/native_terminal.rs` |
| `isDetachedSurfaceError` — a `SESSION_NOT_FOUND` bounds failure is dropped silently; every other failure still shows the banner | `ui/src/components/NativeTerminalPane.tsx` |

The frontend checks `StructuredIpcError.code`, per the `ui/AGENTS.md` anti-pattern against parsing
IPC error message strings.

## Contract tests updated to the new detach semantics

Four tests in `native_terminal_surface_host_contract.rs` and four in
`native_terminal_input_boundary_contract.rs` encoded the *old* "detach destroys the session"
contract and were failing in the working tree before this change. They now assert the intended
behavior rather than the historical one:

- Detach keeps the session queryable; `close_session` is what discards it.
- A **detached session still emits title and bell** — that is the entire point of keeping the pump
  alive. Only `close_session` silences it. (Previously this test asserted the opposite.)
- Detach drains a pending render but keeps the coordinator; close removes it.
- "must not lazily recreate detached native state" is now probed with
  `ensure_surface_attached(...).is_err()` — a rejected input must not re-attach the surface. The old
  probe (`snapshot_for_session == None`) no longer means anything after detach.

New test `native_terminal_geometry_update_after_detach_is_refused_until_reattach` covers the race
directly: attached accepts → detach refuses → unknown session refuses → re-attach accepts.

New test in `ui/src/components/NativeTerminalPane.test.tsx`,
"stays silent when a bounds update loses the race with its own detach", asserts no `role="alert"` and
no console error for a `SESSION_NOT_FOUND` bounds rejection. The pre-existing test asserting a
generic bounds failure *does* still show the banner remains green, so the fix is not a blanket
suppression.

## Follow-up defect: the completion dot never went out

Reported from a real run: a freshly started `omo` with nothing to read still showed a green dot on
its tab, and the dot never cleared.

**Root cause.** `done` was stored as a durable property of the session instead of a request for
attention. Two consequences:

1. An agent that boots with a spinner and settles at its prompt walks `working -> idle` with no user
   turn at all. `SESSION_SCREEN_ACTIVITY` maps that `idle` to `done` (correctly — it only fires when
   the previous state was `working`/`waiting`), so startup manufactured a completion.
2. `ACTIVATE_TAB` cleared `unreadTabIds` but never touched the `done` activity, and
   `resolveActivityIndicator` returns `"done"` whenever `summary.hasDone` is set. So the dot stayed
   lit on the tab the user was actively looking at, forever.

The green `#86efac` dot in the report is `status-success` (`done`), not `unread` (`--primary`,
`#e5e5e5`) — which is why activating the tab did nothing for it.

**Fix.** `TerminalActivity` gains `seen?: boolean`, and `summarizeActivities` stops counting a `done`
that is `seen`. A completion is marked seen in exactly two places:

- `applySessionActivity` — a `done` arriving on a **visible** tab is born acknowledged; the user is
  already watching it.
- `acknowledgeTabCompletions`, called from both `ACTIVATE_TAB` branches — opening a tab is how the
  user reads its completion, mirroring the unread flag that branch already clears.

The activity entry itself is kept rather than deleted, so the tab retains its agent brand icon (C1/C2
would otherwise regress to the terminal glyph). The next `working`/`waiting` turn builds a fresh
activity without `seen`, so a real completion signals again.

Captured RED before any production change (`ui/src/state/screenActivity.test.ts`):

```
FAIL 8. a completion on the tab the user is watching shows no attention dot
  expected 'done' to be null
FAIL 9. activating a tab acknowledges its completion dot
FAIL 10. a new turn after acknowledgement signals again
  expected 'done' to be null
```

Test 10 is the guard against over-correcting: after acknowledgement, a genuine turn that finishes
while the tab is in the background must still raise `unread`.

Notifications are unaffected. `NotificationCoordinator.handleAgentStateChange` gates on
`isFocused()`, and a completion on a visible tab is one the user is already watching — no macOS
banner is wanted there.

## Follow-up defect: the sidebar spinner vanished on a project switch

Reported from a real run: a tab was spinning, the user switched to a worktree in another project, and
that worktree's sidebar row showed no spinner. Switching back made it reappear.

**Root cause.** The workspace store is mounted per project — `useWorkspaceStore({ workspaceId })` —
but the sidebar lists worktrees from **every** registered project. `worktreeActivity` was computed
from the mounted workspace alone, so rows belonging to any other project had no activity data at all.

A same-project worktree switch was never broken: `SELECT_WORKTREE` parks the layout in
`worktreeLayouts`, and `getAllTabs`/`getTabSessionIds` already read parked layouts. The first test in
`worktreeActivityAcrossSwitch.test.ts` pins that so the two cases do not get conflated again.

There was a second half. The native title/agent-state subscriptions resolve a payload through
`stateRef.current`, i.e. the mounted workspace only, and return early otherwise. Events for a project
the user had switched away from were therefore **dropped**, freezing its snapshot at switch time.
Showing snapshot data without fixing that would have traded a missing spinner for one stuck spinning
forever.

**Fix.**

| Change | File |
| :--- | :--- |
| `listWorkspaceSnapshots()` exposes the per-project snapshot cache | `ui/src/state/workspaceSnapshotCache.ts` |
| `selectWorktreeActivitySummariesAcrossWorkspaces(mountedWorkspaceId)` summarises every **other** workspace; the hook spreads it **under** the live map so the mounted workspace always wins | `ui/src/state/workspaceStore.ts` |
| `dispatchToParkedWorkspace` routes an unresolved native event to the snapshot that owns the session, runs the reducer, and stores the result | `ui/src/state/workspaceStore.ts` |
| `parkedActivityVersion` re-renders on snapshot-only changes, which `useMemo` cannot otherwise observe | `ui/src/state/workspaceStore.ts` |

Covered by `ui/src/state/worktreeActivityAcrossSwitch.test.ts` (4 tests): the same-project switch, the
cross-project spinner, a parked workspace advancing past its completion, and the mounted workspace
taking precedence over its own lagging snapshot.

### Known flaky test, pre-existing

`src/App.test.tsx > routes the native Cmd+W menu accelerator to close the focused pane in a split
terminal tab` is order- and load-dependent: it fails 3/3 when the file is run alone and
intermittently in the full suite, **with and without** the change above (verified by stashing the fix
and re-running the full suite: same failure, 871 passed / 1 failed). It shares a mutable
`workspace.storeState` across tests and drives assertions through `waitFor` defaults. Not fixed here;
it needs its own pass to remove the shared-state coupling.

## Unrelated defect found and fixed

`ui/src/agentTabLogoContrast.test.ts` read the stylesheet with `import stylesheet from "./index.css?raw"`.
Vite's CSS pipeline claims the module before the raw loader, so `stylesheet` was `""` and both
assertions passed vacuously — the test could never fail. Switched to `readFileSync`, the convention
already used by `appearanceThemeContract.test.ts` and `devRuntimeContract.test.ts`. The assertions
now genuinely exercise the two `.agent-tab-logo--monochrome` rules in `ui/src/index.css`.

## Verification criteria C1–C5

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| C1 | `omo` shows the omo brand icon, no terminal fallback | Code + test verified | `SUPPORTED_AGENT_LOGOS.omo → assets/agent-logos/omo.svg`; `SortableTab.tsx` renders `[data-testid="tab-agent-icon"][data-agent-type="omo"]` when `resolveAgentLogo` returns non-null, `TerminalSquare` only when it returns null. Asserted directly by `SortableTab.agentIcon.test.tsx` (5 tests green). |
| C2 | `agy` shows the antigravity brand icon | Code verified; no dedicated test | `SUPPORTED_AGENT_LOGOS.antigravity → assets/agent-logos/antigravity.svg`; the same `resolveAgentLogo` render path that C1 tests emits `[data-agent-type="antigravity"]`. `SortableTab.agentIcon.test.tsx` covers `codex`, `omo`, and the unsupported/absent fallbacks, but not `antigravity` specifically. |
| C3 | Working turn shows a rotating spinner | Code + test verified | `StatusDot` returns `LoaderCircle` with `data-status-state="working"` and `animate-spin` (plus `motion-reduce:animate-none`) for state `working`; `resolveActivityIndicator` gives `working` precedence below `waiting`. |
| C4 | Background completion fires a macOS notification and an unread dot | Code verified; **system notification needs a manual run** | `notificationCoordinator.ts` calls `dispatchNotification` → `cmd_notification_dispatch`; `resolveActivityIndicator` returns `unread` → `SortableTab` renders `[data-testid="tab-unread-dot"]`. Whether macOS actually presents the banner depends on the granted notification permission of a real signed build — run section J of `docs/AGENT_ACTIVITY_NOTIFICATION_MANUAL_E2E.md`. |
| C5 | Repeated tab switching produces no bounds error; all suites pass | Verified | Backend refuses the late update as `SESSION_NOT_FOUND`; frontend drops it without `setError`. All four gates in the table above are green. |

### A note on `omp`

The task brief listed `omp` alongside `omo` as an icon-map entry. There is no `omp.svg` in
`ui/src/assets/agent-logos/` (12 logos: antigravity, claude, cline, codex, copilot, cursor, gemini,
grok, kimi, omo, opencode, pi), so `resolveAgentLogo("omp")` returns `null` and the tab renders the
terminal glyph. That is the correct behavior under the standing rule that tab icons use real bundled
brand assets only, with the terminal icon as the fallback for anything unrecognized. Adding an `omp`
map entry without a real `omp` brand asset would violate it. If `omp` should carry a distinct icon,
the asset has to be added first.

### Manual step still required

C1–C4 are verified against the code and the automated suites. Confirming the **rendered** result and
the **system-level** macOS notification requires driving the desktop app, which is not automated here:

1. `cargo tauri dev` (or `bun tauri dev`, which is the same thing via the root `tauri` script). Do
   **not** append `--manifest-path src-tauri/Cargo.toml`: the Tauri CLI has no such flag and exits 2
   with `error: unexpected argument '--manifest-path' found`. It locates `src-tauri/tauri.conf.json`
   itself.
2. Run `omo` in one tab and `agy` in another; confirm each tab shows its brand icon, not the terminal
   glyph.
3. While an agent is mid-turn, confirm the tab shows the rotating spinner.
4. Switch away from that tab and let the turn finish; confirm a macOS notification banner appears and
   the background tab gains an unread dot.
5. Switch tabs rapidly (10+ times) and confirm no `Failed to update native terminal bounds` banner
   appears in any pane.

## Notes for the next reader

- `detach` and `close` are **not** interchangeable. Detach = the pane went away, keep streaming.
  Close = the session is over. Any new code that needs "does a pane own this surface" must call
  `ensure_surface_attached`, never infer it from session existence.
- `daemon_persistence_contract` requires `--test-threads=1`; a bare parallel run is a false red.
- Do not use `import "...css?raw"` in this repo's tests. It silently yields `""`.
