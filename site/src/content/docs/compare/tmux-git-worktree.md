---
title: Ferryx vs tmux and git worktree
description: How Ferryx compares with tmux plus git worktree and the worktree CLIs built on it, what each one covers, and when plain tmux is still the right choice.
---

Running several coding agents at once is a workflow problem before it's a tooling problem. The classic answer has existed for years: create a git worktree per task, open a tmux window or pane per worktree, attach and detach as you go. It's free, it runs anywhere, and it works. Ferryx takes that pattern and wraps it in a desktop workspace. This page explains how the two relate, what the tools in between already solve, and when plain tmux is still the right call.

## The manual baseline

tmux plus git worktree is the standard zero-install way to run several coding agents side by side. The two pieces do different jobs:

- tmux is a mature, free, ubiquitous terminal multiplexer. It runs anywhere, including over plain SSH on a server, and it survives disconnects by design.
- git worktree is a built-in git feature that checks out multiple branches of one repository into separate directories, so each task gets its own checkout and its own branch.
- Combined, the recipe is short: one worktree per task, one tmux window or pane per worktree, attach and detach at will.

This baseline is genuinely good. Many engineers should keep using it. Nothing on this page argues otherwise; the argument is only about who does the bookkeeping.

## The tools that already automate this

Plenty of people have automated the recipe, and a comparison that pretends otherwise isn't useful. The ones worth knowing, all read 2026-09-19:

- **[workmux](https://github.com/raine/workmux)** (MIT, Rust): git worktrees plus tmux windows as isolated dev environments, with kitty, WezTerm, and Zellij as alternative backends. The most direct expression of the pattern.
- **[worktrunk](https://worktrunk.dev/)** ([source](https://github.com/max-sixty/worktrunk), MIT or Apache-2.0): a worktree CLI built for parallel agents, where worktrees are addressed by branch name and paths come from a configurable template. The most popular of the group by stars.
- **[lazyworktree](https://github.com/chmouel/lazyworktree)** (Apache-2.0, Go): a keyboard-first TUI for worktrees with CI and PR status, tmux and Zellij integration, and a pane showing active Claude, Codex, Copilot, and pi sessions per worktree.
- **[tmux-worktree](https://github.com/denesbeck/tmux-worktree)** (MIT): a tmux plugin that creates, switches, and removes worktrees from a floating popup, each in its own tmux window.

If you already live in the terminal and want the bookkeeping gone, one of these is probably a better fit than a desktop app. They're smaller, they're open source in the OSI sense, and they compose with the setup you have. Ferryx isn't trying to be a better workmux.

## What Ferryx actually changes

Ferryx is a source-available desktop app (SUL-1.0) built with Rust and Tauri v2. It parses terminal output with libghostty-vt and renders panes with WGPU on a native surface, with no Electron. The honest framing is ergonomics and bookkeeping, not capability. Every item in this list is something a determined engineer could assemble with tmux plus scripts, or get from the CLIs above:

- Worktree bookkeeping. Each managed worktree lives in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, and worktree paths are jailed to the repository root. You don't invent directory names or branch names by hand; the app does it the same way every time, whichever agent is in the pane.
- Visible state. Each agent gets its own pane in a split layout, and panes and tabs can be rearranged with drag-and-drop, so who's doing what is visible at a glance. Status-detection manifests ship for eleven agents, so a pane reports working, waiting, or idle without you reading the scrollback.
- An embedded browser. Browser tabs use native WebViews and sit beside terminal panes, keeping docs or a dashboard next to the agents instead of in another window.
- Session lifetime. A headless Rust PTY daemon owns the pseudoterminals, so closing or reloading the GUI doesn't kill running agent processes. Output is buffered in a 512 KiB ring buffer with monotonic sequence numbers, and reconnecting replays the output you missed.
- A phone client. An authenticated mobile remote client renders the terminal with a custom DOM grid, no xterm.js involved, so a running session can be checked from a phone.
- Packaging. macOS ships as a universal DMG, Windows as an x64 installer, Linux as AppImage and .deb.

That list is the whole argument. None of it adds capability over tmux; it removes manual steps and keeps state visible without hand-arranged windows. The two items the tmux-side tools don't cover are the daemon owning the PTYs independently of any client, and the phone client.

## What this comparison leaves out

Speed. Ferryx's editorial rules forbid publishing unmeasured third-party comparison benchmarks, so this page has no benchmark numbers and makes no performance argument in either direction. If throughput matters to your decision, measure your own workload on both setups.

## When to just keep using tmux

tmux is beloved and battle-tested, and honesty cuts both ways. Keep it if any of these describe you:

- You live in SSH. tmux runs on any server you can reach, over plain SSH, with nothing beyond the terminal you already have.
- You want zero GUI dependencies. tmux needs a terminal, and that's the whole dependency list.
- You want something that runs anywhere, including a headless server you never sit at.
- You already have a tuned config. Your keybindings, status line, and pane scripts are set up exactly the way you want them.
- You want OSI open source. tmux is ISC licensed and the worktree tools above are MIT or Apache-2.0; Ferryx is source-available under SUL-1.0, which isn't the same thing.
- You don't want another desktop app on your machine.

Put plainly: tmux is free, mature, and everywhere. Ferryx is an early desktop app. If that status alone is reason to wait, it's a sound reason.

## Where to go next

- For the collision problem this whole setup exists to solve, and what Claude Code and Codex now handle themselves, read [running parallel AI coding agents](/use-cases/parallel-ai-agents/).
- To try Ferryx, start with the [setup introduction](/docs/introduction/).
