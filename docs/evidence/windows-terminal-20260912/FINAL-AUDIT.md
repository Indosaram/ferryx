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
  **verified on cold-loaded commit `7f7ecd8e`**. A hidden, ownership-checked
  pointer helper clicked the actual `+`, New Terminal Profile, and Command
  Prompt. The native event was `new-terminal:cmd`; the new PTY identified
  cmd.exe and emitted a separate `FERRYX_WIN_SHELL_OK` output after keyboard
  input. See `runtime/FRESH-RUN.md`, `fresh-menu-actions.log`, and
  `fresh-menu-cmd-receipt.json` under `runtime/artifacts/`.
- Native popup event bug: **not a confirmed product bug**. Visible helper
  consoles stole focus and dismissed the popup. Hiding those consoles
  produced a real `new-terminal:cmd` event. No menu backend edit was made.
- Startup bounds root cause and matching RED/GREEN: **missing**. The installed
  app's actual failure is now positively identified by screenshot and UIA:
  WebView PID 19396, parent installed GUI 17288. The fresh debug app does not
  have that banner. Different builds and profiles mean the comparison alone
  does not prove causation. No speculative Windows bounds fix was written.
- Startup input: actual daemon PTY history includes separate
  `FERRYX_WIN_START_OK` output. `runtime/artifacts/pty-receipts.json` contains
  the original base64 response and sequence range.
- Resize input: actual HWND resized, Tauri resize event received; native pane
  remained presented and no role-alert existed. Actual PTY history contains
  `FERRYX_WIN_RESIZE_OK`. See `runtime/lead-live-results.md`.
- Unobscured visual proof: **verified for the fresh debug run**. The image
  reviewer received all four fresh PNG attachments and confirmed all shell
  choices and three output sentinels, with no terminal/chrome obscuration.
  `runtime/fresh-visual-review.md` records the result and explicitly locates
  the installed app's bounds banner outside the QA window.
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

Fresh-run cleanup also passed: `runtime/FRESH-RUN.md` records exit0, all six
fresh tasks removed, QA processes/root/tunnel gone, and installed user
processes plus daemon53986 preserved.

The original Windows bounds error's raw IPC cause and exact source identity
remain unknown. Its installed executable is 35,909,632 bytes, version 0.1.0,
mtime 2026-08-28 20:02:28 KST, SHA-256
`51AB67EE9064D2B2AF7F52267FC8D887F3C0CA61ED816CC2D998B91A18B21BF7`.
Metadata is not exact commit proof.

Read-only installed-app probes found no CDP listener/DevToolsActivePort. Port
61948 belongs to its browser CLI, not CDP; a valid list request returned
`{"type":"list","sessions":[]}`. UIA preserved the bare bounds text but no
suffix. The historical UI discarded the structured cause from displayed
text. See `runtime/installed-build-diagnosis.md` and the installed-surface
and bounds-owner JSON artifacts. The user has been asked for the retained
Console bounds error without restarting the installed app.

The full menu and visual gaps are closed. The original bounds root fix and
same-seam RED/GREEN remain open and prohibit goal completion.

## Final local verification and commits

Monitor `mon_4GSTA513SVZKQ8VJ` / `bash_7` exited0 after the concurrent HEAD
updates: all four targeted suites passed (62 tests), then `bun run build`
passed TypeScript and Vite (3.73 seconds). TabBar LSP: no errors.

- `1313d387`: restore Windows new-terminal shell choices and forwarding tests.
- `40c2ae5c`: retain exited surface ownership through effect handoff.
- `8dee7072` (another session): already includes TerminalSplitView forwarding.

No push. These commits preserve verified increments, not a declaration that
the Windows startup objective is complete.
