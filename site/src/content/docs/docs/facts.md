---
title: Ferryx product facts
description: "Sourced Ferryx reference facts: Ferryx License, platform packages, shipped agent status manifests, managed worktree paths, and PTY daemon replay behaviour."
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
| License | Ferryx License 1.0, source-available; free for individuals, education, open-source work, non-profits, and companies under 50 people and USD 5M revenue | [`LICENSE`](https://github.com/Indosaram/ferryx/blob/main/LICENSE) |
| Current release | Calendar-versioned (`vYYYY.MM.DD.N`); see the latest published release and its notes | [Releases](https://github.com/Indosaram/ferryx/releases/latest) |
| Source repository | `github.com/Indosaram/ferryx` | GitHub |
| Price | Free to download; larger companies need a Team subscription for work use ([pricing](/docs/pricing/)) | [Releases](https://github.com/Indosaram/ferryx/releases/latest) |

The Ferryx License is not an OSI open-source license. Use and modification are free for personal
use, education, open-source work, non-profits, and companies with fewer than 50 employees and
contractors and less than USD 5M in annual revenue. Larger companies need a Team subscription to
use Ferryx for work, after a transition period that ends 90 days after the Team plan opens.
Self-hosted relays are free for everyone. Distribution must be free of charge and for
non-commercial purposes, with the required notices preserved. Releases published before the
change remain under the Sustainable Use License (SUL-1.0). Read the license for its full terms.

## Platforms and packages

| Platform | Package | Requirement |
| :--- | :--- | :--- |
| macOS | Universal DMG (Apple Silicon and Intel), signed and notarised | macOS 10.15+ |
| Windows | `Ferryx_x64-setup.exe` installer, not code-signed yet | Windows 10/11 x64 with WebView2 |
| Linux | AppImage (x86_64) | glibc 2.31+, WebKitGTK 4.0/4.1 with GTK 3 |
| Linux | Debian/Ubuntu `.deb` (x86_64) | glibc 2.31+, WebKitGTK 4.0/4.1 with GTK 3 |
| Linux headless | `ferryx-cli` single binary, PTY daemon and CLI (x86_64) | No GUI or WebKit dependency |

There is no Microsoft Store listing: the Store catalogue returns no Ferryx product, and the
`Ferryx_x64.msix` in each release is an unsigned Store-ingestion package, not an installable one.
Windows users install the `.exe`, which self-updates.

For the Linux headless `ferryx-cli`, install with the one-line installer, which resolves the
current build rather than a pinned tag:

```bash
curl -fsSL https://relay.ferryx.dev/install.sh | bash
```

Local release assembly stages `ferryx-cli` beside the Linux AppImage and `.deb` (and, on macOS,
as a universal binary), so the installer has a current artifact to fetch; it falls back to the
GitHub release asset if the relay has no local copy. `cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx-cli`
remains available for building from source.

Every release publishes `SHA256SUMS.txt` beside the binaries, so a download can be checked with
`sha256sum -c SHA256SUMS.txt` before it is run.

## Coding agents

Agents are ordinary commands: anything you can start in a terminal runs in a Ferryx pane, including
Claude Code, Codex, and Antigravity (agy). What is agent-specific is *status detection* — reading a pane's
output to decide whether that agent is working, waiting on you, or idle.

Status-detection manifests ship for <!-- agents:count -->16<!-- /agents:count --> agents:
<!-- agents:list -->Aider, Antigravity, Claude Code, Cline, Codex, Crush, Cursor CLI, Droid, GitHub Copilot CLI, GJC, Grok, Kimi, OmO, omp, OpenCode, and pi<!-- /agents:list -->
(`src-tauri/src/agent_detect/manifests/`, registered in `src-tauri/src/agent_detect/engine.rs`; the list and count on this site are generated
from those manifests by `scripts/gen-site-agents.mjs`).
Detection drives the pane status indicator, notifications, and the macOS Dock attention badge.

## Git worktree isolation

| Fact | Value | Source |
| :--- | :--- | :--- |
| Managed worktree path | `.orca-worktrees/<workspace-id>/<slug>` inside the repository | `src-tauri/src/worktree/manager.rs` |
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

Closing or reloading the desktop window does not kill running agent processes while their daemon
remains alive. Stopping the daemon or rebooting its host can end live PTYs.

## Remote access

| Fact | Value | Source |
| :--- | :--- | :--- |
| Gateway | Authenticated Axum WebSocket server | `src-tauri/src/remote/` |
| Pairing | Account sign-in (email magic link); machines join with an enrollment code issued from the signed-in desktop | `src-tauri/src/remote/` |
| Mobile terminal | Custom DOM grid, no xterm.js dependency | `ui/src/remote/` |
| Off-LAN access | Configurable outbound relay tunnel; no inbound SSH port or public IP required | `src-tauri/src/remote/` |
| Paired machine path | Relay first; a live session upgrades to a direct QUIC path (UDP, IPv4) when both sides trust each other, and falls back to the relay on loss | `src-tauri/src/paired_host/`, `src-tauri/src/terminal/paired_runtime.rs` |

A terminal exposed over a network is a shell on your machine. Keep remote access off unless you need
it, and prefer a trusted network path.

## Rendering and architecture

| Layer | Implementation | Source |
| :--- | :--- | :--- |
| Terminal parser | `libghostty-vt` via FFI | `src-tauri/src/native_terminal/sys/ffi.rs` |
| Desktop renderer | WGPU native child surfaces (Metal, Vulkan, DX12) | `src-tauri/src/native_terminal/renderer/` |
| Application shell | Tauri v2 with the OS webview; no Electron or bundled Chromium | `src-tauri/tauri.conf.json` |
| Embedded browser | Native WebView split-tabs beside terminal panes | `ui/src/components/browser/` |
| UI | React 18 with a hook-based workspace store and binary pane split trees | `ui/src/state/` |

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
