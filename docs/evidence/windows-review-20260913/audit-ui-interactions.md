# Windows UI Interactions Correctness Audit

Date: 2026-09-13 | Scope: ui/src/App.tsx, components, hooks, state, lib interaction contracts on Windows

## 1. Domain Coverage Paths
- Shortcuts & POSIX Ctrl keys: `ui/src/lib/shortcuts.ts` (`useShortcuts`, `SHORTCUTS`, `matchesBinding`), `ui/src/components/NativeTerminalPane.tsx` (keydown capture/forwarding, `isPasteShortcut`, `isCopyShortcut`).
- Close/pin/tab/split: `ui/src/App.tsx` (`handleCloseActiveSurface`, `handleCloseTab`), `ui/src/state/workspaceStore.ts` (`closePane`, `closeTab`, `createSpawnedTab`, `SET_TAB_PINNED`), `ui/src/lib/types.ts` (`TerminalTab`).
- Clipboard & IME: `ui/src/components/NativeTerminalPane.tsx` (`copySelectionOrInterrupt`, `performNativePasteFallback`, IME textarea focus sink), `ui/src/lib/contextMenuGuard.ts` (`installContextMenuGuard`).
- Shell menu: `ui/src/components/TabBar.tsx` (`WINDOWS_SHELL_OPTIONS`, `handleNewTabClick`), `ui/src/components/TerminalSplitView.tsx` (`onAddTab` forwarding to `App.tsx`).
- Browser focus: `ui/src/components/BrowserPane.tsx`, `ui/src/App.tsx` (`browserShortcutsActive`, `dispatchBrowserShortcut`, `onBrowserShortcutRequested`), `src-tauri/src/browser/guest.rs`.
- DnD: `ui/src/components/TabBar.tsx` (`startWindowDrag`, `useDroppable`), `ui/src/components/NativeTerminalPane.tsx` (`quoteShellPath`, `onDragDropEvent`, `dragDropPositionToLogical`).
- Wheel normalization: `ui/src/components/NativeTerminalPane.tsx` (`onWheel`), `ui/src/remote/RemoteTerminal.tsx` (`onWheel`).

## 2. TerminalTab.kind Optional Contract Proof (PR 3 / head99c7086b)
- Contract: `ui/src/lib/types.ts:138` defines `TerminalTab = { kind?: "terminal"; id: string; label: string; sessionId: string; pinned?: boolean }`. `kind` is optional.
- Construction: `ui/src/state/workspaceStore.ts:579-583` (`createSpawnedTab`) spawns tabs omitting `kind` (`{ id: tabId, label, sessionId }`). All untagged tabs have `kind === undefined`.
- Bug on main (`b7ad4516`): `ui/src/App.tsx:1777` checked `activeTab?.kind === "terminal" && activeLayout?.root.type === "split"`. Because newly opened terminal tabs have `kind === undefined`, the condition evaluates to `false`. Instead of closing the focused split pane via `closePane(tabId, leafId)`, it falls through to `handleCloseTab(tabId)`, killing the whole tab and all sibling split processes (or doing nothing if pinned).
- Resolution: PR 3 modifies `ui/src/App.tsx:1777` to `activeTab && activeTab.kind !== "browser" && activeLayout?.root.type === "split"`, aligning with all other discriminators in `workspaceStore.ts` (lines 757, 1925, 1944, 2642). Parameterized tests in `App.test.tsx` confirm both `{ kind: undefined, pinned }` and `{ kind: "terminal", pinned }` close only the focused pane.
- Assessment of PR 3 Test Coverage & Evidence Gaps:
  - Missing Windows `ctrlKey` Coverage: The test matrix in `App.test.tsx:630-660` tests the web shortcut using exclusively `fireEvent.keyDown(window, { key: "w", metaKey: true })` (macOS Cmd+W). It does NOT test `{ key: "w", ctrlKey: true }` under non-macOS environment (`isMac = false`). The native menu path (`native.closeMenuHandler?.()`) is platform-neutral at the IPC level, but the web keydown path for Windows remains an unverified test gap.
  - Mock Assertions vs. Sibling PTY Process Survival: PR 3 tests assert only `expect(workspace.closePane).toHaveBeenCalledWith(...)` against a mocked `workspace` store object (`ui/src/App.test.tsx:617, 659`). The real `closePane` (`workspaceStore.ts:1042-1065`) and underlying `services.closeTerminal` IPC calls are not executed in this test. While the store logic preserves non-closing session IDs (`!isSessionReferenced`), real runtime survival of the sibling PTY process on Windows requires integration/daemon evidence beyond these shallow wiring tests.
  - Fix Disposition: The one-line production fix is sound and correct at the contract level; it is not rejected because tests are mocked, but the missing Windows `ctrlKey` coverage and sibling PTY survival remain explicit evidence gaps.

