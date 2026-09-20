---
title: Running parallel AI coding agents without collisions
description: Claude and Codex each manage their own worktrees now. Why agents still collide across vendors, and what one shared convention in Ferryx changes.
---

Running three coding agents in one repository sounds like three times the output until they start stepping on each other. The cause is mundane: agents edit files, and agents that share a working directory edit the same files. Git has solved this for years with one worktree per agent, and the agent vendors have caught up. This page covers why the collisions happen, what Claude Code and Codex now do about it themselves, and what's left over for a workspace to handle.

## Why agents collide in one working directory

An agent works by editing files, running builds, and iterating with its changes uncommitted for long stretches. Several agents in the same checkout therefore share one pile of mutable state:

- Concurrent edits to the same files. Two agents can pick overlapping work or touch the same module, and their edits interleave in whatever order the processes write.
- Conflicting builds. One agent's half-finished refactor can break the build another agent relies on to check its own work.
- One agent reverting another's uncommitted work. Because the other agent never committed, there's nothing in git history to recover from.

None of this is a bug in any particular agent. It's what happens when independent writers share a directory with nothing isolating them from each other.

## The agents now do this themselves

Two things changed, and any page that ignores them is out of date.

**Claude Code.** `claude --worktree <name>` (or `-w`) creates a worktree under `.claude/worktrees/<name>/` at your repository root, on a branch named `worktree-<name>`, and starts the session there. Claude can also enter one mid-session with the `EnterWorktree` tool, and subagents can be isolated the same way with `isolation: worktree` in their frontmatter. On exit, a clean unnamed worktree is removed automatically, and one with work in it prompts you. Claude Code then enforces the isolation rather than trusting it: it blocks edits that target the main checkout, commands whose working directory resolves there, git redirected back into it through `git -C`, `--git-dir`, `GIT_DIR`, `GIT_WORK_TREE`, or a `cd`, and commands whose shape it can't verify. ([code.claude.com/docs/en/worktrees](https://code.claude.com/docs/en/worktrees), read 2026-09-19.)

**Codex.** Pick "Worktree" under the composer in the ChatGPT desktop app and Codex creates a worktree in `$CODEX_HOME/worktrees`, based on the branch you chose, in a detached HEAD so it doesn't consume branch names. Handoff moves a chat between Local and Worktree and does the git work for you. By default Codex keeps the 15 most recent managed worktrees and deletes older ones, saving a snapshot first. ([learn.chatgpt.com/docs/environments/git-worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees), read 2026-09-19.)

If you run one agent from one vendor, use its flag. It's the shortest path, it's maintained by the people who ship the agent, and Claude's enforcement checks are stricter than anything a wrapper can do from outside.

## The manual baseline still works

Nothing stops you from doing it by hand. From inside your repository:

```sh
# one worktree per task
git worktree add ../myapp-auth -b auth-rework
git worktree add ../myapp-cache -b cache-tuning
git worktree list
```

Then give each worktree a terminal. With tmux, the classic setup is one window or pane per worktree:

```sh
tmux new-session -d -s agents -c ../myapp-auth
tmux new-window -t agents -n cache -c ../myapp-cache
tmux attach -t agents
```

That's the zero-install pattern: a worktree per task, a tmux window per worktree, attach and detach at will. It runs anywhere, including over plain SSH on a server, and it survives disconnects by design.

## What's left when you run more than one vendor

Per-agent worktree flags are per agent. Three gaps open as soon as you mix them, and none of them is fixed by a better flag:

- **Three conventions in one repository.** Claude writes `.claude/worktrees/<name>` on `worktree-<name>`, Codex writes `$CODEX_HOME/worktrees` in detached HEAD, and anything else does whatever you scripted. `git worktree list` stops being readable.
- **Branch checkout collisions.** Git allows a branch in exactly one worktree at a time, which is the collision Codex's own docs warn about when you hand a chat back to Local. Two vendors reaching for the same branch is a pure coordination problem.
- **Agents without a flag.** Gemini CLI, Copilot CLI, Cline, a test runner, a migration script: none of them manage a worktree for you.

## What Ferryx does about it

Ferryx is a desktop workspace built with Rust and Tauri v2 that applies one convention to everything in the window. Any agent you can start from a shell runs in a pane:

- **One worktree scheme for every agent.** Managed worktrees live in `.orca-worktrees/wt-<slug>` on branches named `orca/<workspace-id>/<slug>`, and worktree paths are jailed to the repository root. The scheme doesn't depend on which vendor's CLI is in the pane, so Claude Code and Codex on the same repo get the same treatment and don't reach for each other's branches.
- **Status you can see without reading scrollback.** Each agent gets its own pane in a split layout, rearrangeable by drag-and-drop. Status-detection manifests ship for eleven agents, including Claude Code, Codex, Cursor, Cline, GitHub Copilot CLI, OpenCode, Grok, and Kimi, so a pane reports whether its agent is working, waiting on you, or idle.
- **An embedded browser beside the panes.** Browser tabs use native WebViews, so documentation or a preview sits next to the terminals without a second window.

Session lifetime works differently from a plain GUI terminal, and this part no worktree flag addresses:

- A headless Rust PTY daemon owns the pseudoterminals, not the GUI process. Closing or reloading the GUI doesn't kill running agent processes.
- Output is buffered in a 512 KiB ring buffer per session with monotonic sequence numbers. When the GUI reconnects, the daemon replays the output you missed; if the buffer wrapped while you were away, the client is told there's a gap rather than shown a corrupted transcript.

One caveat worth stating: Ferryx manages the worktree and the pane, not the agent's internals. Claude Code's four isolation checks are Claude Code's, and they still apply when it runs in a Ferryx pane. Ferryx doesn't reimplement them and doesn't claim to.

## Checking a long run from your phone

Long runs don't need you at the desk. Ferryx ships an authenticated mobile remote client that renders the terminal with a custom DOM grid, no xterm.js dependency. The practical flow:

1. Start an agent run in its worktree.
2. Step away, restart the GUI, or close it entirely. The daemon keeps the run alive.
3. Open the remote client on your phone and check on the session.

Claude Code and Warp both offer their own remote options, with different requirements; [remote terminal access](/use-cases/remote-terminal-access/) compares them.

## Doing it another way is still fine

Nothing above requires Ferryx. One vendor's worktree flag, or git worktree plus tmux and a couple of scripts, gets you the isolation, and plenty of engineers should keep doing exactly that. The honest summary is that Ferryx adds a single convention across agents, visible per-agent state, daemon-managed session lifetime, an embedded browser, and a phone client. If those are worth an early desktop app to you, start with the [setup introduction](/docs/introduction/), or read the [product facts](/docs/facts/) for the sourced version of every Ferryx claim above. For a closer look at how it compares with the tmux baseline, read [Ferryx vs tmux and git worktree](/compare/tmux-git-worktree/).
