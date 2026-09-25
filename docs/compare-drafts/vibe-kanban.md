---
title: "Ferryx vs Vibe Kanban: Agent Orchestration and Worktrees"
description: Ferryx and Vibe Kanban both manage parallel coding agents across git worktrees. Compare web-based kanban boards with native desktop terminals.
draft: true
verified: Facts cross-checked against docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md (2026-09-12 research) on 2026-09-25.
---

[Vibe Kanban](https://github.com/BloopAI/vibe-kanban) is an open-source git worktree-based agent orchestration board created by Bloop AI. Ferryx is a desktop terminal and workspace manager written in Rust on Tauri v2. While both systems isolate autonomous coding agents using git worktrees, they diverge completely in interface philosophy: a card-based kanban dispatch board versus a native developer terminal.

## The core architectural difference

Vibe Kanban was built by Bloop AI as a browser-accessible kanban interface where developers assign declarative tasks to agents (such as Opencode) across automated worktrees. However, Bloop AI dissolved in April 2026, freezing official commits and leaving maintenance to community forks under Apache-2.0. Prior to dissolving, Bloop pushed an update forcing cloud logins and disabling offline boards, culminating in release 0.1.44, which locked local projects into an export-only state (requiring users to downgrade to 0.1.43 or patch the code). Furthermore, Vibe Kanban does not automatically clean up completed worktrees or build artifacts, with users reporting over 26 GB of disk space consumed by abandoned directories after two days of use.

Ferryx is an actively developed local-first desktop application with no cloud login requirements. It runs a headless Rust PTY daemon that maintains terminal processes independently of the UI. A 512 KiB sequenced ring buffer retains terminal output for reconnection replay, and rendering is handled via WGPU and libghostty-vt on native child surfaces.

## How each isolates parallel work

Vibe Kanban partitions work by automatically spawning git worktrees for each card, allowing multiple agents to modify code in parallel without cross-contaminating files, and enabling status tracking from mobile browsers.

Ferryx places managed worktrees in `.orca-worktrees/wt-<slug>` on branches named `orca/<workspace-id>/<slug>`. This keeps concurrent agent executions in separate git checkouts while providing direct terminal access and native browser splits.

## Capabilities at a glance

- As of September 12, 2026, Vibe Kanban had 28,061 GitHub stars on `BloopAI/vibe-kanban`.
- Vibe Kanban uses an Apache-2.0 licensed community fork after Bloop AI dissolved in April 2026.
- It provides a declarative kanban card board where agents work in parallel worktrees, with mobile browser status tracking.
- Documented issues include uncleaned worktrees consuming 26 GB+ disk space, forced cloud logins, and the 0.1.44 update locking local projects into export-only mode.
- Ferryx is licensed under SUL-1.0 and runs locally without cloud account requirements.
- Ferryx features a headless Rust PTY daemon with 512 KiB ring buffers, WGPU rendering via libghostty-vt, and `.orca-worktrees/wt-<slug>` worktrees.

## What Vibe Kanban does better

Vibe Kanban provides a visual, asynchronous kanban card board where developers and non-technical stakeholders can write declarative task cards and review progress without monitoring raw terminal output. It also allows developers to inspect card status and agent progress remotely via a standard mobile web browser.

## Sources

- Vibe Kanban by Bloop AI, repository `BloopAI/vibe-kanban`, websites, and 28,061 GitHub stars as of 2026-09-12 → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban.
- Asynchronous task recovery during 2-5 minute agent execution intervals → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban, source [S35].
- Parallel worktree isolation with Opencode agents and mobile browser status tracking → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban, source [S36].
- Declarative task dispatch and intuitive card UI → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban.
- Forced cloud login and removal of offline kanban board → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban, source [S37].
- Bloop AI dissolution in April 2026, Apache-2.0 community fork, and 0.1.44 update locking local projects to export-only → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban, source [S38].
- Uncleaned worktrees and build artifacts consuming ~26 GB disk space in 2 days → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §8 Vibe Kanban, source [S39].
- Ferryx desktop architecture, SUL-1.0 license, libghostty-vt, WGPU, headless daemon, ring buffer replay, and worktree layout → site/src/content/docs/compare/crystal.md.

## Where to read next

Compare other agent platforms on the [Ferryx comparison index](/compare/), or inspect the latest Ferryx builds on the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
