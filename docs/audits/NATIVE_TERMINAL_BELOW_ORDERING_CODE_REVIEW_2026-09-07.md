# Code Review: Native Terminal Below Ordering & DOM Transparency Layering

- **Date**: 2026-09-07 (Re-review after transparency remediation)
- **Commits**: `1e97568` + `dc9e8e0` + `0d102da`
- **Scope**:
  - `src-tauri/src/native_terminal/platform/macos.rs` (NSView Z-ordering)
  - `ui/src/index.css` (macOS DOM transparency rules)
  - `ui/src/components/tab-dnd/TabGroupDropSurface.tsx` (Drop surface identifier)
  - `ui/src/nativeTerminalPlatformTransparency.test.ts` (Transparency regression contract)
- **Reviewer**: Senior Engineering Review

---

## 1. Executive Summary

- **Verdict**: APPROVED
- **Confidence**: HIGH
- **Assessment**: The initial change to `NSWindowOrderingMode::Below` placed `FerryxNativeTerminalView` underneath `WKWebView`, which successfully solved the airspace clipping for DOM overlays (toasts). However, because `TabGroupDropSurface` (`ui/src/components/tab-dnd/TabGroupDropSurface.tsx`) possessed an opaque `bg-terminal` (`#282c34`) and was missing from the `html.platform-macos` transparency list in `ui/src/index.css`, WebKit painted an opaque `#282c34` rectangle over the terminal area, obscuring the underlying terminal text. Commit `dc9e8e0` added `data-testid="tab-group-body"` and matching selectors (`[data-testid="tab-group-body"]`, `[data-tab-group-body-id]`, `[data-dnd-type="group-body"]`) to the `index.css` transparency rules. This completes the transparent pipeline from `#root` down to `native-terminal-pane`, allowing the native terminal text and cursor to show through completely, while DOM overlays (such as the Sonner update toast with `bg-popover`) render cleanly on top without clipping.

---

## 2. Layer-by-Layer Architectural Audit

### 2.1 Complete Transparency Hierarchy (`html` to `native-terminal-pane`)
Every DOM node in the ancestor chain between the root and the native terminal pane must be transparent for the native terminal to show through:

1. `html.platform-macos:has([data-testid="native-terminal-pane"])` -> `transparent !important` (in `index.css`)
2. `body:has(...)` -> `transparent !important` (in `index.css`)
3. `#root:has(...)` -> `transparent !important` (in `index.css`)
4. `#root > div:has(...)` -> `transparent !important` (in `index.css`)
5. `div:has(> main ...)` -> `transparent !important` (in `index.css`)
6. `main:has(...)` -> `transparent !important` (in `index.css`)
7. `[data-testid="terminal-layout"]:has(...)` -> `transparent !important` (in `index.css`)
8. `[data-testid="tab-group-split"]:has(...)` -> `transparent !important` (in `index.css`)
9. `[data-testid="tab-group-panel"]:has(...)` -> `transparent !important` (in `index.css`)
10. `[data-testid="tab-group-body"]:has(...)` / `[data-tab-group-body-id]` / `[data-dnd-type="group-body"]` -> **`transparent !important` (Added in `dc9e8e0`)**
11. `[data-testid="pane-split"]:has(...)` -> `transparent !important` (in `index.css`)
12. `[data-testid="pane-leaf"]:has(...)` -> `transparent !important` (in `index.css`)
13. `div.h-full.w-full.min-h-0.flex-1.overflow-hidden` -> No background class (inherits transparency)
14. `[data-testid="terminal-pane-surface"]:has(...)` -> `transparent !important` (in `index.css`)
15. `[data-testid="native-terminal-pane"]` -> `transparent !important` (in `index.css`)

**Audit Finding**: With `tab-group-body` now included, there are no remaining opaque nodes in the chain. The terminal canvas renders without obstruction.

### 2.2 Overlay Rendering & Z-Ordering
- **Sonner Toaster (`<Toaster />`)**:
  - Mounted directly under `#root > div`.
  - Uses fixed positioning (`bottom-right`, `bottom: 24px; right: 24px;`).
  - Has explicit opaque background (`--normal-bg: var(--popover)`, `#171717`).
  - Since `WKWebView` is parented above `FerryxNativeTerminalView`, the toast is painted on top of the native terminal surface.
  - No clipping occurs on either axis.

### 2.3 Event Routing & Pointer Transparency
- **DOM Overlay Interaction**: Clicks on toast buttons ("Update", "Dismiss") are intercepted by `WKWebView` DOM elements immediately.
- **Terminal Input Interaction**: Clicks on the terminal canvas pass through the transparent DOM elements into `NativeTerminalPane`'s focus sink (`textarea`), which receives focus and dispatches input to the PTY via IPC.
- **AppKit Level**: `FerryxNativeTerminalView` overrides `hitTest:` returning `nil`, ensuring fallback transparency.

### 2.4 Cross-Platform Safety
- Windows (`platform/windows.rs`) and Linux (`platform/linux.rs`) remain completely untouched.
- `index.css` transparency rules remain strictly scoped to `html.platform-macos`, ensuring Windows WebView2 does not encounter black screen regressions (guarding commit `75c4d36` / `43071bc`).
- Unit test `ui/src/nativeTerminalPlatformTransparency.test.ts` enforces the platform scoping contract.

---

## 3. Verification Evidence

- `cargo check --manifest-path src-tauri/Cargo.toml`: 0 errors
- `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`: 18/18 PASS
- `cd ui && node node_modules/vitest/vitest.mjs run src/nativeTerminalPlatformTransparency.test.ts src/components/ui/sonner.test.tsx src/lib/updateToast.test.ts`: 13/13 PASS
- `cd ui && node node_modules/vitest/vitest.mjs run src/components/NativeTerminalPane.test.tsx`: 142/142 PASS
- `bun run --cwd ui build`: Clean build (`tsc && vite build`) in 3.18s
