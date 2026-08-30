<div align="center">

<img src="site/public/ferryx-icon.png" alt="Ferryx" width="120" height="120" />

# Ferryx

### Parallel agentic development. Zero bloat.

Native Ghostty terminal engine, wgpu GPU rendering, and a headless Rust PTY daemon.
No Electron anywhere.

[**Download**](https://github.com/Indosaram/ferryx/releases/latest) &nbsp;·&nbsp;
[**Website**](https://indosaram.github.io/ferryx/) &nbsp;·&nbsp;
[**Docs**](https://indosaram.github.io/ferryx/docs/introduction/) &nbsp;·&nbsp;
[**Discord**](https://discord.gg/Z2hBkQEHUG)

</div>

---

## Download

| Platform | Package | |
| :--- | :--- | :--- |
| **macOS** | Universal DMG (Apple Silicon & Intel) | [`.dmg`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_universal.dmg) |
| **Windows** | NSIS installer (x64) | [`.exe`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_x64-setup.exe) |
| **Windows** | MSIX package, Store & sideloading | [`.msix`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_x64.msix) |
| **Linux** | Portable AppImage (x64) | [`.AppImage`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.AppImage) |
| **Linux** | Debian / Ubuntu package (x64) | [`.deb`](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.deb) |

Every link resolves against the latest release. Verify downloads with the `SHA256SUMS.txt`
published alongside the binaries:

```bash
sha256sum -c SHA256SUMS.txt
```

## Speed. Isolation. Total control.

Every layer is engineered for minimal latency and autonomous agent collaboration.

### Native Ghostty & wgpu engine

Desktop terminal panes render through native libghostty and a GPU-accelerated wgpu
pipeline, giving crisp font rasterization and low-latency throughput.

- libghostty terminal core
- wgpu GPU pipeline
- Offscreen WGPU render benchmark: 50-frame p50 3.10 ms on Apple M4 Max

### Multi-agent workspaces

Run parallel AI coding agents (Claude Code, Codex, Gemini CLI) in isolated split panes
with real-time status indicators.

- Isolated worktree per agent
- Live status indicators
- Launch straight from the tab bar

### Flexible split-pane tiling

Arbitrary vertical and horizontal splits with pointer-drag resizing and smooth layout
transitions.

- Vertical and horizontal splits
- Pointer drag resizing
- Drag tabs into any pane

### Mobile web pairing

Authenticated remote access over QR or PIN. Stream terminal output to a dependency-free
DOM terminal grid and steer agents from anywhere.

- 6-digit PIN pairing
- Streamed terminal grid
- Steer agents from a phone

### Zero Electron overhead

Tauri v2 with the platform's native WebKit or WebView2 engine, paired with a headless
Rust PTY daemon. The core runs on macOS, Windows, and Linux; selected OS integrations
such as launchd, Dock badges, color emoji, and vibrancy remain macOS-first.

- Tauri v2 shell
- Headless Rust PTY daemon
- macOS, Windows, Linux

### Resilient persistence

Workspace state snapshots automatically and the background daemon reattaches, so an
exit or crash never costs you work.

- Layout snapshots
- Daemon survives the GUI
- Reattach with output replay

## Built different

An architecture inventory of Ferryx's native Ghostty and Rust components. This is not a
performance benchmark against Electron-based AI IDEs or terminal emulators.

| Architecture component | Ferryx implementation |
| :--- | :--- |
| Terminal parser | libghostty-vt |
| Desktop rendering | WGPU native child surfaces |
| PTY lifecycle | Headless Rust daemon with output replay |
| Embedded browser | Native WebView split-tabs |
| Mobile pairing | PIN/QR gateway with a custom DOM terminal grid |
| Agent supervision | Manifest-driven status detection and notifications |

## Build from source

### Prerequisites

- [Bun](https://bun.sh/) 1.1+
- [Rust](https://www.rust-lang.org/) with Cargo 1.80+
- [Node.js](https://nodejs.org/) 20+
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)

### Clone and install

```bash
git clone https://github.com/Indosaram/ferryx.git
cd ferryx
bun install --cwd ui
bun install --cwd site
```

### Run the desktop app

The Tauri shell starts the UI dev server defined in `src-tauri/tauri.conf.json`:

```bash
cd src-tauri && cargo tauri dev
```

### Run the site and docs

Astro + Starlight on port 14173:

```bash
bun run --cwd site dev
```

- Landing page: <http://localhost:14173/>
- Documentation: <http://localhost:14173/docs/introduction/>
- Shortcut reference: <http://localhost:14173/docs/shortcuts/>

### Validate changes

```bash
bun run --cwd ui test
bun run --cwd site build
cd src-tauri && cargo check
```

## Architecture

```text
ferryx/
├── src-tauri/   # Rust Tauri v2 core: PTY daemon, IPC, notifications, remote gateway
├── ui/          # React frontend: native terminal surface, layout, remote pairing
├── site/        # Astro + Starlight landing page and docs, deployed to GitHub Pages
└── docs/        # Specifications, audit logs, and reference material
```

**Rust native core (`src-tauri`)** owns pseudoterminal lifecycles, manages Git worktree
leases, hosts native child webviews, and runs an authenticated Axum WebSocket gateway
for mobile pairing. Terminal sessions live in a headless daemon rather than the GUI, so
closing the window never touches a PTY. Output lands in a ring buffer with monotonic
sequence numbers, and reattaching replays from the last sequence.

**Frontend (`ui`)** is the React shell rendered inside WebKit or WebView2. It drives pane
tiling, agent title detection, keyboard shortcuts, and terminal search overlays.

**Site (`site`)** is the static Astro and Starlight build that hosts the landing page,
guides, and an embedded demo running the real product components.

## Contributing

Contributions are welcome. Branch from `main`, run the validation commands above, and
open a pull request describing the user-visible result.

Questions and ideas are welcome in [Discord](https://discord.gg/Z2hBkQEHUG).

## License

This project is licensed under the [MIT License](LICENSE).

## Code Signing Policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- **Maintainer**: [@Indosaram](https://github.com/Indosaram)
- **Privacy**: Ferryx does not collect, store, or transmit telemetry or personal data to remote servers.
