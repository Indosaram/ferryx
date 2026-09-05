# Code Review: Workspace/Worktree Deletion Feature (Round 2)

**Date:** 2026-09-04
**Status:** All review blockers fixed (2026-09-04, same day). See "Fix Round" at the bottom.
**Scope reviewed:** `git diff` of 14 declared files (worktree manager/mod/ipc, Sidebar, WorktreeList, ProjectDialogs, WorktreeDeleteDialog, App.tsx, App.test.tsx, tauri.ts) plus untracked ContextMenu.tsx / nativeMenu.ts / native_menu.rs discovered during review.
**Method:** 5 parallel review lanes (review-work orchestrator). Two lanes errored on provider 503/429 and were retried on a different provider; all 5 reached terminal state.

---

## Overall Verdict: FAILED (not approval-ready) — with 2 of 3 blockers being cross-lane-confirmed pre-existing gaps, 1 fixed during review

| # | Review Area | Agent | Verdict | Confidence |
|---|------------|-------|---------|------------|
| 1 | Goal & Constraint Verification | oracle | FAIL (UI scope achieved; fallback + error-surfacing gaps) | HIGH |
| 2 | QA Execution (test/build) | omo-senpi-qa-executor | PASS (152/152 UI, 26/26 Rust, tsc+vite clean, cargo check clean) | HIGH |
| 3 | Code Quality | omo-senpi-code-reviewer | FAIL / REQUEST_CHANGES (3 MAJOR) | HIGH |
| 4 | Security (supplementary) | oracle | FAIL / MEDIUM (revocation gap; 2 LOW) | HIGH |
| 5 | Context Mining | explore | PASS (no missed requirements; contracts respected) | HIGH |

## Blocking Issues (priority order)

