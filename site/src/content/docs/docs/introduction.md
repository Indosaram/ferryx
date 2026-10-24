---
title: Introduction
description: Introduction to Ferryx, a native Rust terminal workspace for running parallel AI coding agents with Git worktree isolation and mobile remote access.
---

Ferryx is a native desktop terminal workspace for running several CLI coding agents — Claude Code, Codex, Antigravity (agy), or anything else you start from a shell — at the same time, each in its own Git worktree. It's written in Rust on Tauri v2, parses terminal output with libghostty-vt, and renders panes with WGPU on a native child surface, so there's no Electron in the stack. The project is SUL-1.0 licensed (source-available; personal, non-commercial, and internal business use are permitted) and ships for macOS, Windows, and Linux. For the checkable version of every claim on this page, see [product facts](/docs/facts/).

## The problem it addresses

Run two coding agents in one working directory and they get in each other's way. Both see the same files, both commit into the same history, and a refactor from one can invalidate the edit the other is halfway through. The usual workaround is to clone the repository once per agent and keep a terminal open for each clone. That works, but the clones drift apart and the upkeep is easy to forget.

Ferryx turns that workaround into a managed feature: each agent gets its own Git worktree, created for you and kept inside the repository. Two agents can edit one repository in parallel without touching each other's files, and you watch every session from a single window. A status indicator shows when an agent needs attention, and on macOS the Dock icon carries a live attention counter.

## The main concepts

Most of Ferryx reduces to six concepts.

- **Workspaces.** A workspace groups tabs, panes, and sessions around one project. ⌘1 through ⌘9 switch between them, and ⌘B toggles the sidebar.
- **Worktrees.** Each agent that needs isolation gets a managed Git worktree under `.orca-worktrees/<workspace-id>/<slug>` on a branch named `orca/<workspace-id>/<slug>`, jailed to the repository root.
- **Panes and tabs.** Terminals open as tabs and split into vertical or horizontal panes. ⌘T opens a terminal tab, ⌘D splits right, ⌘⇧D splits down, and you can drag a tab into any pane to rearrange the layout. Agents such as Claude Code, Codex, and Antigravity (agy) run here like any other command; status-detection manifests ship for <!-- agents:count -->16<!-- /agents:count --> of them, including Claude Code, Codex, Cursor, Cline, GitHub Copilot CLI, OpenCode, Grok, and Kimi, so a pane can report whether its agent is working, waiting on you, or idle.
- **Browser tabs.** ⌘⇧B opens a browser tab beside the terminal panes, using the native WebView instead of a separate app. Documentation, a CI dashboard, or a deployed preview can live next to the sessions that depend on them.
- **The daemon.** A headless Rust PTY daemon owns the pseudoterminals, not the window. Closing or reloading the GUI doesn't kill running processes while the daemon remains alive. Output sits in a ring buffer with monotonic sequence numbers, so reconnecting replays recent output; if the buffer has wrapped, the client is told there's a gap instead of being shown corrupted output. Workspace state snapshots automatically, and the GUI reattaches to the daemon. Stopping the daemon or rebooting the host can still end live PTYs.
- **Remote access.** The daemon serves an authenticated gateway for a mobile web client. The terminal renders as a custom DOM grid rather than xterm.js, and a configurable relay URL covers access from outside your LAN. Between two paired Ferryx machines, a live session moves from the relay onto a direct QUIC path when the machines can reach each other, and returns to the relay if that path drops. [Run a self-hosted relay](/docs/self-hosted-relay/) to operate that tunnel and its account service yourself.

## Install

Download from the [releases page](https://github.com/Indosaram/ferryx/releases/latest); the links below resolve against the latest release. Releases are calendar-versioned, so the newest tag is the one the release page shows.

- **macOS**: universal DMG for Apple Silicon and Intel, [Ferryx_universal.dmg](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_universal.dmg)
- **Windows**: x64 installer, [Ferryx_x64-setup.exe](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_x64-setup.exe). It isn't code-signed yet, so SmartScreen warns on first run; check the SHA-256 below before you run it. There is no Microsoft Store listing today, and the MSIX in the release is an unsigned Store-ingestion package rather than something you can install directly.
- **Linux**: [Ferryx_amd64.AppImage](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.AppImage) or [Ferryx_amd64.deb](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_amd64.deb), both x64

Each release also publishes a `SHA256SUMS.txt` beside the binaries, so you can check a download before running it:

```bash
sha256sum -c SHA256SUMS.txt
```

## Your first session

1. Open a terminal tab with ⌘T and start a shell or an agent.
2. Split the pane with ⌘D (right) or ⌘⇧D (down) when you want two sessions visible at once.
3. Add a browser tab with ⌘⇧B for whatever you keep checking in a second window.
4. Pair a phone over QR or PIN if a long run needs a check-in while you're away.

The command palette (⌘K) is there if you'd rather browse actions than memorize keys, and the [keyboard shortcuts](/docs/shortcuts/) page lists everything, including in-terminal search (⌘F).

## Where to go next

- [Product facts](/docs/facts/), a single sourced reference for the license, platforms, packages, agent manifests, worktree paths, and daemon behaviour.
- [Technical architecture](/docs/architecture/), for how the daemon, the replay protocol, and the rendering pipeline fit together.
- [Keyboard shortcuts](/docs/shortcuts/), the complete reference.
- [Running coding agents in parallel](/use-cases/parallel-ai-agents/), on why agents collide in a shared directory and how a worktree per agent fixes it.
- [Git worktree workflow](/use-cases/git-worktree-workflow/), the manual commands and where they get tedious.
- [Remote terminal access](/use-cases/remote-terminal-access/), for checking a long run from your phone.
- [Comparisons](/compare/), architecture and licensing comparisons with other tools, deliberately without unmeasured performance claims.

## Status

Ferryx is early software: the core is usable, but expect rough edges and breaking changes between releases. Releases are calendar-versioned and published frequently, so check the [releases page](https://github.com/Indosaram/ferryx/releases/latest) for the current build and its notes. Terminal sessions and daemon communication work on macOS, Windows, and Linux. Dock badge counters are macOS-only; launchd helper code exists but automatic login-time daemon supervision is not wired up. Questions and bug reports are welcome in the [Discord](https://discord.gg/Z2hBkQEHUG).
