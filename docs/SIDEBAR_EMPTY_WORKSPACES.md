# Sidebar empty workspaces

Date: 2026-09-08

## Behavior

- A workspace with no tabs starts collapsed, including when its previous expanded state was saved.
- Expanding an empty workspace displays its primary worktree and registered worktrees, allowing the user to select them and open tabs.
- Closing the final tab collapses the workspace automatically. Opening a tab makes its rows available again.
- Browser tabs and tabs parked in another worktree count as open tabs.
- Grouped local/SSH projects remain visible when any member has tabs.
- Workspace selection shortcuts skip rows hidden by the empty-workspace rule.

## Implementation

`ui/src/state/sidebarWorkspaceState.ts` combines the live workspace state with parked snapshots.
The live state's workspace identity takes precedence over the selected project during a project switch.
`ui/src/App.tsx` supplies the resulting empty workspace IDs to the sidebar and its worktree shortcut selection.
`ui/src/components/Sidebar.tsx` applies the collapse transition and suppresses empty group children.
The worktree inventory itself is not deleted or changed.

## Verification

- RED: The three new sidebar regressions failed on the previous implementation; the existing 30 tests passed.
- GREEN: `node node_modules/vitest/vitest.mjs run src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx src/components/Sidebar.dnd.test.tsx src/components/Sidebar.remote.test.tsx --maxWorkers=1` from `ui/`: 50 tests passed.
- `node node_modules/vitest/vitest.mjs run src/App.test.tsx src/state/sidebarWorkspaceState.test.ts --maxWorkers=1` from `ui/`: 94 tests passed, one unrelated update-check test failed.
- Remaining failure: `checks for a signed update when the native app starts` expects the exported `checkForUpdate` mock to be called once, but observes zero calls. The unchanged `startUpdatePolling` calls the module-local implementation, which the partial mock does not replace. No assertion was removed or skipped.
- Four existing shortcut tests now seed actual browser-tab snapshots for their visible inactive workspaces; their selection assertions are preserved.
- `bun run --cwd ui build`: TypeScript and Vite succeeded, exit code 0.
- `git diff --check`: passed.
- LSP diagnostics could not run because the LSP daemon socket was unreachable.
- Desktop UI was not controlled. A separate headless harness failed due to duplicate React loading; a standalone bundle attempt also failed. No visual pass is claimed. The temporary server and browser were closed.

## Manual desktop check

In the debug app launched through `bun tauri dev`, close every tab in a workspace.
Confirm that its sidebar header collapses automatically. Expand it by clicking the chevron or project folder name and confirm that its primary worktree row appears.
Open a terminal or browser tab and confirm that expanding/collapsing the workspace preserves normal behavior.

Other sessions added unrelated remote-selection changes to `App.tsx` during verification; those changes were preserved and excluded from the sidebar commit.
