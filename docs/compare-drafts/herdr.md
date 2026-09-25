---
title: "Ferryx vs Herdr: Parallel Agent Multiplexing"
description: Ferryx and Herdr both orchestrate autonomous coding agents across workspaces. Compare desktop GUI vs terminal multiplexer architectures.
draft: true
verified: Facts cross-checked against docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md (2026-09-12 research) on 2026-09-25.
---

[Herdr](https://github.com/herdrdev/herdr) is a terminal multiplexer written in Rust by Ogulcan Celik specifically designed for autonomous coding agents. Ferryx is a desktop terminal application built with Rust and Tauri v2 that coordinates parallel coding agents. While both tools manage multiple concurrent agents, they approach the problem through fundamentally different interface layers: a terminal multiplexer (TUI) versus a native desktop workspace.

## The core architectural difference

Herdr operates as a single-binary terminal multiplexer that runs inside existing terminal emulators. It provides out-of-the-box mouse wheel scrolling and selection without complex config files, alongside multi-machine remote session management. However, users running agents across multiple remote hosts report that Herdr develops noticeable input lag after several hours, and its rapid TUI redraw cycles can cancel active text selections in outer terminals such as WezTerm. Herdr relicensed from AGPL to Apache-2.0 in July 2026 following contributor consent, and had recorded 85,000 downloads for its v0.9.0 release.

Ferryx is a standalone desktop application rather than an in-terminal multiplexer. Terminal parsing runs through libghostty-vt, and WGPU renders the terminal on a native child surface, avoiding the redraw-collision issues of nested multiplexers. A headless Rust PTY daemon owns the pseudoterminals, meaning closing or reloading the GUI does not kill running agent processes. Output is buffered in a 512 KiB sequenced ring buffer, and reconnecting replays missed stream chunks.

## How each isolates parallel work

Herdr tracks running agents via a collapsible left tab bar that displays workspaces alongside agent status badges (such as working, idle, or blocked) for tools like Claude Code and Codex CLI. It enables users to connect and synchronize sessions across multiple remote machines into one client.

Ferryx isolates parallel work at the filesystem level using git worktrees. Each managed worktree resides in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, keeping concurrent agent modifications in separate physical checkouts.

## Capabilities at a glance

- As of September 12, 2026, Herdr had 37,874 GitHub stars on `herdrdev/herdr`.
- Herdr transitioned its license from AGPL to Apache-2.0 in July 2026 with the agreement of around 30 core contributors, and logged 85,000 downloads for v0.9.0.
- Herdr ships as a single lightweight binary with default mouse wheel and selection support, tracking agent status badges in a left sidebar.
- Ferryx is licensed under the Sustainable Use License (SUL-1.0) and built with Rust and Tauri v2.
- Ferryx renders via WGPU on a native child surface using libghostty-vt, with a headless Rust PTY daemon and 512 KiB ring buffer replay.
- Ferryx isolates worktrees in `.orca-worktrees/wt-<slug>` with branch scheme `orca/<workspace-id>/<slug>`.

## What Herdr does better

Herdr runs directly within your existing terminal emulator as a single lightweight binary, requiring no GUI window manager or desktop environment. It provides built-in multi-host remote connection capabilities that allow developers to aggregate and track agent sessions running across multiple remote machines in a single TUI client. Its collapsible sidebar also exposes live agent status indicators (idle, working, blocked) without switching windows.

## Sources

- Herdr author Ogulcan Celik, Rust language, repository `herdrdev/herdr`, websites, and 37,874 GitHub stars as of 2026-09-12 → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr.
- Collapsible left tab bar showing workspaces and agent status badges (working, idle, blocked) → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr, source [S31].
- Single binary simplicity, default mouse scroll, and multi-remote machine connection → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr.
- Multi-remote machine input lag after hours of running agents → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr, source [S32].
- Rapid screen redraws breaking keyboard/vim text selection in WezTerm → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr, source [S33].
- License transition from AGPL to Apache-2.0 in July 2026, 30 contributor consent, and 85,000 v0.9.0 downloads → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §7 Herdr, source [S34].
- Ferryx desktop architecture, SUL-1.0 license, libghostty-vt, WGPU, headless daemon, ring buffer replay, and worktree layout → site/src/content/docs/compare/crystal.md.

## Where to read next

Compare other agent workspaces on the [Ferryx comparison index](/compare/), or inspect the latest Ferryx builds on the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
