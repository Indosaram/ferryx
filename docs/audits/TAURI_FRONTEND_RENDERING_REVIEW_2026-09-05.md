# Tauri frontend rendering review - 2026-09-05

## Scope and status

Review and repair of the desktop React frontend: startup and workspace restoration, state-derived row/tab rendering, native terminal visibility and geometry, browser child-webview visibility, search result rendering, settings caches, dialogs, CSS and DAG surfaces. Automated verification is complete and independent code review returned APPROVE with no blocking findings. Native desktop pixel verification requires the user's manual check.

The repository was shared with unrelated documentation deletions and DAG/native selection work at the start. Those changes were preserved. During this review another session committed DAG work as `ea27a71`; it is not part of this review implementation. Following the user's commit request, the rendering fixes were committed separately as `9c07825` (workspace activity/publication), `83a3296` (shared preferences/startup background), `2aef606` (masking/search response ordering), and `076e429` (display density). Only the density-listener hunk of `NativeTerminalPane.tsx` was included; concurrent input/IME changes remain uncommitted.

## Confirmed defects

- Browser panes ignored global dialog/search masking. `BrowserPane` now combines owner visibility with the existing native-surface visibility hook. Command palette, settings and other semantic dialogs cause the child browser to hide; dismissal restores only owner-visible panes.
- Browser find responses could arrive out of order and overwrite a newer query. Requests now carry a generation, invalidated by a newer query, clear, close, browser identity change or unmount.
- Terminal search responses and errors could overwrite newer query results. The current generation now owns the result; session changes clear stale results and search the retained query in the new session.
- Every terminal-settings consumer forced another native-preferences fetch. Mounts now share the existing cache; explicit refresh still forces a fetch.
- A new terminal-settings consumer overwrote the shared terminal background with the fallback color before cached preferences arrived. Resolved preferences seed subsequent consumers, and unresolved startup consumers preserve the persisted background. Superseded shared requests adopt the current promise, so an older response in a sibling pane cannot overwrite the refreshed background.
- The native visibility hook ignored `data-native-terminal-yield="off"` on a surface or ancestor. It now evaluates each matching dialog/search surface. Existing opt-out tests had passed before MutationObserver delivery; they now await real observer delivery through React `act`. This is a contract repair: the current production opt-out marker is on a menu outside the dialog/search selector.
- Worktree activity stayed on the old row when a session rebound with a new working directory. The selector now includes sessions, parked layouts, unread tabs and workspace identity in its dependencies. Related agent and notification selectors also include the parked-layout and worktree inputs they read.
- App renders with unchanged terminal inventory republished the same remote payload. Serialized payload equality now suppresses duplicate IPC while a genuine focus or inventory change still publishes. Settings callbacks now retain their stable identity across unrelated renders.
- Native geometry relied only on ResizeObserver, which misses density-only changes. A rearmed resolution media query and window resize listener now update scale at fixed CSS bounds without reattaching the native surface; both listeners are cleaned up.

## Baseline evidence

- `bun run --cwd ui test`: baseline 143 files, 1 failed and 142 passed; 1,466 tests, 13 failed and 1,453 passed. All failures were in `App.test.tsx`. Twelve restore tests supplied no registered project while the non-native test runtime intentionally skips placeholder registration. The fixtures now seed a real project root, preserving all restore/recovery assertions; no production registration bypass was introduced. The thirteenth failure was the unstable settings callback fixed above.
- `workspaceRestoreDuplicateTab.test.tsx` confirms the existing loading guard already prevents duplicate tabs. Deferred promises and the restore callback replace fixed sleeps; no speculative restore rewrite was made.
- SettingsDialog tests now reset the shared preference cache between tests, matching the new shared-cache contract rather than relying on an implicit forced refresh.
- `cd ui && bun x tsc --noEmit`: exit 0 before implementation.
- Language-server diagnostics and symbols could not run: the LSP daemon was unreachable at its configured socket. TypeScript checks are the type-verification fallback.

## Final automated verification

- `CI=1 bun run --cwd ui test --reporter=dot`: 146 test files passed; 1,498 tests passed; exit 0. This includes the concurrent pane-to-tab-row work after its author fixed it. No tests were skipped or assertions weakened to obtain this pass.
- `bun run --cwd ui build`: TypeScript and Vite succeeded; 1,860 modules transformed; Vite built in 3.48 seconds; exit 0.
- `git diff --check`: exit 0.
- LSP remained unavailable; the successful TypeScript build supplies static type verification. No Rust source was modified or native release build launched for this task.
- These results cover the tested tree at the end of implementation. Later concurrent changes to NativeTerminalPane input handling, shortcuts and RemoteTerminal are not part of this task and are not covered by this earlier test/build result.

