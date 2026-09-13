# P05 WIN-UI-13 focused browser targeting

Task st_01a09964, 2026-09-13. Local mounted UI repair; Windows runtime and lead combined build remain pending.

## Scope and mechanism

Changed App.tsx/App.test.tsx, BrowserPane.tsx/BrowserPane.test.tsx, BrowserToolbar.tsx/BrowserToolbar.test.tsx and browserTauri.ts (DOM type only). Preserved the existing PR3 optional-kind close discriminator and updater validator edits. No store/types, remote, native protocol, runtime, release, daemon, SSH, Git ref or commit actions.

Read root/UI/component/lib instructions and C002 in the parent goals.json plus repair-packets P05. Programming/debugging skill documents were not discoverable in the inspected local skill directories; no claim of reading unavailable skills.

App previously gated page shortcuts on top-level tab.kind and emitted action-only DOM events. Both mounted receivers executed those events regardless of browser identity. App now resolves the active leaf (first tree leaf when activeLeafId is null), uses authoritative contentsByLeafId, prefers nested browser.browserId with legacy flat browserId fallback, and only falls back to the top-level browser when leaf content is absent. This matches TerminalSplitView PaneRenderer at 894-899, browser rendering at 1226-1262 and state/layout.ts toPaneContent/defaultContentForTab (read-only). Explicit terminal/DAG content cannot inherit browser capability. Empty IDs do not enable dispatch. The memo dependency includes the ID so switching between browser leaves refreshes the target even while capability stays true.

The shared DOM type requires browserId and action. Both receivers compare DOM browserId exactly, rejecting missing/unrelated targets. Native onBrowserShortcutRequested payload matching remains unchanged. App's activeRemoteHost branch remains unchanged; visibility ownership and masking remain unchanged.

## Mounted evidence

Five App cases cover nested browser content in an optional-kind terminal tab, legacy flat browser content, top-level browser fallback, focused terminal content, and missing-content terminal fallback. Each mounts the real BrowserPane and BrowserToolbar twice alongside App (TerminalSplitView remains the existing shell mock). The native shortcut registration signal is subscribed before mounting and awaited with a cleared two-second failure deadline. Windows platform includes navigator and process platform. Actual registered Ctrl+L/R/F, Ctrl+[/], Alt+Left/Right keys reach the production shortcut hook. Assertions inspect selected address focus, reload callback, exact goBackBrowser/goForwardBrowser request arguments, and selected find input -> findBrowser(browserId, query, false); sibling focus/find/reload/API calls remain untouched. These are browserTauri API-boundary spies, not actual Tauri invokes or native browser execution.

Six receiver cases test missing, unrelated and matching DOM browser IDs for find UI and reload/back/forward. React act/render completes synchronous DOM listener installation before dispatch; no new polling or sleeps. Existing unrelated tests retain their existing behavior.

## Exact command and receipts

Every run used:

```sh
bun run --cwd ui test src/App.test.tsx src/components/BrowserPane.test.tsx src/components/BrowserToolbar.test.tsx
```

- `/tmp/st_01a09964-red.log`: exit 1, 7 failed / 144 passed (151), all three files failed. Four receiver failures are intended missing/unrelated targeting assertions. Three App failures are NOT accepted behavioral RED: the first fixture changed navigator.platform but overlooked process.platform=darwin.
- First attempted GREEN, before fixture correction: exit 1, 3 failed / 148 passed. Receiver cases green; same App fixture issue. That log path was subsequently overwritten by final GREEN; failure output remains in tool transcript.
- Corrected Windows fixture, then restored only the original App capability gate and action-only dispatch while retaining receiver fixes: `/tmp/st_01a09964-corrected-red.log`, exit 1, 3 failed / 148 passed. Nested, flat and top-level App cases fail selected address focus. Both component files pass. This is intended App producer RED with corrected Windows bindings.
- Restored targeted App dispatch, and fixed fixture TS2322 with an explicitly typed WorkspaceTab[] assigned to the pre-existing narrowly inferred mock layout. Assertions unchanged. `/tmp/st_01a09964-green.log`: exit 0, **3 files / 151 tests passed**, App 133, BrowserPane 6, BrowserToolbar 12. Duration 4.83s; start 15:19:02 local.

Diagnostics on all seven changed source/test files: no errors or warnings. App.tsx alone has existing TypeScript async-conversion hints at 1102 and 1109. App.test.tsx TS2322 is resolved. Scoped git diff --check exited 0. No whole UI build launched by this child.

## Additional ownership needed / limits

Producer/consumer census found one production DOM producer (App) and two production receivers. Existing action-only find fixtures outside this allocation require target-only adaptations, without changing assertions:

- BrowserPane.findRace.test.tsx:77,107,140.
- BrowserPane.parity.test.tsx:78.

Reported this need before source edits; these files remain untouched pending lead allocation. Their action-only fixtures will no longer open find under the required contract. Lead must update them before full-suite acceptance. No full-suite pass is claimed.

Native payload filtering, hidden-owner behavior and remote-host guards were preserved by source inspection, not newly exhaustively exercised by this added matrix. Focus-switch memo dependency is included but lacks a dedicated rerender regression. Real Windows browser address/find/history isolation, native guest bridge behavior and lead combined build remain pending; this receipt does not close C002 or overall runtime acceptance.

## Cleanup

New tests clean mounted DOM, restore browser spies/platform descriptors and clear their bounded readiness timeout in finally. Component tests use existing cleanup hooks. No temporary config, server, browser, daemon, PTY or native process was created. Logs remain at the listed /tmp paths. Changes are uncommitted in a shared moving tree; foreign work was not reverted.
