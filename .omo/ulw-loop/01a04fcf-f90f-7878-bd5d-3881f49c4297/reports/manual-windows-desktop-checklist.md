# Ferryx Windows Desktop and WSLg Manual Verification

Automated SSH verification proves build, daemon protocol, ConPTY bytes, terminal working directory, structured errors, and process/file cleanup. It cannot prove that pixels are visible on the user's physical Windows desktop. Complete these checks in the debug app only.

## Launch Contract

1. In the isolated verified checkout, launch Ferryx with exactly:
   ```powershell
   bun tauri dev
   ```
2. Do not launch `target\debug\ferryx.exe` directly for desktop QA.
3. Do not build or launch a release bundle.

## Native Terminal Presentation

1. Open two native terminal panes in a split.
2. Confirm both terminal surfaces are visibly unobscured and render text, cursor, and background correctly.
3. Open terminal search with the native pane active.
4. Confirm the search input and match controls are visible above the terminal rather than hidden behind the native child window.
5. Search for text visible in scrollback, use next and previous, and confirm the viewport visibly moves to each match.

Pass only if the rendered search UI and target text are visibly present. Window handles or process existence are not evidence.

## Focus, Keyboard, Clipboard, and IME

1. Click pane A once and type `pane-a`; confirm only pane A receives it.
2. Click pane B once and type `pane-b`; confirm only pane B receives it.
3. Switch panes and immediately type `한글`; confirm there is no isolated first jamo or lost first syllable.
4. With Korean 2-set active, copy plain text in Windows, press physical Command/Control+V as appropriate for the Windows shortcut, and confirm the full text appears once in the active pane.
5. Copy an image or file object and paste; confirm Ferryx follows the image/file attachment path rather than inserting corrupt text.
6. Place another application over Ferryx, click that application, and confirm Ferryx does not steal focus or route typing to a terminal.

## Mouse and Split Geometry

1. Run a mouse-aware terminal application such as `vim` or another available TUI.
2. Confirm click selection, drag selection, and wheel scrolling work in each pane.
3. Click directly on both sides of a split divider and confirm input routes to exactly one pane.
4. Rapidly switch tabs, then click once in the newly visible pane; confirm input never lands in the outgoing session.
5. Rapidly open and close split panes; confirm no stale black child surfaces remain.

## Mixed-DPI Displays

If two monitors with different scaling are available:

1. Move Ferryx from a 100% display to a higher-scale display.
2. Click near every terminal edge and split divider.
3. Confirm the cursor, hit target, and rendered child surface remain aligned.
4. Repeat after resizing the window and reopening terminal search.

## WSL and WSLg

1. Open a WSL terminal pane.
2. Run:
   ```sh
   printf 'PWD=%s\n' "$PWD"
   printf 'SESSION=%s\n' "$FERRYX_SESSION_ID"
   ```
3. Confirm the PTY output is visible and the working directory is the expected mounted Windows path or selected Linux path.
4. Launch one installed WSLg GUI application from that pane.
5. Confirm its actual window is visibly unobscured on the Windows desktop, accepts focus, and can be moved in front of and behind Ferryx.

Process existence, Wayland/X11 sockets, `IsWindow`, and successful WGPU presentation calls do not satisfy step 5.

## Result Receipt

Record:

- Windows version and display scaling.
- `bun tauri dev` start timestamp.
- PASS/FAIL for each section above.
- Exact visible symptom and reproduction sequence for every failure.
- Whether WSLg was available; if unavailable, mark only the WSLg window check as NOT RUN, not PASS.
