### Terminal POSIX Control Codes Collide with Global Shortcuts on Windows and Linux
- **ID**: L5-UI-FRONTEND-1
- **Severity**: BLOCKER
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/shortcuts.ts:407` - `          !isTerminalTarget(event.target) &&`
- **Why it breaks**: On Windows and Linux, `mod: true` maps to `Ctrl`, and `useShortcuts` specifically excludes terminal targets from the editable target check (`!isTerminalTarget`), allowing global application shortcuts to fire during active terminal sessions. This causes critical POSIX terminal control signals to be intercepted: `Ctrl+D` (terminal EOF) triggers `terminal.splitRight`, `Ctrl+W` (readline backward-kill-word) triggers `tab.close` and kills the session, `Ctrl+B` (tmux prefix) triggers `sidebar.left.toggle`, `Ctrl+K` (kill line) triggers `commandPalette.open`, and `Ctrl+T` (transpose) triggers `tab.newTerminal`.
- **Fix**: In `ui/src/lib/shortcuts.ts`, guard terminal targets so bare `mod` shortcuts that conflict with standard terminal control sequences are skipped when `!isMacShortcutPlatform()`, or rebind conflicting app-level shortcuts to `Alt` or `Ctrl+Shift` chords on Windows and Linux.
- **Status**: OPEN

### Native Terminal Transparency Scoped to macOS
- **ID**: L5-UI-FRONTEND-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `ui/src/index.css:182` - `html.platform-macos:has([data-testid="native-terminal-pane"]),`
- **Why it breaks**: An earlier unscoped version of the native terminal transparency rules applied `background-color: transparent !important` globally across all operating systems. On Windows, WebView2 rendered an entirely black window when background transparency was enabled without DWM composition support.
- **Fix**: Scoped all terminal transparency CSS rules strictly to `html.platform-macos` in `ui/src/index.css:182-197`.
- **Status**: FIXED (handled in `ui/src/index.css:182-197` and `ui/src/main.tsx:10`)

### Workspace Selection Collides with Tab Selection on Windows and Linux
- **ID**: L5-UI-FRONTEND-3
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/shortcuts.ts:445` - `  const expectedControl = Boolean(binding.control || (binding.mod && !isMac));`
- **Why it breaks**: On macOS, `tab.select1..9` uses `Ctrl+1..9` (`binding.control: true`) while `workspace.select1..9` uses `Cmd+1..9` (`binding.mod: true`). On Windows and Linux, `mod` resolves to `Ctrl`, mapping both actions to identical `Ctrl+1..9` chords. Because `tab.select1..9` appears earlier in the `SHORTCUTS` list, `workspace.select1..9` is shadowed and can never be triggered via keyboard on non-macOS platforms.
- **Fix**: In `ui/src/lib/shortcuts.ts`, update `workspace.select1..9` bindings on non-macOS platforms to use `alt: true` (e.g., `Alt+1..9`) so workspace switching chords do not collide with tab selection chords.
- **Status**: OPEN

