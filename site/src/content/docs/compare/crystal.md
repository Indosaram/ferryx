---
title: "Ferryx vs Crystal (Nimbalyst): Parallel Agent Worktrees"
description: Ferryx and Nimbalyst, formerly Crystal, both run coding agents in parallel git worktrees. Compare the desktop architectures before choosing one.
---

[Nimbalyst](https://github.com/stravu/crystal), formerly called Crystal, is the closest peer to Ferryx in concept: a desktop app that runs multiple coding agents in parallel git worktrees. Its repository describes the project as "Run multiple Codex and Claude Code AI sessions in parallel git worktrees. Test, compare approaches & manage AI-assisted development workflows in one desktop app." [Ferryx](https://github.com/Indosaram/ferryx) sits in the same space: an MIT-licensed desktop terminal that also keeps parallel agent work in git worktrees. The overlap is real, and the differences are architectural. Those differences decide which tool fits your setup.

One naming note before the details: the project was renamed from Crystal to Nimbalyst. Both names point to the same repository, so this page uses both.

## The core architectural difference

The two apps are both desktop software, so the interesting differences sit inside the client and its runtime.

Ferryx is built with Rust and Tauri v2. Terminal parsing runs through libghostty-vt, and WGPU renders the terminal on a native child surface, with no Electron in the stack. A headless Rust PTY daemon owns the pseudoterminals, so closing or reloading the Ferryx GUI doesn't kill running agent processes. Output is buffered in a ring buffer with monotonic sequence numbers, and a reconnect replays what was missed.

Nimbalyst's primary language is TypeScript, and its stated focus is managing multiple Codex and Claude Code sessions from one desktop app.

Ferryx has no cloud service: code and agent processes stay on your machine. That's a trade-off, not a pure win. The machine has to stay on for agents to keep running, and there's no shared cloud state for anyone else to see.

## How each isolates parallel work

Both tools lean on the same git mechanism, which keeps this part simple.

Ferryx puts each managed worktree in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, so concurrent agents work in separate checkouts of the same repository and merges stay explicit.

Nimbalyst, per its repository description, runs multiple Codex and Claude Code sessions in parallel git worktrees. Agents get separate checkouts the same way, while you test and compare approaches from one window.

## Capabilities at a glance

- As of September 2026, the repository had 3,115 GitHub stars. The project is MIT licensed and its primary language is TypeScript.
- Its core workflow, per the repository description: run multiple Codex and Claude Code AI sessions in parallel git worktrees, test and compare approaches, and manage AI-assisted development in one desktop app.
- A rename from Crystal to Nimbalyst means both names refer to the same project.
- As of September 2026, Ferryx is at v0.1.0-alpha and ships as a macOS universal DMG, a Windows build through the Microsoft Store, and Linux AppImage and .deb packages.
- Terminal parsing goes through libghostty-vt and rendering through WGPU on a native child surface, with no Electron.
- The headless PTY daemon keeps agent processes alive across GUI closes and reloads and replays missed output on reconnect.
- Embedded browser tabs on native WebViews split beside terminal panes.
- An authenticated mobile remote client renders the terminal with a custom DOM grid and doesn't depend on xterm.js.

## When Crystal (Nimbalyst) is the better choice

Nimbalyst is the more established project in exactly this niche, with a larger community: 3,115 GitHub stars as of September 2026. If you want a tool that more users have already exercised, that's a practical reason. It also explicitly supports Codex and Claude Code sessions, which matters when those are the agents you run day to day. Check [the repository](https://github.com/stravu/crystal) before deciding, since both projects change quickly.

## Where to get Ferryx

Ferryx is early software, so treat what's on this page as a snapshot too. If the trade-offs above suit how you work, the latest builds are on the [Ferryx releases page](https://github.com/Indosaram/ferryx/releases/latest).
