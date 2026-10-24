---
title: "Ferryx vs Conductor: A Conductor Alternative on Windows and Linux"
description: Conductor is macOS only. Compare it with Ferryx on platforms, local worktrees, session survival, agent support, and Conductor Cloud's paid sandboxes.
---

The first question most people ask about [Conductor](https://www.conductor.build/) has a documented answer: it doesn't run on your machine unless that machine is a Mac. Conductor's install page states plainly, *"Conductor is not available for Windows or Linux yet"* ([conductor.build/docs/installation](https://www.conductor.build/docs/installation), checked 2026-10-24). [Ferryx](https://github.com/Indosaram/ferryx) ships a macOS universal DMG, an x64 Windows installer, and x86_64 AppImage and .deb packages for Linux, so the same workspace runs on all three.

That's the practical difference. The rest of this page covers what Conductor actually is, since its headline and its shipping product emphasise different things, and where the two tools genuinely diverge.

## Availability at a glance

| | Ferryx | Conductor |
| :--- | :--- | :--- |
| macOS | Universal DMG | Yes, the only supported platform |
| Windows | x64 installer (not code-signed yet) | Not available yet |
| Linux | AppImage and .deb (x86_64), plus a headless `ferryx-cli` | Not available yet |
| Licensing | Source-available under the Ferryx License, free for individuals and small companies | Closed source; free tier plus paid plans from $50/mo |
| Agents | Any command you can start in a shell; status detection ships for eleven | Claude Code, Codex, Cursor, and OpenCode |
| Mobile | Authenticated mobile web client, PIN pairing | Conductor for iOS app; the mobile app is listed under Pro |
| Cloud service | None | Conductor Cloud, on paid plans |

Verified 2026-10-24 against Ferryx v2026.10.11.1 and Conductor's [docs](https://www.conductor.build/docs), [pricing](https://www.conductor.build/pricing), and [changelog](https://www.conductor.build/changelog) (latest dated entry 0.91.0, 2026-10-15). Both products move quickly, so check the linked pages before you decide anything on them.

## What Conductor actually is

Conductor's homepage headline is "Run a team of coding agents in the cloud," which makes it sound like a hosted service. The app underneath is a Mac desktop application from Melty Labs, and its free tier runs entirely on your machine. Conductor's own docs describe local workspaces as git worktrees, usually created under `~/conductor/workspaces/<repo name>/<workspace name>`, with the agent editing files and running commands in that directory ([Git worktrees](https://www.conductor.build/docs/concepts/git-worktrees)).

Conductor Cloud is the hosted half, and it's a paid add-on rather than the default. Cloud workspaces are available on Pro, Teams, and Enterprise plans, starting at $50 a month; Conductor's pricing page says each cloud workspace is a Vercel sandbox with an 8-core CPU and 16 GB of RAM, running Amazon Linux 2023 in `us-east-1`, and that chat messages sent in cloud workspaces are stored on Conductor's servers ([pricing](https://www.conductor.build/pricing)). Multiplayer, the Conductor API, and the mobile app sit on the same paid tiers.

So the honest framing isn't local versus cloud. Both tools run agents locally by default, both use git worktrees to keep them apart, and Conductor adds a paid cloud tier that Ferryx has no equivalent for.

## Where the two actually differ

**Who owns the session.** This is the sharpest architectural split. Conductor's pricing FAQ states that local workspace sessions run on your machine and that "when your machine shuts off or you close the app, sessions terminate," which is why the cloud tier exists. In Ferryx, a headless Rust PTY daemon owns the pseudoterminals and the GUI is only a client, so closing or reloading the window leaves agents running. Output goes into a 512 KiB ring buffer per session with monotonic sequence numbers, and a reconnect replays what you missed. If the buffer wrapped while you were away, the client is told there's a gap instead of being shown a mangled transcript.

**Which agents run.** Conductor's docs say it runs Claude Code, Codex, Cursor, and OpenCode in parallel ([docs](https://www.conductor.build/docs)), and its changelog shows it bundling and updating its own copies of all four ([changelog](https://www.conductor.build/changelog)). Ferryx treats agents as ordinary commands: anything you can start in a terminal runs in a pane. What's agent-specific is status detection, and manifests ship for eleven of them, including Claude Code, Codex, Cursor, Cline, GitHub Copilot CLI, OpenCode, Grok, and Kimi.

**Where worktrees live.** Conductor puts workspaces outside the repository, under `~/conductor/workspaces/`. Ferryx keeps managed worktrees inside it, at `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`, with worktree paths jailed to the repository root. Neither is better in the abstract; they trade a tidy home directory against a repository that carries its own worktrees.

**Checking on a run from elsewhere.** Both tools now ship a phone path, and they work differently. Conductor released Conductor for iOS in 0.90.0 on 2026-10-02, for chatting with agents and merging PRs from your phone in cloud workspaces, and 0.91.2 added an experimental Remote control setting that lets the iOS app drive agents in your local workspaces ([changelog](https://www.conductor.build/changelog), checked 2026-10-24). Its pricing page lists the mobile app among the Pro features ([pricing](https://www.conductor.build/pricing)). Ferryx's mobile client is an authenticated web client rather than a native app: pairing uses a 6-digit PIN, the terminal renders through a custom DOM grid rather than xterm.js, it shows whatever process is in the pane rather than an agent chat, and an outbound relay covers machines that aren't on your LAN.

## When Conductor is the better choice

If you're on a Mac and your team wants agents that keep working after the laptop closes, Conductor Cloud answers that directly and Ferryx doesn't: there's no Ferryx cloud, so your machine has to stay awake. Multiplayer is the second clear reason, since sharing a workspace link, seeing who's active, and prompting agents together are built into the paid plans. Conductor also carries the things a funded product carries and an early project doesn't: a SOC 2 Type II attestation, enterprise plans with SSO and SCIM, bundled agent installations, a native iOS app, a diff viewer, and a review-and-merge flow that ends in a pull request.

Ferryx's counteroffer is narrower and easier to state: it runs on Windows and Linux, its source is public, it's free for individuals and small companies, it runs any CLI agent rather than four, and agents survive the GUI going away.

## Where to get Ferryx

Ferryx is early software and both projects change weekly, so treat this page as a dated snapshot rather than a standing fact. Current builds are on the [Ferryx releases page](https://github.com/Indosaram/ferryx/releases/latest), and every claim above about Ferryx is sourced on the [product facts](/docs/facts/) page.
