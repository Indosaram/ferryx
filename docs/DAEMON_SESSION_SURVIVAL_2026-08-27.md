# Surviving a daemon restart

**Requirement.** An agent session must outlive a restart of the daemon, not merely a restart of the
GUI.

Today it does not, and no amount of UI-side repair can change that. This is an ownership problem in
the backend.

## What happens today

`PtySession` holds `pair.master` in daemon memory (`src-tauri/src/terminal/session.rs`). The slave is
dropped right after spawn, so the daemon process is the sole owner of the master fd. When that
process dies the fd closes, and the kernel sends `SIGHUP` to the foreground process group of the pty
session. The agent dies with it.

Verified directly rather than assumed — a parent that owns a pty master, spawns a child, and exits:

```
child pid: 63677
RESULT: child DIED after master closed (iter 1)
```

`iter 1` means the child was already gone within 100ms. `Drop for PtySession` never kills anything;
it only releases the master, and the kernel does the rest.

So **daemon death is unconditionally fatal to every session it owns.** The `daemonEpoch` mechanism
does not prevent this; it exists to *detect* it, and `src-tauri/src/daemon/AGENTS.md` codifies the
resulting policy: *"DO NOT Auto-Respawn Missing Sessions."* Reporting the death honestly is
deliberate.

## Correction to the earlier diagnosis

The previous write-up attributed the reported loss to the restore-side epoch bug. The evidence does
not support that as the cause of *this* incident:

| Probe | Value |
| :--- | :--- |
| daemon epoch | `1787837134521` = 2026-08-27 22:25:34 |
| daemon process start | Thu Aug 27 22:25:34 2026 |
| live sessions | 6, all `running: true`, `endSequence` 6-10 |

The epoch equals the process start time, so the running daemon booted at 22:25 — it is not the one
that held the agents. Its sessions carry single-digit sequence numbers, i.e. a bare prompt and
nothing else. The persisted `session_state.json` was written at 22:37, *after* that boot, which is
why its backend ids matched the live daemon and made the earlier reading look consistent.

A daemon restart alone fully explains the loss. The epoch bug is real and worth having fixed — it
destroys sessions in the case where the daemon *survives* and the UI wrongly detaches — but it is a
different failure, and fixing it does not deliver the requirement above.

## The constraint that shapes every option

It is not enough for something to hold the master fd open. Whoever holds it must also keep **reading**
it. A pty buffer is small and finite; once it fills, the agent blocks on `write` and freezes
mid-task. So a passive "fd keeper" does not work — the survivor must drain output into a buffer,
which means the survivor owns the ring buffer too.

That single fact is why the answer is a process split rather than a clever fd trick.

## Options

### A. Per-session holder process (dtach / abduco model)

Each session gets a small long-lived process that owns the pty master, drains it into the sequenced
ring buffer, and serves a per-session socket. The daemon becomes a router that connects to holders.

- Survives daemon crash, restart, and upgrade. Scrollback survives too, because the buffer lives in
  the holder.
- Costs one process per session and a new per-session protocol; the ring buffer and sequence logic
  move out of the daemon.

### B. One stable pty-core process, feature daemon on top

A single core process owns every pty and ring buffer. The feature-rich daemon — the part that churns
with each app release — talks to it and may restart freely.

- One extra process instead of N, same survival properties while the core stays stable.
- The core is a single point of failure for all sessions, and still needs a restart when its own code
  changes.

### C. Graceful handoff: `exec()` in place, or fd passing over `SCM_RIGHTS`

On a *planned* restart the old daemon `exec()`s the new binary. File descriptors survive `exec` unless
`FD_CLOEXEC` is set, so the pty master is never closed and the agent never receives the fatal `SIGHUP`.

Demonstrated end to end — a process owns a pty master, spawns a child on it, then replaces its own
image with a different binary:

```
[predecessor] pid=56434 owns master fd=3, child=56546
[predecessor] exec()ing the successor binary in place...
[successor] new process image, pid=56434, inherited master fd=3
[successor] read from pre-exec pty: tick0 tick1 tick2 tick3 tick4 tick5 tick6 tick7 tick8
[successor] child 56546 alive across exec: True
```

The successor is a different program, yet it inherits the live master and reads output the child wrote
*before* the handoff. Nothing is lost in the gap: the pty itself buffers what is in flight.

- Cheap relative to A and B, and it covers the most frequent case: app update and dev rebuild.
- Does not survive a crash or `SIGKILL`. A partial answer, not the requirement.

## Why disk alone cannot do this

The intuitive fix is to serialize sessions on shutdown and reload them on boot. It does not work,
because of what a session actually is.

| Part of a session | Persistable? |
| :--- | :--- |
| cwd, worktree, argv, env, title | yes |
| ring buffer / scrollback | yes |
| **the pty master fd** | **no** |
| **the agent process and its memory** | **no** |

An fd is a handle into a kernel table, not data; the integer `3` means nothing in a different process.
And the process cannot be persuaded to wait around either — hardening it against the signal does not
help:

```
SIGHUP-ignoring child: DIED (iter 1)
```

A child that explicitly ignores `SIGHUP` still dies immediately, because once the master closes its
stdio is invalid and reads/writes fail with `EIO`. There is no flag that makes a process outlive the
owner of its terminal.

So reload-from-disk yields a tab showing the old transcript attached to a **brand new shell**. That is
fatal for a plain shell or a running build, which have no state anywhere else.

It is *not* fatal for an agent, and an earlier draft of this document was wrong to say so. See below.

