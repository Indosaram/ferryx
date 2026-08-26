# Project / Worktree Full Audit — 2026-08-26

Scope: every code path that registers a project, lists worktrees, attributes a worktree row to a
project, switches the active project, and persists per-project workspace state.

Method: 6 read-only audit lanes over `ui/src/App.tsx`, `ui/src/components/Sidebar.tsx`,
`ui/src/state/*`, `src-tauri/src/ipc/project.rs`, `src-tauri/src/worktree/*`, plus independent
verification of every claim (a lane report was only trusted after I reproduced it).
Each finding below was locked with a failing test first, then fixed.

## Reported symptom

Switching back and forth between projects produced a sidebar where a `superwiki-*` row carrying the
`primary` badge was nested under the `orca-lite` project, while `superwiki-mail-otp` itself showed
"No Git worktrees found for this repository." Adding a worktree then dimmed the screen with no
error and no interaction possible.

Root cause was not one bug. Eleven defects across four layers combined: rows were attributed to
whichever project was active rather than to their owning path, async work started for one project
landed in another, and per-project state was persisted under the wrong ID during a switch.

## Findings

| ID | Layer | Defect | Fix |
|----|-------|--------|-----|
| F1 | Registration | `WORKSPACE_ALREADY_REGISTERED` was treated as fatal, permanently blocking the project | Treat as success; the workspace exists, which is the desired end state |
| F2 | Async ownership | Store mutations from an in-flight request applied to whatever workspace was mounted at resolve time. `openTab`, `ensureSessionBackends`, and `createBrowserTab` all landed work after a switch | Each re-checks the owning workspace after `await`; a superseded spawn closes its PTY and a superseded browser closes its webview instead of leaking |
| F3 | Persistence | Switching away saved the incoming project's state under the outgoing ID, and an empty layout wiped a valid saved layout | Snapshot the outgoing workspace on swap; persist only when the state's `workspaceId` stamp matches the target |
| F4 | Restore | A worktree refresh created tabs before restore ran, so restore saw a non-empty layout and short-circuited forever | Refresh syncs worktrees without creating tabs until restore has completed |
| F5 | Registration | The UI kept its locally derived ID instead of the canonical ID the backend returned | Adopt the returned `workspaceId` as the source of truth |
| F6 | Async ownership | Refresh dedupe was global, so a stale refresh could satisfy the new project's request | Dedupe keyed per workspace with a generation token |
| F7 | Async ownership | `pendingBackendRecovery` held bare session IDs and `pendingRemoteSlug` held a bare slug, so a switch mid-flight respawned the previous project's sessions or opened a same-named worktree in the wrong project | Both payloads carry their owning `workspaceId` and are dropped on mismatch |
| F8 | Registration | A transient registration failure permanently gated the project | Retry transient failures instead of latching a permanent gate |
| F9 | Ownership | A trailing slash on a registered root broke path attribution, and a sibling directory sharing a name prefix looked owned | Normalize roots, match on path boundaries, deepest owning root wins. The finding's Rust half is rejected below |
| F10 | Registration | Find-then-insert was not atomic: two concurrent registrations could bind two IDs to one canonical root | `register_unique_root` finds and inserts under a single write lock and returns the existing owner |
| F11 | Persistence | `recoveredFromHmr` was pinned to the first mounted workspace, so restore chose the wrong HMR-vs-disk path after a switch | Provenance tracked per mounted workspace and updated on swap |

Two further items were investigated and deliberately not changed:

- The daemon "registration hang" theory is false. `src-tauri/src/daemon/client.rs` already enforces
  a 15s response timeout, so an added 5s timeout only swallowed real daemon errors. That patch was
  reverted.
- `WorkspaceRegistry::register` intentionally allows many IDs per root (a documented contract test
  depends on it). Uniqueness belongs at the `register_canonical_project` layer, which is where the
  F10 guard now lives.

### Rejected: F9's Rust half

The finding also asked that `registry.rs` require `identity.ws_id` to equal the registered
workspace ID. Rejected with evidence:

- `src-tauri/src/worktree/mod.rs:317-338` resolves `ws_id: "ws-direct"` under workspace
  `"workspace-test"` as documented behavior. Worktrees created while a repo carried a previous
  workspace ID keep that ID in their `orca/<ws-id>/<slug>` branch, so requiring equality would
  make those worktrees permanently unresolvable.
- Safety does not depend on that equality. `worktree_path_for`
  (`src-tauri/src/worktree/manager.rs:251-258`) routes every `ws_id` through
  `format_branch_name` validation and then `validate_new_worktree_path`, and `resolve_worktree`
  (`registry.rs:119-142`) additionally enforces `canonical_allowed_path` plus an exact expected-branch
  match. `foreign_identity_ws_id_cannot_escape_the_registered_repo_root` proves `../../etc`, `..`,
  `a/b`, and `-x` are all rejected while a benign differing ID still resolves inside the root.

## Evidence

Every fix was driven RED first. Representative failing-then-passing pairs:

- F9: `expected 'fallback' to be 'alpha'` (trailing slash) and `expected 'beta' to be 'alpha'`
  (mislabeled branch) -> 5/5 pass, 86/86 with consumers.
- F10: `a canonical root must not gain a second workspace ID alias` -> `rorca_native_contract` 17/17.
- F11: `expected false to be true` -> `projectSwitchBack` 6/6.
- F3: session save asserted against the outgoing workspace -> `App.test.tsx` 56/56.
- F2 (browser leak): `expected "spy" to be called with arguments: [ 'browser-late' ]` -> 34/34.
- F7 (remote slug): `expected "spy" to not be called with arguments: [ ObjectContaining{...} ]`,
  re-confirmed by removing the guard and watching the same assertion fail -> `App.test.tsx` 57/57.

Gates: UI suite 79 files / 641 tests pass, `tsc --noEmit` clean, `bun run build` clean,
`cargo check` clean, `cargo test --lib` 234 pass, `ipc_hardening_contract` and
`rorca_native_contract` pass.

## Manual QA script

Each step has a single binary observable. Run against a fresh app launch.

1. Register project A (a Git repo) and project B (`superwiki-mail-otp`).
   Observable: both appear in the sidebar, each expanded showing only its own worktrees.
2. Click project A's **name** (not the chevron).
   Observable: A collapses. Click again: A expands. B's expansion state does not change.
3. Open a terminal in A, then select B, then select A again.
   Observable: A's terminal tab is still open with its scrollback; B shows its own tabs only.
4. In B (plain or Git), click **Add worktree**.
   Observable: the dialog opens and is interactive. The screen never dims with a frozen overlay.
5. Force an error (e.g. add a worktree with an invalid name).
   Observable: the error toast stays until dismissed with `×`; clicking its body copies the
   `code: message` plus details JSON and shows "Copied error to clipboard".
6. Switch A → B → A rapidly, five times.
   Observable: no row ever appears under the wrong project, and no project shows
   "No Git worktrees found" while its rows are visible elsewhere.
7. Quit and relaunch.
   Observable: the last active project restores with its own tabs; the other project's tabs
   restore when selected.
