# Ferryx Terminal File Drag-and-Drop & Cmd+Click Link/Path Implementation

**Date:** 2026-09-06  
**Status:** Completed, Remediated, and Verified  

## 1. File Drag-and-Drop Path Insertion (파일 드래그앤드롭 경로 삽입)

### Root Causes
1. **AppKit Drag Interception in `FerryxNativeTerminalView`**:
   `FerryxNativeTerminalView` sits above the `WKWebView` in the NSWindow contentView hierarchy. It had overridden `draggingEntered:`, `draggingUpdated:`, `performDragOperation:`, and `draggingExited:` calling `super`. In AppKit, the default `NSView` implementation of `draggingEntered:` returns `NSDragOperationNone` (0) and `performDragOperation:` returns `false`. Because the topmost view rejected the drag session, AppKit never forwarded the drag to sibling/underlying `WKWebView`, causing drops from Finder to be completely ignored on macOS.
2. **Missing Input Sink Focus**:
   When files were pasted into the terminal on drop, `inputRef.current?.focus()` and `sendFocus(true)` were not called, leaving the terminal unfocused. The user had to click again before they could type or submit the prompt.
3. **No Trailing Space**:
   Multiple dropped files or sequential drops concatenated directly without separation from following commands/flags.

### Fixes Applied
1. **`src-tauri/src/native_terminal/platform/macos.rs`**:
   Removed the dummy `draggingEntered:`, `draggingUpdated:`, `performDragOperation:`, and `draggingExited:` overrides from `FerryxNativeTerminalView`. The overlay view is now fully transparent to AppKit drag routing, allowing drops to pass cleanly to `WKWebView` (wry) which implements `NSDraggingDestination`. Also removed dead `switch_debug_log` to keep compiler warnings clean.
2. **`ui/src/components/NativeTerminalPane.tsx`**:
   - Added trailing space separation (`payload.paths.map(quoteShellPath).join(" ") + " "`).
   - Added explicit terminal input sink focus on drop (`lastFocusedNativeTerminalSessionId = targetSessionId; inputRef.current?.focus(); sendFocus(true);`).
   - Cleaned up dead HTML5 drop handlers that were inactive under native drag-and-drop.

---

## 2. Cmd + Click to Open URLs and File Paths (Cmd + 클릭 링크 및 파일 경로 열기)

### Root Causes & Remediation
1. **No Link/Path Detection or Click Interception**:
   `NativeTerminalPane.tsx` forwarded all mouse clicks directly to Ghostty via `sendMouse`. There was no mechanism to distinguish Cmd+click (macOS) or Ctrl+click (Windows/Linux) from standard selection drags or CLI mouse reporting.
2. **No Terminal Line Text Query**:
   Ghostty VT FFI had methods for selections, but lacked an API to read the unwrapped line of text at a given viewport coordinate `(col, row)` without altering active user selection.
3. **Grid Column vs Trimmed String Indentation Bug (Remediated)**:
   Ghostty VT `select_line` bounds to non-whitespace by default. When formatted, leading spaces were missing, causing column index drift on indented lines (rustc, tsc, eslint output). Remediated by querying `ordered.start` column in Ghostty VT and padding leading spaces so visual grid column aligns 1:1 with string character index.
4. **CJK / Wide Character Alignment (Remediated)**:
   Added `gridColToCharIndex` in `linkRouting.ts` to map 2-column East Asian wide characters (Hangul, CJK ideographs) to string character indices before token lookup.
5. **No File Path Opener Command & Editor Hijacking (Remediated)**:
   Added `cmd_open_file_path(path, cwd, line, col)` which resolves relative paths against `cwd`, expands `~`, and uses the reliable OS default opener (`open`, `xdg-open`, `start`), avoiding arbitrary editor hijacking.
6. **False-Positive Token Parsing & Quoted Path Support (Remediated)**:
   `resolveTokenAtCol` handles balanced parentheses in URLs, quoted paths with spaces, paths with apostrophes, and suppresses false positives on IP:port (`127.0.0.1:3000`), version strings, and decimal numbers.
7. **Listener Leak (Remediated)**:
   Fixed `handleBlur` listener leak in `NativeTerminalPane.tsx` by using a stable named function reference.

---

## 3. Verification Evidence

1. **Rust Tests**:
   - `native_terminal::terminal::tests::test_line_text_at_reads_unwrapped_line_without_modifying_selection` -> PASS
   - `native_terminal::terminal::tests::test_line_text_at_preserves_leading_whitespace_for_indented_lines` -> PASS
   - `ipc::browser::tests::test_cmd_open_file_path_rejects_nonexistent_file` -> PASS
   - `ipc::browser::tests::test_cmd_open_file_path_resolves_relative_with_cwd` -> PASS
   - `tests/native_terminal_input_boundary_contract.rs` (15 tests) -> PASS
2. **Frontend Tests**:
   - `ui/src/lib/linkRouting.test.ts` (21 tests) -> PASS
   - `ui/src/components/NativeTerminalPane.test.tsx` (142 tests) -> PASS
   - `ui/src/components/TerminalLinkActions.test.tsx` (4 tests) -> PASS
3. **Build & Formatting**:
   - `tsc && vite build` -> Exit code 0
   - `cargo check --manifest-path src-tauri/Cargo.toml` -> Exit code 0
   - `rustfmt --edition 2021 --check` on all modified files -> Exit code 0
