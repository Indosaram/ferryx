# Split browser navigation verification

Date: 2026-09-09

## Finding

The user reported a white page when opening `https://google.com` in the development
app and confirmed that the browser was a pane split with a terminal.

The debug browser socket initially listed the visible child session at
`about:blank`. `TerminalSplitView` rendered the correct browser ID, but its
navigation and reload callbacks supplied only the owning tab ID. The workspace
store rejected terminal tabs before invoking browser IPC. When the owner was a
different browser tab, the same callbacks targeted that parent browser instead
of the requested pane.

This is separate from the native-terminal main-window lookup fix documented in
`NATIVE_TERMINAL_BROWSER_WINDOW_20260909.md`.

## Change

- `ui/src/components/TerminalSplitView.tsx` passes the pane browser ID through
  navigation and reload callbacks.
- `ui/src/App.tsx` forwards that ID to the store.
- `ui/src/state/workspaceStore.ts` resolves the requested browser against its
  owning tab and pane contents. It updates matching nested browser state and,
  only when the IDs match, top-level browser metadata. The same update applies
  to parked worktree layouts. Calls without an explicit browser ID retain
  standalone-tab behavior.
- Other panes and the existing SSH changes in the shared store are preserved.

## Automated evidence

Before the production change, the two regression files produced four expected
failures and one standalone-browser pass:

- The split component omitted the child browser ID.
- A terminal parent discarded navigation.
- A browser parent navigated the wrong browser session.
- A terminal parent discarded reload.

After the change, this final command passed all 89 tests in nine files:

```sh
bun run --cwd ui test \
  src/state/workspaceStore.browserNavigation.test.tsx \
  src/state/workspaceStore.browserLifecycle.test.tsx \
  src/state/workspaceStore.browserPaneDrop.runtime.test.tsx \
  src/state/workspaceStore.browserPaneLeafMove.runtime.test.tsx \
  src/state/workspaceStore.test.tsx \
  src/components/TerminalSplitView.mixedPaneLeaf.runtime.test.tsx \
  src/components/TerminalSplitView.browserPaneDrop.runtime.test.tsx \
  src/components/TerminalSplitView.browserDragVisibility.runtime.test.tsx \
  src/components/TerminalSplitView.test.tsx
```

`bun run --cwd ui build` passed TypeScript checking and Vite bundling, exit code
0. An initial build caught one missed callback type and a legacy browser-state
type mismatch; both were corrected before the final run.

`git diff --check` passed. LSP diagnostics were unavailable because the LSP daemon
socket was unreachable; TypeScript compilation provided the type check.

## Development runtime and remaining manual check

The debug app was launched using `bun tauri dev`. A read-only request to Vite at
`http://127.0.0.1:5173/src/components/TerminalSplitView.tsx` confirmed that the
served code includes the browser ID in both callbacks.

The latest read-only browser session list was empty. This check does not prove
that Google rendered successfully. No desktop input automation was performed.
The earlier read-only snapshot also returned `BROWSER_AUTOMATION_FAILED` with
`webview evaluation was cancelled`; that observation is not a diagnosed cause
of this navigation bug.

Manual verification requested from the user:

1. In the development app, open a browser pane beside a terminal.
2. Enter `https://google.com` and press Enter. Confirm that Google appears.
3. Reload that browser and confirm that it reloads.
4. Confirm that the neighboring terminal still accepts input.

The implementation and automated checks are complete; actual desktop page
rendering remains unverified until this manual check.

The changes are uncommitted in the shared working tree and can be affected by
other sessions. No project commit was created.
