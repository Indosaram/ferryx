# Fresh committed-source verification

Purpose: remove the previous HMR stale-callback confounder. No new product
change or Windows bounds fix is claimed. Use committed `7f7ecd8e`, containing
the shell and retained-presentation commits, from first page load.

The previous QA root was removed. This run owns
`C:\Users\sook\ferryx-qa-fresh-0912`, task `FerryxFresh0912`, and CDP9233.
Runtime, sessions, APPDATA and WebView profile are isolated under that root.
The existing `ferryx-winbuild\orca-lite` checkout remains read-only; only its
Ghostty source is referenced through a QA-owned junction.

Pinned sequence:

1. `git bundle create /tmp/ferryx-fresh-0912.bundle main`; clone and checkout
   exact `7f7ecd8e` in the new QA root.
2. Install root/UI dependencies; verify Ghostty gitlink SHA.
3. Register hidden InteractiveToken task running `launch-fresh.cmd`.
4. Launch only `bun tauri dev`; monitor build log and CDP readiness.
5. Cold-loaded `+` menu, actual Command Prompt click, CDP key input
   `echo FERRYX_WIN_SHELL_OK`, real PTY receipt and screenshot.
6. Startup/resize screenshot/geometry follow the previous verified command
   patterns, with actual QA HWND/PID discovered afresh, never reused.
7. Preserve artifacts, then clean this exact owned process/task/root.

Read-only original-install probe: `C:\tmp\ferryx-switch-debug.jsonl` contains
1806 records from August30 and September12, but only34 bounds-start records
and no bounds error records. Installed executable version resource is0.1.0,
insufficient to identify the original failing build. This widens the evidence
search without pretending the missing original error has been recovered.

## Current execution handles

- Bundle transfer completed; setup monitor `mon_5NMA10A19RJ6PHF9` / `bash_8`
  exited0. Checkout output confirms `7f7ecd8`; root2 and UI365 packages
  installed; `FRESH_DEV_STARTED`.
- Original readiness monitor `bash_9` failed before observing the build because
  a JavaScript string corrupted Windows path backslashes. This is a probe
  failure, not a build failure.
- The shared-log-readiness monitor `mon_26F6P4B0W63PPBRW` / `bash_11`
  completed with `FRESH_CDP_READY`. Earlier `bash_10` failed on file sharing,
  not on the app. No duplicate desktop instance was started.
- This run has now been cleaned; the separate receipt is below.

## Shell E2E observed, 2026-09-13 00:19 KST

The fresh build reached actual CDP readiness. QA GUI PID24236, daemon18012,
main HWND3213230; WebView22540 is a direct GUI child with the isolated profile.
The initial pane opened without a bounds banner.

`select-fresh.ps1` ran as a hidden InteractiveToken task. It verified both
the GUI executable and the WebView child ownership, then physically clicked
the DOM-derived `+` coordinate, the real New Terminal Profile menu, and
Command Prompt. No direct callback invocation or synthetic menu event was
used. The Tauri listener received `new-terminal:cmd`; task exit0.

New session `6a20835f-7276-44a2-9c1c-a6e4c8de0f1b` returned actual PTY history
identifying `C:\Windows\system32\cmd.exe`. Trusted CDP key input of
`echo FERRYX_WIN_SHELL_OK` produced a separate `FERRYX_WIN_SHELL_OK` output
line and a new cmd prompt. Evidence:

- `artifacts/fresh-menu-cmd-receipt.json`.
- `artifacts/fresh-menu-actions.log`.
- `artifacts/fresh-shell-menu.png`.
- `artifacts/fresh-cmd-output.png`.

Current native pane: visible, presented, input enabled; no role-alert.
This closes the previous HMR-confounded menu-to-cmd execution gap.

## Startup, resize, and original-error attribution

Initial PowerShell session `b7fb8d07-2ccf-40ee-9e0e-d4f30454507b` produced
separate `FERRYX_WIN_START_OK` and `FERRYX_WIN_RESIZE_OK` output lines.
The latter followed an ownership-checked actual HWND resize and a subscribed
Tauri resize event (1482x1078). Native pane geometry was 950.4x831.2 at
(236,32). Raw PTY records and HWND data are in the fresh artifacts.

`fresh-visual-review.md` confirms all four PNGs were received and inspected:
all menu choices and three echo outputs were visible without chrome overlap.
It also found the original bounds banner outside the QA window. Read-only
UIA tied that text to installed WebView19396, parent17288, not this debug run.
The installed app's cause remains unknown; see `installed-build-diagnosis.md`.

## Fresh cleanup receipt

`cleanup-fresh.ps1` verified launcher PID4824's exact command before terminating
only its tree. Monitor `mon_V778220TZ7PF0JGA` / `bash_16` completed with exit0:

- Six exact fresh scheduled tasks removed.
- `FRESH_OWNED_PROCESSES_ZERO`.
- Installed PIDs17288,1756,20196 checked against their original creation times
  and executable paths; all preserved.
- Ghostty junction removed without deleting its shared target.
- `FRESH_QA_ROOT_REMOVED`; bundle/setup/launcher remote files removed.
- Tunnel `bash_12` closed and remote cleanup script removed.
- Final probe: QA root does not exist; no CDP9233 listener; user daemon53986
  remains owned by20196.

The first cleanup command failed on shell path escaping before executing the
script; using a slash-safe path fixed the invocation. No user settings or
installed app state was restored or overwritten.
