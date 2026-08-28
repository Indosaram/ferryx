# Windows and WSL GUI Terminal/Browser Verification — 2026-08-28

## Windows browser CLI transport

Windows browser CLI support is implemented, not disabled.

- Endpoint: loopback TCP bound to `127.0.0.1:0`
- Pointer: `%LOCALAPPDATA%\Ferryx\runtime\browser.port`
- Protocol: the same newline-delimited JSON request/response protocol used by
  the Unix-domain socket implementation
- Missing, malformed, zero, out-of-range, and stale pointers return typed
  unavailable errors without panicking
- Existing regular pointer files are replaced on application restart; symlink
  and directory replacements are rejected

Real two-process Windows verification:

```text
PORT_READY=True
PORT=57650
CLI_EXIT=0
CLI_STDOUT=[]
GUI_ALIVE=True
GUI_RESPONDING=True
```

`[]` was the correct browser inventory because no browser tab was open.

## WSL build and GUI environment

- Runtime: WSL2 Ubuntu with WSLg
- Display: X11 `:0`; Wayland socket also present
- GTK: `3.24.52`
- WebKitGTK: `2.52.3`
- Bun: upgraded to `1.4.0`
- Zig: installed `0.16.0`
- Ghostty source: `6a508fd5e34c7e222c052a6d00bb3891ff3feace`
- Linux release binary:
  `/home/sook/ferryx-wsl-build/src-tauri/target/release/ferryx`

The current working tree has unrelated TypeScript errors in `ui/src/App.tsx`
and `ui/src/App.test.tsx`. To test the Linux native binary without changing
those files, the verified UI bundle from the successful Windows build was used
as `ui/dist`, and Tauri's `beforeBuildCommand` was disabled for this WSL build.
The Rust/Tauri Linux release build completed successfully.

## WSL GUI verification

Ferryx was launched in WSLg with X11 selected for GTK. The app created a real
1280x850 desktop window:

```text
0x600004 "Ferryx": ("ferryx" "Ferryx") 1280x850
```

The GUI, daemon, browser socket, terminal session, WebKit network process, and
WebKit web process remained alive.

## WSL terminal verification

The GUI automatically created a PTY session. The daemon protocol was used to:

1. list the GUI's live terminal session;
2. write `printf 'FERRYX_WSL_TERMINAL_OK_2\n'` into the PTY;
3. attach from sequence zero and decode the replay history.

The replay contained the exact marker:

```text
FERRYX_WSL_TERMINAL_OK_2
TERMINAL_MARKER_OK=1
```

Ctrl+T was also delivered to the real WSLg window. The daemon session count
increased from one to two, confirming the GUI shortcut created a second real
terminal tab.

## WSL browser verification

Ctrl+Shift+B was delivered to the real WSLg Ferryx window. The browser CLI then
returned a live browser session. The address bar was focused through the GUI,
`https://example.com` was entered, and WebKit loaded the page.

Browser inventory:

```json
[
  {
    "url": "https://example.com/",
    "title": "Example Domain",
    "visible": true
  }
]
```

Browser automation snapshot:

```json
{
  "generation": 2,
  "url": "https://example.com/",
  "title": "Example Domain",
  "elements": [
    {
      "reference": "e1",
      "role": "a",
      "name": "Learn more",
      "tagName": "a"
    }
  ]
}
```

The captured WSLg window also visibly showed the Example Domain page rendered
inside Ferryx. The browser list and snapshot therefore verified both the GUI
surface and the browser automation transport.
