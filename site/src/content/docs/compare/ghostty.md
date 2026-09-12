---
title: Ferryx and Ghostty
description: Ferryx is not a Ghostty competitor or fork. How Ferryx uses the libghostty-vt parser, credit to the Ghostty project, and when to simply use Ghostty.
---

Let's put the most important thing first: Ferryx is not a Ghostty competitor, not a Ghostty fork, and not affiliated with or endorsed by the Ghostty project. Ghostty is an excellent standalone terminal emulator, and many people should simply use it on its own.

If you came here looking for Ghostty, you'll find it at [ghostty.org](https://ghostty.org/) and [github.com/ghostty-org/ghostty](https://github.com/ghostty-org/ghostty). This page exists because Ferryx embeds a Ghostty library internally, and we'd rather explain that relationship plainly than leave you guessing.

## What Ghostty is

Ghostty describes itself as "a fast, feature-rich, and cross-platform terminal emulator that uses platform-native UI and GPU acceleration." It's MIT licensed and written in Zig. The repository had 60,989 GitHub stars as of September 2026; counts and feature sets change over time, so treat that number as a snapshot. What it does show is that a substantial community has formed around the project.

## How Ferryx uses libghostty-vt

Ferryx is a separate desktop application for running coding agents in parallel. Inside its terminal pipeline, the parsing layer is libghostty-vt, the Ghostty project's VT parsing library. Everything above that layer is Ferryx's own code:

- Terminal parsing: handled by libghostty-vt.
- Rendering: Ferryx's own WGPU pipeline, drawn on a native child surface.
- Process ownership: a headless Rust PTY daemon owns the pseudoterminals, so closing or reloading the GUI doesn't kill running agent processes. Output is buffered in a ring buffer with monotonic sequence numbers, so a reconnect replays what was missed.
- Agent isolation: git worktrees, with each managed worktree in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`.
- Around the terminal: embedded browser tabs on native WebViews, splittable beside terminal panes, plus an authenticated mobile remote client that renders the terminal with a custom DOM grid.

So the relationship is one library, used for parsing, embedded in an application that isn't a terminal emulator in the traditional sense. Ferryx is independent. It isn't maintained by the Ghostty team, doesn't speak for them, and shouldn't be read as their product.

## When to just use Ghostty

- You want a great terminal for everyday shell work. Ghostty is built for exactly that, as a focused standalone app. Use it.
- Platform-native UI and GPU acceleration appeal to you. That's Ghostty's stated approach, and Ferryx isn't a replacement for that experience.
- You simply need a terminal and nothing else. There's no reason to install Ferryx to get one; that's not what it's for.

## When Ferryx might fit

- You're running several coding agents in parallel and want each one isolated in its own git worktree rather than sharing a single checkout.
- Agent processes should survive closing or reloading the GUI, with missed output replayed on reconnect.
- You want browser tabs beside terminal panes, or an authenticated mobile remote view.

A caveat belongs here: Ferryx is at v0.1.0-alpha, with a narrow scope. It doesn't aim to be a general purpose terminal emulator, and it isn't one yet.

## Credit

Ferryx's terminal parsing rests on the Ghostty project's work through libghostty-vt. That's a real debt, worth stating plainly: thank you to the Ghostty maintainers and contributors for building it and releasing it under the MIT license. Both projects ship under MIT. If this page sends you to Ghostty instead of Ferryx, that's a good outcome, not a failure of the page.

Project links: [ghostty.org](https://ghostty.org/), [github.com/ghostty-org/ghostty](https://github.com/ghostty-org/ghostty), and Ferryx at [github.com/Indosaram/ferryx](https://github.com/Indosaram/ferryx).
