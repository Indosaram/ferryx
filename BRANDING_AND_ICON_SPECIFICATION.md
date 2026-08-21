# Rorca Branding & Icon Assets Specification

## 1. Canonical Branding & Transparent Icon Policy (STRICT - DO NOT REVERT)

### Master Asset Standard
* **Master Vector Source**: `ui/src/assets/rorca-icon.svg`
  - Pure **transparent background** with alpha channel (no opaque `<rect fill="#151921"/>` or stroke rectangles).
* **Generator Command**: `cargo tauri icon ui/src/assets/rorca-icon.svg`
  - Automatically generates all cross-platform icons with transparent alpha channels.
* **macOS Multi-Resolution Asset**: `src-tauri/icons/icon.icns` (Transparent background).
* **Windows Multi-Resolution Asset**: `src-tauri/icons/icon.ico` (Transparent background).
* **App Master PNG**: `src-tauri/icons/icon.png` (Transparent background, full alpha).

### Web & PWA Synchronization (`ui/public/`)
* All web favicons (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`), touch icons (`apple-touch-icon.png`), and PWA icons (`icon-192.png`, `icon-512.png`, `pwa-*.png`) must remain strictly synchronized with the transparent master asset.

### Forbidden Actions
1. **DO NOT ADD OPAQUE SQUIRCLES OR RECTANGLES** to `ui/src/assets/rorca-icon.svg`.
2. **DO NOT OVERWRITE WITH OPAQUE BLACK BOX ICONS**: All generated icons must preserve true PNG/ICNS alpha transparency.

---

## 2. Desktop Vite + Tauri HMR Topology (Single Loopback Endpoint)

* **Host Binding**: Strictly `127.0.0.1` (IPv4 loopback). Avoid ambiguous `localhost` or IPv6 `::1`.
* **Port**: Single unified port `5173` with `strictPort: true` for both HTTP dev server and WebSocket HMR (`ws://127.0.0.1:5173`).
* **Watcher**: Polling watcher enabled (`usePolling: true, interval: 100`) in `ui/vite.config.ts` to prevent macOS FSEvents event-drop during atomic file saves.
* **Child Webview Isolation**: `BrowserPane` must hide its native child webview on unmount/cleanup to prevent covering React Fast Refresh updates.

---

## 3. TabBar & NewTabPopover UX Specification

* **New Tab Button Layout**: The `+` button and `NewTabPopover` must always be positioned immediately adjacent to the right edge of the last tab in the scrollable tab list (`ui/src/components/TabBar.tsx`), never pushed to the far right screen edge.
* **Clean Lucide Icons Only**: No AI-generated or slop decoration icons. Standard Lucide tokens (`TerminalSquare`, `Globe`, `Plus`, `X`) only.
* **Shortcuts**: `⌘T` / `Ctrl+T` for New Terminal, `⌘⇧B` / `Ctrl+Shift+B` for New Browser Tab.

---

## 4. Verification Gates (Must Pass 4/4 Before Any Commit)

1. `cargo test --manifest-path src-tauri/Cargo.toml` (Exit 0)
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` (Exit 0, 0 warnings)
3. `bun run --cwd ui test` (Exit 0, 38 test suites, 222 tests passing)
4. `bun run --cwd ui build` (Exit 0, TypeScript 0 errors, Vite production build)
