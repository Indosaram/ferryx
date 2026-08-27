# Agent Activity — Commit Plan (pending user approval)

> **Later work supersedes the architecture described here.** The reporting path is no longer inferred
> from terminal titles alone: the agent reports its own state over a daemon UDS socket. See
> `docs/AGENT_ACTIVITY_STATE_MODEL_2026-08-27.md`, and
> `docs/AGENT_ACTIVITY_NOTIFICATION_MANUAL_E2E.md` section J for the checks that path needs.

Written 2026-08-26. Nothing has been committed. The work is verified and sits in the working tree.

## Why this is not committed yet

A different concurrent session is writing this same tree and has **21 files staged in the git index**,
covering native-terminal work unrelated to this change set:

```
src-tauri/src/ipc/debug.rs
src-tauri/src/ipc/mod.rs
src-tauri/src/ipc/native_terminal.rs
src-tauri/src/lib.rs
src-tauri/src/native_terminal/surface_host.rs
src-tauri/tests/native_terminal_surface_host_contract.rs
ui/src/components/NativeTerminalPane.tsx
ui/src/components/NativeTerminalPane.test.tsx
ui/src/components/NativeTerminalPane.lifecycle.test.tsx
ui/src/lib/nativeTerminalLifecycle.ts
ui/src/lib/nativeTerminalLifecycle.test.ts
ui/src/lib/tauri.ts
ui/src/state/workspaceRestore.ts        (+ its test)
ui/src/state/workspaceRuntime.ts
ui/src/state/projectSwitchBack.test.tsx
ui/src/App.test.tsx
docs/PROJECT_WORKTREE_SWITCHING_FIX_REPORT.md
...
```

Proof the index is not this change set's: `git show :ui/src/App.tsx | rg -c NotificationCoordinator`
returns **0**, so the staged `App.tsx` predates the notification wiring. A plain `git commit` would
sweep the other session's in-flight work into this commit, which is hard to unwind on shared state.

`ui/src/App.tsx`, `ui/src/state/workspaceStore.ts`, and `ui/src/state/workspaceStore.test.tsx` are
touched by **both** sessions, so those three need care: stage them only when the other session's work
is settled, or split by hunk.

## Files owned by this change set

Wholly owned — safe to stage as whole files:

```
ui/src/lib/agentTitle.ts
ui/src/lib/agentTitle.test.ts
ui/src/lib/activity.ts
ui/src/lib/activity.test.ts
ui/src/lib/notificationSettings.ts
ui/src/components/WorktreeList.tsx
ui/src/components/WorktreeList.test.tsx
ui/src/components/tab-dnd/SortableTab.tsx
ui/src/components/tab-dnd/SortableTab.agentIcon.test.tsx
ui/src/state/workspaceActivity.test.tsx
ui/src/App.notifications.test.tsx
docs/AGENT_ACTIVITY_NOTIFICATION_MANUAL_E2E.md
docs/AGENT_ACTIVITY_IMPLEMENTATION_VERIFICATION_2026-08-26.md
.omo/plans/agent-activity-notification-icons.md
```

Shared with the concurrent session — stage by hunk, not whole file:

```
ui/src/App.tsx                  (notification wiring: the coordinator ref, the agent-state effect,
                                 handleTerminalBell, and onBell on TerminalSplitView)
ui/src/state/workspaceStore.ts  (isAgent, isTabVisible + its two call sites, the four useMemo
                                 wrappers, ActivityNotificationTarget/selectActivityNotificationTargets)
```

## Proposed commits

Convention observed from `git log --oneline -20`: Conventional Commits, `fix(scope): imperative`,
short subjects, `fix(ui)` for frontend work.

1. `fix(ui): anchor agent title status classification`
   `ui/src/lib/agentTitle.ts` + test. Start-or-end status-segment anchoring with the spinner gate.

2. `fix(ui): correct agent activity state and unread scope`
   `ui/src/state/workspaceStore.ts` (in-scope hunks) + `ui/src/state/workspaceActivity.test.tsx`.
   Real `isAgent`, group-visible unread via `isTabVisible`, memoized selectors,
   `selectActivityNotificationTargets`.

3. `feat(ui): show agent type on terminal tabs`
   `ui/src/lib/activity.ts` + test, `ui/src/components/tab-dnd/SortableTab.tsx` + its new test.
   `ActivitySummary.agentType` precedence plus the icon with the status dot overlaid.

4. `feat(ui): notify on background agent completion and bell`
   `ui/src/App.tsx` (in-scope hunks) + `ui/src/App.notifications.test.tsx`.

5. `fix(ui): use theme token for idle worktree status`
   `ui/src/components/WorktreeList.tsx` + test, `ui/src/lib/notificationSettings.ts`
   (ferryx event namespace).

6. `docs: record agent activity verification and manual E2E`
   The three markdown artifacts.

Each of 1-5 keeps the suite green on its own; the split follows the write-scope boundaries the
implementation lanes already respected.

## Before committing

- Confirm the concurrent session is done, or stage by hunk for the two shared files.
- Re-run `cd ui && npx vitest run` and `bun run --cwd ui build` at the tip.
- Stage explicit paths. Never `git add .` while the shared index holds another session's work.
