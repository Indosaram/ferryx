---
title: "Ferryx vs Orca: Agent IDE or Daemon-Backed Terminal Workspace"
description: Orca is an Electron agent IDE with an editor, diff review and design mode. Compare it with Ferryx on architecture, sessions, worktrees and footprint.
draft: false
---

[Orca](https://github.com/stablyai/orca) is an MIT-licensed agent development environment from Stably AI that runs Codex, Claude Code, OpenCode, Pi and other CLI agents side by side, each in its own git worktree. [Ferryx](https://github.com/Indosaram/ferryx) is a source-available desktop terminal workspace built in Rust on Tauri v2 that does the same core job. They overlap more than any other pair on this site: both run any CLI agent, both isolate agents in worktrees, both ship on macOS, Windows and Linux, and both reach your phone. The differences are in how much IDE you want around the terminal and how each one is built.

Facts as of 2026-10: checked on 2026-10-24 against the Orca [README](https://github.com/stablyai/orca#readme), its latest release [v1.4.239](https://github.com/stablyai/orca/releases/tag/v1.4.239) (2026-10-24), its `package.json`, and the linked issues; Ferryx facts are from [v2026.10.11.1](https://github.com/Indosaram/ferryx/releases/latest) and the [product facts](/docs/facts/) page. Orca ships almost daily, so re-check anything you plan to rely on.

## Availability at a glance

| | Ferryx | Orca |
| :--- | :--- | :--- |
| Platforms | macOS universal DMG, Windows x64 installer, Linux AppImage and .deb | macOS (Apple Silicon and Intel), Windows .exe, Linux AppImage, AUR |
| Shell | Tauri v2 with the OS webview, terminal rendered by WGPU | Electron (`electron` 43.8.1 in `package.json`) |
| Agents | Any CLI agent; status detection ships for <!-- agents:count -->16<!-- /agents:count --> | Any CLI agent |
| Editor | Read-only file previews, no editor or LSP | Built-in editor (Monaco) with autosave |
| Mobile | Authenticated mobile web client, PIN or QR pairing | iOS App Store app and Android APK |
| License | Source-available under the Ferryx License, free for individuals and small companies | MIT |

## Where the two actually differ

**How much IDE.** Orca is closer to an IDE than a terminal. Its README lists a built-in editor with autosave, rich previews of Markdown, PDFs and images, diff annotation that ships comments back to the agent, native GitHub and Linear boards, account switching with Claude and Codex usage tracking, and Computer Use. Ferryx is a terminal workspace: panes, splits, native WebView browser tabs and worktrees, with files opening as read-only previews. If you edit in your own editor, that's a feature; if you want one window for everything, it's a gap.

**Architecture and footprint.** Orca is an Electron app, which means a bundled Chromium; Ferryx uses the operating system's webview for its UI and renders terminals with WGPU on a native child surface, parsed by libghostty-vt. Orca users have filed battery and memory complaints, including [battery usage that's too high](https://github.com/stablyai/orca/issues/7919) and a report of [140 GB of application memory with six agents](https://github.com/stablyai/orca/issues/7017), both since closed as completed. Ferryx publishes no comparative resource numbers, because none have been measured under a shared methodology.

**Sessions across updates.** Both tools keep terminals in a background daemon. On Orca, [app updates leave previous daemon generations running](https://github.com/stablyai/orca/issues/9138): the reporter found four generations alive, killed 370 orphaned processes, and recovered about 20 GB of swap. That issue was still open on 2026-10-24. Ferryx's update handover keeps sessions alive by design: the old daemon keeps its PTYs and retires only after its last session ends. Ferryx has its own limit: if the daemon crashes, its PTYs die, and only macOS restarts it automatically (launchd).

**Worktrees.** Ferryx puts managed worktrees inside the repository at `.orca-worktrees/<workspace-id>/<slug>` on a branch named `orca/<workspace-id>/<slug>`, with paths jailed to the repository root. Orca fans one prompt across several agents, each in its own worktree, so you can compare the results and merge the winner. Neither tool does multi-repo workspaces yet; on Orca that is the [most-reacted open request](https://github.com/stablyai/orca/issues/1099), and Ferryx maps one workspace to one repository as well.

## What Orca does better

Orca's design mode is the standout: click any element in a real Chromium window and its HTML, CSS and a cropped screenshot go straight into the agent's prompt ([docs](https://www.onorca.dev/docs/browser/design-mode)). Its diff review lets you drop comments on any diff line and send them back to the agent, then edit and commit without leaving the app ([docs](https://www.onorca.dev/docs/review/annotate-ai-diff)). It has a real editor, SSH worktrees with auto-reconnect and port forwarding, native mobile apps on both stores, and an `orca` CLI that lets agents drive the app. Ferryx has none of the editor, diff-annotation or issue-tracker features. Orca's editor still has gaps users point out, such as the open request for [Vim and Emacs keybinding presets](https://github.com/stablyai/orca/issues/9699), but it exists and Ferryx's doesn't.

## When Orca is the better choice

If you want one application that covers agents, editing, review and merge, Orca is the better choice: design mode, diff annotation, GitHub and Linear inside the app, and native mobile apps are all things Ferryx doesn't do. Its MIT license also matters if you need OSI open source, which the Ferryx License is not, and its release cadence means features land fast.

Ferryx is the better fit when you'd rather keep your own editor and want a lighter terminal workspace with no bundled Chromium, where sessions outlive the window and survive an update handover.

## Where to read next

Compare other tools on the [Ferryx comparison index](/compare/), check every Ferryx claim on the [product facts](/docs/facts/) page, or download a build from the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
