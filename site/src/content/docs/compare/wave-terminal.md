---
title: Ferryx vs Wave Terminal
description: Wave Terminal and Ferryx are open source desktop tools with terminals at the center, but they optimize for different jobs. An honest, factual comparison.
---

Wave Terminal and Ferryx are both open source desktop applications that put terminals next to other panels. The similarity ends about there. They're built for different jobs: Wave is one powerful workspace terminal that sees your files, while Ferryx is a home for many coding agents running in parallel, each isolated in its own git worktree.

This page sticks to verifiable facts: license, language, documented features, and architecture. No benchmark numbers, because we haven't run any, and the project doesn't publish unmeasured third-party comparisons.

## The basics

- License: Wave Terminal is Apache-2.0. Ferryx is MIT. Both are permissive open source licenses.
- Implementation: Wave is written in Go. Ferryx is built with Rust and Tauri v2, contains no Electron, and renders through WGPU on a native child surface.
- Platforms: Wave runs on macOS, Linux, and Windows. Ferryx ships a macOS universal DMG, a Windows build through the Microsoft Store, and Linux AppImage and .deb packages.
- Maturity: Wave is an established project with a large user base. Ferryx is at v0.1.0-alpha.
- Popularity: Wave's repository had 22,258 GitHub stars as of September 2026. Star counts and feature sets change, so check both projects for the current picture.

## What Wave Terminal does

Wave describes itself as "The open source, AI-native terminal that sees your entire workspace" and as "an open-source terminal with superpowers, integrating file previews, file editing, AI, web browsing, and workspace organization."

Its documented capabilities include:

- An SSH connection manager for remote machines, with WSL support.
- Screen splitting and layouts that arrange terminals, editors, and web views into workspaces and dashboards.
- Remote directory navigation with markdown and image preview.
- A built-in VSCode-like editor for remote file editing.

Project links: [waveterm.dev](https://waveterm.dev/) and [github.com/wavetermdev/waveterm](https://github.com/wavetermdev/waveterm).

## What Ferryx does

Ferryx is designed around running multiple coding agents in parallel, each isolated in its own git worktree. Concretely:

- Git worktree isolation: each managed worktree lives in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`.
- A headless Rust PTY daemon owns the pseudoterminals. Closing or reloading the GUI doesn't kill running agent processes.
- Output is buffered in a ring buffer with monotonic sequence numbers, so a reconnect replays what you missed.
- Terminal parsing runs through libghostty-vt, with GPU rendering via WGPU on a native child surface.
- Embedded browser tabs use native WebViews and can be split beside terminal panes.
- An authenticated mobile remote client renders the terminal with a custom DOM grid, with no xterm.js dependency.

The [technical architecture](/docs/architecture/) page goes deeper on the internals.

## Where Wave Terminal is the better choice

- You want a mature, full-featured terminal. Wave combines shells, remote machines, file editing, and browsing in one workspace, and the breadth of what's built in is the mark of a mature project.
- You work on remote machines. Wave's SSH connection manager, its remote directory navigation with markdown and image preview, and its built-in VSCode-like editor cover a workflow Ferryx doesn't attempt yet.
- Dashboards matter to you. Wave's layouts arrange terminals, editors, and web views into workspaces and dashboards, a broader canvas than Ferryx's agent-focused model.
- You'd rather use a tool with a large, established user base. Wave has one. Ferryx is a v0.1.0-alpha release with a narrow focus and a much smaller feature surface.

## Where Ferryx differs

The differences come down to architecture and scope, not to one tool being better:

- Different job. Wave optimizes for one powerful workspace terminal that sees your files. Ferryx optimizes for many parallel agents, each isolated in its own git worktree.
- Worktree isolation. Ferryx gives every managed checkout its own worktree under `.orca-worktrees/wt-<slug>` and its own `orca/<workspace-id>/<slug>` branch, so agents don't fight over one directory. Worktree orchestration isn't part of Wave's documented feature set.
- Process survival. Ferryx's headless Rust PTY daemon owns the pseudoterminals, so agent processes keep running when the GUI closes or reloads, and the ring buffer replays missed output on reconnect. That design exists because long agent runs shouldn't die with their window.
- Implementation. Ferryx is Rust and Tauri v2 with no Electron, parses terminal output through libghostty-vt, and renders with WGPU on a native child surface. Wave is a Go application. We list these as architecture facts, not as a performance claim.

## The short version

If you want a mature workspace terminal with SSH, remote file editing, and dashboards, Wave Terminal is the stronger choice today. But if your daily loop is several coding agents running in parallel across git worktrees, with processes that survive GUI restarts, that's the job Ferryx is built for, though it's still at v0.1.0-alpha. Both projects are open source and worth a look; the links above lead to each one.
