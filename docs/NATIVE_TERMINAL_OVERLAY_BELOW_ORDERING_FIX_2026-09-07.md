# Native Terminal Overlay Below Ordering Fix (2026-09-07)

## Problem Summary
When update notifications (Sonner toasts at `bottom-right`) or other DOM overlays are rendered, they were partially occluded or clipped by the macOS native terminal surface (`FerryxNativeTerminalView`).
Specifically, the toast was positioned at `bottom: 24px; right: 24px;` in the DOM with a width of ~356px, but only the rightmost ~74px (the "Update" action button) was visible outside the terminal surface boundary, while the entire left portion (title, description, icon) was covered by the opaque terminal canvas.

## Root Cause
In `src-tauri/src/native_terminal/platform/macos.rs` (line 272):
```rust
content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
```
The AppKit child NSView (`FerryxNativeTerminalView`) was explicitly parented **above** all sibling views, including the `WKWebView`. Because the native terminal layer is completely opaque, any DOM element inside the underlying `WKWebView` was painted underneath the terminal, making CSS `z-index` completely ineffective (the AppKit "airspace" problem).

## Solution
Change the macOS child view ordering in `src-tauri/src/native_terminal/platform/macos.rs` from `NSWindowOrderingMode::Above` to `NSWindowOrderingMode::Below`:
```rust
content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
```

### Why this works:
1. `tauri.conf.json` already sets `"transparent": true` on the main window.
2. `ui/src/index.css` (commit `75c4d36`) already scopes `background: transparent !important` to `html.platform-macos:has([data-testid="native-terminal-pane"])` and its entire ancestor DOM chain down to the terminal pane surface.
3. With `WKWebView` placed above the native terminal view, the native terminal is visible through the transparent DOM, while DOM elements with opaque backgrounds (such as `<Toaster />` with `--normal-bg: var(--popover)`) naturally render **on top of** the native terminal.
4. `TabGroupDropSurface` (`[data-testid="tab-group-body"]`, `[data-tab-group-body-id]`, `[data-dnd-type="group-body"]`) is also explicitly included in the `html.platform-macos` transparency rule so its `bg-terminal` (`#282c34`) does not mask the native terminal canvas underneath.
5. The toast stays in its canonical `bottom-right` position and renders on the topmost visual layer without being clipped by the terminal.

## Verification Evidence
- `cargo check --manifest-path src-tauri/Cargo.toml`: exit code 0
- `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`: 18/18 passed
- `cd ui && node node_modules/vitest/vitest.mjs run src/components/ui/sonner.test.tsx src/lib/updateToast.test.ts`: 12/12 passed
- `cd ui && node node_modules/vitest/vitest.mjs run src/components/NativeTerminalPane.test.tsx`: 142/142 passed
- `bun run --cwd ui build`: built in 2.65s (exit code 0)
