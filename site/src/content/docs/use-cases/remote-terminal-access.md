---
title: "Remote Terminal Access: Check Agent Sessions From Your Phone"
description: Claude and Warp now stream agent sessions to your phone. What each requires, and how Ferryx's self-hosted gateway covers any process in a pane.
---

A long build or an agent run doesn't need you constantly, but it needs you occasionally: to check progress, to approve a prompt, to type the next command. Agent runs are the worst case, since they can run for tens of minutes and sometimes need a decision in the middle. Walking back to the desk each time works until you're in another room, in a meeting, or away from the machine entirely. What you want is a way to look at the session and act on it from whatever device is at hand.

## The first-party options, which are real

Two vendors now solve this inside their own products, and any honest comparison has to start there.

**Claude Code Remote Control.** Run `claude remote-control` and the session becomes drivable from claude.ai/code or the Claude mobile app. It displays a session URL, and pressing space shows a QR code for your phone. Claude keeps running locally the whole time, so your filesystem, MCP servers, and project config stay available; the conversation stays in sync across terminal, browser, and phone; and Claude Code reconnects on its own after a laptop sleep or a network drop, queuing messages and permission prompts while the connection rebuilds. Server mode can hand each on-demand session its own git worktree with `--spawn worktree`. Push notifications land on your phone when a task finishes or Claude needs a decision. ([code.claude.com/docs/en/remote-control](https://code.claude.com/docs/en/remote-control), read 2026-09-19.)

**Warp Remote Control.** Click the `/remote-control` chip in Warp's agent utility bar and the running third-party agent session publishes to Warp's cloud, with the shareable link copied to your clipboard. Anyone with the link can watch from a browser with nothing installed, and you can grant edit access so a viewer sends input or approves commands. It covers every CLI agent Warp recognises, Claude Code and Codex included. ([docs.warp.dev/agents/cli-agents/remote-control](https://docs.warp.dev/agents/cli-agents/remote-control/), read 2026-09-19.)

If you're a Claude Code user on a paid plan, or a Warp user, one of those is probably your shortest path. Use it.

## Where they stop

Both carry boundaries their own docs state.

Claude Code Remote Control needs a claude.ai subscription on a Pro, Max, Team, or Enterprise plan; API keys aren't supported. It's unavailable on Amazon Bedrock, Google Cloud's Agent Platform, and Microsoft Foundry, and unavailable when `ANTHROPIC_BASE_URL` points anywhere other than `api.anthropic.com`, which rules out an LLM gateway or proxy. While the session is connected, the transcript is stored on Anthropic's servers, so organisations with Zero Data Retention requirements can't enable it. The local `claude` process also has to keep running: quit the terminal and the session goes offline, which is why Anthropic's docs suggest starting it inside tmux or screen on a remote machine. And it drives Claude Code, not the build script or the database shell in the next pane.

Warp's version publishes session state to Warp's cloud and stops syncing when you stop publishing. Warp's client is open source under AGPL-3.0, but the infrastructure the session publishes to isn't, so the session lives on Warp's servers while it's shared.

## What Ferryx does instead

Ferryx (SUL-1.0, Rust and Tauri v2) takes the same problem from the other end: it exposes the terminal itself, not one vendor's agent, and the gateway is yours.

- **Sessions outlive the GUI.** A headless Rust daemon owns the pseudoterminals instead of the desktop window, so closing or reloading the app doesn't kill running processes. There's no separate multiplexer to remember.
- **Reconnects replay what you missed.** Output lives in a 512 KiB ring buffer per session with monotonic sequence numbers. A reconnecting client receives the output it missed, and if the buffer overflowed it's told there's a gap rather than shown a silently corrupted transcript.
- **Pairing uses a 6-digit PIN or a QR code.** You pair a phone with the running daemon, and nothing in the flow depends on an account with anyone.
- **The mobile client is a custom DOM grid.** Ferryx doesn't ship xterm.js to the phone; the remote terminal renders through a purpose-built grid.
- **Off-LAN access runs over your own relay.** The daemon holds an outbound tunnel to a relay, so reaching a machine behind NAT needs no inbound port and no public IP. The relay is the `ferryx-relay` binary in the same repository, so you can run it on your own host instead of the default one.
- **Any process, not any agent.** Whatever is in the pane is what you see: Claude Code, Codex, a test runner, a migration, a shell.

In practice: start a run at your desk, leave, and open the paired web client on your phone to check progress or type the next command. The run never depended on the desktop window being open, and reconnecting after a while replays the output you missed.

The trade is honest. There's no push notification to your lock screen, no vendor keeping a transcript in sync for you, and no support contract. What you get is no subscription, no account, no per-agent limitation, and infrastructure you can host yourself.

## A note on security

The verifiable parts are the ones listed above: the gateway requires authentication, pairing uses a 6-digit PIN, and the relay URL is configurable. That's the extent of what's documented today, so don't read more into it. Ferryx is early software, and anyone exposing a terminal over a network should think about their threat model first. A terminal is a shell on your machine, and whoever can steer it can do anything that shell can. Keep remote access off unless you need it, and prefer a trusted network path when you can.

## Trying it

Ferryx ships as a macOS universal DMG, an x64 Windows installer, and Linux AppImage and .deb packages; a standalone `ferryx-cli` binary runs the same headless daemon on a Linux server with no GUI dependencies, though it isn't published by every release and can be built from source. The project is SUL-1.0 licensed and still early software. For the rest of the workspace, including split panes, embedded browser tabs, and managed git worktrees, see the [introduction](/docs/introduction/) and [git worktree workflow](/use-cases/git-worktree-workflow/); the [product facts](/docs/facts/) page lists the pairing, relay, and packaging details with their sources.
