# Browser shortcut dispatch repair

## Root cause

The embedded browser adds a differently labelled child WebView to the main
window. In the pinned Tauri 2.11.5 implementation, `get_webview_window("main")`
then returns `None`: `Window::is_webview_window` requires all child labels to
match the window label. A hidden browser still participates in this check.

The macOS key monitor used this lookup and consumed matching keys even when
no event was emitted. The macOS menu callback used the same invalid lookup.

## Change

- `src-tauri/src/shortcut_dispatch.rs` owns event dispatch through
  `get_window("main")`, which supports multiple child WebViews.
- All nine affected key-monitor branches (eleven event names) use this shared
  path, including workspace and tab selection, tab cycling, new/close tab,
  split requests, command palette, sidebar, and settings.
- The monitor consumes a key only after successful emission. Missing windows
  and emission errors are logged and return the original native event.
- New/close-tab menu actions use the same dispatch path. The close-window
  menu action also resolves `Window` instead of `WebviewWindow`.
- Native copy/paste latching and non-macOS production routing are unchanged.

## Regression evidence

The regression uses Tauri MockRuntime with the real manager, a main WebView,
and `Window::add_child`. It subscribes before dispatch, checks payload delivery
and duplicate absence, and repeats for a visible and hidden browser child.
It is not a mocked lookup or a source-text assertion.

- RED: `cargo test --manifest-path src-tauri/Cargo.toml --lib shortcut_dispatch::tests -- --nocapture`
  failed with exit 101 at `menu_select_worktree, hidden=false` using the old
  lookup. The absent-window case passed.
- GREEN: the same two tests passed with `get_window`.
- Related Rust tab-navigation/split and Ctrl-digit predicate tests passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib` passed, exit 0.
  Existing warnings in unrelated browser, file-drop, renderer, daemon, and
  worktree code were not suppressed or changed.
- Frontend: `bun run --cwd ui test src/lib/shortcuts.test.tsx src/lib/shortcutDiagnostics.test.tsx src/lib/browserTauri.test.ts`
  passed 90 tests in three files, exit 0.
- LSP reported no errors in the two changed Rust files; `git diff --check`
  passed.

MockRuntime verifies the actual Tauri manager/event path, not AppKit keyboard
delivery or a live WebKit UI. No desktop input was automated, no release was
built, and no live daemon was stopped.

## Manual desktop verification

Launch the debug application with exactly `bun tauri dev`. With at least two
workspaces and two tabs available:

1. Open an embedded browser and click its page body.
2. Verify Cmd+1/2 workspace selection, Ctrl+1/2 tab selection, Ctrl+Tab,
   Ctrl+Shift+Tab, and Cmd+Shift+[/] tab cycling.
3. Verify Cmd+T, Cmd+K, Cmd+B, and Cmd+, plus the File menu new-tab action.
4. Return to a terminal while keeping the browser tab open and repeat switching.
5. Check that one key press performs one action. Test tab closing only on a
   disposable tab, then close the browser and repeat switching.

These live desktop checks require user confirmation; automated event tests
must not be represented as live GUI verification.
