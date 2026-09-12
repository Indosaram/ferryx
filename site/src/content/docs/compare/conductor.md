---
title: "Ferryx vs Conductor: Local Agents vs Cloud Agents"
description: Ferryx and Conductor both orchestrate coding agents. Compare a local-first workflow with Conductor Cloud's microVMs, multiplayer, and enterprise support.
---

Comparing [Conductor](https://conductor.build/) and [Ferryx](https://github.com/Indosaram/ferryx) is mostly a question of where the agents run. Conductor's positioning is "Run a team of coding agents in the cloud": Conductor Cloud executes agents on isolated microVMs, and multiplayer features let a team share a workspace link, see who's active, follow the work, and prompt agents together in real time. Ferryx is an MIT-licensed desktop terminal built with Rust and Tauri v2 that keeps every agent on your own machine, with no Ferryx cloud service at all.

This page is for engineers choosing between them. It covers the architectural difference, how each tool separates parallel work, and where each one is the better fit. Both products change quickly, so verify details on the linked sites.

## The core architectural difference

Ferryx runs entirely on your machine. Terminal parsing goes through libghostty-vt, rendering goes through WGPU on a native child surface, and there's no Electron in the stack. A headless Rust PTY daemon owns the pseudoterminals, so closing or reloading the Ferryx GUI doesn't kill running agent processes. Output is buffered in a ring buffer with monotonic sequence numbers, so a reconnect replays what was missed.

Conductor runs agents on isolated microVMs in Conductor Cloud. Workspaces live on the vendor's infrastructure instead of on one laptop, so keeping agents alive is the service's job rather than yours.

Local-first is a trade-off, not a free win. With Ferryx, your machine has to stay on for agents to keep working, and there's no shared cloud state for teammates to gather around. Conductor inverts that: agents run on hosted infrastructure, so nobody's laptop is the dependency, and in exchange your workflow depends on the service being available.

## How each isolates parallel work

Both tools run more than one agent, but the fence around each piece of work is built differently.

Ferryx uses git worktrees. Each managed worktree lives in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, so concurrent agents edit separate checkouts of the same repository and merges stay explicit.

Conductor's fence is the microVM. Agents on Conductor Cloud run on isolated microVMs, so the boundary lives at the virtualization level in the cloud rather than inside your repository.

## Capabilities at a glance

- As of September 2026, Ferryx is at v0.1.0-alpha and ships as a macOS universal DMG, a Windows build through the Microsoft Store, and Linux AppImage and .deb packages. It's MIT licensed.
- A headless PTY daemon keeps agent processes alive across GUI closes and reloads, and replays buffered output on reconnect.
- Git worktree isolation puts each agent's work in `.orca-worktrees/wt-<slug>` on an `orca/<workspace-id>/<slug>` branch.
- Embedded browser tabs run on native WebViews and can split beside terminal panes.
- An authenticated mobile remote client renders the terminal with a custom DOM grid and doesn't depend on xterm.js.
- Conductor Cloud runs agents on isolated microVMs.
- Its multiplayer covers shared workspace links, presence, following work, and prompting agents together in real time.
- As of September 2026, Conductor's site states "Trusted by 100k+ builders". That's the vendor's own claim, not an independent count.
- Conductor publishes changelog, docs, pricing, and enterprise pages, and offers a desktop download.

## When Conductor is the better choice

Choose Conductor when the team wants agents running in the cloud on isolated microVMs and doesn't want progress tied to one developer's laptop staying awake. Real-time collaboration is the second clear reason: sharing a workspace link, seeing who's active, and prompting agents together are part of the product. If your organization wants an enterprise conversation, Conductor publishes enterprise pages alongside [docs and pricing](https://conductor.build/).

## Where to get Ferryx

Ferryx is early software and both projects change quickly, so treat details on this page as a snapshot. If the local-first trade-off suits how your team works, the latest builds are on the [Ferryx releases page](https://github.com/Indosaram/ferryx/releases/latest).
