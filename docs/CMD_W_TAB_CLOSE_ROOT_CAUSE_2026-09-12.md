# Cmd+W tab-vs-pane close behavior and missing agent close confirmation — root cause analysis

Date: 2026-09-12 (updated 2026-09-13 after runtime store inspection)
Scope: investigation only. No code changed. Evidence: source reads + tests on HEAD (`8dee7072`), shipped bundle disassembly, and the user's live persisted store.

## User-reported issues

1. Pressing Cmd+W closes the whole tab instead of the pane.
2. Closing a tab with a running agent exits immediately with no confirmation.

## Issue 1 — Cmd+W routing

### Actual dispatch chain (verified in source)

- macOS desktop: AppKit local monitor intercepts unshifted Cmd+W, emits `menu_close_tab`, and swallows the
  event (`src-tauri/src/lib.rs:467-480`, returns `ptr::null_mut()`). The webview `keydown` never fires on macOS.
  The File-menu "Close Tab" item (`lib.rs:60`, accelerator `CmdOrCtrl+W`) is therefore never keyboard-triggered;
  it emits the same event on click.
- All four entry paths converge on ONE handler, `handleCloseActiveSurface`:
  - native menu/monitor event: `ui/src/App.tsx:2098-2101` (`onCloseTabMenu`)
  - webview keydown registry: `ui/src/App.tsx:2345` (`"tab.close": handleCloseActiveSurface`)
  - embedded-browser guest bridge forwarded `tab-close` action: `ui/src/App.tsx:2281`
  - (`ui/src/lib/tauri.ts:634-637` maps the native event to the handler)
- `handleCloseActiveSurface` (`ui/src/App.tsx:1749-1766`) closes the FOCUSED PANE only when
  `activeTab.kind === "terminal"` AND `layoutsByTabId[activeTabId].root.type === "split"`;
  otherwise it calls `handleCloseTab` (whole tab).
- `closePane` on a split dispatches `CLOSE_PANE`; the reducer removes one leaf and keeps the tab
  (`ui/src/state/workspaceStore.ts:1937-1973`, `ui/src/state/layout.ts`). Only a leaf-root pane close
  falls back to `CLOSE_TAB`.
- Regression tests on HEAD pass (run 2026-09-12): "routes the native Cmd+W menu accelerator to close the
  focused pane in a split terminal tab" and "routes the web Cmd+W shortcut to close the focused pane in a
  split terminal tab" — `2 passed` (`bun run --cwd ui test src/App.test.tsx -t 'focused pane in a split'`).
- The pane-first logic ships in every recent build: introduced in `5d883f05` (2026-08-28); the installed
  release app `2026.908.1` (commit `6a96a31`, replaced 2026-09-10, per
  `docs/releases/MACOS_APP_REPLACEMENT_20260910.md`) contains it.

### Conclusion (2026-09-13, runtime-verified — supersedes the tab-group theory)

The earlier tab-group-split hypothesis is REFUTED by the user's real persisted store
(`~/Library/Application Support/com.ferryx.app/session_state.json`, live app, snapshots 09:24/09:44 KST):

- Every workspace persists a single `group-default` tab group — no tab-group splits exist anywhere.
- The orca-lite workspace's two tabs are REAL 3-pane splits (`tab.terminal.paneTree` root type `split`,
  3 leaves, live `agent="omo"` sessions on every leaf). Hydration writes the split into runtime
  `layoutsByTabId[tabId].root` (`sessionPersistence.ts:552-628`), so renderer and Cmd+W handler read the
  same source; on those tabs Cmd+W does close one pane.
- The user's incident tab was a single-pane (root=leaf) agent tab. The store holds several such tabs
  right now (e.g. omon-gateway "main (3)", maho-workspace, orca-lite-release-verify "main (2)").
- The user then stated the requirement explicitly: Cmd+W must close the focused PANE **regardless of
  root type**; a leaf-root tab must not instantly vanish the tab + agent.

Therefore issue 1 is a **design-behavior gap, not a routing bug**: `handleCloseActiveSurface` only
routes to pane-close when `root.type === "split"`; a single-pane terminal tab falls through to
`handleCloseTab`, which tears down the agent PTY immediately (`closeTab` → `services.closeTerminal`),
with no confirmation. Both symptoms the user reported are the same instant-kill behavior seen from a
leaf-root tab. Nothing needs to be "found" further in the dispatch chain; the chain behaves exactly as
coded, and the code's fallback contradicts the product expectation.

