---
title: Running parallel AI coding agents without collisions
description: Why coding agents collide when they share one working directory, how a git worktree per agent fixes it, and how Ferryx automates the whole setup.
---

Running three coding agents in one repository sounds like three times the output until they start stepping on each other. The cause is mundane: agents edit files, and agents that share a working directory edit the same files. Built-in git machinery has solved this for years, one worktree per agent. This page covers why the collisions happen, how worktrees prevent them, and what Ferryx automates if you'd rather not script it yourself.

## Why agents collide in one working directory

An agent works by editing files, running builds, and iterating with its changes uncommitted for long stretches. Several agents in the same checkout therefore share one pile of mutable state:

- Concurrent edits to the same files. Two agents can pick overlapping work or touch the same module, and their edits interleave in whatever order the processes write.
- Conflicting builds. One agent's half-finished refactor can break the build another agent relies on to check its own work.
- One agent reverting another's uncommitted work. Because the other agent never committed, there is nothing in git history to recover from.

None of this is a bug in any particular agent. It's what happens when independent writers share a directory with nothing isolating them from each other.

## The fix: a worktree per agent

git worktree is a built-in git feature that checks out multiple branches of one repository into separate directories. Give each agent its own worktree and its own branch, and the shared pile of state goes away:

- Each agent edits only its own checkout.
- Builds and test runs happen in separate directories, so they stop failing each other.
- Uncommitted work is safe, because no other agent can see that directory.
- Integration moves into git merges, which is where merge conflicts belong.

The commands are short. From inside your repository:

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

That's the standard zero-install pattern: a worktree per task, a tmux window per worktree, attach and detach at will. It runs anywhere, including over plain SSH on a server, and it survives disconnects by design. If that covers your needs, stop here and keep your setup. The rest of this page is about what changes when an app does the bookkeeping for you.

## What Ferryx automates

Ferryx is a desktop workspace built with Rust and Tauri v2 that applies the same pattern with the bookkeeping handled:

- Worktrees per workspace. Managed worktrees live in `.orca-worktrees/wt-<slug>` on branches named `orca/<workspace-id>/<slug>`, and worktree paths are jailed to the repository root. Naming and placement stay consistent because the app, not you, does them every time.
- Panes per workspace. Each agent gets its own terminal pane in a split layout, so state is visible at a glance, and panes and tabs rearrange by drag-and-drop.
- An embedded browser beside the panes. Browser tabs use native WebViews, so documentation or a preview can sit next to the terminals without a second window.

Session lifetime works differently from a plain GUI terminal:

- A headless Rust PTY daemon owns the pseudoterminals, not the GUI process. Closing or reloading the GUI does not kill running agent processes.
- Output is buffered in a ring buffer with monotonic sequence numbers. When the GUI reconnects, the daemon replays the output you missed, so the transcript picks up where it left off.

## Checking a long run from your phone

Long runs don't need you at the desk. Ferryx ships an authenticated mobile remote client that renders the terminal with a custom DOM grid, no xterm.js dependency. The practical flow:

1. Start an agent run in its worktree.
2. Step away, restart the GUI, or close it entirely. The daemon keeps the run alive.
3. Open the remote client on your phone and check on the session.

## Doing it manually is still fine

Nothing above requires Ferryx. git worktree plus tmux and a couple of scripts gets you the same isolation, and many engineers should keep using exactly that setup. The honest summary is that Ferryx adds ergonomics and bookkeeping: automated worktree creation and branch naming, visible per-agent state, daemon-managed session lifetime, an embedded browser, and a phone client. If those conveniences are worth a v0.1.0-alpha desktop app to you, start with the [setup introduction](/docs/introduction/). For a closer look at how it compares with the tmux baseline, read [Ferryx vs tmux and git worktree](/compare/tmux-git-worktree/).
