---
title: "Keep Agent Sessions Alive After an SSH Drop"
description: "A dropped SSH connection should not kill your coding agents. Learn which layers keep processes running and how daemon-owned sessions remove the risk."
---

**Keep Agent Sessions Alive After an SSH Drop.** The fix depends on which layer owns your agent's process: if the agent belongs to the SSH connection itself (a shell started by sshd), the drop kills it unless you disown it; if it belongs to a layer below — tmux, screen, or a headless daemon — the connection is merely a viewport that can come and go. Getting agents to survive Wi-Fi hops, laptop lids, and flaky tunnels means moving ownership down a layer, and the settings that keep connections healthy are a separate concern from the settings that keep *processes* alive.

![keep-alive-agent-sessions-ssh-drop cover](/images/blog/keep-alive-agent-sessions-ssh-drop/cover.png)

## Which layer owns the process today

Run `ps -o pid,ppid,command -p <agent-pid>` on the machine hosting your agent and read the parent chain. Three shapes appear in practice:

**Owned by sshd's shell.** The common case: you ran `ssh host`, got a shell, launched the agent there. The agent's grandparent is `sshd`. When the connection drops, the shell receives SIGHUP, the kernel walks the process group, and the agent dies with it. Every solution below is, at root, a way of re-parenting the agent away from this chain.

**Owned by a multiplexer server.** You ran `tmux attach`, launched the agent inside a tmux pane. Its parent chain leads to `tmux-server`, a process that predates the SSH session and outlives it. The connection drop now only detaches your *view*; the agent never hears about it. This is why the long-standing advice for agent runs is "put it in tmux."

**Owned by a daemon.** The agent was started by a headless service — Ferryx's daemon, a user service, a supervisor — and the SSH session (if any) attached to *it*. The daemon owns every PTY independently of any client connection; sessions exist in daemon memory, not in any terminal's. There is no connection whose death matters, because no connection ever owned anything.

The distinction is visible before you test it: check whether your agent's parent is `sshd`, `tmux-server`, or a daemon process. If it is sshd, you are one network blip from losing the run.

## Keepalive settings versus ownership redesign

Two families of settings get conflated here, and they solve different problems.

**Keepalives keep the *connection* from going idle.** `ServerAliveInterval` on the client sends traffic every N seconds; `ClientAliveInterval` on the server does the equivalent inbound. `TCPKeepAlive` handles the transport level. These defeat idle-timeout disconnections — the router that drops you after fifteen silent minutes — and they are genuinely useful for interactive sessions. They do nothing for the ownership problem: when a drop happens anyway (IP change, sleep, tunnel crash), the agent still dies if sshd owned it. Keepalives reduce the frequency of the fatal event; they do not remove its consequence.

**Ownership changes remove the consequence.** `nohup`/`disown` re-parent a single command out of the shell's blast radius — a fine patch for one batch run, awkward for interactive agents you reattach to. Multiplexers re-parent everything by construction. Daemons take it further: because the PTY belongs to the daemon, reattaching is not "resuming a survivor" but "connecting to something that never noticed you left." The agent's stdin/stdout are daemon-owned pipes; clients attach and detach as pure consumers.

The layered answer many teams land on: keepalives for connection hygiene, *plus* an ownership layer for survivability. They compose rather than compete — the keepalive makes drops rarer, the ownership layer makes remaining drops boring.

## Migration path that keeps running work

You cannot re-parent a running agent the way you can edit a config file; the change applies to sessions launched *after* the new layer exists. The safe migration keeps today's runs alive while tomorrow's move down a layer:

1. **Audit what is running and what owns it.** `ps` the parent chains (as above). Anything whose parent is sshd and has unsaved task state is the migration's protected set — do not touch it yet.
2. **Stand up the lower layer beside the current one.** Install/start tmux or the daemon service now, verify with a throwaway session: launch something long, kill the SSH client, confirm survival by reattaching. This is the drill that proves the layer before real work depends on it.
3. **Move the next launch, not the current one.** New agent runs start inside the new layer — a tmux window, or a daemon session — while existing runs continue under their old ownership until they finish naturally. No kill, no resurrection risk.
4. **Point reconnection habit at the layer, not SSH.** After migration, your ritual becomes "attach to the session manager" (tmux attach / open the client), not "SSH and find where I was." The SSH hop becomes one viewport among several — including the phone client a paired remote setup gives you.
5. **Add keepalives where sessions stay interactive over SSH.** With ownership safe, `ServerAliveInterval`-style settings earn their place again by keeping the *viewport* sticky; misconfigured, they still cause the opposite (too-frequent pings read as abuse on some servers).

Ferryx's placement in this map is the daemon row: sessions launch under the daemon regardless of client, which is what the [remote sessions documentation](/use-cases/remote-terminal-access/) describes and what [PTY persistence across sleep](/blog/pty-persistence-across-sleep/) exercises under the harshest local condition — a sleeping laptop.

![Keep Agent Sessions Alive After an SSH Drop illustration](/images/blog/keep-alive-agent-sessions-ssh-drop/body-1.png)

## FAQ

### Does nohup make an interactive agent reattachable?

No — `nohup` lets a process *survive* hangup but gives you no way back into its terminal: output goes to a file, and there is no stdin to type into. It is the right tool for batch jobs whose result you read from logs, and the wrong tool for interactive coding agents you need to drive after reconnecting. Survival and reattachability are separate features.

### What SSH settings actually prevent disconnections?

The useful ones are keepalive families: `ServerAliveInterval`/`ServerAliveCountMax` client-side (or `ClientAliveInterval` server-side) to defeat idle timeouts, and correct `TCPKeepAlive` behavior for transport-level liveness. They cannot survive IP changes, carrier switches, or deep NAT rebinding — for those, connection-*migration* tools or simply owning sessions below the connection are the answer.

### Is tmux enough to survive SSH drops, or do I need something more?

For pure connection loss, tmux is sufficient and battle-tested: the server owns processes, drops only detach your view. What tmux does not change: per-machine scope (the server lives on one host), narrative recovery after crashes (layout plugins restore panes, not task state), and repository isolation (still your worktrees to manage). Whether "more" is needed depends on which of those three you actually hit.

### How does a paired phone client fit into surviving connection drops?

It treats the daemon as the session's home and each client — laptop now, phone later — as a disposable view. A drop on one network path is a viewport event: reattach from any client and the session stream continues, because input/output live at the daemon layer. The [pairing guide](/blog/pair-ferryx-with-phone/) covers the pairing side; the [architecture page](/docs/architecture/) describes the ownership model underneath.
