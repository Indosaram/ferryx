### Missing WSLg detection causes Wayland subsurface mis-scaling and offset over app chrome
- **ID**: L3-NATIVE-SURFACE-1
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/linux.rs:303` - `            ) if std::env::var_os("FERRYX_DISABLE_WAYLAND_SUBSURFACE").is_none() => {`
- **Why it breaks**: Under WSLg's embedded Weston Wayland compositor, `wl_subsurface` positioning and buffer scaling ignore parent coordinate offsets and mis-scale across the window chrome. `linux.rs` checks only the manual `FERRYX_DISABLE_WAYLAND_SUBSURFACE` environment variable, lacking detection for WSLg environment variables (`WSL_DISTRO_NAME`, `WSL_INTEROP`) or automatic fallback to X11 (`GDK_BACKEND=x11`), breaking native terminal rendering on WSLg.
- **Fix**: In `LinuxCompositorTarget::new` (`src-tauri/src/native_terminal/platform/linux.rs`), detect WSLg via `std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()` and bypass `WaylandChild::create`, forcing X11 child window fallback.
- **Status**: OPEN

### Native terminal clipboard content retrieval is stubbed to empty on Linux
- **ID**: L3-NATIVE-SURFACE-2
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/ipc/native_terminal.rs:409` - `    (NativeTerminalClipboardContent::Empty, Vec::new())`
- **Why it breaks**: On Linux, `read_native_pasteboard()` is a stub returning `(NativeTerminalClipboardContent::Empty, Vec::new())`, causing `cmd_native_terminal_clipboard_content` to unconditionally return `Empty`. When a user pastes via terminal shortcuts or context menus on Linux, no text is retrieved, leaving clipboard paste non-functional.
- **Fix**: In `src-tauri/src/ipc/native_terminal.rs`, implement `read_native_pasteboard()` for Linux using GTK's `gdk_clipboard_read_text_async` / `gtk_clipboard_wait_for_text` or the `arboard` crate to read UTF-8 strings from X11 and Wayland clipboards.
- **Status**: OPEN

### Linux compositor target fails to create child surface for XCB window handles
- **ID**: L3-NATIVE-SURFACE-3
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/linux.rs:306` - `            _ => None,`
- **Why it breaks**: `LinuxCompositorTarget::new` matches only `(Xlib, Xlib)` and `(Wayland, Wayland)` pairs when constructing `child`. If Tauri provides an `Xcb` window handle, `child` evaluates to `None`, which disables pointer transparency, breaks viewport updates, and forces wgpu to render directly into the unisolated root window.
- **Fix**: In `src-tauri/src/native_terminal/platform/linux.rs`, implement an `XcbChild` struct using `xcb_create_window` and `xcb_shape_rectangles` with `XCB_SHAPE_SO_SET`, and handle `(LinuxWindowHandleInner::Xcb, LinuxDisplayHandleInner::Xcb)` to construct it.
- **Status**: OPEN

### File drag-and-drop destination implemented only for macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/lib.rs:858` - `            install_macos_file_drop_destination(app)?;`
- **Why it breaks**: macOS registers `FerryxFileDropView` via `install_macos_file_drop_destination` to capture dropped file URLs and forward them to the frontend via the `ferryx://file-drop` event. Windows and Linux lack native drag-and-drop targets (`IDropTarget` / `RegisterDragDrop` on Win32, `gtk_drag_dest_set` on Linux), causing file drags over native terminal panes to be dropped or discarded.
- **Fix**: Implement `install_windows_file_drop_destination` in `src-tauri/src/lib.rs` using Win32 OLE `RegisterDragDrop` on the window HWND, and implement `install_linux_file_drop_destination` using GTK drag-motion/drag-drop signals, emitting the `ferryx://file-drop` event.
- **Status**: OPEN

### Native terminal selection copy writes to clipboard only on macOS
- **ID**: L3-NATIVE-SURFACE-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/ipc/native_terminal.rs:1106` - `    #[cfg(not(target_os = "macos"))]`
- **Why it breaks**: In `cmd_native_terminal_copy_selection`, writing selection text to the system pasteboard is gated with `#[cfg(target_os = "macos")]`. On Windows and Linux, the command returns the text string without writing to the OS clipboard, relying on webview JavaScript clipboard access which fails when focus is captured by the native child surface.
- **Fix**: In `src-tauri/src/ipc/native_terminal.rs`, implement native pasteboard writing for Windows using Win32 `OpenClipboard`/`SetClipboardData(CF_UNICODETEXT)` and for Linux using GTK clipboard or `arboard`, executing inside `cmd_native_terminal_copy_selection`.
- **Status**: OPEN

### Windows and Linux child surfaces fail to hide when viewport bounds are None
- **ID**: L3-NATIVE-SURFACE-6
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/windows.rs:293` - `            return;`
- **Why it breaks**: When viewport bounds are `None` (pane hidden, tab switched, or window minimized), `MacosCompositorTarget::update_viewport` calls `view.setHidden(true)` and zeros the frame. On Windows and Linux, `update_viewport` returns early without calling `ShowWindow(hwnd, SW_HIDE)` or `XUnmapWindow`, leaving orphaned native terminal child surfaces visible at their last geometry and occluding newly active tabs.
- **Fix**: In `WindowsCompositorTarget::update_viewport` (`src-tauri/src/native_terminal/platform/windows.rs`), call `ShowWindow(self.handle.hwnd.get() as Hwnd, SW_HIDE)` when `bounds` is `None`. In `LinuxCompositorTarget::update_viewport` (`src-tauri/src/native_terminal/platform/linux.rs`), call `XUnmapWindow` for X11 or detach the subsurface buffer for Wayland when `bounds` is `None`.
- **Status**: OPEN

### Native terminal scroll wheel monitor exists only on macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-7
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/lib.rs:862` - `            install_macos_terminal_scroll_monitor(app, Arc::clone(&scroll_daemon_client))?;`
- **Why it breaks**: macOS intercepts wheel events via `install_macos_terminal_scroll_monitor` to translate mouse wheel delta into PTY arrow-key escapes or mouse tracking sequences in alternate screen mode (e.g., inside `vim` or `less`). On Windows and Linux, no native scroll monitor exists, forcing scrolling through the webview React `onWheel` handler which ignores mouse cursor coordinates and cannot natively drive curses alternate-screen scrolling.
- **Fix**: Implement `install_windows_terminal_scroll_monitor` via a low-level mouse hook (`WH_MOUSE_LL` on `WM_MOUSEWHEEL`) in `windows_focus.rs` and equivalent GDK/X11 event filtering on Linux, invoking `compute_wheel_outcome` and writing PTY sequences when alternate screen mode is active.
- **Status**: OPEN

### First responder focus restoration after frame presentation is stubbed on non-macOS
- **ID**: L3-NATIVE-SURFACE-8
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/mod.rs:116` - `    pub fn restore_first_responder<R: Runtime>(&self, _window: &WebviewWindow<R>) {`
- **Why it breaks**: After every rendered frame, `surface_host.rs` calls `self.target.restore_first_responder(window)` to ensure keyboard focus remains with the hosting webview. In `platform/mod.rs`, `restore_first_responder` is compiled as a no-op on Windows and Linux, preventing keyboard focus recovery after native surface updates or window activations.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, implement `restore_webview_first_responder` by invoking `windows_focus::best_effort_focus_webview` to set Win32 focus to the WebView2 `Chrome_WidgetWin_1` window, and wire it to `PlatformCompositorTarget::restore_first_responder`.
- **Status**: OPEN

### Native terminal mouse focus monitor installed on macOS and Windows but absent on Linux
- **ID**: L3-NATIVE-SURFACE-9
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/lib.rs:864` - `            install_windows_terminal_focus_monitor(app)?;`
- **Why it breaks**: Both macOS (`install_macos_terminal_focus_monitor`) and Windows (`install_windows_terminal_focus_monitor`) install low-level mouse monitors to detect clicks over native surfaces and emit `native_terminal_focus` to the frontend. Linux has no terminal focus monitor, so clicking over an inactive terminal pane fails to trigger the `native_terminal_focus` IPC event to activate the hidden textarea focus sink.
- **Fix**: Implement `install_linux_terminal_focus_monitor` using GTK event controllers (`GtkGestureClick`) or X11/Wayland pointer event hooks on the parent window to detect button-up events over native terminal bounds and emit `NATIVE_TERMINAL_FOCUS_EVENT`.
- **Status**: OPEN

### Windows child HWND raised to HWND_TOP occludes HTML DOM overlays
- **ID**: L3-NATIVE-SURFACE-10
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/platform/windows.rs:325` - `                HWND_TOP as Hwnd,`
- **Why it breaks**: In `WindowsCompositorTarget::reveal`, the child HWND is unconditionally raised to `HWND_TOP` above WebView2 because WebView2 otherwise occludes the child. Because the terminal HWND sits above the webview in the Win32 window hierarchy, any overlapping DOM elements (popovers, toast notifications, autocomplete dialogs, command palettes) are occluded and hidden by the native terminal surface.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, replace top-level Win32 `WS_CHILD` window parenting with DirectComposition visual tree embedding or host WebView2 with transparency and order the terminal HWND below the WebView2 HWND via `WS_CLIPCHILDREN`/`SetWindowPos` relative to the child hierarchy.
- **Status**: OPEN

### Wayland subsurface geometry rounds fractional scale factors to integer, causing rendering distortion
- **ID**: L3-NATIVE-SURFACE-11
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/child_surface.rs:72` - `        let buffer_scale = bounds.scale_factor.round().max(1.0);`
- **Why it breaks**: `WaylandSubsurfaceGeometry::from_logical_bounds` rounds `bounds.scale_factor` to the nearest integer because `wl_surface.set_buffer_scale` only accepts integers. On Wayland environments using fractional display scaling (e.g., 125% or 150%), rounding forces the terminal buffer to render at 1x or 2x, resulting in mis-scaled, blurred, or truncated terminal surfaces relative to the parent GTK window.
- **Fix**: Bind the `wp_fractional_scale_manager_v1` and `wp_viewport` protocols in `wayland_child.rs` (`src-tauri/src/native_terminal/platform/wayland_child.rs`), allowing fractional buffer scaling and explicit source/destination viewport rectangle configuration.
- **Status**: OPEN

### Natural text editing chords intercept Windows key on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-12
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/input.rs:114` - `    let super_only = mods.super_key && !mods.alt;`
- **Why it breaks**: `natural_text_editing_bytes` translates `super_key` chords (intended for macOS Cmd+Left/Right/Backspace) into readline control bytes (`\x01`, `\x05`, `\x15`). On Windows and Linux, `super_key` represents the Windows/Super key; pressing Win+Arrow (system window snapping) or Win+Backspace injects control characters into the terminal session instead of being handled by the operating system.
- **Fix**: Gate the `super_only` branch of `natural_text_editing_bytes` in `src-tauri/src/native_terminal/input.rs` with `#[cfg(target_os = "macos")]` (or runtime platform check), and add `#[cfg(not(target_os = "macos"))]` mappings that translate Ctrl+ArrowLeft / Ctrl+ArrowRight to word-movement sequences (`\x1bb`, `\x1bf`).
- **Status**: OPEN

### `window_backing_scale_factor` capability only implemented on macOS, missing on Windows and Linux
- **ID**: L3-NATIVE-SURFACE-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/mod.rs:59` - `    #[cfg(target_os = "macos")]`
- **Why it breaks**: `PlatformCompositorTarget` exposes `window_backing_scale_factor()` exclusively under `#[cfg(target_os = "macos")]`. On Windows and Linux, callers cannot query the hardware DPI scale factor through the compositor target, creating an API asymmetry that prevents backend platform-level scale factor queries.
- **Fix**: In `src-tauri/src/native_terminal/platform/windows.rs`, implement `window_backing_scale_factor` using `GetDpiForWindow(self.handle.hwnd.get() as Hwnd) as f64 / 96.0`, implement GTK window scale query on Linux in `linux.rs`, and expose them unconditionally in `PlatformCompositorTarget`.
- **Status**: OPEN

### Key encoder maps punctuation characters to unidentified with suppressed UTF-8 under Ctrl
- **ID**: L3-NATIVE-SURFACE-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/key_encoder.rs:156` - `            if !event.modifiers.ctrl && !event.modifiers.super_key {`
- **Why it breaks**: In `key_encoder.rs`, `map_key_code_to_c` maps all punctuation characters to `GHOSTTY_KEY_UNIDENTIFIED`. When `event.modifiers.ctrl` is active, the encoder suppresses UTF-8 fallback, preventing encoding of common control chords like `Ctrl+[` (Escape), `Ctrl+]`, and `Ctrl+\` when dispatched as `KeyCode::Character`.
- **Fix**: In `src-tauri/src/native_terminal/key_encoder.rs`, expand `map_key_code_to_c` to recognize ASCII punctuation (`'['`, `']'`, `'\\'`, `'/'`, etc.) and map them to their respective `GHOSTTY_KEY_*` constants, or allow ASCII control byte derivation when `modifiers.ctrl` is set on character keys.
- **Status**: OPEN

### Pixel-perfect nearest-neighbour scaling configuration exists only for macOS Metal layers
- **ID**: L3-NATIVE-SURFACE-15
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/platform/macos.rs:185` - `unsafe fn configure_terminal_layers(view: &AnyObject, scale_factor: f64) {`
- **Why it breaks**: On macOS, `configure_terminal_layers` traverses the layer hierarchy to configure `setContentsGravity: topLeft` and `setMagnificationFilter: nearest` on the CAMetalLayer, preventing bilinear blurring during fractional resizing. Windows and Linux lack equivalent swapchain or surface scaling configurations, allowing fractional layout dimensions to cause bilinear filtering softness.
- **Fix**: In `WindowsCompositorTarget` (`src-tauri/src/native_terminal/platform/windows.rs`) and `LinuxCompositorTarget` (`src-tauri/src/native_terminal/platform/linux.rs`), configure swapchain scaling modes or viewport snapping so native presentation avoids bilinear interpolation on fractional scale factors.
- **Status**: OPEN

### Drag-and-drop coordinate test suite is gated to macOS only (test-only defect)
- **ID**: L3-NATIVE-SURFACE-16
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/tests/native_terminal_drag_drop_coordinates.rs:6` - `#![cfg(all(feature = "native-terminal", target_os = "macos"))]`
- **Why it breaks**: (Test-only defect) The integration test for native terminal drag-and-drop coordinate resolution is gated with `#![cfg(all(feature = "native-terminal", target_os = "macos"))]`. The coordinate math and backing scale factor division tests are never executed on Windows or Linux CI runners, leaving non-macOS coordinate mapping logic unverified.
- **Fix**: Remove the top-level `#![cfg(target_os = "macos")]` gate from `src-tauri/tests/native_terminal_drag_drop_coordinates.rs` and isolate only the `FerryxNativeTerminalView` test behind `#[cfg(target_os = "macos")]`, allowing the platform-agnostic `logical_from_raw` tests to run on all platforms.
- **Status**: OPEN

### Physical KeyboardEvent.code KeyV/KeyC prioritized before layout-dependent key
- **ID**: L3-NATIVE-SURFACE-17
- **Severity**: MEDIUM
- **Platforms affected**: macOS
- **Evidence**: `ui/src/components/NativeTerminalPane.tsx:233` - `    return event.code === code;`
- **Why it breaks**: Non-Latin keyboard layouts (such as Korean 2-Set) emit localized characters (`key: "ㅍ"` for physical V, `key: "ㅊ"` for physical C), causing standard Cmd+V paste and Cmd+C copy shortcuts to fail when matching against layout-dependent `event.key`. Prioritizing `event.code` ("KeyV", "KeyC") before `event.key` resolves the physical shortcut regardless of active keyboard layout.
- **Fix**: Prioritize `event.code === code` before checking `event.key` in `isShortcutKey` (`ui/src/components/NativeTerminalPane.tsx:232-236`).
- **Status**: FIXED (implemented in `ui/src/components/NativeTerminalPane.tsx:232-236` and verified by tests in `ui/src/components/NativeTerminalPane.test.tsx:1754-1760`)

### Pointer-transparent child surface focus via non-consuming AppKit mouse event monitor
- **ID**: L3-NATIVE-SURFACE-18
- **Severity**: HIGH
- **Platforms affected**: macOS
- **Evidence**: `src-tauri/src/lib.rs:551` - `        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseUp, &block)`
- **Why it breaks**: Because `FerryxNativeTerminalView` overrides `hitTest:` to return `nil` for pointer transparency, AppKit does not naturally trigger standard window activation or webview first responder updates on child view clicks. Installing a non-consuming AppKit local event monitor for mouse-up events detects clicks over terminal surfaces and emits `native_terminal_focus` to the frontend to restore textarea sink focus.
- **Fix**: Install non-consuming `NSEvent::addLocalMonitorForEventsMatchingMask_handler` for `LeftMouseUp` in `install_macos_terminal_focus_monitor` (`src-tauri/src/lib.rs:513-558`).
- **Status**: FIXED (implemented in `src-tauri/src/lib.rs:513-558`)
