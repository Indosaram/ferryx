# Terminal file links

## Implemented

- macOS Command-click and Windows/Linux Control-click use the existing native pane pointer flow.
- POSIX, Windows drive and UNC tokens, Unicode paths and quoted spaces are recognized, with line/column suffixes.
- Terminal settings now select System default, Visual Studio Code, Cursor or Zed.
- File opening passes the backend session ID and editor preference. Errors are displayed through the existing toast boundary.
- The daemon DescribeSession request now queries the shell process cwd in a blocking worker. Relative links do not fall back to stale UI cwd when that lookup fails.
- Windows cwd reads are isolated in `ipc/windows_process_cwd.rs`; macOS/Linux reuse existing process queries.
- Modifier hover displays a token underline and pointer cursor. Pointer leave clears it and stale replies are discarded.
- SSH workspace and paired-host sessions are refused for local file opening.

## Evidence

- New parser/error tests first failed for five requested regressions, then passed.
- Frontend related suite: 4 files, 206 tests passed; TypeScript/Vite build exited 0.
- Backend final file-link suite: 17 tests passed; `cargo check` exited 0 after the Windows launcher-discovery adjustment.
- Real isolated daemon/PTY integration: subscribe to output, execute `cd changed-directory`, await an encoded completion marker, then resolve a relative file through the production command. Passed.
- Windows cwd module compiled as metadata for x86_64-pc-windows-msvc through `windows-cwd-check.rs`. This is a compile check, not Windows runtime evidence.
- Isolated WebView rendered the real TerminalSection. Selecting Cursor persisted `cursor`; the 327px-wide container had equal clientWidth and scrollWidth (no horizontal overflow).
- Settings captures: `evidence/file-links-20260915/settings-desktop.png` and `settings-narrow.png`.

## Remaining acceptance checks

This is not a three-platform GUI acceptance claim. The actual native compositor hover and OS editor launch have not been exercised on the user's desktop. The current assistant cannot inspect screenshot pixels, and independent visual workers failed due to provider rate limits. The captures and DOM measurements do not prove visual correctness.

Windows cwd FFI has a target compile check but still requires native execution, including WOW64 and permission-denied cases. Linux has not been built or run in this session. Existing running daemons need the updated implementation before the live cwd behavior is available; none were restarted or killed.

Manual acceptance on each OS, using `bun tauri dev` only:

1. Print an existing absolute path and modifier-click it; verify the OS default app opens that file.
2. Change the shell directory, print a relative path, click it, and confirm it resolves against the new directory.
3. Select each installed editor, print `path:12:3`, and verify the actual editor location.
4. Check quoted spaces, Korean names, Windows drive and UNC paths on Windows.
5. Hold Command/Control over a path, then plain text; verify underline/cursor geometry and no selection regression.
6. Click a missing file and a remote session path; verify visible failure and no local file launch.

All changes remain uncommitted. No release build, desktop input injection, or daemon termination was performed.
