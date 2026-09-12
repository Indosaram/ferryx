# Lead live Windows results

## Proven at the real runtime

- Existing QA app PID 21620, Session 1, launched through `bun tauri dev`.
  Baseline checkout `fa429aac`; only `TabBar.tsx` and `TerminalSplitView.tsx`
  subsequently copied from this task, with clean remote status checked first.
- The lead connected CDP through owned SSH tunnel `bash_5`, local port 19223
  forwarding only QA port 9223. The target URL is `http://127.0.0.1:5173/`.
- Session `2a08ef6d-0036-4143-9c90-a80dbf91d10a` is running in the QA checkout.
  `attachTerminal` returned base64 PTY history, decoded by the lead.
  Its sequence 16 history contains both the entered `echo FERRYX_WIN_START_OK`
  and separate `FERRYX_WIN_START_OK` output lines (twice).
- The lead resized the actual QA HWND through `position-qa.ps1` in
  `FerryxPosition_st01a0958a`, after subscribing to Tauri `onResized`.
  The event reports physical size 1232 by 903; CSS viewport is 986 by 723.
  Native pane CSS rectangle is x236, y32, width750.4000, height691.2000.
- After resizing, the lead sent `echo FERRYX_WIN_RESIZE_OK` and Enter using
  CDP trusted key events targeting the existing terminal textarea.
  The subsequent decoded PTY history contains a separate
  `FERRYX_WIN_RESIZE_OK` output line followed by the PowerShell prompt.
- Current DOM: native pane visible, presented, input enabled; no role-alert.
- Start and resize screenshots are saved as `artifacts/start-output.png` and
  `artifacts/resize-output.png`. The lead's model cannot receive images.
  The second visual reviewer also reported no image capability; these current
  screenshots are saved but visual obscuration/output is not independently
  verified. Earlier visual review applies only to earlier images.

The `terminal_output` event subscription returned no events in native mode;
the decoded daemon PTY history, not that empty array, proves command execution.
Earlier claims that the CDP keys failed were based on stale captures.

## Actual shell menu regression found in use

After syncing the two shell files, a real click on `+` produced a Windows
native popup. UIA, restricted to PID21620, returned:

- New Terminal.
- New Terminal Profile.
- PowerShell.
- Windows PowerShell.
- Command Prompt.
- WSL.
- New Browser Tab.
- Agent settings.

`artifacts/shell-profiles.png` records the expanded menu. UIA bounds placed
Command Prompt at x818, y226, width251, height26.

`click-qa-menu.ps1` first verifies that WindowFromPoint belongs to the exact
QA executable before sending the pointer click. The action log records:

```text
CLICKED label=New Terminal Profile pid=21620 hwnd=1246474 x=505 y=187
CLICKED label=Command Prompt pid=21620 hwnd=197858 x=843 y=239
```

Initial menu attempts closed without selection because visible helper consoles
stole focus. After hiding those consoles, the actual `new-terminal:cmd` event
arrived and a session spawned. That session was PowerShell: HMR had retained
the old zero-argument TabGroupView callback despite the corrected file on disk.
Page reload applied the corrected `(shell)` forwarding callback.

A later direct invocation of that actual callback with `cmd` created session
`4d7a10ef-f02b-4a35-af81-fdbbbff9dfe7`. Its decoded PTY history identifies
`C:\Windows\system32\cmd.exe`, Microsoft Windows version10.0.26200.9445, and
separate `FERRYX_WIN_SHELL_OK` output. This is real callback-to-spawn/input
integration evidence, not a completed uninterrupted `+` menu E2E scenario.
The latter remains open. No native-menu production change was justified.

## Startup status

The requested bounds error has not been reproduced in this baseline.
Observed early errors concern `set_focus` without attached surface and
scrollbar/attention `NoValue`; none is `cmd_native_terminal_set_bounds`.
The initial WebView2 failure was a separate QA profile problem, resolved by
the isolated profile launch. No Windows startup production fix is claimed.