## 3. Confirmed Findings (Sorted by Severity)

### [WIN-UI-01] Terminal POSIX Ctrl Chords Intercepted by Global Shortcuts on Windows (HIGH)
- File & Line: `ui/src/lib/shortcuts.ts:429-436`, `472-473`
- Reachable Call Chain: `window.addEventListener("keydown", handleKeyDown, true)` (`shortcuts.ts:446`) -> `WorkspaceApp` (`App.tsx:2344`) registers `SHORTCUTS`. `isEditableTarget(event.target) && !isTerminalTarget(event.target)` evaluates to `false` because `isTerminalTarget` is `true`. On Windows (`!isMac`), `mod: true` maps to `ctrlKey: true` (`shortcuts.ts:473`). `handleKeyDown` matches and calls `event.preventDefault()`.
- Colliding Actions: `terminal.splitRight` (Ctrl+D / EOF), `tab.close` (Ctrl+W / unix-word-rubout), `sidebar.left.toggle` (Ctrl+B / tmux prefix), `commandPalette.open` (Ctrl+K / kill-line), `tab.newTerminal` (Ctrl+T / transpose-chars), `terminal.focusNext/Previous` (Ctrl+[/] / Vim Escape).
- Failing-First Test: In `ui/src/lib/shortcuts.test.tsx`, dispatch `keydown` `{ key: "d", ctrlKey: true }` on a focused textarea inside `.terminal-host` with non-macOS platform. Assert `event.defaultPrevented === false` and `terminal.splitRight` handler is not called.
- Binary Runtime Observable: In a running CLI (e.g. bash/python), typing `cat` and pressing `Ctrl+D` does not send EOF; instead, Ferryx splits the pane right. Pressing `Ctrl+W` in readline kills the active tab instead of deleting a word.
- Smallest Fix: In `ui/src/lib/shortcuts.ts`, do not bypass editable-target rejection for terminal targets on bare `mod` chords when `!isMacShortcutPlatform()`, or remap colliding chords to `Alt` or `Ctrl+Shift` on Windows.

### [WIN-UI-02] Untagged TerminalTab.kind Closes Whole Tab Instead of Focused Pane (HIGH)
- File & Line: `ui/src/App.tsx:1777`
- Reachable Call Chain: `useShortcuts("tab.close")` (`App.tsx:2345`) or native menu `onCloseTabMenu` (`App.tsx:2098`) -> `handleCloseActiveSurface` (`App.tsx:1770`). Evaluates `activeTab?.kind === "terminal"`. Newly opened tabs have `kind: undefined`, failing the guard and falling through to `handleCloseTab(activeTabId)`.
- Failing-First Test: In `ui/src/App.test.tsx`, render split tab with `{ kind: undefined, pinned: false }` and fire `tab.close` (Ctrl/Cmd+W). Assert `workspace.closePane` called with focused leaf ID and `workspace.closeTab` not called.
- Binary Runtime Observable: Opening a terminal tab, splitting it into two panes, and pressing Ctrl+W kills the entire tab and closes both terminal sessions simultaneously.
- Smallest Fix: In `ui/src/App.tsx:1777`, replace `activeTab?.kind === "terminal"` with `activeTab && activeTab.kind !== "browser"` (PR 3).

