---
title: Ferryx product facts
description: "Sourced Ferryx reference facts: SUL-1.0 license, platform packages, shipped agent status manifests, managed worktree paths, and PTY daemon replay behaviour."
---

A single reference page of concrete, checkable facts about Ferryx. Every row names the file in
[the repository](https://github.com/Indosaram/ferryx) that backs it, so nothing here depends on
marketing copy. Figures that have not been measured are marked as such rather than estimated.

## What Ferryx is

Ferryx is a desktop terminal workspace for running several CLI coding agents at the same time.
Each agent runs in its own terminal pane and, when isolation is requested, its own managed Git
worktree. The desktop shell is Rust on Tauri v2, terminal output is parsed with `libghostty-vt`,
panes render through WGPU on a native child surface, and a headless Rust daemon owns the
pseudoterminals so sessions outlive the window.

## Identity and licensing

| Fact | Value | Source |
| :--- | :--- | :--- |
| Project name | Ferryx | `src-tauri/tauri.conf.json` (`productName`) |
| Bundle identifier | `com.ferryx.app` | `src-tauri/tauri.conf.json` |
| License | Sustainable Use License 1.0 (SUL-1.0), source-available; permits your own internal business purposes, non-commercial use, or personal use | [`LICENSE`](https://github.com/Indosaram/ferryx/blob/main/LICENSE) |
| Current release | See the latest published release and its release notes | [Releases](https://github.com/Indosaram/ferryx/releases/latest) |
| Source repository | `github.com/Indosaram/ferryx` | GitHub |
| Price | Free to download | [Releases](https://github.com/Indosaram/ferryx/releases/latest) |

SUL-1.0 is not an OSI open-source license. It permits use and modification for your own internal
business purposes or for non-commercial or personal use. Distribution must be free of charge
and for non-commercial purposes, with the required notices preserved. Read the license for its
full terms.

## Platforms and packages

| Platform | Package | Requirement |
| :--- | :--- | :--- |
| macOS | Universal DMG (Apple Silicon and Intel) | macOS 10.15+ |
| Windows | Microsoft Store (MSIX, auto-updating) | Windows 10/11 x64 with WebView2 |
| Linux | AppImage (x86_64) | glibc 2.31+, WebKitGTK 4.0/4.1 with GTK 3 |
| Linux | Debian/Ubuntu `.deb` (x86_64) | glibc 2.31+, WebKitGTK 4.0/4.1 with GTK 3 |
| Linux headless | `ferryx-cli` single binary, PTY daemon and CLI (x86_64) | No GUI or WebKit dependency |

Every release publishes `SHA256SUMS.txt` beside the binaries, so a download can be checked with
`sha256sum -c SHA256SUMS.txt` before it is run.

## Coding agents

Agents are ordinary commands: anything you can start in a terminal runs in a Ferryx pane, including
Claude Code, Codex, and Gemini CLI. What is agent-specific is *status detection* — reading a pane's
output to decide whether that agent is working, waiting on you, or idle.

Status-detection manifests ship for eleven agents: Antigravity, Claude Code, Cline, Codex, GitHub
Copilot CLI, Cursor, Gajae Code (gjc), Grok, Kimi, OMO, and OpenCode
(`src-tauri/src/agent_detect/manifests/`, registered in `src-tauri/src/agent_detect/engine.rs`).
Detection drives the pane status indicator, notifications, and the macOS Dock attention badge.

## Git worktree isolation

| Fact | Value | Source |
| :--- | :--- | :--- |
| Managed worktree path | `.orca-worktrees/wt-<slug>` inside the repository | `src-tauri/src/worktree/` |
| Branch naming | `orca/<workspace-id>/<slug>` | `src-tauri/src/worktree/` |
| Path safety | Managed worktree paths are jailed to the repository root | `src-tauri/src/worktree/` |

The application manages worktree creation, naming, and removal using these conventions.

## Sessions and the daemon

| Fact | Value | Source |
| :--- | :--- | :--- |
| PTY owner | Headless Rust daemon, not the GUI process | `src-tauri/src/daemon/` |
| Transport | Unix domain socket on Unix; loopback TCP on Windows | `src-tauri/src/daemon/` |
| Output buffer | 512 KiB ring buffer per session with monotonic sequence numbers | `src-tauri/src/terminal/output_hub.rs` |
| Reconnect behaviour | Missed output is replayed; if the buffer wrapped, the client is told there is a gap instead of being shown a corrupted transcript | `src-tauri/src/terminal/output_hub.rs` |

Closing or reloading the desktop window does not kill running agent processes.

## Remote access

| Fact | Value | Source |
| :--- | :--- | :--- |
| Gateway | Authenticated Axum WebSocket server | `src-tauri/src/remote/` |
| Pairing | 6-digit PIN (also available as a QR code) | `src-tauri/src/remote/` |
| Mobile terminal | Custom DOM grid, no xterm.js dependency | `ui/src/remote/` |
| Off-LAN access | Configurable outbound relay tunnel; no inbound SSH port or public IP required | `src-tauri/src/remote/` |

A terminal exposed over a network is a shell on your machine. Keep remote access off unless you need
it, and prefer a trusted network path.

## Rendering and architecture

| Layer | Implementation | Source |
| :--- | :--- | :--- |
| Terminal parser | `libghostty-vt` via FFI | `src-tauri/src/native_terminal/sys/ffi.rs` |
| Desktop renderer | WGPU native child surfaces (Metal, Vulkan, DX12) | `src-tauri/src/native_terminal/renderer/` |
| Application shell | Tauri v2 with the OS webview; no Electron or bundled Chromium | `src-tauri/tauri.conf.json` |
| Embedded browser | Native WebView split-tabs beside terminal panes | `ui/src/components/browser/` |
| UI | React 18 with Zustand state and binary pane split trees | `ui/src/state/` |

## What is not claimed

No comparative performance numbers against Electron applications, other terminals, or other agent
workspaces are published here, because none have been measured under a shared methodology. The only
figure the project publishes is an internal offscreen WGPU render benchmark (50-frame p50 of 3.10 ms
on an Apple M4 Max), which measures Ferryx's own render path and nothing else.

## Where to go next

- [Introduction](/docs/introduction/) — install and first session.
- [Technical architecture](/docs/architecture/) — how the daemon, replay protocol, and renderer fit together.
- [Running coding agents in parallel](/use-cases/parallel-ai-agents/) — the workflow these facts serve.
- [Comparisons](/compare/) — architecture and licensing comparisons with other tools.
