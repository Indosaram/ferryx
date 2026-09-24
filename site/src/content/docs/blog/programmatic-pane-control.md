---
title: "Send Keystrokes to a Terminal Pane Programmatically"
description: "Driving a specific pane from scripts or agents needs a real control channel. Compare tmux send-keys, control mode, and daemon APIs for pane input."
---

**Send Keystrokes to a Terminal Pane Programmatically.** The reliable ways to inject input into a specific terminal pane are tmux's `send-keys` (target a pane by session/window/pane address and write bytes to its PTY), tmux control mode (a structured client/server protocol over which scripts receive events and send commands), and daemon-style session APIs such as Ferryx's Unix domain socket protocol (address sessions as objects over a socket instead of addressing panes in a grid). Which one you should build on depends on how the pane is identified, how input and output are synchronized, and whether the driving program is a shell script or a long-lived agent.

![programmatic-pane-control cover](/images/blog/programmatic-pane-control/cover.png)

## Control channels compared

**tmux send-keys** is the blunt instrument, and for good reason: it is immediate and needs no daemon design. You name a target (`-t session:window.pane`), pass literal keys or key names (`Enter`, `C-c`, `Escape`), and tmux writes them to that pane's PTY as if a human typed. Scripts use it to drive tests in one pane while watching another, or to wake an agent with a confirmation it expects.

Its limits are the limits of pretending to be a human. There is no acknowledgement that the target *processed* the input — only that tmux handed bytes to the PTY. Timing becomes guesswork: insert a sleep after send-keys and you have encoded a race you will lose under load. Special keys and bracketed-paste sequences must be spelled correctly or an agent's editor-mode TUI receives garbage. And identity is positional — if the grid re-splits, your saved `2.1` address now points at a different program.

**tmux control mode** (`tmux -C attach`) upgrades the relationship. Instead of poking panes by address, the controller speaks a line protocol: tmux pushes structured events (pane output, layout changes, session lifecycle) to every control client, and the client submits commands back — including `send-keys` targets resolved against live state. This is how terminal multiplexer integration tests and fleet supervisors build real loops: *observe event, decide, inject input*, rather than *sleep, inject, hope*.

Control mode still inherits tmux's model: sessions live in one per-user server on one machine, targets are pane addresses, and the output stream is text events rather than typed characters — you see what happened, with the same reconstruction burden scrollback always has. For multi-machine fleets, the controller must reach each server (SSH, sockets), which is where the architecture starts to strain.

**Daemon session APIs** flip the addressing model. Instead of "pane 2.1 in this grid," sessions are first-class objects with stable identities that clients connect to over a socket — commands in, structured output frames out — independent of any pane arrangement. A driver does not need to know where a session is *displayed*; it needs the session's identity. Reattaching, resizing, and routing output are the daemon's job, not the script's. Ferryx's UDS protocol is built exactly this way: a fixed binary stream framing over a Unix domain socket, with pane splits represented as a tree the client renders, rather than addresses the client must track.

For pane-input scripting the practical difference is synchronization. send-keys gives bytes with no receipt; control mode gives events plus commands over one server; a framed daemon protocol can give you sequenced output — you know input N was consumed because you can observe the stream position after it, not because you slept.

## Pinning input to the right pane safely

Whichever channel you pick, three failure classes recur, and each has a mechanical defense:

1. **Wrong-target input.** Positional addresses (`0.1`) drift when panes split, close, or layouts restore. Defense: address by stable identity — tmux pane titles plus a lookup you re-resolve per command, or (stronger) session IDs the daemon assigns once and never reuses.
2. **Input arriving mid-edit.** A keystroke injected while the target TUI is between reads can land in the wrong buffer state — the classic "agent got `y` while its prompt was open." Defense: sequence through observable state: wait for the target's prompt marker in the output stream, then send, then confirm the expected response frame. Sleeps are a bet against the scheduler; event waits are a handshake.
3. **Input that must not leak.** Sending a secret (token, password) through `send-keys` leaves it in shell history and tmux control-prompt logs. Defense: use channels whose payloads stay out of history — paste-style input with bracketed sequences, stdin piping, or protocol fields the daemon treats as data.

Add idempotence to the list when the driver is an agent: a retried command after a timeout should either be safely re-sendable or guarded by an operation ID, because "send again" is how confirmation prompts get double-answered.

## Patterns for agent-driven panes

**Supervisor pattern.** One control client owns the tmux server via control mode; workers request "run this in pane X" through it. Centralizes addressing and lets the supervisor re-resolve pane identities after every layout change. The supervisor is the single point that knows the grid.

**Session-object pattern.** Each unit of work is a session with an ID; drivers address the ID over the daemon socket, and the display (split layout, remote client, phone) is irrelevant to driving. This is what makes the same input path work for a local desktop pane and a remote browser pane — they are viewports onto the same addressed session.

**Echo-guard pattern.** After injecting input, wait for an explicit completion signal — an exit banner, a result frame, a marker line the target is guaranteed to emit — before injecting the next. Combined with stable addressing, this converts a timing-based script into an event-driven one, which is the only kind that survives a loaded machine.

Each pattern maps to a real Ferryx mechanism: sessions are daemon-owned objects (session-object), input flows over a framed UDS protocol with sequenced output (echo-guard via stream positions), and pane arrangement is a client-side tree the daemon never confuses with identity. The [daemon protocol notes](/docs/architecture/) and [UDS protocol post](/blog/daemon-uds-protocol/) carry the details of framing and addressing.

![Send Keystrokes to a Terminal Pane Programmatically illustration](/images/blog/programmatic-pane-control/body-1.png)

## FAQ

### Can a coding agent drive tmux panes programmatically today?

Yes — practically every serious tool in this space does. The standard path is tmux control mode (events plus commands) or `send-keys` against addressed panes; CI harnesses and fleet supervisors run agents this way. The engineering effort is not the injection itself but identity resolution and synchronization: knowing *which* pane and *when it is ready*, which control mode's event stream helps with but does not fully solve.

### What's wrong with just sleeping after send-keys?

A sleep converts a synchronization problem into a scheduling bet. Under load (exactly when parallel agents run), the target can be slow to consume input — so the next keystroke lands early — or fast, leaving dead time that compounds across hundreds of injections. Event-driven waits (observe the target's response, then proceed) remove the bet; sleeps only shrink the window in which you lose.

### How is injecting keys into a pane different from writing to its PTY directly?

Conceptually nothing — a pane *is* a PTY consumer — but ownership differs. Writing to a PTY file descriptor bypasses whatever multiplexer or daemon believes it owns that terminal's input, risking interleaving with human typing and losing audit context. Channels like send-keys or a daemon API go through the owner, so input is serialized with everything else the pane receives.

### Does Ferryx expose a pane-driving API?

Yes, at the protocol level: sessions and panes are addressed over the daemon's Unix domain socket with structured framing, so a driver sends input to a session ID and observes sequenced output back — independent of whether a desktop pane, remote client, or no client at all is displaying it. The framing and lifecycle details live in the [architecture documentation](/docs/architecture/).
