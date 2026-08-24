# Empty Workspace Default View (Zero Tabs Allowed)

> REIMPLEMENTED 2026-08-23 ~00:40 after the first implementation was silently
> reverted by harness cleanup while its worker ran. Verified on-disk before
> reporting this time.

## Behavior delivered

- Closing ALL tabs now leaves an EMPTY workspace with 0 tabs instead of auto-spawning a replacement terminal. This was the root cause of "마지막 남은 탭이 안 닫혀": with the old code live, closing the last tab spawned a fresh terminal instantly, so it looked like the close did nothing.
- When the active worktree has 0 tabs, `App.tsx` renders `EmptyWorkspaceView`:
  - Centered "No open tabs" message with hint text.
  - "New Terminal" (primary) and "New Browser Tab" (outline) buttons wired to existing handlers (`handleAddTerminalTab` / `handleAddBrowserTab`).
- This REVERSES the earlier never-empty invariant (sole-tab replacement terminal), per user direction on 2026-08-23.

## Production changes

- `ui/src/state/workspaceStore.ts` `closeTab`:
  - Browser branch: sole-tab special case removed — always `closeBrowser(browserId)` then `CLOSE_TAB`, no worktree-resolution early return, no replacement spawn.
  - Terminal branch: sole-tab path keeps full session disposal (`closeBackendSessionAndWait` + `terminalHostManager.destroy`) but dispatches plain `CLOSE_TAB`; the no-worktree early-return guard was removed so the last tab always closes.
- CLOSE_TAB reducer tolerates transitioning to zero tabs; `activeTabId` ends as null.
- Focus-refresh resurrection guard: `ensureTabForWorktree` takes `{ allowCreate }`; passive refresh paths (`workspaceRuntime.ts` window focus and onWorktreeChanged) pass `{ allowCreate: false }`, so switching away/back to the window no longer respawns a terminal into an intentionally empty workspace. Explicit navigation still creates tabs.

## Revert incident (root cause of the reported bug)

The first implementation round passed all gates but its disk changes were rolled back to HEAD by harness child-cleanup racing (the store worker was cancelled as a "rogue child" and the view worker's files vanished). The running app therefore kept the old replacement-spawn behavior while reports claimed success. Lesson applied: independent on-disk verification (grep/sed proof) is now mandatory before believing any worker completion report.

## Failing-first evidence (RED)

```text
bun run --cwd ui test -- workspaceStore.browserLifecycle workspaceStore.test --reporter=dot
3 failed | 23 passed:
- workspaceStore.test.tsx "closes the last tab to an empty workspace after lifecycle-confirmed writer release"
  expected tabs length 0, received 1 (replacement still spawned)
- workspaceStore.test.tsx "closes the native browser and leaves an empty workspace when closing the sole browser tab"
  expected tabs length 0, received 1
- workspaceStore.browserLifecycle.test.tsx "closes the native child webview before removing its React tab"
  expected tabs length 0, received 1
```

## Final verification (GREEN)

```text
bun run --cwd ui test -- workspaceStore.browserLifecycle workspaceStore.test EmptyWorkspaceView --reporter=dot
PASS: 3 files, 29 tests

bun run --cwd ui test
567 passed / 568 total; the single failure is unrelated:
  src/lib/terminalHostManager.lru.test.ts "suspends inactive output ... replays upon reactivation"
  expects terminalReset, but another session's uncommitted edit deleted two
  terminal.reset() calls in ui/src/lib/terminalHostManager.ts (diff verified).

bun run --cwd ui build
PASS: tsc && vite build
```

## New/changed spec tests

- `workspaceStore.test.tsx`: last-terminal close → 0 tabs, `activeTabId` null, `spawnTerminal` not called again; sole-browser close → native `closeBrowser` then 0 tabs, 0 sessions.
- `workspaceStore.browserLifecycle.test.tsx`: native child webview disposed, 0 tabs, no terminal spawned.
- `EmptyWorkspaceView.test.tsx`: message + both buttons render; each button invokes its handler once.
- `workspaceRuntime.test.tsx`: PASS (focus refresh does not spawn into an empty workspace).

## Manual desktop check (user-run)

1. Launch Ferryx GUI.
2. Close every tab (terminal and browser).
3. Expected: window stays open showing the centered "No open tabs" empty view; app does NOT exit and does NOT spawn a terminal.
4. Click "New Terminal" — a terminal tab appears. Repeat with "New Browser Tab".
