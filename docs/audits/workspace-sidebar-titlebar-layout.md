# Workspace Sidebar Titlebar Layout

Date: 2026-08-22

## Final layout

- The sidebar titlebar, immediately to the right of the macOS traffic-light spacer, contains exactly these actions: **Hide sidebar**, **Add worktree**, and **Add project**.
- The sidebar begins directly with the project tree. The separate **Workspace**, **Search workspaces**, and **Projects** heading/count rows are removed.
- The project-level `Add worktree` affordance is removed so the global titlebar action is the sole entry point.
- The main workspace branch/header row is not rendered. When the sidebar is closed, the existing floating **Show sidebar** control remains available.

## Changed files

- `ui/src/components/Sidebar.tsx`
- `ui/src/App.tsx`
- `ui/src/components/TerminalSplitView.tsx`
- `ui/src/components/Sidebar.test.tsx`
- `ui/src/workspaceThemeContract.test.ts`

## Verification

- `npx vitest run src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx src/workspaceThemeContract.test.ts` — **21 tests passed**.
- `npm run build` was attempted but is blocked by pre-existing shared-worktree type errors in agent detection, browser settings, pane-drop types, and stale tests. None point at the sidebar layout files changed here.
