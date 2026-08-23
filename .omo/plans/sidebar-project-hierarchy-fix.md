# Sidebar project hierarchy correction

## Scope

Fix the Ferryx sidebar so the titlebar keeps only **Add project**; each project row has an accessible worktree-create action at the right edge; a clean startup resolves the repository's canonical folder-derived project ID instead of displaying `default`; and worktree rows no longer show Refresh worktree status.

## Constraints

- Keep existing user changes outside this feature untouched.
- Do not create duplicate `default` and derived workspace aliases for one repository.
- Retain project-specific creation routing for active and non-active projects.
- Use failing-first tests for each behavior change.
- Do not commit; the user did not request one.

## Execution topology

1. **Architecture lane** (read-only): decide the smallest typed IPC/bootstrap contract that exposes one canonical initial project without duplicate aliases.
2. **Sidebar lane**: own `Sidebar`/`WorktreeList` plus their tests. Remove global worktree creation and Refresh status; add the project-row action and prove its target.
3. **Backend lane**: own Rust registry/project IPC/startup registration and Rust tests. Implement the canonical initial-project contract and remove the alias.
4. **App lane**: after the contract is settled, own `App`, Tauri client types, and App tests. Use the canonical startup project and ensure non-active-project creation refreshes/opens from the owner context.
5. **Verification lane**: run focused tests, UI typecheck/build, Rust tests, and report captured evidence.

## Acceptance criteria

1. `Sidebar` rendering contains `Add project` in the titlebar but no `Add worktree`; each project row renders `Add worktree to <workspaceId>` and clicking it calls `onCreateWorktree` with exactly that project.
2. The `Refresh worktree status` action is absent while selection/deletion behavior remains covered.
3. With empty persisted project storage, the initial project resolved by the real typed IPC equals the canonical repository folder ID (`orca-lite` in this checkout), and no registered `default` alias is emitted for the same root.
4. Creating a worktree from a non-active project's row lists branches, creates it for that project, then switches/refreshes the owning project before opening the created worktree.
5. Focused Vitest suites, `bun run build`, Rust tests, and visual QA are green; all transient QA resources are removed.