### Terminal Input Intercepts Unshifted Ctrl+V Hijacking Vim Visual Block and Quoted Insert
- **ID**: L5-UI-FRONTEND-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:240` - `    ((event.ctrlKey || event.metaKey) &&`
- **Why it breaks**: `isPasteShortcut` treats `(event.ctrlKey || event.metaKey) && !event.altKey` with `KeyV` as a paste shortcut on all platforms regardless of `event.shiftKey`. On Linux and Windows terminals, `Ctrl+V` (ASCII 0x16 SYN) is an essential terminal control character used in vim for block visual mode and in readline for quoted literal character insertion, whereas terminal paste is conventionally `Ctrl+Shift+V`. Intercepting bare `Ctrl+V` unconditionally prevents vim block selection and quoted insertions from functioning.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx::isPasteShortcut`, require `event.shiftKey` when `event.ctrlKey` is active on non-macOS platforms (`!isMacShortcutPlatform()`), preserving bare `mod+V` paste only for `event.metaKey` on macOS.
- **Status**: OPEN

### Terminal Selection Copying Fails Due to Asynchronous Clipboard API Without User Activation
- **ID**: L5-UI-FRONTEND-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:972` - `        if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {`
- **Why it breaks**: On macOS, `cmd_native_terminal_copy_selection` writes selection text directly to `NSPasteboard` on the native thread. On Windows and Linux, the frontend attempts to copy by awaiting the asynchronous IPC response and then calling `navigator.clipboard.writeText(text)`, which fails with `NotAllowedError` because the transient user activation from the keydown or click event has expired across the IPC boundary.
- **Fix**: In `src-tauri`, make `cmd_native_terminal_copy_selection` write the selected text directly to the native OS clipboard on Windows (via Windows API / arboard) and Linux (via X11/Wayland clipboard), matching macOS behavior and removing `navigator.clipboard.writeText` from `copySelectionOrInterrupt` in `NativeTerminalPane.tsx`.
- **Status**: OPEN

### Ctrl+Click Intercepted for URL Opening and Pointer Cursor on Windows and Linux
- **ID**: L5-UI-FRONTEND-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:1223` - `      const held = isMac ? event.metaKey : event.ctrlKey;`
- **Why it breaks**: On macOS, holding `Cmd` (`event.metaKey`) toggles link navigation mode. On Windows and Linux, `held` evaluates `event.ctrlKey`, which turns the mouse cursor into a clicking hand (`cursor-pointer`) across the entire terminal pane whenever `Ctrl` is pressed for ordinary shell commands (Ctrl+C, Ctrl+R, etc.), and holding `Ctrl` while clicking swallows the pointer event (`cmdClickDownRef`), preventing terminal TUI applications (such as tmux or vim) from receiving Ctrl-click mouse sequences.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx`, avoid using bare `event.ctrlKey` as the URL click modifier on Windows/Linux; gate link clicking behind an explicit modifier or link hover hit-test rather than toggling `isCmdHeld` across the entire terminal container on any `ctrlKey` event.
- **Status**: OPEN

### Window Caption Controls Overlap TabBar on Windows with Overlay TitleBarStyle
- **ID**: L5-UI-FRONTEND-7
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/TabBar.tsx:333` - `      className="relative flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1 select-none"`
- **Why it breaks**: In `src-tauri/tauri.windows.conf.json`, `titleBarStyle` is configured as `"Overlay"` with `hiddenTitle: true`, positioning the native Windows minimize/maximize/close caption controls at the top-right corner of the window. `TabBar.tsx` uses only `pr-1` at its trailing edge without reserving space for caption controls, causing the Windows system buttons to overlay and block interaction with the rightmost tabs and tab strip action buttons.
- **Fix**: In `ui/src/components/TabBar.tsx`, add a trailing spacer or right padding (approximately 138px) when running on Windows with `titleBarStyle: "Overlay"` to keep tab actions clear of the native caption buttons.
- **Status**: OPEN

### Discrete Mouse Wheel and Non-Pixel DeltaMode Divided by 20 Causes Sluggish Scrolling
- **ID**: L5-UI-FRONTEND-8
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:2067` - `        const rows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);`
- **Why it breaks**: Both `NativeTerminalPane.tsx` and `RemoteTerminal.tsx` divide `event.deltaY` by 20, assuming macOS trackpad pixel scrolling (`deltaMode: DOM_DELTA_PIXEL` = 0). On Windows, Linux, and non-WebKit browsers like Firefox, discrete mouse wheel ticks dispatch `deltaMode: DOM_DELTA_LINE` (1) with values like 1, 2, or 3 lines. Dividing these values by 20 truncates to 0 (defaulting to 1), making wheel scrolling feel unresponsive and erratic on standard PC mice.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx` and `ui/src/remote/RemoteTerminal.tsx`, inspect `event.deltaMode`: if `event.deltaMode === WheelEvent.DOM_DELTA_LINE` (1), treat `event.deltaY` directly as line counts without dividing by 20; if `event.deltaMode === WheelEvent.DOM_DELTA_PAGE` (2), scale by the visible row count.
- **Status**: OPEN

### Shell Path Quoting Wraps Backslashes in POSIX Single Quotes Breaking Windows Command Interpreters
- **ID**: L5-UI-FRONTEND-9
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:370` - `function quoteShellPath(path: string): string {`
- **Why it breaks**: `quoteShellPath` checks `/[\s'"\\$`!*?[\]();&|<>]/.test(path)` and wraps matches in POSIX single quotes (`'...'`). Because every Windows path contains backslashes (e.g., `C:\Users\test`), all dropped Windows paths are formatted as POSIX single-quoted strings. In Windows `cmd.exe`, single quotes are not valid path delimiters (causing command not found or invalid syntax errors), and PowerShell requires the `&` call operator to execute single-quoted paths.
- **Fix**: In `ui/src/components/NativeTerminalPane.tsx::quoteShellPath`, branch on platform: on Windows, wrap paths containing spaces or special characters in double quotes (`"..."`) with Windows shell escaping instead of POSIX single quotes.
- **Status**: OPEN

### Proportional Font In Terminal Stack Distorts Character Grid on Windows and Linux Remote Clients
- **ID**: L5-UI-FRONTEND-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/tauri.ts:3` - `export const DEFAULT_TERMINAL_FONT_STACK = 'MesloLGS NF, "Noto Sans KR", monospace';`
- **Why it breaks**: `DEFAULT_TERMINAL_FONT_STACK` places `"Noto Sans KR"` (a proportional sans-serif font) before `monospace`. When `MesloLGS NF` is absent on Windows or Linux, systems with `Noto Sans KR` installed fall back to a proportional typeface. In `RemoteTerminal.tsx`, cell widths are calculated from a single `1ch` element (`width: "1ch"`), which in proportional fonts measures only the advance of '0', causing characters of different widths to misalign with grid columns and cursor positions.
- **Fix**: In `ui/src/lib/tauri.ts`, insert standard platform monospaced fonts (such as `Consolas, "Courier New"`) before `"Noto Sans KR"` in `DEFAULT_TERMINAL_FONT_STACK`, ensuring the terminal stack always resolves to a fixed-pitch font on all platforms.
- **Status**: OPEN

### Bare Ctrl+V Intercepted Without PTY Forwarding in Remote Web Terminal
- **ID**: L5-UI-FRONTEND-11
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/remote/RemoteTerminal.tsx:681` - `            if (ctrlChordChar === "v") return;`
- **Why it breaks**: In `RemoteTerminal.tsx::onKeyDown`, any Ctrl chord with 'v' returns immediately to defer to the browser paste event. On Windows and Linux, this intercepts unshifted `Ctrl+V` alongside `Ctrl+Shift+V`, preventing the terminal from forwarding the `ctrl-v` control code (ASCII 0x16 SYN) needed for vim visual block mode or readline quoted insert in remote sessions.
- **Fix**: In `ui/src/remote/RemoteTerminal.tsx`, require `event.shiftKey` when checking for paste chord exemption on non-macOS clients (`if (ctrlChordChar === "v" && event.shiftKey) return;`), allowing unshifted `Ctrl+V` to fall through to `sendKey("ctrl-v")`.
- **Status**: OPEN

### TabBar Leading Spacer Renders Empty Dead Space on Windows and Linux
- **ID**: L5-UI-FRONTEND-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/App.tsx:2047` - `            leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}`
- **Why it breaks**: When the sidebar is collapsed, `App.tsx` provides a `108px` spacer on macOS to clear native window traffic light controls. On non-macOS platforms, it passes `36px`, rendering an empty 36-pixel box with a right border (`border-r border-border`) at the left of the `TabBar`. On Linux (native titlebar) and Windows (caption buttons at top-right), there are no controls at the top-left, making this 36px spacer an unnecessary visual artifact.
- **Fix**: In `ui/src/App.tsx`, set `leadingSpacer` to `0` when `!isMacShortcutPlatform()`, passing non-zero values only for macOS traffic light clearance (`isSidebarOpen || !isMacShortcutPlatform() ? 0 : 108`).
- **Status**: OPEN

### TabBar Pointerdown PreventDefault Cancels Native Double-Click Window Maximize
- **ID**: L5-UI-FRONTEND-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/TabBar.tsx:316` - `    event.preventDefault();`
- **Why it breaks**: In `TabBar.tsx::startWindowDrag`, calling `event.preventDefault()` on `pointerdown` cancels the default mouse event cycle before invoking `getCurrentWindow().startDragging()`. On Windows and Linux desktops, double-clicking the empty tab bar / title bar area is the standard gesture to maximize or restore the window; suppressing the default pointer behavior aborts the second click registration and breaks double-click maximize.
- **Fix**: In `ui/src/components/TabBar.tsx`, remove `event.preventDefault()` from `startWindowDrag`, or use Tauri's native `data-tauri-drag-region` attribute on the tab bar container to let the OS handle drag and double-click gestures naturally.
- **Status**: OPEN

### Deprecated navigator.platform Sniffing Misidentifies iPads and Touch Devices as Desktop Mac
- **ID**: L5-UI-FRONTEND-14
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `ui/src/lib/shortcuts.ts:514` - `  if (/Mac|iPhone|iPad|iPod/.test(navigator.platform)) return true;`
- **Why it breaks**: `navigator.platform` is deprecated by current web standards and frozen in modern browsers. Furthermore, iPadOS desktop Safari sends `MacIntel` as `navigator.platform` and includes `Macintosh` in `navigator.userAgent`, causing `detectMacPlatform()` to classify iPads in remote web client sessions as macOS desktops, displaying Mac Command glyphs (`⌘`) and enabling desktop-only keyboard paths on touch devices.
- **Fix**: In `ui/src/lib/shortcuts.ts::detectMacPlatform`, check `navigator.userAgentData?.platform` where available and check `navigator.maxTouchPoints > 0` to avoid misclassifying touch iPads as desktop Mac clients in the web interface.
- **Status**: OPEN

### Universal Context Menu Guard Suppresses Windows Terminal Paste and System Titlebar Menu
- **ID**: L5-UI-FRONTEND-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `ui/src/lib/contextMenuGuard.ts:14` - `    event.preventDefault();`
- **Why it breaks**: `installContextMenuGuard` calls `event.preventDefault()` on all contextmenu events that do not originate from an HTML input or textarea. On Windows, right-clicking terminal panes is the standard convention for pasting text or opening terminal actions, and right-clicking custom titlebars opens the Windows system menu (Restore, Move, Size, Minimize, Maximize, Close). Suppressing all right-clicks without providing custom menus breaks expected Windows desktop interactions.
- **Fix**: In `ui/src/lib/contextMenuGuard.ts`, allow right-click events on `.terminal-host` elements to trigger paste or a terminal context menu, and exempt window drag regions on Windows to allow the OS titlebar system menu to appear.
- **Status**: OPEN

### macOS Option as Alt Setting Toggle Rendered Unconditionally on Non-Mac Platforms
- **ID**: L5-UI-FRONTEND-16
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/settings/TerminalSection.tsx:224` - `              macOS Option as Alt`
- **Why it breaks**: `TerminalSection.tsx` displays the "macOS Option as Alt" switch unconditionally across all platforms. Keyboards on Windows and Linux do not have an Option key (they use Alt and AltGr), making this toggle confusing and irrelevant on non-macOS operating systems.
- **Fix**: In `ui/src/components/settings/TerminalSection.tsx`, wrap the "macOS Option as Alt" setting block in a platform check (`isMacShortcutPlatform()`) so it only renders on macOS.
- **Status**: OPEN

### Proprietary -webkit-app-region CSS Drag Properties Ignored by WebKitGTK on Linux
- **ID**: L5-UI-FRONTEND-17
- **Severity**: LOW
- **Platforms affected**: Linux
- **Evidence**: `ui/src/index.css:145` - `    -webkit-app-region: drag;`
- **Why it breaks**: The `.drag-region` and `.no-drag` utility classes in `ui/src/index.css` specify `-webkit-app-region: drag` and `-webkit-app-region: no-drag`. These CSS properties are Chromium-specific extensions and are not supported by WebKitGTK on Linux, leaving CSS-based drag regions ineffective on Linux unless backed by Tauri drag attributes or event handlers.
- **Fix**: In `ui/src/index.css`, document the limitation and ensure all draggable elements consistently use Tauri's cross-platform `data-tauri-drag-region` attribute rather than relying on `-webkit-app-region`.
- **Status**: OPEN

### Full Disk Access macOS Permission Alert Rendered Unconditionally on Windows and Linux
- **ID**: L5-UI-FRONTEND-18
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/components/settings/PermissionsSection.tsx:183` - `        <Alert className="border-amber-500/30 bg-amber-500/10 text-amber-300">`
- **Why it breaks**: In `PermissionsSection.tsx`, if permissions are not fully granted, an alert card states: "Granting Full Disk Access stops macOS from showing alerts such as 'Ferryx would like to access your Photo Library'". On Windows and Linux, Full Disk Access does not exist, so displaying macOS-specific security guidance is irrelevant and misleading.
- **Fix**: In `ui/src/components/settings/PermissionsSection.tsx`, gate the Full Disk Access alert with `isMac` or `status?.platform === "macos"`, rendering platform-appropriate guidance or omitting the card on non-macOS platforms.
- **Status**: OPEN
