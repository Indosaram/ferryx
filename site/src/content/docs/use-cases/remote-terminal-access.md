---
title: "Remote Terminal Access: Check Sessions From Your Phone"
description: Why long terminal runs are hard to watch from a phone, the usual SSH and VPN answers, and how Ferryx's remote gateway and PIN pairing work.
---

A long build or an agent run doesn't need you constantly, but it needs you occasionally: to check progress, to approve a prompt, to type the next command. Agent runs are the worst case, since they can run for tens of minutes and sometimes need a decision in the middle. Walking back to the desk each time works until you're in another room, in a meeting, or away from the machine entirely. What you want is a way to look at the session and act on it from whatever device is at hand.

## The usual answers

- **Plain SSH from a phone.** Nothing to install on the network side, and you get a real shell. The trade-offs are comfort and durability: mobile SSH clients vary in how well they handle touch keyboards and terminal sizing, and if the connection drops, processes started in that session die with it unless they were already detached.
- **tmux attached over SSH.** The session survives disconnects, which fixes durability. The cost is a second tool to learn, plus keybindings that fight a touch keyboard.
- **A VPN or Tailscale.** These solve reachability rather than sessions. They get your phone onto the same network as the machine, but you still need SSH and usually a multiplexer on top to keep a session alive across dropped connections.

All of these are legitimate. Engineers have used them for decades. Each one asks you to assemble the pieces yourself and accept some friction on a small screen.

## What Ferryx does

Ferryx (MIT licensed, Rust and Tauri v2) takes a different cut at this. Its headless Rust daemon owns the pseudoterminals instead of the desktop GUI, and an authenticated gateway serves a mobile web client. The pieces:

- **Sessions outlive the GUI.** Closing or reloading the desktop app doesn't kill running processes, because the daemon, not the window, owns the PTYs.
- **Reconnects replay what you missed.** Output lives in a ring buffer with monotonic sequence numbers. When a client reconnects, it receives the output it missed. If the buffer overflowed, the client is told there's a gap rather than being shown silently corrupted output.
- **Pairing uses a 6-digit PIN.** You pair a phone with the running daemon by entering a 6-digit PIN.
- **The mobile client is a custom DOM grid.** Ferryx doesn't ship xterm.js to the phone; the remote terminal renders through a purpose-built DOM grid.
- **Off-LAN access is configurable.** A relay or proxy URL can be set for reaching a machine that isn't on the same LAN or VPN.

In practice: start a run at your desk, leave, and open the paired web client on your phone to check progress or type the next command. The run itself never depended on the desktop window being open, and if you reconnect after a while, the client replays the output you missed from the buffer.

## A note on security

The verifiable parts are the ones listed above: the gateway requires authentication, pairing uses a 6-digit PIN, and the relay URL is configurable. That's the extent of what's documented today, so don't read more into it. Ferryx is at v0.1.0-alpha, and anyone exposing a terminal over a network should think about their threat model first. A terminal is a shell on your machine, and whoever can steer it can do anything that shell can. Keep remote access off unless you need it, and prefer a trusted network path when you can.

## Trying it

Ferryx ships as a macOS universal DMG, through the Microsoft Store on Windows, and as Linux AppImage and .deb packages. The project is MIT licensed and currently at v0.1.0-alpha. For the rest of the workspace, including split panes, embedded browser tabs, and managed git worktrees, see the [introduction](/docs/introduction/) and [git worktree workflow](/use-cases/git-worktree-workflow/).
