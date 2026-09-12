---
title: Introduction
description: Introduction to Ferryx, a native Rust terminal workspace for running parallel AI coding agents with Git worktree isolation and mobile remote access.
---

Ferryx is a native desktop terminal workspace for running several AI coding agents at the same time. It's written in Rust on Tauri v2, parses terminal output with libghostty-vt, and renders panes with WGPU on a native child surface, so there's no Electron in the stack. The project is MIT licensed and ships for macOS, Windows, and Linux.

## The problem it addresses

Run two coding agents in one working directory and they get in each other's way. Both see the same files, both commit into the same history, and a refactor from one can invalidate the edit the other is halfway through. The usual workaround is to clone the repository once per agent and keep a terminal open for each clone. That works, but the clones drift apart and the upkeep is easy to forget.

Ferryx turns that workaround into a managed feature: each agent gets its own Git worktree, created for you and kept inside the repository. Two agents can edit one repository in parallel without touching each other's files, and you watch every session from a single window. A status indicator shows when an agent needs attention, and on macOS the Dock icon carries a live attention counter.

## The main concepts

Most of Ferryx reduces to six concepts.

- **Workspaces.** A workspace groups tabs, panes, and sessions around one project. ⌘1 through ⌘9 switch between them, and ⌘B toggles the sidebar.
- **Worktrees.** Each agent that needs isolation gets a managed Git worktree under `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, jailed to the repository root.
- **Panes and tabs.** Terminals open as tabs and split into vertical or horizontal panes. ⌘T opens a terminal tab, ⌘D splits right, ⌘⇧D splits down, and you can drag a tab into any pane to rearrange the layout. Agents such as Claude Code, Codex, and Gemini CLI run here like any other command.
- **Browser tabs.** ⌘⇧B opens a browser tab beside the terminal panes, using the native WebView instead of a separate app. Documentation, a CI dashboard, or a deployed preview can live next to the sessions that depend on them.
- **The daemon.** A headless Rust PTY daemon owns the pseudoterminals, not the window. Closing or reloading the GUI doesn't kill running processes. Output sits in a ring buffer with monotonic sequence numbers, so reconnecting replays what you missed; if the buffer has wrapped, the client is told there's a gap instead of being shown corrupted output. It's the persistence instinct of a terminal multiplexer, without giving up a graphical interface. Workspace state snapshots automatically, and the daemon reattaches, so an exit or crash doesn't cost you work.
- **Remote access.** The daemon serves an authenticated gateway for a mobile web client. Pairing uses a 6-digit PIN, the terminal renders as a custom DOM grid rather than xterm.js, and a configurable relay URL covers access from outside your LAN.

## Install

Download from the [releases page](https://github.com/Indosaram/ferryx/releases/latest); the links below resolve against the latest release.

- **macOS**: universal DMG for Apple Silicon and Intel, [Ferryx_universal.dmg](https://github.com/Indosaram/ferryx/releases/latest/download/Ferryx_universal.dmg)
- **Windows**: [Microsoft Store](https://apps.microsoft.com/search?query=Ferryx), x64, with auto-updates
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

- [Technical architecture](/docs/architecture/), for how the daemon, the replay protocol, and the rendering pipeline fit together.
- [Keyboard shortcuts](/docs/shortcuts/), the complete reference.
- [Running coding agents in parallel](/use-cases/parallel-ai-agents/), on why agents collide in a shared directory and how a worktree per agent fixes it.
- [Git worktree workflow](/use-cases/git-worktree-workflow/), the manual commands and where they get tedious.
- [Remote terminal access](/use-cases/remote-terminal-access/), for checking a long run from your phone.
- [Comparisons](/compare/), architecture and licensing comparisons with other tools, deliberately without unmeasured performance claims.

## Status

Ferryx is at v0.1.0-alpha. The core is usable, but expect rough edges and breaking changes between releases. Terminal sessions and daemon communication work on macOS, Windows, and Linux; a few OS integrations, such as Dock badge counters and launchd supervision, are macOS-only today. Questions and bug reports are welcome in the [Discord](https://discord.gg/Z2hBkQEHUG).
