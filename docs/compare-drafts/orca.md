---
title: "Ferryx vs Orca: Agent IDE and Worktree Architectures"
description: Ferryx and Orca both manage parallel coding agents across git worktrees. Compare embedded browser IDEs with daemon-backed terminal workspaces.
draft: true
verified: Facts cross-checked against docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md (2026-09-12 research) on 2026-09-25.
---

[Orca](https://github.com/stablyai/orca) is an Agent Development Environment (ADE) created by Stably AI. Ferryx is a source-available desktop terminal and worktree manager built with Rust and Tauri v2. Both systems address the operational frictions of multi-agent development by running parallel agents inside isolated git worktrees. Their differences lie in how they manage process lifecycles, daemon upgrades, and text editing.

## The core architectural difference

Orca packages an all-in-one desktop GUI environment combining multi-model CLI switching, an embedded Chromium browser with DOM element selection, and a built-in code editor. However, users document severe operational issues with Orca's daemon management during application upgrades: previous generation daemons (`daemon-vNN`) are not cleanly terminated, which in reported cases accumulated up to 370 orphan processes and consumed 25 GB of swap space (issue #9138). Furthermore, users report typing lag in Orca's built-in code editor alongside a lack of Vim keybindings, making the editor sluggish for daily use. Running Orca headlessly on remote Linux servers is also difficult without installing full desktop environments like XFCE.

Ferryx decouples its GUI from a headless Rust PTY daemon that owns the terminal lifecycle. Terminal parsing is handled by libghostty-vt and rendering runs on WGPU native child surfaces. Closing, crashing, or reloading the desktop GUI leaves running PTY processes intact. Ferryx maintains a 512 KiB sequenced ring buffer per session, allowing reconnecting clients to replay unread output without orphaned processes or leaking swap.

## How each isolates parallel work

Orca executes multiple LLM agents across independent git worktrees in parallel, enabling users to switch between models (such as Claude Code, Codex CLI, and models via OpenRouter or local hosts) without platform lock-in.

Ferryx allocates managed git worktrees in `.orca-worktrees/wt-<slug>` on branches named `orca/<workspace-id>/<slug>`. This enforces strict workspace isolation while preserving direct access to the underlying terminal emulator and native WebView browser tabs.

## Capabilities at a glance

- As of September 12, 2026, Orca had 67,032 GitHub stars on `stablyai/orca`.
- Orca functions as an Agent IDE with multi-model switching (OpenRouter, local models, Claude Code, Codex) across git worktrees.
- Orca includes an embedded Chromium browser with element-selection design mode, sending DOM context back to agents.
- Documented Orca issues include stale versioned daemon accumulation (up to 370 processes and 25 GB swap) upon updates, and typing lag with missing Vim bindings in its editor.
- Ferryx is licensed under SUL-1.0 and built with Rust and Tauri v2.
- Ferryx uses libghostty-vt and WGPU for native surface rendering, backed by a persistent headless Rust PTY daemon and 512 KiB ring buffers.
- Ferryx isolates worktrees in `.orca-worktrees/wt-<slug>` on `orca/<workspace-id>/<slug>`.

## What Orca does better

Orca provides a built-in Chromium browser with an interactive design mode that allows developers to click DOM elements directly, attach comments, and send that visual context straight to agents. It also acts as a vendor-neutral orchestrator hub featuring flexible multi-model switching (supporting OpenRouter, local LLMs, Claude Code, and Codex CLI) with an integrated code diff reviewer and editing environment in a single application.

## Sources

- Orca by Stably AI, repository `stablyai/orca`, websites, and 67,032 GitHub stars as of 2026-09-12 → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca.
- Multi-model parallel execution across git worktrees → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca, source [S7].
- Neutral hub switching across OpenRouter, local LLMs, Claude Code, and Codex → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca, source [S8].
- Embedded browser with DOM element selection and prompt feedback → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca, source [S9].
- App updates leaving stale daemons (370 processes, 25 GB swap pressure) → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca, source [S11].
- Built-in editor typing latency and lack of Vim keybindings → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca, source [S12].
- Difficult headless Linux server deployment requiring XFCE desktop workarounds → docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md §2 Orca.
- Ferryx desktop architecture, SUL-1.0 license, libghostty-vt, WGPU, headless daemon, ring buffer replay, and worktree layout → site/src/content/docs/compare/crystal.md.

## Where to read next

Compare other agent platforms on the [Ferryx comparison index](/compare/), or download the latest desktop packages on the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest).
