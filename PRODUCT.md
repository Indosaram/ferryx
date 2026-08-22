# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

AI engineers, autonomous agent developers, and systems programmers who orchestrate multiple coding agents (Claude Code, Codex, Gemini Flash, etc.) concurrently and need instant, reliable terminal/browser workspaces across desktop and remote mobile devices.

## Product Purpose

Ferryx delivers an ultra-lightweight, high-performance workspace engineered for parallel agentic AI development. It eliminates the heavy multi-gigabyte memory bloat and sluggish startup times of traditional Electron IDEs while providing native split-tiling, embedded browser companions, background daemon persistence, and secure remote mobile web browser access.

## Positioning

A zero-Electron, Rust-native PTY daemon and workspace launcher that launches in under 150ms with less than 85MB of base RAM, supporting native desktop apps alongside pairing-authenticated mobile web browser remote control over local network or Tailscale.

## Operating Context

- **Desktop Host**: Rust Tauri v2 desktop application (`src-tauri` + `ui`) managing native PTY streams, workspace layouts, and embedded Webview2/WebKit child views.
- **Mobile Remote**: Secure web app interface accessible via mobile phone/tablet browser with pairing code authentication, live terminal output mirroring, and remote agent control.
- **Marketing & Docs**: Standalone Bun + React + Tailwind landing page and documentation (`site/`) hosted on GitHub Pages.

## Capabilities and Constraints

- **Native Rust PTY Host**: Direct OS pseudoterminal allocation with sub-millisecond I/O latency and persistent session lifecycles.
- **Multi-Agent Workspace**: Isolated tabs and split-panes with real-time agent status classification (working, waiting, done, error).
- **Mobile Web Remote Connection**: Built-in HTTP/WebSocket remote server with cryptographic pairing (`RemoteClient`, `PairingPage`, `RemoteSessionList`) enabling full mobile browser interaction.
- **Embedded Browser Companion**: Side-by-side webview tabs connected to local dev server loopback with Fast Refresh support.
- **Session Persistence**: Durable workspace layout serialization and background daemon reattachment across window reloads or crashes.

## Brand Commitments

- **Product Name**: Strictly **Ferryx** (`ferryx`, `com.ferryx.app`).
- **Visual Identity**: Dark monochrome squircle master icon with 100% transparent outer margins (`ui/src/assets/ferryx-icon.svg`, `src-tauri/icons/icon.png`).
- **Aesthetic Policy**: Dark modern developer workspace theme. Zero colored alterations, no outer black box padding, and no AI-generated slop decorations.

## Evidence on Hand

- `BRANDING_AND_ICON_SPECIFICATION.md`: Authoritative specification for icons, Tauri loopback HMR, TabBar UX, and verification gates.
- `src-tauri/`: Rust backend with portable-pty daemon, IPC controllers, notifications, and remote server.
- `ui/`: React 18 + Vite frontend with xterm.js WebGL terminals, workspace state store, and remote pairing UI (`ui/src/remote/`).
- `site/`: Marketing landing page and showcase built with Bun and Tailwind.
- `README.md`: Developer-focused architecture and quickstart guide.

## Product Principles

1. **Sub-Millisecond Responsiveness**: Prioritize raw execution speed, instant typing response, and minimal latency across all terminals and IPC channels.
2. **Zero Bloat & Tiny Footprint**: Maintain under 100MB idle RAM consumption and instant cold startup by avoiding bloated Electron runtimes.
3. **Seamless Multi-Device Continuity**: Allow developers to monitor and steer running desktop agents from mobile web browsers without disruption.
4. **Resilient Session Ownership**: Never lose running terminal jobs or process output during app restarts, crashes, or network disconnects.