Notes from adjacent evidence: the 2026-09-09 shortcut diagnostics doc
(`docs/SHORTCUT_DIAGNOSTICS_2026-09-09.md`) recorded a Tauri-internal
`TypeError: listeners[eventId].handlerId` during unlisten in the installed app, and dev runs use
React StrictMode double-mount. Simulation shows duplicate dispatch alone cannot close a 2-pane split
tab (the second `CLOSE_PANE` is a no-op), so this is not the primary cause.

## Issue 2 — no confirmation when closing a tab with an active agent

- **Agent-aware close confirmation does not exist anywhere in the codebase.** No
  `hasActiveAgent`/`isAgentActive` concept exists (rg: zero matches in `ui/src`, `src-tauri/src`).
- The only close confirmation is a manual global toggle: General settings "Confirm before closing a tab"
  (`ui/src/components/settings/GeneralSection.tsx:265-279`), stored at `ferryx.settings.general.confirmCloseTab`,
  DEFAULT `false` (`ui/src/lib/generalSettings.ts:14`). When enabled it confirms ALL tab closes, not
  agent-active ones. It is honored in `handleCloseTab` (`App.tsx:1755-1763`) only.
- `closeTab` (`ui/src/state/workspaceStore.ts:998-1037`) checks only `pinned`; it immediately tears down
  sessions via `services.closeTerminal(backendSessionId)` (`workspaceStore.ts:2836-2843`) — a running agent
  PTY is killed without any prompt. Pane close has the same behavior.
- There is no window-close guard either (no `CloseRequested` handler in `src-tauri/src`), but closing the
  window does NOT kill sessions — the headless daemon owns PTY master fds. Only TAB/PANE close ends the agent.
- Therefore issue 2 is a missing feature, not a regression. The agent-running state to gate it already
  exists: `activityBySessionId` / `summarizeActivities` (`ui/src/lib/activity.ts`) — the same source that
  drives TabBar activity dots and attention notifications.

## Required behavior change (user-confirmed, pending approval)

1. Cmd+W (and every tab.close path) targets the focused pane's SESSION first, regardless of
   `root.type`. When the focused pane is the tab's last pane, the tab closes too — but only after
   the agent-aware confirmation below. No silent instant-kill path remains for terminal surfaces.
2. Closing any pane/tab whose sessions include an active agent (`activityBySessionId` /
   `summarizeActivities`) must show a confirmation dialog; the existing global
   `confirmCloseTab` toggle stays as an additional always-confirm option.
3. Browser/DAG panes keep their own semantics (browser leaf close = pane close, unchanged).

## Verification performed

- Source reads of every Cmd+W entry path and the close reducers (files/lines above).
- Shipped bundle check: `/Applications/Ferryx.app/Contents/Resources/ui/dist/assets/App-DN3V4Wgz.js`
  contains the same pane-first-when-split / tab-close-when-leaf wiring (minified symbols
  `nr`=handler, `$e`=closePane, `wa`=handleCloseTab) — the installed 2026.908.1 app matches HEAD behavior.
- Runtime store inspection (2026-09-13 09:24/09:44 KST snapshots): no tab-group splits anywhere;
  orca-lite active tabs are real 3-pane splits with 6 live `omo` sessions; several 1-pane agent tabs
  exist across workspaces (the incident shape).
- Hydration re-check: persisted `tab.terminal.paneTree` is restored into runtime
  `layoutsByTabId[tabId].root` (single source of truth) — no renderer/handler divergence.
- `bun run --cwd ui test src/App.test.tsx -t 'focused pane in a split'` → 2 passed, 91 skipped.
- Commit dating: `5d883f05` 08-28 (pane-first), `a7351196` 08-22 (native menu), `44f1578d` 09-02
  (Cmd+W monitor), `57abc83d` 09-10 (shortcut restore), installed app = 09-10 build.
- No edits made; working tree left untouched (foreign uncommitted files observed and preserved).

## Open questions for the user (needed to pick the fix)

None remaining. Root cause and required behavior are confirmed; implementation awaits explicit
user go-ahead (scope-approval ≠ start approval).
