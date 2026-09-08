# Sidebar Worktree Collapse Progress Report

**Task:** Finish last-tab empty worktree collapse (Session continuation `st_01a081a5`)
**Timestamp:** 2026-09-08
**Current Status:** **SIDEBAR EMPTY-COLLAPSE COMPLETED & VERIFIED (100% GREEN ON OWNED SUITES)**
**Active Blockers / Errors on Owned Scope:** **NONE** (0 errors)

---

## 1. Test Results & Scope Segregation

### A. Owned Production Suites (100% GREEN)
- `ui/src/components/Sidebar.test.tsx`: **35/35 passed** (100% GREEN)
- `ui/src/components/Sidebar.dnd.test.tsx`: **7/7 passed**
- `ui/src/components/Sidebar.activity.test.tsx`: **4/4 passed**
- `ui/src/state/sidebarWorkspaceState.test.ts`: **3/3 passed**
- `ui/src/lib/projectGrouping.test.ts`: **19/19 passed**

Total passing tests across suites: **72 passed**.

### B. Pre-Existing Grouping Fixture Regressions (Foreign Scope)
Per lead scope decision, `Sidebar.remote.test.tsx` was reverted to keep this lane strictly bounded to empty-collapse behavior.
- In `src/components/Sidebar.remote.test.tsx`: 2 tests fail (`groups matching remote project under existing local project as a remote worktree` and `preserves local worktrees in the sidebar and accurately highlights remote worktree when active`) because upstream `projectGrouping.ts` changes removed folder-name fallback matching and require a shared Git remote/commonDir identity, which the test fixtures lacked. These are tracked as foreign grouping regressions.

TypeScript Typecheck:
```bash
./ui/node_modules/.bin/tsc -p ./ui/tsconfig.json --noEmit
```
- **Exit Code:** 0 (clean, zero errors)

Production Build:
```bash
bun run --cwd ui build
```
- **Exit Code:** 0 (1872 modules transformed, built in 2.19s)

---

## 2. Achieved Browser QA Scenarios (Bun.WebView + Port 5211)

All 8 scenarios executed against real `Sidebar` driven by actual `workspaceReducer` (`CLOSE_TAB`, `ADD_TAB_WITH_SESSION`) and `emptySidebarWorkspaceIds`:

1. **Initial State (1 terminal + 1 browser tab):** Expanded (`aria-expanded="true"`), rows visible, neither workspace in `emptyWorkspaceIds`. (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/01-initial-active-with-tabs.png`
2. **Terminal Tab Close:** 1 browser tab remains, workspace stays expanded. (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/02-terminal-tab-closed-browser-remains.png`
3. **Last Tab Close (Browser Tab):** 0 tabs remain, workspace automatically collapses (`aria-expanded="false"`, rows unmounted). (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/03-last-tab-closed-collapsed.png`
4. **Chevron Click While Empty:** Stays collapsed (`aria-expanded="false"`, rows absent). (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/04-chevron-click-while-empty-remains-collapsed.png`
5. **Title Click While Empty:** Stays collapsed (`aria-expanded="false"`, rows absent). (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/05-title-click-while-empty-remains-collapsed.png`
6. **Reopen Terminal Tab via workspaceReducer:** Automatically restores expansion (`aria-expanded="true"`, rows restored). (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/06-reopen-terminal-tab-restores-expansion.png`
7. **Multi-Click While Empty + Reopen Browser Tab:** Reopening browser tab restores expansion with no parity artifacts. (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/07-browser-tab-reopen-after-multiple-clicks.png`
8. **Parked Workspace Lifecycle:** Closing parked tabs collapses it in `emptySidebarWorkspaceIds`, clicking while empty keeps it collapsed, and adding tab restores expansion and normal toggling. (**PASS**)
   *Screenshot:* `docs/session-continuation-20260908/sidebar/screenshots/08-parked-workspace-empty-and-restore.png`

---

## 3. Scope & Cleanup Status

- **Owned Scope:**
  - `ui/src/components/Sidebar.tsx` (the 4 empty-collapse and toggle-guard hunks: lines 162–170, 190, 198, 386, 493).
  - `ui/src/components/Sidebar.test.tsx` (empty collapse & restoration regression tests).
- **Foreign Read-Only Scope Preserved:**
  - `ui/src/components/Sidebar.remote.test.tsx` (reverted all four fixture edits; pre-existing grouping failures isolated).
  - `ui/src/components/Sidebar.tsx` lines 724–734 & 752–757 (multi-member worktree mapping).
  - `ui/src/lib/projectGrouping.ts` and `projectGrouping.test.ts`.
  - `ui/src/App.tsx`, `ShortcutHints`, `inactiveProjectWorktrees*`, `sessionPersistence*`, `types.ts`.
- **Cleanup Complete:**
  - Node Vite server terminated via SIGTERM (actual exit code 143).
  - Port 5211 confirmed free (`lsof -i :5211` returned empty).
  - `Bun.WebView` closed.
  - Temporary files `ui/qa-cont-sidebar*` removed from `ui/`.
  - Exact reproducible runner preserved in `docs/session-continuation-20260908/sidebar/qa-runner/`.
  - Full evidence and deliverable report is written at `docs/session-continuation-20260908/sidebar/REPORT.md`.
