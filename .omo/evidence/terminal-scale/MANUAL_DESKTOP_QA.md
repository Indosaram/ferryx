# Ferryx Terminal Scale Manual Desktop QA

## Why this is manual

The desktop Ferryx surface is a Tauri application. The workspace owner has explicitly prohibited agent-driven desktop input automation, so this document is the required real-surface acceptance procedure rather than a claimed automated desktop pass.

## Preconditions

1. Start Ferryx from the repository root:

   ```bash
   bun run dev
   ```

2. Have five registered workspaces available. In each workspace, open at least 20 terminal tabs or panes with a continuing command so every terminal has an already-live PTY.

## Scenario A: Fast switch among known terminals

1. In workspace one, leave a continuing command running in terminal A.
2. Visit terminal B, then terminal C, then return to terminal A using the normal tab controls or shortcuts.

**Pass:** every tab activates without a new shell prompt or a terminal-spawn wait, and terminal A continues its prior process/output.

## Scenario B: Warm-cache eviction and reattach

1. With the warm cache's default two inactive renderers, visit terminal A, then terminal B, terminal C, and terminal D so A is older than the warm inactive budget.
2. Return to terminal A.

**Pass:** terminal A remounts without spawning a new backend PTY, shows the bounded replay tail in the original order, and its continuing process remains alive. No duplicated prompt or duplicate output stream appears.

## Scenario C: Five-workspace navigation

1. Move through all five already-loaded workspaces.
2. In each, select a non-first known terminal tab.
3. Return to workspace one and its previously selected terminal.

**Pass:** every known workspace/tab selection is immediate, does not show a loading/spawn transition, and retains each terminal's existing output and activity state.

## Scenario D: Reactivation viewport alignment

This is the focused acceptance check for the terminal viewport one-row offset fix. It does not require the Scenario C five-workspace setup.

1. In any workspace, start a terminal with enough output to create scrollback, then leave its viewport at the live prompt (the bottom of the output).
2. Select another terminal tab and return to the first tab several times. If possible, resize the Ferryx window or its terminal pane once between returns.
3. Confirm that the live prompt and newest line remain fully visible. The terminal must not be left exactly one text row above the newest output, and it must not show an unexplained one-row viewport gap.
4. Scroll the terminal upward deliberately, switch away and back, then resize once.

**Pass:** a terminal that was following its live output remains aligned to the newest line after reactivation/fit; deliberately scrolled-up history is not snapped to the bottom by a resize.

**Deferred:** Scenario C may remain unperformed when five workspaces with twenty live terminals each are unavailable. Report it as `DEFERRED` rather than `FAIL`.

## Scenario E: Cmd+W tab close and confirmation preference

This check requires only two tabs and can be run independently of the terminal-scale scenarios.

1. Fully quit the Ferryx **GUI** process and launch the rebuilt Ferryx GUI. Frontend hot reload cannot apply this native macOS event-monitor change. (The daemon process is separate; do not treat it as the GUI.)
2. Enable the normal Korean/CJK input source if it is normally active. This explicitly validates the former failing key-composition condition.
3. Open a **Browser Tab** as the only tab. Click inside the loaded browser page so its native browser webview owns keyboard focus.
4. Press Cmd+W.
5. Confirm that the Ferryx window remains open, the native Browser Tab is gone, and one replacement terminal tab is visible. This distinguishes a real tab close from an app/window exit or an empty workspace.
6. Open a terminal tab and a second Browser Tab. Click inside the Browser Tab page, then press Cmd+W again.
7. Confirm that only the Browser Tab closes and the terminal tab becomes active.
8. Open Settings → General. Turn on **Confirm before closing a tab**.
9. Open another Browser Tab, click inside its page, press Cmd+W, then select **Cancel**. Confirm the Browser Tab remains open.
10. Press Cmd+W again and select **Close tab**. Confirm that only the Browser Tab closes. Return to Settings → General and turn the preference back off if immediate closing is preferred.
11. Optionally verify Cmd+Shift+W retains its separate window-close behavior.

**Pass:** with focus inside either a terminal or Browser Tab, Cmd+W never closes the Ferryx window; it closes the active Ferryx tab, and the optional confirmation policy is honored for native keyboard/menu close requests and tab-bar closes. Cancel never releases the tab. Cmd+Shift+W is the window-close shortcut.

## Evidence to return

Capture one screenshot or short screen recording that includes a selected workspace/tab and continuing terminal output, then report PASS/FAIL for scenarios A, B, D, and E. Report Scenario C as PASS, FAIL, or DEFERRED. No agent-created process, server, or browser artifact requires cleanup for this user-run procedure.
