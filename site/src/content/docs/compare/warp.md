---
title: "Ferryx vs Warp: Two Open Codebases, Different Licenses"
description: Warp open-sourced its client under AGPL-3.0 and sells cloud agent infrastructure. How that compares with Ferryx's SUL-1.0 local desktop workspace.
---

Ferryx and Warp are both built for engineers who run coding agents in a terminal, and that's roughly where the overlap ends. Warp is a commercial platform from Denver Technologies with three products: the Warp Terminal, the Warp Agent CLI, and Warp Factories, cloud infrastructure for fleets of coding agents. [Ferryx](https://github.com/Indosaram/ferryx) is a source-available desktop application under the Sustainable Use License (SUL-1.0), still early software on calendar-versioned releases, that runs locally on macOS, Windows, and Linux.

This page compares the two on what can be verified: source availability, licensing, architecture, and process handling. No benchmarks; the site's editorial rules bar publishing unmeasured performance claims. Verified 2026-09-19 against Ferryx v2026.09.18.1 and Warp's public repository, docs, and pricing page. Details in this space change weekly, so confirm anything that matters at the source.

## Source and licensing

Both codebases are readable, under licenses that permit very different things.

Warp's client is open source. The [`warpdotdev/warp`](https://github.com/warpdotdev/Warp) repository carries the client crates, and the README states that the `warpui_core` and `warpui` crates are MIT licensed while the rest of the repository is under AGPL v3. The server portion is not in that repository. So Warp is an open-source client talking to a closed, commercial backend, with community contributions accepted through a labelled issue-to-PR flow.

Ferryx's source is public on GitHub under SUL-1.0, which permits use and modification for your own internal business purposes or for non-commercial or personal use. SUL-1.0 is not an OSI open-source license, and AGPL-3.0 is. If reciprocal-copyleft open source is what you need, Warp's client qualifies on that axis and Ferryx doesn't; say so plainly rather than around it.

Ferryx's stack is Rust and Tauri v2, parsing terminal output with libghostty-vt and rendering through WGPU on a native child surface, with no Electron. The decision with the most practical weight is that a headless Rust PTY daemon owns the pseudoterminals and the GUI is just a client of it; close, reload, or crash the window, and running agent processes keep going. Output is buffered in a 512 KiB ring buffer per session with monotonic sequence numbers, so a reconnect replays exactly what was missed.

## How each handles running multiple agents

Warp's answer spans products. The Warp Terminal recognises third-party CLI agents automatically and layers features on them: a rich input editor, agent notifications, inline code review, and Remote Control. Its [docs](https://docs.warp.dev/agents/cli-agents/overview/) list fifteen supported agents, including Claude Code, Codex, OpenCode, Amp, Auggie, Copilot CLI, Cursor CLI, Gemini CLI, Droid, and Grok Build. The Warp Agent CLI is Warp's own coding agent, usable in any terminal. Warp Factories is the cloud tier: infrastructure for running fleets of agents across the development lifecycle, with documented enterprise use cases in code review, bug investigation, refactors and migration, and incident response, plus named solutions for financial services, insurance, and telecommunications.

Remote Control is worth calling out because Ferryx has a counterpart. In Warp you click the `/remote-control` chip in the agent utility bar and the session publishes to Warp's cloud with a shareable link, viewable from any browser with nothing installed and steerable if you grant edit access.

Ferryx approaches the same problem locally and without a cloud. The daemon owns the pseudoterminals independent of the GUI, so agents survive the window closing and replay the gap on reconnect. Parallel agent streams get git worktree isolation: each managed worktree lives in `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, so concurrent work doesn't share a checkout. Its remote client is self-hosted rather than published: an authenticated gateway, a 6-digit PIN or QR pairing, a custom DOM grid instead of xterm.js, and an outbound relay you can run yourself for machines behind NAT.

## A plain-language rundown

- **Source and license.** Warp's client is open source, AGPL-3.0 with MIT for its two UI crates; its server is not published. Ferryx is source-available under SUL-1.0, which isn't OSI open source.
- **Products.** Ferryx is one desktop application. Warp sells a terminal, an agent CLI, and cloud infrastructure for agent fleets.
- **Pricing.** Warp's free tier is $0, with paid tiers starting at $20 a month and a $200 tier, plus enterprise plans. Ferryx is free for personal and non-commercial use under SUL-1.0.
- **Where work runs.** Ferryx runs entirely locally. Warp runs locally too, and Warp Factories runs fleets of agents in cloud infrastructure.
- **Process survival.** Closing or reloading the Ferryx GUI doesn't kill running agent processes, and reconnecting replays missed output from a ring buffer with monotonic sequence numbers.
- **Remote viewing.** Warp publishes a session to its cloud and hands you a link. Ferryx pairs a device directly with your daemon over your own gateway or relay.
- **Agent breadth.** Warp documents enhanced support for fifteen named CLI agents. Ferryx runs any command in a pane and ships status detection for eleven agents.
- **Worktree isolation.** Ferryx manages worktrees at `.orca-worktrees/wt-<slug>` on `orca/<workspace-id>/<slug>` branches; Warp leaves worktrees to you or to the agent.
- **Enterprise surface.** Warp markets named solutions for financial services, insurance, and telecommunications and holds SOC 2 certification. Ferryx has no enterprise program; it's an early local desktop app.

## When Warp is the better choice

Here's the other side stated plainly. Warp is a mature commercial product with enterprise support, a dedicated agent CLI, cloud infrastructure for agent fleets, and a client codebase under a recognised open-source license; Ferryx is an early desktop app with none of that. If you need a vendor relationship, paid tiers, and documented industry solutions, particularly in financial services, insurance, or telecommunications, Warp covers that today. If AGPL matters to you more than SUL-1.0, that's a clean reason to pick Warp and it doesn't need qualifying.

What Ferryx offers instead is a workspace with no cloud in it: agents that survive a GUI restart, replay on reconnect, per-worktree git isolation, and a remote path that terminates on hardware you control. If those are the properties you care about, the trade makes sense; if they aren't, Warp's maturity wins.

## Where to get Ferryx

Current builds are on the [GitHub releases page](https://github.com/Indosaram/ferryx/releases/latest): a macOS universal DMG, an x64 Windows installer, and AppImage and .deb packages for Linux.
