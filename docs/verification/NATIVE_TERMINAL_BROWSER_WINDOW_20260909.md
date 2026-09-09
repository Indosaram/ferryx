# Native terminal / embedded browser window lookup

Date: 2026-09-09

## Outcome

Fixed the native terminal lookup failure reproduced when an embedded browser
child webview exists. The regression tests and debug library build pass.
Interactive desktop confirmation remains for the user; no running application
or daemon was restarted, and no release bundle was built or installed.

## Mechanism

`ipc/browser.rs` adds an embedded browser using `Window::add_child`.
Tauri 2.11.5 `Manager::get_webview_window("main")` only returns a value if
`Window::is_webview_window()` accepts the window. That check requires every
child webview label to match the window label, so an embedded browser makes the
lookup return `None` even though the main native window still exists.

This broke two independent terminal paths:

- `src-tauri/src/ipc/native_terminal.rs:564`: bounds updates returned
  `IpcError { code: InternalError, message: "Main Ferryx window is unavailable", details: None }`.
- `src-tauri/src/native_terminal/surface_host.rs:1330`: the daemon-output pump
  silently consumed the scheduled render instead of dispatching a frame.

## Changes

- Native terminal IPC and the output pump now use `get_window("main")`.
- Surface host and platform compositor interfaces accept `tauri::Window<R>`,
  including macOS, Windows, Linux, and the unsupported-platform implementation.
- Native focus/scroll monitors, terminal copy/paste event dispatch, clipboard
  image dispatch, and preference-triggered redraw use the same window lookup.
- Existing native handle behavior is preserved: Tauri's
  `WebviewWindow::ns_view()` delegates directly to `Window::ns_view()`.
- Two behavioral regressions use Tauri's real `MockRuntime` manager and
  `Window::add_child`, the production bounds IPC/output pump, and the existing
  injected frame target. They do not fake the window lookup.

## Verification

Failing-first command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib browser_child_is_open -- --nocapture
```

Before the production fix:

- `bounds_ipc_presents_when_browser_child_is_open` failed with the exact
  `Main Ferryx window is unavailable` IPC error.
- `output_presents_when_browser_child_is_open` failed because no render task
  reached the already-subscribed dispatch channel.
- Result: 0 passed, 2 failed; exit 101.

After the fix, the same command passed: 2 passed, 0 failed; exit 0.

Related tests:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal
```

Result: 160 passed, 0 failed, 615 filtered out; exit 0.

Debug build:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --lib
```

Result: finished the dev profile; exit 0. Existing unused-code/import/unsafe
warnings remain outside this patch. `git diff --check` passed.

## Limits and existing issues

- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` was blocked by
  existing PNG API mismatches in `src-tauri/src/ferryx_scope/design/mod.rs:61`
  and `src-tauri/tests/scoped_design.rs:30,57`: `output_buffer_size()` returns
  `Option<usize>`, but the callers use it as a `usize`. These files have no
  changes from this session and were left alone.
- LSP diagnostics were attempted, but the local LSP daemon was unreachable.
  Rust compiler validation is recorded above.
- MockRuntime does not prove visible WKWebView/WGPU interaction. Windows and
  Linux native runtime behavior was not executed on this macOS workstation.
- Other main-window lookups still use `get_webview_window` in app menu,
  worktree shortcut, and notification-activation code in `src-tauri/src/lib.rs`.
  Those adjacent non-terminal paths were not changed by this terminal fix.
- The patch is uncommitted in a shared working tree. Other sessions' SSH,
  frontend, site, and documentation changes were not modified.

## User desktop verification

Use the debug application launched through exactly `bun tauri dev`.

1. Keep at least two terminal panes open and open an in-app browser.
2. Switch back to each terminal, type a command, and confirm both input and
   output continue while the browser tab remains open.
3. Resize a terminal split and switch pane focus. Confirm no native-terminal
   bounds error appears and each pane redraws.
4. Check scrolling, Korean IME input, and copy/paste with the browser still open.
5. Close and reopen the browser, then repeat the terminal input check.

Do not restart the background daemon for these checks.
