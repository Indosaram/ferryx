# Branch & Worktree Cleanup — 2026-08-29

Merged all outstanding branch work into `main`, then removed every extra worktree and local branch. `main` now has exactly one worktree and one branch.

## Merged

| Branch | Commits | Rationale |
|---|---|---|
| `updater-v11-e2e-evidence` (chain: v10 → v11 → e2e) | 3 | v10 content was already in `main` via different commits (verified: `recoverProjectBootstrap` + recovery docs present), but **v11's `isRepresentedSingleProject` catalog-recovery fix was absent**. Merged the chain tip to capture v11 + E2E doc. |
| `Indosaram/perf-bottleneck-audit` | 7 | Fully unmerged: daemon handshake timeout, render coordinator pending-until-frame, ordered backlog replay, session persistence gating, dag pane exports, duplicate `ui/dist` packaging drop, perf audit doc. |

Merge commits: `e379139` (updater chain), `e3a8ec8` (perf audit).

## Conflict resolutions

- `ui/src/App.test.tsx` — kept main's multi-line `layout` fixtures; restored v11's "restores multi-project native session catalog" test.
- `ui/src/App.tsx` — kept main's `dagStore` import; adopted v11's `isRepresentedSingleProject` guard.
- `ui/src/lib/types.ts` — kept main's tolerant persisted-pane schema (`dad050f`, newer than the branch's stricter variant); removed the duplicate branch-side `createDagPaneContent`.
- `src-tauri/src/native_terminal/surface_host.rs` — adopted the branch's `dispatch_scheduled_render` helper (render-coordinator perf fix) and upgraded its body to main's selection-aware `session_render_snapshot()` so both the perf fix and selection rendering survive.

## Deleted (no unique content lost)

- Worktrees (8): `ferryx-clean-updater-overlay.Jqc066` (tmp), `orca-lite-notarization-workflow-32070`, `orca-lite-release-lockfix-12834`, `orca-lite-release-trigger-push`, `orca-lite-release-verify-13768`, `orca-lite-two-action-updater-push`, `orca-lite-updater-cta-push`, `~/orca/workspaces/orca-lite/perf-bottleneck-audit`.
  - Dirty worktree content verified disposable before forced removal: release version stamps (`0.1.0 → 2026.826.x`), regenerated `gen/schemas`, `bun.lock` dev-dep drift, `.notarization/`/`.release-assets*` build outputs, dead `.disposable-ferryx.*` runtime logs.
  - perf worktree contained the vendored `ghostty` submodule, so it was removed via `rm -rf` + `git worktree prune` (git refuses submodule worktrees).
- Branches (5): `Indosaram/perf-bottleneck-audit`, `updater-v10-hotfix`, `updater-v11-hotfix`, `updater-v11-e2e-evidence` (merged; also kept reachable via tags `v2026.08.26.10/.11`), `backup/pre-settings-squash` (pre-squash backup; all content superseded by main's shadcn settings rebuild chain `6ef65ba…52f9667`).

## Verification

- `cargo check` — clean (pre-existing dead-code warnings only).
- `cargo test --test native_terminal_surface_host_contract` — 17/17 passed.
- `bunx tsc --noEmit` — no errors.
- `bunx vitest run src/lib/terminalEvents.bus.test.ts` — 8/8 passed.

## Leftovers (intentionally untouched)

- `main` is **62 commits ahead of `origin/main`, unpushed** (push was not requested).
- Remote branches `origin/updater-v10-hotfix`, `origin/updater-v11-hotfix`, `origin/updater-v11-e2e-evidence` still exist (remote deletion not requested).
- The Orca app still lists the `perf-bottleneck-audit` worktree card; its directory is gone — remove the card from the Orca UI.
- Two lingering zsh shells had their cwd inside deleted worktree directories.
- The in-progress DAG pane-badge WIP (16 modified files + untracked `DagNodeCard.test.tsx`) is intact; its staged/unstaged split was flattened to unstaged by the stash cycle used during merging.
