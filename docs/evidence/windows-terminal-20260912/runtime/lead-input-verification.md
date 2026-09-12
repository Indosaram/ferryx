# Direct review of initial input attempt

The lead read `artifacts/echo-result-222135.json` in full.

- Timestamp: `2026-09-12T22:21:40.9124378+09:00`.
- App PID: `21620`; main HWND: `15140440`; title: `Ferryx`.
- Requested command: `echo FERRYX_WIN_START_OK`.
- Recorded mode: `postmessage-wmchar hwnd=15140440`.
- `focusVerified`: **false**.
- Native child: HWND `164496696`, class `FerryxNativeTerm`, visible flag true.
- Native child screen rectangle: left 320, top 139, right 1364, bottom 958
  (1044 by 819 pixels).
- WebView screen rectangle: left 84, top 107, right 1364, bottom 957.

The nonzero native geometry is real recorded evidence, but it does not prove
unobscured rendering or keyboard delivery. The script's WM_CHAR fallback was
previously rejected; this attempt must remain classified as failed/unverified
input, regardless of an `ECHO_DONE` marker.

The independent screenshot reviewer reported PowerShell startup text in
`screen-221752-after-new-terminal.png`. The lead's model could not receive
images from `read`, so that observation is attributed to the visual reviewer,
not claimed as the lead's own visual inspection. No required echo output is
established by that frame.

The runtime worker was directed to use the QA WebView's actual frontend
keyboard path with an output-event subscription, not a direct terminal IPC
write or another WM_CHAR fallback. Startup input, shell selection, resize
input, unobscured screenshots, and the original native-bounds RED/GREEN all
remain open.
