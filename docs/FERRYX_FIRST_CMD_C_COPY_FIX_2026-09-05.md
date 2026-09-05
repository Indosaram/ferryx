# First Cmd+C native terminal copy

Date: 2026-09-05

## Report and scope

Dragging to select terminal text and pressing Cmd+C sometimes requires repeated
keypresses before the clipboard changes. The fix is limited to native terminal
copy delivery. Cmd+C remains copy-only; Ctrl+C behavior, focus recovery, and drag
selection geometry are outside this change.

## Investigation

- The macOS AppKit key monitor consumes Cmd+C when a native terminal is focused
  and emits `native_terminal_copy_or_interrupt` to the main webview.
- `NativeTerminalPane.tsx` invokes `cmd_native_terminal_copy_selection`, then
  calls `navigator.clipboard.writeText` after the asynchronous IPC response.
- The old browser write discarded every rejection. The Tauri event is not a
  trusted DOM keyboard event, making this path dependent on WebKit clipboard
  activation and document-focus rules even though the selection lives in Rust.
- The backend already reads the macOS pasteboard on the main thread for paste.
  Native copy can use the same platform boundary without a new dependency.
- WebKit's [Async Clipboard API documentation](https://webkit.org/blog/10855/async-clipboard-api/)
  states that `write` and `writeText` outside a user gesture immediately reject.

The browser rejection mechanism is inferred from the current code and WebKit's
clipboard requirements. A rejection from the user's running app has not been
captured. Focus synchronization immediately after mouse release is a separate
possible contributor and has not been established as the cause of this report.

## Correction

`src-tauri/src/ipc/native_terminal.rs` writes non-empty selected text to the macOS
pasteboard inside the existing copy command and acknowledges completion before
returning its existing string result. `ui/src/components/NativeTerminalPane.tsx`
skips the browser write on macOS. Windows and Linux retain browser copy, with
write failures reported instead of silently discarded. Empty selection neither
clears the clipboard nor sends terminal input.

## Verification record

- Before the fix, the new macOS first-event regression failed because the denied
  browser clipboard spy was called once with `selected text on first copy`.
  The new non-macOS error regression also failed because the rejection was not
  reported. Both pass with the correction.
- `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`:
  **139 passed**, including the native copy event, Korean Cmd+C, non-macOS copy,
  and empty-selection cases. The same file also passed in the full suite.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal`:
  final reviewed implementation **13 passed, 0 failed** (2.53 seconds). The real `NativeTerminal` consumes
  press/motion/release, extracts the selected text, and the production writer
  writes it to a unique private macOS pasteboard. Read-back matches exactly;
  an empty write preserves the previous text. Test pasteboard content is cleared
  after read-back. Production copy dispatch uses `app.run_on_main_thread` directly.
- `rustfmt --check --edition 2021 src-tauri/src/ipc/native_terminal.rs` and scoped
  `git diff --check`: **exit 0** after review corrections.
- Rust test compilation reported seven unrelated existing warnings in
  `native_terminal/input.rs`, `native_terminal/renderer/font_manager.rs`,
  `ipc/notifications.rs`, `terminal/session.rs`, and `worktree/manager.rs`.
  No warning points to the changed copy implementation.
- `cargo check --manifest-path src-tauri/Cargo.toml`: **exit 0**, debug profile
  completed in 2 minutes 18 seconds. It also reported seven unrelated warnings,
  including unused `notification/macos_submission.rs::decode_target` instead of
  the test-only unused input variable.
- `bun run --cwd ui build`: **exit 0**, including TypeScript checking and Vite
  bundling (`built in 2.54s`).
- LSP diagnostics on all three changed code/test files: **no diagnostics**.
- `bun run --cwd ui test`: **1,576 passed, 6 failed** across 160 files.
  Failures are outside the changed copy files:
  - `ui/src/lib/tauri.test.ts:395`, `:442`: expected request objects omit the
    existing `attentionInventory: []` field.
  - `ui/src/lib/tauri.test.ts:582`: expected probe args omit the existing
    `sound: "system"` argument.
  - `ui/src/features/ferryx/push/client.test.ts:5`, `:11`, `:16`: the unrelated
    client is a stub returning `null`, `"enabled"`, and `"disabled"` without
    implementing link validation, permission checks, or server unsubscribe.
- Independent reproduction without loading the native pane test:
  `CI=1 bun run --cwd ui test src/lib/tauri.test.ts src/features/ferryx/push/client.test.ts --reporter=dot`
  produced the same **6 failures and 26 passes**. These files were not edited by
  this session, and their failures were left unchanged.

The macOS writer uses safe objc2 APIs. Test read-back accesses AppKit's external
string constant in a documented unsafe block. No Miri or sanitizer run was made:
the exercised Ghostty/AppKit/pasteboard FFI is a real-system integration test, not
an interpreted-Miri test. Windows and Linux platform builds were not run on this
macOS host; their browser-copy behavior was covered at the frontend boundary.

## Desktop confirmation

Desktop automation is not authorized for this workspace. The exact physical
drag-and-Cmd+C scenario therefore needs user confirmation in the debug app:

1. Start the app with exactly `bun tauri dev`.
2. Drag to select terminal text, release the mouse, and press Cmd+C once.
3. Paste into a text editor and verify the selected text appears on the first try.
4. Repeat immediately after selecting text in another split pane, in both Korean
   and English keyboard layouts.
5. With no selection, verify Cmd+C leaves the clipboard and terminal input alone.

## Shared workspace and cleanup

The workspace already contained unrelated uncommitted changes, including native
selection, notification, context-menu, and pane-drag work. Those changes are not
owned by this fix. The existing `.debug-journal.md` is also unrelated and is left
untouched. No desktop app, live daemon, or general system clipboard was modified
by this verification. This report is the retained evidence record;
no temporary instrumentation has been added by the lead session.
