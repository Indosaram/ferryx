# Ferryx

Ferryx is an ultra-lightweight, native workspace for terminal-based development and AI-assisted workflows. It combines a high-throughput Rust pseudoterminal daemon with Tauri v2, flexible split-pane tiling, embedded web companion tabs, and pairing-authenticated mobile remote control.

## Web and Documentation

GitHub Pages deployment is configured in [`.github/workflows/deploy-pages.yml`](./.github/workflows/deploy-pages.yml). After a successful Pages workflow run on `main`, the site is available at:

https://indosaram.github.io/ferryx/

When running locally:

- Landing page: <http://localhost:14173/>
- Documentation: <http://localhost:14173/docs/introduction/>
- Shortcut reference: <http://localhost:14173/docs/shortcuts/>

## Features

- **Native Rust PTY Host**: Direct kernel pseudoterminal allocation with minimal I/O latency and persistent sessions.
- **Multi-Agent Orchestration**: Parallel AI coding sessions in dedicated tabs or split panes with real-time status and activity detection.
- **Flexible Split-Pane Tiling**: Horizontal and vertical terminal splits with draggable divider resizing.
- **Embedded Browser Companion**: Side-by-side native webview tabs for live local web previews.
- **Mobile Remote Gateway**: Built-in HTTP and WebSocket server for monitoring and steering sessions from a phone or tablet.
- **Session Persistence**: Automatic workspace layout saving and daemon reattachment across app restarts.

## Prerequisites

- [Bun](https://bun.sh/) (v1.1 or higher)
- [Rust](https://www.rust-lang.org/) (Cargo 1.80 or higher)
- [Node.js](https://nodejs.org/) (v20 or higher)

## Quick Start

### 1. Clone the repository

```bash
git clone https://github.com/Indosaram/ferryx.git
cd ferryx
bun install --cwd ui
bun install --cwd site
```

### 2. Run the desktop application

Start the desktop app with Vite Fast Refresh and Tauri backend:

```bash
bun run dev
```

You can also run the frontend and backend separately:

```bash
# Terminal 1: frontend Vite server
bun run ui:dev

# Terminal 2: native Tauri shell
cargo tauri dev
```

### 3. Run the documentation site

Launch the local Astro and Starlight documentation server on port 14173:

```bash
bun run --cwd site dev
```

## Available Scripts

### Root Workspace

| Command | Description |
| --- | --- |
| `bun install --cwd ui` | Install frontend dependencies |
| `bun install --cwd site` | Install documentation-site dependencies |
| `bun run dev` | Run Tauri desktop app in development mode (`cargo tauri dev`) |
| `bun run build` | Build Tauri release desktop package (`cargo tauri build`) |
| `bun run ui:dev` | Start UI Vite development server |
| `bun run ui:test` | Run UI Vitest test suite |
| `bun run ui:build` | Typecheck and build production UI bundle |

### Site and Documentation

| Command | Description |
| --- | --- |
| `bun run --cwd site dev` | Start local documentation server at http://localhost:14173/ |
| `bun run --cwd site build` | Build production static documentation for GitHub Pages |
| `bun run --cwd site preview` | Preview production documentation build locally |

### Rust Backend

| Command | Description |
| --- | --- |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Typecheck Rust backend targets |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Run Rust unit and integration tests |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Run Rust linter gates |

## Architecture

```text
ferryx/
├── src-tauri/   # Rust Tauri v2 desktop core, PTY daemon, IPC, notifications, and mobile remote server
├── ui/          # React 18 frontend with xterm.js WebGL canvas, layout management, and remote pairing UI
├── site/        # Astro + Starlight static showcase and documentation deployed to GitHub Pages
└── docs/        # Architectural audit logs and performance records
```

- **Rust Native Core (`src-tauri`)**: Owns pseudoterminal lifecycle via `portable-pty`, manages Git worktree leases, provides native child webviews, and runs an authenticated Axum WebSocket gateway for mobile pairing.
- **Frontend Application (`ui`)**: React 18 shell rendered inside WebKit or Webview2. It drives pane tiling, agent title detection, keyboard shortcuts, and terminal search overlays.
- **Docs & Landing (`site`)**: Static Astro and Starlight site hosting guides and interactive browser demos.

## Contributing

Contributions are welcome. Please follow these steps when preparing changes:

1. Create a feature branch from `main`.
2. Keep edits focused and ensure existing test suites stay green:
   ```bash
   bun run ui:test
   bun run ui:build
   bun run --cwd site build
   cargo check --manifest-path src-tauri/Cargo.toml
   ```
3. Open a pull request with a clear description of your changes.

## License

Distributed under the MIT and Apache 2.0 dual licenses. See [`LICENSE`](./LICENSE) for full terms.
