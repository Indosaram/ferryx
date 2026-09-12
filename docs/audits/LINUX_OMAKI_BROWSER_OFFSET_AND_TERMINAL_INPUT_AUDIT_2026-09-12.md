# Linux Omaki Browser Offset, Terminal Black Screen & Sidebar Alignment Audit

**Date:** 2026-09-12
**Target:** omaki (Arch Linux, Hyprland, Wayland, GDK_SCALE=2)

## 1. Executive Summary

This investigation identified and resolved three core issues observed on the Linux `omaki` workstation:
1. **Browser Tab GTK Child Webview Offset Displacement & Window Squeeze**: When opening a browser tab, the main Ferryx application was squeezed into the top 50% of the window while the browser webview occupied the bottom 50%, with split and drag positioning completely ignored.
2. **Terminal Tab Black Screen & Input Transmission Failure (`Failed to send terminal input`)**: Switching tabs left terminal sessions detached and screens black, with keystrokes failing under an unrecoverable `Failed to send terminal input` error banner.
3. **Sidebar Header Action Icon Alignment**: The collapse button on the far-left shifted the host/project title away from the left margin, breaking vertical alignment with folder icons and tree items below.

---

## 2. Issue 1: Browser GTK Child Webview Displacement & Window Squeeze

### Root Cause
In Tauri v2 on Linux (GTK), child webviews created via `Window::add_child` are directly packed into the window's vertical `GtkBox` (`default_vbox()`). Because both the main webview and child webviews are packed into `default_vbox` with `expand=true, fill=true`, GTK divides the vertical window geometry equally (50% Ferryx UI on top, 50% child webview on bottom).
Furthermore, in `wry`'s WebKitGTK implementation, `WebView::set_bounds` checks `if self.is_in_fixed_parent`. Because the webview was placed in a `GtkBox`, `is_in_fixed_parent` is `false`, rendering all subsequent `set_bounds` calls silent no-ops.

### Fundamental Fix
1. Created `src-tauri/src/browser/linux.rs` implementing `LinuxBrowserOverlay`:
   - Wraps the main WebKitGTK application webview in a `gtk::Overlay` as the base child.
   - Adds a `gtk::Fixed` layer as the overlay container (`set_overlay_pass_through(&fixed, true)`).
   - Keeps `default_vbox` holding only `overlay` (100% full window dimensions, eliminating the 50:50 squeeze and dark void).
2. When a child browser webview is created:
   - Removes the child widget from `default_vbox`.
   - Places it into the `gtk::Fixed` layer at the exact logical bounds `(x, y)` with size `(width, height)`.
3. In `cmd_browser_set_bounds`:
   - Invokes `container.fixed.move_(widget, x, y)` and `widget.set_size_request(w, h)` to provide real-time repositioning during pane resizing and window layout changes.
4. In `cmd_browser_set_visible` and `cmd_browser_close`:
   - Manages widget visibility (`show()` / `hide()`) and detaches widgets from `gtk::Fixed` on session close.

---

## 3. Issue 2: Terminal Black Screen & Input Transmission Failure

### Root Cause
1. **Error Code Downgrade & Recovery Block**: In `src-tauri/src/ipc/native_terminal.rs`, `require_attached_surface` mapped `Err(NativeTerminalError::SessionDetached(_))` to `Err(NativeTerminalError::NoValue)`. Converting `NoValue` to `IpcError` yielded `INTERNAL_ERROR: "No value"`. The frontend's `classifyNativeTerminalAttachError` classified this as an unrecoverable `operational-error` instead of `needs-reattach` (which requires `SESSION_NOT_FOUND`). Consequently, `NativeTerminalPane.tsx` displayed `Failed to send terminal input` without attempting automatic reattachment.
2. **Missing Surface Host in Scheduled Render**: When a tab was detached, `detach_session` emptied `self.hosts`. Upon tab re-selection, `reattach_existing_session_with_bounds` marked the session attached but did not repopulate `self.hosts`. When the background pump task triggered `dispatch_scheduled_render`, `hosts.get_mut(&session_id)` returned `None`, silently discarding the frame and leaving the terminal screen permanently black.
3. **X11 Window Stacking**: In `src-tauri/src/native_terminal/platform/linux.rs`, `X11Child` mapped the terminal subwindow without calling `XRaiseWindow`, allowing sibling GTK drawing areas to occlude the native terminal view.

### Fundamental Fix
1. **Preserve `SessionDetached`**: Updated `require_attached_surface` in `src-tauri/src/ipc/native_terminal.rs` to return `state.ensure_surface_attached(session_id)` directly. When detached, it emits `IpcErrorCode::SessionNotFound`, allowing `NativeTerminalPane.tsx` to automatically reattach and replay input.
2. **Lazy Surface Host Construction & Wayland Geometry Normalization**: In `src-tauri/src/native_terminal/surface_host.rs`, updated `dispatch_scheduled_render` to lazily instantiate `NativeTerminalSurfaceHost::new` via `hosts_guard.entry(session_id)` whenever an attached session with valid layout and bounds exists. Crucially, the bounds are resolved against `active_presentation_geometry().resolve(logical_bounds)` to enforce Wayland `buffer_scale` divisibility (e.g. 500.5pt at 2x scaling into 1002px rather than an invalid 1001px buffer) and prevent `invalid_size` subsurface commit crashes.
3. **Subwindow Elevation**: Added `XRaiseWindow` calls to `X11Child` in `reveal` and `update_viewport` within `src-tauri/src/native_terminal/platform/linux.rs` to ensure the terminal surface sits on top of parent window hierarchy.

---

## 4. Issue 3: Sidebar Header Action Icon Alignment

### Root Cause
In `ui/src/components/Sidebar.tsx`, `[PanelLeftClose]` ("Hide sidebar") was rendered on the far left before `[RemoteHostSwitcher]` (`[💻 omaki]`). On Linux and Windows where no macOS traffic light pads exist, this pushed `[💻 omaki]` into the middle of the titlebar, breaking vertical alignment with folder icons and tree items below.

### Fundamental Fix
Reordered `Sidebar.tsx` header:
1. `RemoteHostSwitcher` rendered on the left (`min-w-0 flex-1`), immediately aligning with project rows and folder trees below.
2. Action icons grouped neatly on the right: `[Plus]` ("Add project") followed by `[PanelLeftClose]` ("Hide sidebar").

---

## 5. Verification Matrix

| Area | Test Suite | Result |
|---|---|---|
| Frontend Sidebar | `bun run --cwd ui test src/components/Sidebar.test.tsx` | 37/37 PASSED |
| Frontend Build | `bun run --cwd ui build` | PASSED (3.36s) |
| Backend Native Terminal IPC | `cargo test --lib ipc::native_terminal` | 19/19 PASSED |
| Backend Surface Host | `cargo test --lib native_terminal::surface_host` | 47/47 PASSED |
| Backend Browser IPC | `cargo test --lib ipc::browser` | 19/20 PASSED (1 xdg-open env skip) |
| Linux Target Compilation | `cargo check --target x86_64-unknown-linux-gnu` | 0 errors |
| Linux Debug Binary Build | `cargo build --target x86_64-unknown-linux-gnu` | Complete |