### [WIN-UI-03] Workspace Selection Shortcuts Collide with and Shadow Tab Selection on Windows (MEDIUM)
- File & Line: `ui/src/lib/shortcuts.ts:160-290`, `472-473`
- Reachable Call Chain: `tab.select1..9` binds `{ key: "1..9", control: true }` (`shortcuts.ts:160-240`). `workspace.select1..9` binds `{ key: "1..9", mod: true }` (`shortcuts.ts:245-290`). On Windows, `mod` resolves to `ctrlKey: true`. Because `tab.select` appears earlier in `SHORTCUTS`, `workspace.select1..9` is shadowed and unreachable.
- Failing-First Test: In `ui/src/lib/shortcuts.test.tsx`, dispatch `keydown` on Windows with `{ key: "1", altKey: true }` and `{ key: "1", ctrlKey: true }`. Assert workspace selection handler is reachable via keyboard.
- Binary Runtime Observable: Pressing `Ctrl+1` through `Ctrl+9` on Windows selects tabs 1-9; keyboard selection of workspaces is completely broken. Note `src-tauri/src/browser/guest.rs:146` already expects `Alt+1..9` for workspaces on Windows.
- Smallest Fix: In `ui/src/lib/shortcuts.ts`, assign `{ key: "1..9", alt: true }` to `workspace.select1..9` on non-macOS platforms.

### [WIN-UI-04] Bare Ctrl+V Intercepted for Paste, Hijacking Vim Visual Block and Quoted Insert (MEDIUM)
- File & Line: `ui/src/components/NativeTerminalPane.tsx:244-254`, `ui/src/remote/RemoteTerminal.tsx:681`
- Reachable Call Chain: `isPasteShortcut` returns `true` for `(event.ctrlKey || event.metaKey) && !event.altKey && KeyV`. It does not require `event.shiftKey`. `onKeyDown` (`NativeTerminalPane.tsx:2311`) invokes `performNativePasteFallback()` instead of forwarding `0x16` (SYN) to the PTY.
- Failing-First Test: In `ui/src/components/NativeTerminalPane.test.tsx`, dispatch `keydown` `{ key: "v", code: "KeyV", ctrlKey: true, shiftKey: false }` on Windows focus sink. Assert `sendInput` receives key event and paste fallback is not triggered.
- Binary Runtime Observable: In `vim` on Windows, pressing `Ctrl+V` pastes clipboard text instead of entering `-- VISUAL BLOCK --` mode. In readline, quoted literal insertion (`Ctrl+V <char>`) fails.
- Smallest Fix: In `isPasteShortcut` (`NativeTerminalPane.tsx:244`), require `event.shiftKey` when `event.ctrlKey` is active on non-macOS platforms (`!isMacShortcutPlatform()`).

### [WIN-UI-05] Terminal Selection Copy Fails via Asynchronous Clipboard API (MEDIUM)
- File & Line: `ui/src/components/NativeTerminalPane.tsx:964-972`
- Reachable Call Chain: `copySelectionOrInterrupt` (`NativeTerminalPane.tsx:953`) -> `invoke("cmd_native_terminal_copy_selection")`. On macOS, backend writes to NSPasteboard. On Windows (`!isMac`), frontend executes `navigator.clipboard.writeText(text)` after asynchronous IPC resolution. Transient user activation expires across the IPC boundary, throwing `NotAllowedError`.
- Failing-First Test: In `ui/src/components/NativeTerminalPane.test.tsx`, invoke copy with mock `navigator.clipboard.writeText` rejecting outside transient activation. Assert clipboard write failure.
- Binary Runtime Observable: Pressing `Ctrl+Shift+C` after selecting text in terminal on Windows fails to copy text; console logs `Native terminal browser clipboard write failed NotAllowedError`.
- Smallest Fix: Implement native Win32 clipboard write (`OpenClipboard` / `SetClipboardData`) in `cmd_native_terminal_copy_selection` (`src-tauri/src/ipc/native_terminal.rs`), eliminating frontend `navigator.clipboard.writeText`.

### [WIN-UI-06] Bare Ctrl Key Swallows Clicks and Toggles Pointer Cursor Across Terminal (MEDIUM)
- File & Line: `ui/src/components/NativeTerminalPane.tsx:1264-1275`, `2168`, `2190-2201`
- Reachable Call Chain: `handleKeyChange` sets `held = isMac ? event.metaKey : event.ctrlKey`. On Windows, pressing `Ctrl` sets `isCmdHeld = true`, giving `.terminal-host` `cursor-pointer`. On `pointerdown`, `event.button === 0 && isCmdOrCtrl` captures click in `cmdClickDownRef` and returns early, swallowing the mouse event and preventing `sendMouse` from dispatching to PTY.
- Failing-First Test: In `ui/src/components/NativeTerminalPane.test.tsx`, press `Ctrl` on Windows; assert terminal container does not receive `cursor-pointer`. Dispatch `pointerdown` with `ctrlKey: true`; assert `sendMouse` is called.
- Binary Runtime Observable: Holding `Ctrl` to type shell shortcuts turns mouse into a clicking hand across the whole terminal. In tmux/vim mouse mode, Ctrl+Click cannot send mouse tracking escapes to the application.
- Smallest Fix: In `NativeTerminalPane.tsx`, gate link activation behind link-token hover hit-testing rather than toggling container cursor and click interception on bare `event.ctrlKey`.

