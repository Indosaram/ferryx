# Audit: bundle
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/vite.config.ts, ui/index.html, ui/src/main.tsx, ui/src/index.css, ui/package.json, HMR_ROOT_CAUSE_AND_FIX.md, ui/src/assets/ferryx-icon.test.ts, ui/src/test/viteHmrConfig.test.ts
Date: 2026-08-22

## Findings
### F-bundle-01
- Severity: Medium
- File: ui/src/main.tsx:3
- Mechanism: Static top-level import of both `App` and `RemoteApp` in `main.tsx` causes all desktop components and remote client components to be compiled into a single initial JavaScript bundle (`index-*.js`, 490.8 KB), forcing unnecessary JS parse/eval during Tauri app startup.
- Hot path: yes
- Suggested fix: Dynamically import `RemoteApp` (and/or `App`) using `React.lazy` or conditional dynamic `import()` so the Tauri desktop build does not eagerly evaluate unused remote client modules.
- Write scope: ui/src/main.tsx
- RED proof: `import App from "./App";\nimport { RemoteApp } from "./remote/RemoteApp";\n... {isTauriApp ? <App /> : <RemoteApp />}` bundles both execution paths synchronously into the root entry chunk regardless of runtime environment.

### F-bundle-02
- Severity: Low
- File: ui/index.html:4
- Mechanism: `<link rel="icon" type="image/svg+xml" href="/src/assets/ferryx-icon.svg" />` references a non-existent asset file (`ferryx-icon.svg`), generating a 404 network request during webview/browser initialization.
- Hot path: yes
- Suggested fix: Update the favicon `<link>` href to reference an existing icon asset or remove the dead SVG link tag.
- Write scope: ui/index.html
- RED proof: `<link rel="icon" type="image/svg+xml" href="/src/assets/ferryx-icon.svg" />` in `ui/index.html` fails to resolve because `ui/src/assets` only contains `ferryx-icon.png`, `geist-variable.woff2`, and `orca-logo.svg`.

## Non-findings / accepted
- Vite dev polling (`server.watch.usePolling`): `ui/vite.config.ts` specifies `server.watch.usePolling: true` with a 100ms interval for reliable file watching during atomic file replacement on macOS development (as detailed in `HMR_ROOT_CAUSE_AND_FIX.md`). It is confined to the `server` development block and does not leak into the production build bundle.
- xterm and terminal addon code-splitting: `xterm` (332.6 KB) and addons (`addon-webgl` 112.0 KB, `addon-search` 32.5 KB, `addon-unicode11` 25.1 KB, `addon-fit` 1.6 KB) are cleanly separated into dynamic chunk modules rather than loaded synchronously in the initial bundle.
- CSS layout performance: `ui/src/index.css` contains zero transitions or animations affecting layout-triggering properties (`width`, `height`, `top`, `left`, `margin`, `padding`).
- Font delivery: `Geist` font is served as a local variable WOFF2 font (`ui/src/assets/geist-variable.woff2`, 69.4 KB) with `font-display: swap`, avoiding external network roundtrips.
- Service worker registration: In `ui/src/main.tsx`, `navigator.serviceWorker.register` is explicitly deferred to `window.addEventListener("load")` and gated with `!isTauriApp`, avoiding service worker overhead in the desktop Tauri webview.
- Public asset sizes: `ui/public/ferryx-icon.png` (805.8 KB) and `ui/public/icon-512.png` (258.7 KB) are static standalone PWA/manifest icons served on-demand and not bundled into the JS bundle.

## Scan coverage
- files read: ui/vite.config.ts, ui/index.html, ui/src/main.tsx, ui/src/index.css, ui/package.json, HMR_ROOT_CAUSE_AND_FIX.md, ui/src/assets/ferryx-icon.test.ts, ui/src/test/viteHmrConfig.test.ts
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
