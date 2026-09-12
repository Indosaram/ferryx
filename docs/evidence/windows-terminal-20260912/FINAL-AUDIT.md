# Windows terminal repair: completion audit

Status: **PARTIAL; goal not achieved.** This report supersedes earlier draft
claims in the producer/runtime reports.

## Requested deliverables and current evidence

- Restore four Windows shell choices and exact callback forwarding:
  implemented in TabBar and TerminalSplitView. RED/GREEN tests in
  `shell-implementation.md`; lead independently ran the same cases.
  TerminalSplitView forwarding was subsequently included by another session
  in commit `8dee7072`; it is no longer an uncommitted change here.
- Real `+` menu with all choices: UIA read actual PowerShell, Windows
  PowerShell, Command Prompt and WSL entries; `runtime/artifacts/shell-profiles.png`
  and `menu-clicks.log` record the interaction.
- Real `+ -> Command Prompt -> echo` in one uninterrupted scenario:
  **not fully verified**. One selection emitted `new-terminal:cmd`, but an
  HMR-retained old forwarding callback spawned PowerShell. Reloading the page
  applied the corrected callback. A later direct invocation of that actual
  callback with `cmd` spawned cmd.exe and executed `FERRYX_WIN_SHELL_OK`;
  this is a real spawn/input integration check, not full menu E2E.
- Native popup event bug: **not a confirmed product bug**. Visible helper
  consoles stole focus and dismissed the popup. Hiding those consoles
  produced a real `new-terminal:cmd` event. No menu backend edit was made.
- Startup bounds RED/GREEN: **missing**. The committed Windows baseline
  `fa429aac` opened and rendered a native terminal after QA WebView profile
  isolation. The reported bounds error was not reproduced. No speculative
  Windows bounds fix was written.
- Startup input: actual daemon PTY history includes separate
  `FERRYX_WIN_START_OK` output. `runtime/artifacts/pty-receipts.json` contains
  the original base64 response and sequence range.
- Resize input: actual HWND resized, Tauri resize event received; native pane
  remained presented and no role-alert existed. Actual PTY history contains
  `FERRYX_WIN_RESIZE_OK`. See `runtime/lead-live-results.md`.
- Unobscured visual proof: **incomplete**. Earlier image reviewer saw
  PowerShell startup text and partial Start-menu obscuration. The lead moved
  the QA window and captured current start/resize/cmd images, but both current
  models received image-omission errors. Saved images alone are not a visual
  pass. `runtime/final-images-review.md` records this limitation.
- Retained-presentation regression: three original failing cases fixed,
  lifecycle30 + presentation9 independently passed without assertion changes.
  This is macOS retained-frame behavior, not the Windows startup root fix.
- Diagnostics/build: prior direct run 71 passed, two known exitAttach callback
  assertion failures; frontend build exit0, LSP no errors. Historical backend
  browser-child bounds tests2 passed. Final current-HEAD check recorded below.
- Workflow: two disjoint implementation lanes followed by combined check.
  Runtime workers suffered provider quota/stream failures; lead took over
  actual QA, decoded raw PTY records, and rejected overstated evidence.
- No release, publish, push, or user-daemon restart. Foreign changes preserved.

## Cleanup receipt

`cleanup-owned.ps1` checked exact root PID3972 command line before terminating
its QA-only descendants. Monitor `mon_E28FVBMC2FZ4A6HK` exited0:

- Eight exact `st01a0958a` scheduled tasks removed.
- `OWNED_PROCESSES_ZERO`.
- QA Ghostty junction removed without deleting the shared target.
- `QA_ROOT_REMOVED`.
- Installed app/daemon PIDs17288,1756,20196 remained with unchanged creation
  times. Subsequent network query: user daemon port53986 owned by20196;
  no QA CDP9223 listener.
- CDP SSH tunnel `bash_5` closed. Temporary remote cleanup script removed.
- Evidence was copied before deleting the QA root; no user settings snapshot
  was restored over live user data.

## Remaining required verification

The original Windows bounds error's exact suffix/build is unknown; the user
question timed out. Reproduction and a confirmed root fix remain open.
The full real-menu cmd scenario and current unobscured screenshot review
also remain open. These gaps prohibit `update_goal complete`.

## Final local verification and commits

Monitor `mon_4GSTA513SVZKQ8VJ` / `bash_7` exited0 after the concurrent HEAD
updates: all four targeted suites passed (62 tests), then `bun run build`
passed TypeScript and Vite (3.73 seconds). TabBar LSP: no errors.

- `1313d387`: restore Windows new-terminal shell choices and forwarding tests.
- `40c2ae5c`: retain exited surface ownership through effect handoff.
- `8dee7072` (another session): already includes TerminalSplitView forwarding.

No push. These commits preserve verified increments, not a declaration that
the Windows startup objective is complete.
