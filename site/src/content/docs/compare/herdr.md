---
title: "Ferryx vs Herdr: Desktop Workspace or Terminal Multiplexer"
description: Herdr is a Rust TUI multiplexer for coding agents that runs inside your terminal. Compare it with Ferryx on architecture, sessions, remote work and status.
draft: false
---

[Herdr](https://github.com/herdrdev/herdr) calls itself "the runtime your coding agents live on": a single Rust binary that runs inside whatever terminal you already use, keeps agent terminals in a background server, and marks every pane as working, blocked or idle. [Ferryx](https://github.com/Indosaram/ferryx) is a desktop terminal workspace written in Rust on Tauri v2 that also keeps agents in a background daemon. The real difference is the interface layer: Herdr is a tmux-style multiplexer (TUI) inside your terminal, Ferryx is its own desktop window.

Facts as of 2026-10: checked on 2026-10-24 against the Herdr [README](https://github.com/herdrdev/herdr#readme), its latest release [v0.9.5](https://github.com/herdrdev/herdr/releases/tag/v0.9.5) (2026-10-14), its [documentation](https://herdr.dev/docs/), and the linked issues; Ferryx facts are from [v2026.10.11.1](https://github.com/Indosaram/ferryx/releases/latest) and the [product facts](/docs/facts/) page. Both projects move quickly, so re-check anything you plan to rely on.

## Availability at a glance

| | Ferryx | Herdr |
| :--- | :--- | :--- |
| Form | Desktop app (Tauri v2), WGPU-rendered terminals | TUI multiplexer, one Rust binary, runs in your terminal |
| Platforms | macOS universal DMG, Windows x64 installer, Linux AppImage and .deb, headless `ferryx-cli` | Linux and macOS via install script, Homebrew or mise; Windows via PowerShell installer |
| Agents | Any CLI agent; status detection ships for <!-- agents:count -->16<!-- /agents:count --> | Any CLI agent, with working/blocked/idle status per pane |
| Remote | SSH workspaces, mobile web client, outbound relay | Saved SSH machines in one window with independent reconnects |
| License | Source-available under the Ferryx License, free for individuals and small companies | Apache-2.0 (relicensed from AGPL-3.0 in July 2026, [#1340](https://github.com/herdrdev/herdr/issues/1340)) |

## Where the two actually differ

**Interface layer.** Herdr needs no window manager or desktop environment: it runs over SSH, in a headless server, or inside the terminal you already like, with tmux-style prefix keys and mouse support. Ferryx is a separate desktop window with native WebView browser tabs and a mobile web client. The flip side of nesting inside another terminal is fidelity: Herdr has an open [60 Hz frame limit](https://github.com/herdrdev/herdr/issues/1134) and open reports of [Neovim colored underlines rendering differently](https://github.com/herdrdev/herdr/issues/1178) than in the outer terminal, and a Hacker News user [couldn't keep a keyboard selection in WezTerm](https://news.ycombinator.com/item?id=48823155) because of constant redraws. Ferryx renders its own surface with WGPU at display rate, so the outer-terminal layering doesn't apply.

**What survives what.** Both keep agents alive when the client goes away. Herdr's README is precise about the rest: after a server or machine restart it restores the saved layout and can resume supported agent sessions, but "the original processes do not survive" ([session state](https://herdr.dev/docs/session-state/)). Ferryx is in the same position: a headless Rust PTY daemon owns the pseudoterminals, closing or reloading the window leaves agents running, and a 512 KiB sequenced ring buffer replays missed output on reconnect, but if the daemon or its host dies, the PTYs end. Both have one server as a single point of failure. Herdr users have reported [one crash taking down every Claude Code session at once](https://github.com/herdrdev/herdr/issues/1228) (closed as not planned), and Ferryx only restarts a crashed daemon automatically on macOS.

**Worktrees.** Both manage git worktrees and open each one as its own workspace. Ferryx keeps them inside the repository at `.orca-worktrees/<workspace-id>/<slug>` on a branch named `orca/<workspace-id>/<slug>`, with paths jailed to the repository root. Herdr's `herdr worktree create` puts checkouts under a configurable directory, `~/.herdr/worktrees/<repo>/<branch-slug>` by default, and groups them with the parent repo workspace ([CLI reference](https://herdr.dev/docs/cli-reference/)). Herdr's location is configurable and Ferryx's is not; the trade-off is a tidy repository against worktrees that live with the code.

**Agent status.** Both read pane state to tell you which agent is stuck. Herdr also lets agents drive it through a CLI and socket API: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked ([agent skill](https://herdr.dev/docs/agent-skill/)). Ferryx has no equivalent agent-to-agent control surface.

## What Herdr does better

Herdr runs anywhere a terminal does, as one lightweight binary with no Electron and no desktop environment, so it works over plain SSH on a headless box. It puts several machines in one window with a combined agent list and independent reconnects. Its socket API lets agents orchestrate other agents, it has a plugin marketplace, and its permissive Apache-2.0 license clears corporate open-source policies that the source-available Ferryx License may not.

## When Herdr is the better choice

If you live in the terminal, already have a terminal you like, and mostly work on remote machines over SSH, Herdr is the better choice: nothing to install on a desktop, tmux-style keys you already know, and agents that can coordinate through its socket API. It's also the safer pick when your company requires an OSI-approved license.

Ferryx is the better fit when you want a desktop window with browser tabs beside terminals, managed worktrees, a rendering surface of its own rather than one nested in another terminal, and a phone client for checking on runs.

## Where to read next

Compare other tools on the [Ferryx comparison index](/compare/), check every Ferryx claim on the [product facts](/docs/facts/) page, or download a build from the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
