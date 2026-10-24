---
title: "Ferryx vs cmux: A cmux Alternative for Windows and Linux"
description: cmux is a native macOS terminal for coding agents. Compare it with Ferryx on platforms, session survival, notifications, the browser, and remote work.
draft: false
---

[cmux](https://github.com/manaflow-ai/cmux) is a native macOS terminal from Manaflow, built in Swift and AppKit on top of libghostty, with vertical tabs and notifications for AI coding agents. [Ferryx](https://github.com/Indosaram/ferryx) is a desktop terminal workspace written in Rust on Tauri v2 that also parses terminal output with libghostty-vt. The two share a terminal core and a target user, and they split on two questions: which operating systems you can run, and what owns your sessions when the app goes away.

Facts as of 2026-10: checked on 2026-10-24 against the cmux [README](https://github.com/manaflow-ai/cmux#readme), its latest stable release [v0.65.2](https://github.com/manaflow-ai/cmux/releases/tag/v0.65.2) (2026-10-16), and the linked issues; Ferryx facts are from [v2026.10.11.1](https://github.com/Indosaram/ferryx/releases/latest) and the [product facts](/docs/facts/) page. Both projects ship weekly, so re-check anything you plan to rely on.

## Availability at a glance

| | Ferryx | cmux |
| :--- | :--- | :--- |
| macOS | Universal DMG | Yes, native Swift/AppKit app (DMG or Homebrew cask) |
| Windows | x64 installer (not code-signed yet) | No desktop app; [Windows request](https://github.com/manaflow-ai/cmux/issues/1012) is open |
| Linux | AppImage and .deb (x86_64), plus a headless `ferryx-cli` | No desktop app; [Linux request #330](https://github.com/manaflow-ai/cmux/issues/330) is open, its most-reacted issue |
| Terminal core | libghostty-vt parser, WGPU renderer | libghostty, reads your existing Ghostty config |
| License | Source-available under the Ferryx License, free for individuals and small companies | The README calls it "Free and GPL-licensed" |
| Remote | SSH workspaces, mobile web client, outbound relay | `cmux ssh user@remote`, plus Cloud workspaces |

## Where the two actually differ

**Platforms.** This is the deciding fact for most people. cmux is a Mac app by design: the README sells it as "Native macOS app — Built with Swift and AppKit, not Electron." The [Linux support issue](https://github.com/manaflow-ai/cmux/issues/330) had 221 reactions and was still open on 2026-10-24, and the [Windows request](https://github.com/manaflow-ai/cmux/issues/1012) had 52. Ferryx ships a macOS universal DMG, a Windows x64 installer, and Linux AppImage and .deb packages from one codebase.

**Who owns the session.** In Ferryx a headless Rust PTY daemon owns the pseudoterminals and the window is only a client, so closing or reloading the GUI leaves agents running. Output goes into a 512 KiB ring buffer per session with monotonic sequence numbers, and a reconnect replays what you missed or reports a gap if the buffer wrapped. On cmux, users ask for exactly this: [Sessions are lost after app restart](https://github.com/manaflow-ai/cmux/issues/2823) and a request for [live process preservation across cmd+q](https://github.com/manaflow-ai/cmux/issues/1663) were both open on 2026-10-24. Ferryx's limit is honest too: if the daemon itself dies, its PTYs go with it, and only macOS restarts a crashed daemon automatically (launchd).

**Memory reports.** cmux users have reported memory growing past 70 GB on a 16 GB Mac until macOS froze ([#2871](https://github.com/manaflow-ai/cmux/issues/2871), closed as completed; the follow-up [#2962](https://github.com/manaflow-ai/cmux/issues/2962) against v0.63.2 is still open). Ferryx publishes no comparative memory numbers, because none have been measured under a shared methodology.

**Agent status.** Both tools tell you which agent needs you. cmux picks up OSC 9/99/777 sequences and offers a `cmux notify` CLI for agent hooks; its sidebar indicators draw complaints, with [flaky "Running" and "Needs Input" status](https://github.com/manaflow-ai/cmux/issues/1027) among its most-reacted open issues. Ferryx ships status-detection manifests for eleven agents and drives pane indicators and notifications from them; for Claude Code and Codex that still means reading screen output.

**Worktrees.** Ferryx creates managed git worktrees inside the repository at `.orca-worktrees/<workspace-id>/<slug>` on a branch named `orca/<workspace-id>/<slug>`, with paths jailed to the repository root. cmux's sidebar shows the git branch, linked PR status, working directory, and listening ports per workspace, but the README presents cmux as "a primitive, not a solution" and leaves the worktree workflow to you.

## What cmux does better

cmux's notification rings are the feature people come for: a pane gets a blue ring and its tab lights up when an agent needs you, a notification panel collects everything pending, and Cmd+Shift+U jumps to the latest unread. Its in-app browser has a scriptable API ported from [agent-browser](https://github.com/vercel-labs/agent-browser), so an agent can snapshot the accessibility tree, click, fill forms, and evaluate JavaScript in a pane beside the terminal. It imports cookies and sessions from Chrome, Firefox, Arc, and 20+ other browsers, and `cmux claude-teams` runs Claude Code's teammate mode as native splits. Ferryx has native WebView browser tabs beside terminals, but nothing as agent-scriptable.

## When cmux is the better choice

If you only work on a Mac and want the most native-feeling agent terminal there is, cmux is the stronger pick: a Swift/AppKit app that reads your existing Ghostty config, a notification system built around "which pane needs me," and an agent-drivable browser. Its GPL-licensed app and CLI also matter if you need OSI open source on the desktop, which the Ferryx License is not. cmux additionally offers Cloud workspaces from the app (Cmd+Y), and Ferryx has no hosted tier at all.

Ferryx is the better fit when the same workspace has to run on Windows or Linux, or when agents must keep running after the window closes or reloads.

## Where to read next

Compare other tools on the [Ferryx comparison index](/compare/), check every Ferryx claim on the [product facts](/docs/facts/) page, or download a build from the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
