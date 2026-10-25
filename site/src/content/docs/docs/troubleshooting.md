---
title: Troubleshooting
description: Find the Ferryx troubleshooting guide for a symptom - stuck agents, exited shells, pairing failures, input and rendering glitches, layout recovery, and daemon problems.
---

Start from what you see, then follow the linked guide. If none of them matches, ask in
[Discord](https://discord.gg/Z2hBkQEHUG) or open an issue on
[GitHub](https://github.com/Indosaram/ferryx/issues). Report security problems privately as
described in [SECURITY.md](https://github.com/Indosaram/ferryx/blob/main/SECURITY.md).

## Agents and sessions

| Symptom | Guide |
| :--- | :--- |
| An agent shows "working" but nothing moves | [When an agent looks stuck](/blog/agent-appears-stuck/) |
| A pane shows a "Shell exited" overlay | [What Shell Exited means and how to recover](/blog/fix-shell-exited-overlay/) |
| A session is wedged and does not respond | [Debugging stuck daemon sessions](/blog/debugging-daemon-sessions/) |
| More than one daemon process is running | [Multiple daemon processes](/blog/multiple-daemon-processes/) |
| Sessions after closing the window, a crash, or a reload | [Why closing the window does not kill your agents](/blog/session-survival-gui-crash/) |
| Sessions after the laptop slept | [Terminal sessions across sleep and suspend](/blog/pty-persistence-across-sleep/) |
| Output has a gap marker after reconnecting | [Understanding ReplayGaps](/blog/understanding-replaygaps/) |
| Inline images disappear from scrollback | [When inline images truncate in scrollback](/blog/scrollback-truncated-images/) |

## Worktrees and parallel agents

| Symptom | Guide |
| :--- | :--- |
| `.git/index.lock` errors with several agents | [Why parallel agents fight over index.lock](/blog/agent-conflict-git-lock/) |
| Dev servers collide on the same port | [Why parallel agents fight over ports](/blog/agent-port-conflicts-across-worktrees/) |
| Agents overwrite each other's database or migrations | [Parallel agents and database collisions](/blog/parallel-agent-database-conflicts/) |
| A worktree is on a detached HEAD | [Detached HEAD states and agent worktrees](/blog/detached-head-and-agent-worktrees/) |

## Layout and workspace

| Symptom | Guide |
| :--- | :--- |
| Panes restore into the wrong tree | [Recovering a corrupted workspace layout](/blog/recover-corrupted-layout/) |
| The layout did not come back after a restart | [Restoring your workspace layout](/blog/restoring-workspace-layout/) |

## Remote access and pairing

| Symptom | Guide |
| :--- | :--- |
| Sign-in link not arriving, device will not connect, relay unreachable | [Remote pairing troubleshooting checklist](/blog/pairing-troubleshooting/) |
| Typing on a remote session feels slow | [Understanding remote terminal input latency](/blog/remote-input-latency/) |
| Remote sessions after the VPN dropped | [Remote terminals when the VPN drops](/blog/remote-when-vpn-down/) |
| An SSH session froze | [Recovering an unresponsive SSH session](/blog/unresponsive-ssh-recovery/) |
| Agents died when SSH disconnected | [Keep agent sessions alive after an SSH drop](/blog/keep-alive-agent-sessions-ssh-drop/) |
| Korean (Hangul) input misbehaves on the mobile client | [Fixing Korean IME input in remote terminals](/blog/korean-ime-issues/) |

## Input, rendering, and platform

| Symptom | Guide |
| :--- | :--- |
| Paste does nothing or pastes the wrong thing | [Clipboard and paste failures across platforms](/blog/clipboard-paste-failures/) |
| Stale or ghosted grid after a monitor change | [Terminal resize ghosting after monitor changes](/blog/pty-resize-ghosting/) |
| Blurry text under Wayland fractional scaling | [Fixing blurry output under Wayland scaling](/blog/wayland-scaling-blur/) |
| Agent TUIs misbehave on Windows | [Windows ConPTY quirks with agent TUIs](/blog/windows-conpty-quirks/) |
| Notifications or the Dock badge never appear | [Notifications and Dock badge not appearing](/blog/notifications-not-firing/) |

## Install

| Platform | Guide |
| :--- | :--- |
| macOS | [Install Ferryx on macOS](/blog/ferryx-install-macos/) |
| Windows | [Install Ferryx on Windows](/blog/ferryx-install-windows/) |
| Linux | [Install Ferryx on Linux](/blog/ferryx-install-linux/) |
| Headless Linux host | [Headless Linux host setup](/blog/headless-linux-host-setup/) |