1. **[Cross-lane: Security MEDIUM + CodeQuality MAJOR] Daemon-registry revocation gap.**
   `cmd_project_unregister` clears only the Tauri-app `WorkspaceRegistry`. The daemon's separate registry (which backs the remote gateway: `POST/DELETE /worktrees`, workspace selection, spawn) keeps the binding — `DaemonRequest` has no `UnregisterWorkspace`, and `cmd_project_register` does propagate while unregister does not. A paired remote device with a `Control` token retains full access to a "removed" repo for the daemon's lifetime.
   Fix: add `UnregisterWorkspace` to the daemon protocol + server handler (also terminating that workspace's live sessions), expose `DaemonClient::unregister_workspace`, call it from `cmd_project_unregister`; propagate failures.

2. **[CodeQuality MAJOR] Deleting an inactive project's worktree leaves a stale row.**
   `onDeleted` refreshes only the active workspace; the deleted worktree remains in `inactiveProjectWorktrees` cache and stays visible/actionable in the sidebar.
   Fix: evict the path or re-list the owning inactive project after successful deletion; add an App-level regression test.

3. **[Found at report time — concurrent workstream] Native popup commands unregistered.**
   During this review a parallel native-menu migration replaced the React `ContextMenu` in `ProjectHeader`/`WorktreeRow` with `openNativePopupMenu("cmd_native_sidebar_context_menu", ...)`, but `lib.rs` does not register `cmd_native_sidebar_context_menu` (nor install the menu-event forwarder). Until registered, right-click menus fail silently in the real app (`.catch(() => undefined)`) — i.e. the original user complaint would reproduce. The migration is evidently in flight (`ipc/tests.rs` currently has 9 `SpawnTerminalRequest.inherit_from_session_id` compile errors from the same workstream); this item belongs to that lane.

## Fixed during this review

- Active-worktree deletion fallback now prefers the owning project's primary worktree (repoRoot match), then any remaining row of the owner, then any row (`ui/src/App.tsx`), per the stated requirement.
- `unregisterProject` no longer swallows errors; `handleConfirmRemoveProject` awaits it and routes failures to `reportRuntimeError` (only success is toasted).
- Stale Rust test renamed: `dirty_worktree_survives_force_remove_but_yields_to_explicit_destructive_delete` (name now matches behavior incl. destructive yield).
- Sidebar/WorktreeList context-menu tests migrated from the removed React-menu DOM assertions to the native-popup contract (mocked `openNativePopupMenu`, assert `Delete Worktree`/`Remove Project` items with correct `enabled` gating, dispatch `onAction("delete"/"remove")`).
- `App.test.tsx` tauri mock gained `spawnTerminalsBatch` (required by the concurrent workspaceStore change).
- `WorktreeList.test.tsx` trailing newline restored.

## Non-blocking findings

- `unregisterProject`/`cmd_path_reveal`: LOW — Windows `explorer /select,"path"` manual quoting is escape-unsafe; consider `.arg(format!("/select,{}", path))` and confining reveal paths to registered roots.
- LOW — `cmd_path_reveal` discards `spawn()` results (`let _ =`), so reveal failure reports success.
- MINOR — `remove_worktree(path, force)` dirty semantics now duplicated with `remove_worktree_locked`; `force` on the public method is dead for the dirty case — consider renaming/removing.
- MINOR — destructive dirty-worktree Rust test asserts directory removal but not branch deletion; add a branch-absent assertion.
- MINOR — `App.test.tsx` WorktreeDeleteDialog mock uses `any` props.
- INFO — dead code in `manager.rs` (`WriterLeaseGuard`, `acquire_writer_lease`) predates this change.
- INFO — `ipc::native_menu` restoration in `ipc/mod.rs` has no deletion relevance; history shows it was absent (not commented out) upstream.

## What was verified as correct

- Path traversal closed end-to-end (canonicalize + root-jail + `--` git args); workspaceId/slug validated; `resolve_worktree` re-derives paths server-side.
- Project removal never touches the filesystem (verified by grep of the full call path); dialog copy is accurate.
- Safe deletion still refuses dirty worktrees; destructive deletion is reachable only after a failed safe delete with `UNMERGED_BRANCH`/`DIRTY_WORKTREE`, behind a distinct labeled button; remote gateway exposes only non-destructive deletion.
- Multi-project `workspaceId` routing confirmed (`data-workspace-id="project-2"` while project-1 active).
- Writer-lease and delete-lock invariants preserved (no self-deadlock from the new dirty check).

---

# Fix Round (2026-09-04, after review)

All blocking issues and actionable minor findings were fixed and verified:

## 1. Daemon revocation gap (Review blocker #1 — Security MEDIUM / Quality MAJOR) — FIXED
- `daemon/protocol.rs`: new `DaemonRequest::UnregisterWorkspace { workspaceId }` and `DaemonResponse::UnregisterWorkspaceOk`.
- `daemon/server.rs`: new `handle_unregister_workspace` — removes the daemon `WorkspaceRegistry` binding (idempotent; unknown workspace is a success no-op, e.g. after daemon restart) and **terminates every live PTY session owned by the workspace** (local sessions via `handle_close`, legacy peer sessions via `peer.close`, then `release_session_ownership` for idempotency cache / metadata / provider claims), so paired remote devices cannot keep using already-spawned sessions of a removed project.
- `daemon/client.rs`: new `DaemonClient::unregister_workspace` (mutating request — never retried, matching `Close`).
- `ipc/project.rs`: `cmd_project_unregister` now takes the daemon client, revokes the daemon binding FIRST (failures propagate to the UI), and only then cleans the local registry — registration and removal are now symmetric.
- New Rust test: `test_server_unregister_workspace_is_idempotent_and_revokes_registration`.

## 2. Stale inactive-project worktree row after deletion (Review blocker #2 — Quality MAJOR) — FIXED
- `ui/src/state/inactiveProjectWorktrees.ts`: the hook now subscribes to the backend `worktree_changed` push events (per the no-polling convention) and re-lists the affected inactive project on `deleted` / `destructivelyDeleted` / `pruned`, preserving the plain-folder root fallback. This also covers deletions made from another desktop or a remote client, not just the sidebar.
- Tests: "re-lists an inactive project when the backend reports one of its worktrees deleted" and "ignores worktree change events for projects it does not track" (`inactiveProjectWorktrees.test.tsx`).

## 3. Native popup commands unregistered (Review blocker #3 — concurrent native-menu stream) — FIXED
- `lib.rs`: registered `cmd_native_terminal_context_menu`, `cmd_native_tab_context_menu`, `cmd_native_new_tab_menu`, `cmd_native_sidebar_context_menu` in the invoke handler and installed `register_menu_event_forwarder(app.handle())` in setup, so `ferryx://menu-action` events reach the JS listeners and the sidebar/TabBar native context menus actually open.

## Minor findings fixed
- `cmd_path_reveal`: spawn failures now return a structured `IpcError` instead of being discarded (`let _ =`); Windows argument changed from the manually quoted `/select,"{path}"` to a single unquoted `/select,{path}` so Rust's arg escaping owns quoting (no argument-boundary injection via paths containing quotes).
- `worktree/mod.rs`: destructive dirty-worktree test now also asserts the branch is gone after `delete_worktree_and_branch_destructive`.
- Doc comment moved out of the `cmd_native_terminal_attach` parameter list (syntax error blocking the lib build).

## Fix-round verification
- Frontend: `bun run --cwd ui test` — **139 files / 1412 tests, all passed** (69.4s); `tsc --noEmit` exit 0.
- Backend: `cargo test --lib` — **539 passed / 0 failed** (includes the new unregister revocation test and the 30 worktree tests).
- Note: `dag::watcher::tests::test_dag_watcher_detects_checkpoint_changes` showed intermittent 5s-timeout flakiness during the session (FSEvents latency under heavy parallel build load; passed in the final full run). Unrelated to the deletion change — watcher code was last touched by commit `1c838ed`, not this work. Worth a dedicated timing-flake pass.