### [WIN-UI-07] Window Caption Controls Overlap TabBar with Overlay TitleBarStyle on Windows (MEDIUM)
- File & Line: `ui/src/components/TabBar.tsx:365`, `src-tauri/tauri.windows.conf.json:14`
- Reachable Call Chain: `src-tauri/tauri.windows.conf.json` sets `"titleBarStyle": "Overlay"` with `"hiddenTitle": true`. Windows draws system caption buttons (minimize, maximize, close) in top-right (~138px). `TabBar.tsx:365` has only `pr-1`.
- Failing-First Test: In `ui/src/components/TabBar.test.tsx`, assert TabBar applies trailing caption clearance padding on Windows desktop.
- Binary Runtime Observable: On Windows, the rightmost tab's close button, new-tab `+` button, or tab action buttons sit underneath the native minimize/maximize/close window controls and cannot be clicked.
- Smallest Fix: In `ui/src/components/TabBar.tsx`, add trailing padding (`pr-36` / ~140px) when running on Windows desktop.

### [WIN-UI-08] File Drag-and-Drop Formats Windows Paths in POSIX Single Quotes (MEDIUM)
- File & Line: `ui/src/components/NativeTerminalPane.tsx:376-381`, `1653`
- Reachable Call Chain: `quoteShellPath` checks `/[\s'"\\$`!*?[\]();&|<>]/.test(path)` and returns `'${path}'`. Because all Windows absolute paths contain `\\` (`C:\Users\...`), every dropped path is wrapped in single quotes.
- Failing-First Test: In `NativeTerminalPane.test.tsx`, assert `quoteShellPath("C:\\Users\\test\\file.txt")` on Windows produces `"C:\Users\test\file.txt"` or unquoted string, not `'C:\Users\test\file.txt'`.
- Binary Runtime Observable: Dragging a file from Windows Explorer into Command Prompt pastes `'C:\path\to\file'`, which `cmd.exe` rejects with syntax/command not recognized.
- Smallest Fix: In `quoteShellPath`, branch on Windows platform/path to quote with double quotes (`"..."`) when spaces exist, avoiding POSIX single quotes.

### [WIN-UI-09] Discrete Mouse Wheel Line Mode Divided by 20 Causes Sluggish Scrolling (MEDIUM)
- File & Line: `ui/src/components/NativeTerminalPane.tsx:2223`, `ui/src/remote/RemoteTerminal.tsx:493`
- Reachable Call Chain: `onWheel` computes `Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1)`. Windows PC mice emit `deltaMode === 1` (`DOM_DELTA_LINE`) with 1-3 lines per notch. `Math.trunc(1 / 20)` is 0, clamped to 1.
- Failing-First Test: In `NativeTerminalPane.test.tsx`, dispatch `WheelEvent` with `deltaMode: 1` and `deltaY: 3`. Assert `cmd_native_terminal_scroll` receives `rows: 3`, not 1.
- Binary Runtime Observable: Scrolling a notched PC mouse wheel on Windows moves at 1 line per notch regardless of Windows mouse wheel settings.
- Smallest Fix: Inspect `event.deltaMode`: if `DOM_DELTA_LINE`, use `event.deltaY` directly without dividing by 20.

### [WIN-UI-10] Universal Context Menu Guard Suppresses Windows Right-Click Paste and System Menu (MEDIUM)
- File & Line: `ui/src/lib/contextMenuGuard.ts:6-15`
- Reachable Call Chain: `installContextMenuGuard` (`App.tsx:2654`) cancels `contextmenu` on anything outside `input, textarea`. `.terminal-host` and `[data-testid="tab-strip"]` are `div`s, so right-clicks are unconditionally cancelled.
- Failing-First Test: In `contextMenuGuard.test.ts`, dispatch `contextmenu` on `.terminal-host` or tab strip on Windows; assert `event.defaultPrevented === false`.
- Binary Runtime Observable: Right-clicking terminal pane on Windows fails to paste or open menu. Right-clicking custom titlebar fails to open Windows System Menu (Restore, Move, Size, Minimize, Maximize, Close).
- Smallest Fix: In `contextMenuGuard.ts`, exempt `.terminal-host` elements and `.drag-region` on Windows.

### [WIN-UI-11] TabBar Pointerdown PreventDefault Breaks Native Double-Click Window Maximize (LOW)
- File & Line: `ui/src/components/TabBar.tsx:344-349`
- Reachable Call Chain: `startWindowDrag` calls `event.preventDefault()` before `getCurrentWindow().startDragging()`. Suppressing `pointerdown` cancels default double-click registration, preventing Windows `WM_NCLBUTTONDBLCLK`.
- Failing-First Test: In `TabBar.test.tsx`, verify `startWindowDrag` does not call `event.preventDefault()`.
- Binary Runtime Observable: Double-clicking empty space in the tab bar on Windows fails to maximize/restore window.
- Smallest Fix: Remove `event.preventDefault()` from `startWindowDrag` in `TabBar.tsx`.

### [WIN-UI-12] TabBar Leading Spacer Renders 36px Empty Dead Space on Windows (LOW)
- File & Line: `ui/src/App.tsx:2591`, `ui/src/components/TabBar.tsx:366-372`
- Reachable Call Chain: `App.tsx:2591` passes `leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}`. When sidebar is collapsed on Windows, renders a 36px blank spacer with border. Windows has no top-left window controls.
- Failing-First Test: In `App.test.tsx`, assert `leadingSpacer` is 0 when `!isMacShortcutPlatform()`.
- Binary Runtime Observable: When sidebar is collapsed on Windows, an empty 36px box sits at the top-left of the window.
- Smallest Fix: In `App.tsx:2591`, pass `leadingSpacer={isSidebarOpen || !isMacShortcutPlatform() ? 0 : 108}`.

### [WIN-UI-13] Split Tab Browser Pane Inactivates Browser Shortcuts (LOW)
- File & Line: `ui/src/App.tsx:2252`, `2346-2350`, `2374-2382`
- Reachable Call Chain: `browserShortcutsActive` is computed as `activeShortcutTab?.kind === "browser"`. In a split tab with terminal + browser panes, `activeShortcutTab.kind` is undefined or `"terminal"`. Browser actions (focus address, reload, back, forward, find) remain disabled even when the focused leaf is the browser pane.
- Failing-First Test: In `App.test.tsx`, activate browser pane leaf in a split tab; assert `browser.focusAddress` is active.
- Binary Runtime Observable: In a split tab, focusing browser pane and pressing Ctrl+L / Ctrl+F triggers terminal actions instead of browser actions.
- Smallest Fix: Compute `browserShortcutsActive` using active leaf pane content kind.

## 4. Prior Audit Refutations & Fixed Items
- [FIXED] Windows Shell Menu Selection Dropped in `TerminalSplitView.tsx`: Commit `8dee7072` updated `TerminalSplitView.tsx:774-778` to accept and forward `shell?: string` via `onAdd={(shell) => { focusGroup(); if (shell !== undefined) onAddTab(shell); else onAddTab(); }}`. Runtime UIA probe verified functional PowerShell, Windows PowerShell, cmd, and WSL choices (`FINAL-AUDIT.md:14-16`).
- [REFUTED/NOT A DEFECT] "Option as Alt" Setting on Windows: `TerminalSection.tsx` option is macOS-scoped; benign on Windows.

## 5. Unknowns & Non-Deterministic Boundaries
- Windows `ctrlKey` Web Shortcut Integration Gap: PR 3 test matrix mocks `metaKey: true` only; Windows browser keyboard dispatch (`ctrlKey: true` with `isMac = false`) is untested in unit suites.
- Sibling PTY Process Runtime Survival Gap: Unit tests mock `workspace.closePane`. End-to-end confirmation that sibling daemon PTY sessions survive without exit or orphan termination on Windows requires daemon/runtime evidence.
- WebView2 touch gestures: Whether touch-screen Windows devices dispatch PointerEvent or WheelEvent for pinch-zoom in `RemoteTerminal.tsx`.
- Fractional DPI scaling (125%/175%) coordinate alignment in Win32 mouse hook over WebView2 child surfaces.