Disk does have a necessary role, just not that one: `exec()` replaces process memory, so the ring
buffers and session metadata **must** be serialized across the handoff. The correct split is fds via
`exec`, state via disk — neither substitutes for the other.

### D. Persist the ring buffer to disk

Does not save the process, only the transcript. Worth doing regardless, but it is a consolation
prize against the stated requirement.

## The agent case has a much cheaper answer

Claude Code and Codex do not keep the conversation only in memory — they continuously journal it to
disk and are built to resume from it:

```
~/.claude/projects/<escaped-cwd>/<session-uuid>.jsonl
~/.codex/sessions/2026/08/24/rollout-2026-08-24T00-20-13-<uuid>.jsonl
```

```
claude  --session-id <uuid>     use a specific session id for this conversation
claude  -r, --resume [value]    resume a conversation by session id
codex   resume [SESSION_ID]     resume a previous interactive session
```

So the process dying does not destroy the conversation. Recovery does not require saving the fd or the
process — only remembering **which agent, which session id, which cwd** a pane was running, and
relaunching with the resume flag.

### Read the agent's session id; do not mint one

An earlier draft proposed having Ferryx mint a uuid and inject `claude --session-id <uuid>`. That was
wrong, and the agent matrix is why. `SUPPORTED_AGENT_LOGOS` covers twelve agents — antigravity,
claude, codex, gemini, opencode, pi, copilot, cursor, grok, kimi, cline, omo — and of the ones
installed here:

| agent | resume flag | identity it resumes by |
| :--- | :--- | :--- |
| claude | `-r, --resume [id]`, `-c` | uuid (also mintable via `--session-id`) |
| codex | `resume [SESSION_ID]`, `--last` | uuid |
| gemini | `-r, --resume` | **positional index** or `latest` |
| cursor-agent | `--resume [chatId]`, `--continue` | chatId |

Minting exists for exactly **one of twelve**. Every other agent forces discovery of an id it generated
itself, so a mint path would be a second mechanism serving one agent while the general mechanism gets
built anyway. Worse, gemini does not resume by uuid at all — it takes an index — so there is no
universal "inject an id" story available even in principle.

**The rule: Ferryx never mints or injects a session id. Restore always reads back the id the agent
generated itself.** `--session-id` is not a fallback or an optimization for claude; it is not used.

Discovery is keyed on the agent **process**, not the cwd: resolve the agent pid under the pane's pty,
then read the journal file that process has open. The agent is normally a descendant of the pty's
shell rather than its direct child, so this walks the process tree. Process-keyed discovery is what
makes two panes running the same agent in the same cwd unambiguous — which is the only thing minting
appeared to offer.

The irreducible complexity is that identities are not uniform in *kind* — uuid vs index vs chatId.
That wants a small per-agent adapter (`resumeArgs(ref)`) behind one shared discovery step. Minting
would not have removed that; it only would have hidden it for claude.

One further reason to prefer reading over injecting: `--session-id` mutates the launch command the
user configured in `agentsSettings.ts`, and claude's `--fork-session` shows the agent reserves the
right to allocate its own ids across a resume.

Record at agent-start detection, which the app already performs (`ui/src/lib/agentTitle.ts`,
`agentIcon.ts` already classify claude / codex / gemini per pane).

**This beats the `exec()` handoff on the axis that matters most: it survives a crash or `SIGKILL`,**
because it depends on no graceful shutdown path at all. The agent journals as it goes.

Limits, stated honestly:

- The **in-flight turn is lost**. Resume replays up to the last journaled entry; a turn interrupted
  mid-tool-call does not complete.
- **Non-agent panes get nothing** — a plain shell, `npm run dev`, a long build. No journal, no resume.
- Resume is a *relaunch*: the agent rebuilds context by re-reading its journal, which costs time and
  tokens and may re-read files that have since changed.
- Pane **scrollback** still needs the ring buffer persisted (option D) for the pane to look continuous;
  otherwise resume presents a fresh screen with the conversation reloaded inside the agent.

## Recommendation

**Agent-resume first.** It directly solves the reported complaint ("the agent's content was lost"),
it is far less work than A/B/C, and it covers crashes that no handoff scheme can. Everything below is
secondary to it.

**B, with C as an early increment**, for the remainder — plain shells and long-running commands, which
have no journal to resume from.

C's update path is concretely: stage the new binary, serialize ring buffers and metadata, clear
`FD_CLOEXEC` on every pty master, `exec()` the new binary with the fd numbers and state path, then
re-adopt both on the other side. The agent never notices.

The reason this hurts here but not in tmux is worth stating plainly: tmux has exactly the same
ownership model — kill the tmux *server* and every session dies — yet nobody notices, because that
server is stable for months at a time. Our daemon ships inside the app and is replaced on every
update and every dev rebuild, so the fatal event happens constantly. Splitting the stable pty owner
from the churning feature layer removes the frequency, which is the actual problem.

C is worth landing first on its own: it is small, and it converts the common planned-restart case
from fatal to seamless while B is built.

## Also found

No launchd plist is installed (`~/Library/LaunchAgents` has no `com.rorca.daemon`), and `launchctl
list` does not know the job. The current daemon runs with `ppid=1` — an orphan reparented to init
after the GUI that spawned it exited, not a supervised service. The `KeepAlive: true` in
`src-tauri/src/daemon/launchd.rs` therefore never applies on this machine: when the daemon dies,
nothing restarts it, and the next GUI launch spawns a fresh one.

This matters for whichever option is chosen — a survivor process needs real supervision, otherwise it
is one crash away from the same loss.
