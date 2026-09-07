# Code Review: Native Terminal Below Ordering (`NSWindowOrderingMode::Below`)

- **Date**: 2026-09-07
- **Target**: `src-tauri/src/native_terminal/platform/macos.rs`
- **Scope**: Reordering macOS native child NSView relative to WKWebView to resolve DOM overlay occlusion (update notification clipping).
- **Reviewer**: Senior Engineering Review

---

## 1. Executive Summary

- **Verdict**: APPROVED with architectural notes
- **Confidence**: HIGH
- **Assessment**: The one-line change in `src-tauri/src/native_terminal/platform/macos.rs` changing `NSWindowOrderingMode::Above` to `NSWindowOrderingMode::Below` directly resolves the fundamental AppKit "airspace" problem on macOS. By placing the native terminal child NSView behind `WKWebView`, DOM overlays with opaque backgrounds (such as the update notification Sonner toast at `bottom-right`) naturally render on the topmost visual layer without being clipped by the terminal canvas. Existing transparency CSS rules (`html.platform-macos:has([data-testid="native-terminal-pane"]) ... transparent !important`) and `"transparent": true` window configuration support this layer inversion seamlessly.

---

## 2. Detailed Technical Audit

### 2.1 View Hierarchy & Airspace (Z-Ordering)
- **Previous State**:
  - `content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None)`
  - `FerryxNativeTerminalView` was parented on top of `WKWebView`.
  - Result: Any DOM element in `WKWebView` (including `<Toaster />`) was drawn underneath the opaque terminal NSView. The update toast at `bottom-right` had ~80% of its body occluded, showing only the ~74px slice containing the "Update" action button extending outside the terminal surface.
- **Current State**:
  - `content_view.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None)`
  - `FerryxNativeTerminalView` is parented underneath `WKWebView`.
  - Result: `WKWebView` is on top. Over terminal panes, DOM background is transparent (`index.css`), allowing the Metal/WGPU terminal frame to show through with full fidelity. Overlays with opaque backgrounds (`var(--popover)` `#171717`) render cleanly on top.

### 2.2 Event Routing & First Responder
- **Hit Testing**:
  - When `view` was `Above`, `hit_test` returning `nil` was mandatory so mouse clicks fell through to `WKWebView`.
  - With `view` placed `Below`, `WKWebView` is the topmost subview and naturally intercepts all pointer events first. Clicks on DOM buttons (e.g. "Update", drag handles, close buttons) trigger without interference. Clicks on the terminal canvas hit the transparent DOM textarea/input-sink, preserving terminal focus routing.
- **First Responder**:
  - Previously, child view presentation could steal first responder from `WKWebView`, requiring defensive recovery via `restore_webview_first_responder`.
  - Under `Below` ordering, `WKWebView` is frontmost in the responder chain, reducing first-responder churn.

### 2.3 Cross-Platform Isolation
- The change is strictly isolated to `src-tauri/src/native_terminal/platform/macos.rs`, compiled only under `#[cfg(target_os = "macos")]`.
- Windows (`platform/windows.rs`) and Linux (`platform/linux.rs`) are completely unaffected.
- Note for Windows: WebView2 uses child HWNDs where transparency behind the HWND is not supported by default. Future cross-platform overlay parity on Windows requires DirectComposition (`ICoreWebView2CompositionController`), as previously documented in `docs/native-terminal-overlay-occlusion-solutions.md`.

### 2.4 Rendering & Visual Quality
- `layer.setOpaque: true` and `CompositeAlphaMode::Opaque` remain intact on the Metal layer.
- CoreAnimation composites the opaque Metal layer beneath the transparent parts of `WKWebView`.
- No font smoothing, subpixel AA, or gamma discrepancies were introduced.

---

## 3. Verification Evidence

- `cargo check --manifest-path src-tauri/Cargo.toml`: 0 errors
- `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`: 18/18 PASS
- `node node_modules/vitest/vitest.mjs run src/components/ui/sonner.test.tsx src/lib/updateToast.test.ts`: 12/12 PASS
- `node node_modules/vitest/vitest.mjs run src/components/NativeTerminalPane.test.tsx`: 142/142 PASS
- `bun run --cwd ui build`: Clean build (2.65s)

---

## 4. Risks & Mitigations

- **Risk**: A parent DOM element accidentally regaining an opaque background could obscure the terminal underneath.
  - **Mitigation**: Scoped CSS in `ui/src/index.css` (`html.platform-macos:has([data-testid="native-terminal-pane"])`) comprehensively marks every ancestor container down to `native-terminal-pane` as `background: transparent !important`.
- **Risk**: Multi-pane split terminals creating multiple child views.
  - **Mitigation**: Each pane targets `NSWindowOrderingMode::Below`, so all child terminal NSViews remain underneath the single `WKWebView`. Verified by `native_terminal_hit_test_maps_exact_split_coordinates_with_half_open_boundaries` test.
