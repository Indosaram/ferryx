---
title: Ferryx vs tmux and git worktree
description: How Ferryx compares with the classic tmux plus git worktree setup for running parallel coding agents, and when plain tmux is still the right choice.
---

Running several coding agents at once is a workflow problem before it is a tooling problem. The classic answer has existed for years: create a git worktree per task, open a tmux window or pane per worktree, attach and detach as you go. It is free, it runs anywhere, and it works. Ferryx takes that same pattern and wraps it in a desktop workspace. This page explains how the two relate, and when plain tmux is still the right call.

## The manual baseline

tmux plus git worktree is the standard zero-install way to run several coding agents side by side. The two pieces do different jobs:

- tmux is a mature, free, ubiquitous terminal multiplexer. It runs anywhere, including over plain SSH on a server, and it survives disconnects by design.
- git worktree is a built-in git feature that checks out multiple branches of one repository into separate directories, so each task gets its own checkout and its own branch.
- Combined, the recipe is short: one worktree per task, one tmux window or pane per worktree, attach and detach at will.

This baseline is genuinely good. Many engineers should keep using it. Nothing in the rest of this page argues otherwise; the argument is only about who does the bookkeeping.

## What Ferryx actually changes

Ferryx is an MIT licensed desktop app built with Rust and Tauri v2. It parses terminal output with libghostty-vt and renders panes with WGPU on a native surface, with no Electron. The honest framing is ergonomics and bookkeeping, not capability. Every item in this list is something a determined engineer could assemble manually with tmux plus scripts:

- Worktree bookkeeping. Each managed worktree lives in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, and worktree paths are jailed to the repository root. You don't invent directory names or branch names by hand; the app does it the same way every time.
- Visible state. Each agent gets its own pane in a split layout, and panes and tabs can be rearranged with drag-and-drop, so who is doing what is visible at a glance.
- An embedded browser. Browser tabs use native WebViews and sit beside terminal panes, keeping docs or a dashboard next to the agents instead of in another window.
- Session lifetime. A headless Rust PTY daemon owns the pseudoterminals, so closing or reloading the GUI does not kill running agent processes. Output is buffered in a ring buffer with monotonic sequence numbers, and reconnecting replays the output you missed.
- A phone client. An authenticated mobile remote client renders the terminal with a custom DOM grid, no xterm.js involved, so a running session can be checked from a phone.
- Packaging. macOS ships as a universal DMG, Windows through Microsoft Store, Linux as AppImage and .deb.

That list is the whole argument. None of it adds capability over tmux; it removes manual steps and keeps state visible without hand-arranged windows.

## What this comparison leaves out

Speed. Ferryx's editorial rules forbid publishing unmeasured third-party comparison benchmarks, so this page has no benchmark numbers and makes no performance argument in either direction. If throughput matters to your decision, measure your own workload on both setups.

## When to just keep using tmux

tmux is beloved and battle-tested, and honesty cuts both ways. Keep it if any of these describe you:

- You live in SSH. tmux runs on any server you can reach, over plain SSH, with nothing beyond the terminal you already have.
- You want zero GUI dependencies. tmux needs a terminal, and that's the whole dependency list.
- You want something that runs anywhere, including a headless server you never sit at.
- You already have a tuned config. Your keybindings, status line, and pane scripts are set up exactly the way you want them.
- You don't want another desktop app on your machine.

Put plainly: tmux is free, mature, and everywhere. Ferryx is a v0.1.0-alpha desktop app. If that status alone is reason to wait, it's a sound reason.

## Where to go next

- For the collision problem this whole setup exists to solve, read [running parallel AI coding agents](/use-cases/parallel-ai-agents/).
- To try Ferryx, start with the [setup introduction](/docs/introduction/).
