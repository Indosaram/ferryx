---
title: "Ferryx vs cmux: Agent Workspaces and Ghostty Rendering"
description: Ferryx and cmux both leverage libghostty for terminal rendering in agent workspaces. Compare cross-platform headless daemons with macOS native UI.
draft: true
verified: Facts cross-checked against docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md (2026-09-12 research) on 2026-09-25.
---

[cmux](https://github.com/manaflow-ai/cmux) is a macOS-native agent terminal workspace developed by Manaflow. Ferryx is a cross-platform desktop terminal application built with Rust and Tauri v2. Both projects share a core heritage: using Mitchell Hashimoto's `libghostty` parser for high-performance terminal emulation. However, they make opposite architectural choices regarding operating system boundaries and daemon detachment.

## The core architectural difference

cmux is built specifically for macOS using Swift and AppKit. It integrates a vertical workspace sidebar, completion notification rings, and a scriptable embedded browser controlled via socket APIs. However, cmux operates entirely as a local macOS application with no remote headless daemon mode, making it impossible to run as a backend on headless Linux servers or remote cloud instances. In documented user issues, cmux has suffered from severe memory leaks reaching up to 80 GB RAM leading to system freezes and OOM crashes, as well as a SwiftUI layout loop in its sidebar that consumed 65% to 101% CPU in sessions with many workspaces and panes. Linux support remains excluded (issue #330).

Ferryx decouples the terminal backend from the desktop surface using a headless Rust PTY daemon. The daemon manages terminal lifecycles independently of the GUI, buffering output in a 512 KiB sequenced ring buffer and supporting reconnection without killing active processes. Renders execute across platforms via WGPU on a native child surface using libghostty-vt.

## How each isolates parallel work

cmux organizes work using vertical tabs grouped by project folders, with side-by-side pane layouts for coding, builds, and monitoring, and supports managing multiple worktrees. It incorporates an embedded browser that autonomous agents can drive directly via internal sockets to inspect frontend changes.

Ferryx isolates parallel agents by provisioning dedicated git worktrees in `.orca-worktrees/wt-<slug>` on branches named `orca/<workspace-id>/<slug>`. It also embeds native WebView browser tabs alongside terminal splits while isolating background processes behind the headless daemon.

## Capabilities at a glance

- As of September 12, 2026, cmux had 27,038 GitHub stars on `manaflow-ai/cmux`.
- cmux is built with Swift/AppKit using libghostty, featuring vertical project tabs and an agent-scriptable embedded browser.
- Documented cmux issues include memory leaks reaching up to 80 GB RAM resulting in OOM crashes, and sidebar layout loops consuming 65% to 101% CPU.
- cmux is restricted to macOS, lacking Linux desktop support and remote headless server execution.
- Ferryx runs cross-platform on macOS, Windows, and Linux via Tauri v2 and WGPU, under the SUL-1.0 license.
- Ferryx's headless Rust PTY daemon persists sessions across GUI reloads with 512 KiB sequenced ring buffers.
- Ferryx structures worktrees in `.orca-worktrees/wt-<slug>` on `orca/<workspace-id>/<slug>`.

## What cmux does better

cmux provides visual notification rings that alert developers immediately when a background agent (such as Claude Code) completes its task. It also features a socket-controllable embedded browser designed for agent automation, enabling autonomous agents to programmatically launch, manipulate, and visually inspect browser states in a closed loop right beside the terminal.

## Sources

- cmux development by Manaflow, repository `manaflow-ai/cmux`, libghostty basis, and 27,038 GitHub stars as of 2026-09-12 → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux.
- Vertical tabs, folder groupings, and visual completion notification rings for Claude Code → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S40].
- Scriptable embedded browser accessible to agents via socket APIs → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S41].
- Memory leaks growing up to 80 GB RAM and OOM kernel crashes → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S42].
- Sidebar SwiftUI layout loop consuming 65-101% CPU → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S43].
- macOS exclusivity and lack of remote headless server daemon execution → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S44].
- Exclusion of Linux desktop environments (issue #330) → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §9 cmux, source [S45].
- Ferryx cross-platform stack, SUL-1.0 license, libghostty-vt, WGPU, headless daemon, ring buffer, and worktree paths → site/src/content/docs/compare/crystal.md.

## Where to read next

Compare other agent platforms on the [Ferryx comparison index](/compare/), or download the latest desktop packages on the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
