# Ferryx Branding & Icon Assets Specification

## 1. Canonical Branding Policy (STRICT STANDARD: Ferryx Dark Squircle Icon)

### Product Identity
* **Product Name**: **Ferryx** (`ferryx`)
* **Bundle Identifier**: `com.ferryx.app`
* **Core Philosophy**: Ultra-lightweight, high-performance Rust native workspace & AI agent launcher.

### Master Asset Standard
* **Asset Style**: **Ferryx Crab Dark Monochrome Squircle with 100% Transparent Outer Background**.
* **Master Vector Source**: `ui/src/assets/ferryx-icon.svg`
* **Master High-Res PNG**: `src-tauri/icons/icon.png` (1024x1024 RGBA canvas with centered 860x860 squircle and transparent `alpha=0` outer margins).
* **macOS Multi-Resolution Asset**: `src-tauri/icons/icon.icns` (~2.07 MB, generated with zero outer black padding).
* **Windows Multi-Resolution Asset**: `src-tauri/icons/icon.ico`.
* **Cross-Platform Suites**: Complete 52-icon suite in `src-tauri/icons/` (Android mipmap, iOS AppIcons, Windows Store logos).

### Web & PWA Synchronization (`ui/public/`)
* All web favicons (`favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`), touch icons (`apple-touch-icon.png`), and PWA icons (`icon-192.png`, `icon-512.png`, `pwa-*.png`) are strictly synchronized from the transparent-margin master icon.

### Strict Non-Reversion Rules
1. **NO OUTER BLACK BOX**: The canvas outside the rounded squircle corners must remain 100% transparent (`RGBA [0, 0, 0, 0]`).
2. **NO COLOR OVERWRITES**: Do not overwrite with red/blue colored assets.
3. **CANONICAL IDENTITY**: The application name is strictly **Ferryx** (`ferryx`).

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
3. `bun run --cwd ui test` (Exit 0, 38 test suites, 226 tests passing)
4. `bun run --cwd ui build` (Exit 0, TypeScript 0 errors, Vite production build)