## Independent review status

- The first reviewer terminated without a verdict on HTTP 429, QUOTA_EXHAUSTED. This is not an approval or a code rejection.
- The bounded replacement reviewer returned APPROVE with no criterion-linked regressions or blocking findings. Its scope excludes concurrent input/IME and pane-drag edits and retains the native desktop verification boundary below.
- Full reviewer verdict: [TAURI_FRONTEND_RENDERING_GATE_REVIEW_2026-09-05.md](TAURI_FRONTEND_RENDERING_GATE_REVIEW_2026-09-05.md). The reviewer did not rerun tests/build; the results above are the lead's captured execution evidence.
- Non-blocking notes: native pixels remain manual, later foreign changes are not covered by the earlier build, and QA observations are recorded here rather than in a standalone matrix. The review's BrowserPane coverage note overlooked the separate untracked test files `ui/src/components/BrowserPane.findRace.test.tsx` and `ui/src/components/BrowserPane.masking.test.tsx`, which were included in the passing full test run. The preserved review text is unchanged.

## Browser runtime evidence

The lead used an isolated headless system WebKit (`Bun.WebView`, 1100 x 750), serving the actual source components and Tailwind/CSS through an in-memory Vite server. Only the Tauri IPC boundary was faked and recorded; React components, effects, MutationObserver, event dispatch and browser layout were real. No user desktop window was driven.

- Before the browser masking fix, mounting the real `BrowserPane` emitted `cmd_browser_set_visible(true)`. Opening the real `CommandPalette` emitted no visibility command.
- After the fix, opening that palette emitted `cmd_browser_set_visible(false)`. Closing it emitted `true`. Dismissing a dialog while the owner was hidden emitted zero show commands and left the final visibility false.
- Browser viewport: x=0, y=39, width=1100, height=711. Palette: x=214, y=92, width=672, height=363. Document width equaled viewport width, with no horizontal overflow.
- Terminal search: issued `err` then `error`; resolving the newer request with 2 matches and the older request with 5 left the visible result at `1/2`. Clearing the input cleared the result display.
- Settings: two mounted consumers issued one fetch and preserved startup background `#123456`. After a forced refresh and out-of-order old response, both consumers and the CSS background remained `#bbbbbb`.
- Monitor density: simulated resolution changes in WebKit emitted scale 2 then 1 with unchanged bounds `{x:0,y:12,width:800,height:500}` and no attach/detach IPC. This verifies the frontend event bridge, not physical monitor pixels.
- Worktree state: rendering the real store hook and rebinding a session moved the displayed working count from main=1/feature=0 to main=0/feature=1.
- Machine-readable runtime evidence: `docs/evidence/rendering-review-2026-09-05/runtime-checks.json`.
- Screenshots: `docs/evidence/rendering-review-2026-09-05/browser-command-palette.png` and `docs/evidence/rendering-review-2026-09-05/terminal-search.png`. The blank browser content and terminal background are expected because native pixels are not simulated; these captures prove DOM layout, not native compositor output.

## Reviewed and not changed

- Browser show/hide IPC is already serialized by `enqueueBrowserLifecycle`. A proposed additional request counter would not improve native side-effect ordering and was rejected.
- Position-only browser bounds drift was not reproduced at a concrete caller; speculative continuous frame polling was not added.
- Native attach/detach serialization and same-turn reparenting were reviewed and retained.
- The macOS-only transparency CSS avoids applying the native terminal transparency block to Windows/Linux.
- Existing DAG rendering work was reviewed as current source, but belongs to another session.
- A concurrent pane-to-tab-row fix added `TerminalSplitView.paneToTabRow.runtime.test.tsx` during verification. Its initial failure and edits to TabBar, TerminalSplitView, tabDragTypes and the development harness belong to that session and were not changed here.
- Terminal search previous/next currently changes the match index only. The backend selection API supports all/word/line/none, not a precise match range. Full match highlighting/navigation is an existing capability gap beyond the frontend response-rendering correction; no unsupported IPC was invented.

## Native desktop verification boundary

Headless WebKit plus recorded IPC cannot prove WGPU, WKWebView child-view stacking, WebView2 or Linux native surface pixels. The user has prohibited desktop automation. Actual desktop verification must use the debug app launched with exactly `bun tauri dev`.

Manual checks: open Settings and Command Palette over browser/terminal splits; close the modal and verify the correct content reappears; switch tabs and projects; resize split panes; add a terminal after preferences have loaded; type and clear searches rapidly; move the app between monitors with different scale factors. Confirm that no stale result, background flash, obscured control, blurred terminal or black region appears.
