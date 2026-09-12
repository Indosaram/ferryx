---
title: "Git Worktrees in Practice: Parallel Work Without Stashing"
description: How git worktree works, the commands to run by hand, where the manual flow gets tedious, and how Ferryx manages worktrees for you.
---

A single checkout of a repository holds one branch at a time. When a build, a test run, or a coding agent occupies that checkout, the usual moves are to wait, to stash, or to switch branches and hope nothing was half-finished. Worktrees remove that constraint: git checks out several branches of the same repository into separate directories, all sharing one .git object store. Parallel work stops depending on the state of one directory.

## The manual flow

Everything below is standard git. No plugins, no extra tools.

```bash
# create a worktree with a new branch
git worktree add ../feature-x -b feature-x

# list every worktree registered for this repository
git worktree list

# remove one you no longer need
git worktree remove ../feature-x
```

The main checkout keeps its branch untouched. Each worktree is a full working directory with its own index, so you can run tests in one while you edit in another without stashing anything. Branches, commits, and objects stay shared between worktrees, which is why a second worktree costs far less disk than a second clone.

For many people, this is enough. If you keep two or three worktrees and remember their paths, plain git worktree is a complete answer and it asks for no additional software.

## Where the manual flow gets tedious

The friction shows up when worktrees multiply, or when you come back to a project after a few weeks:

- **Paths and names drift.** One worktree lives in `../feature-x`, the next in `~/code/feature-x-wip`, and the third has a typo in its branch name. Nothing enforces a convention, so `git worktree list` eventually reads like an archaeology dig.
- **Stale worktrees pile up.** A branch merges and its worktree directory stays behind, or a directory gets deleted and its branch lingers. Reconciling the two is on you.
- **Every worktree wants its own terminal.** The point is running things side by side, so you open a new tab per directory and retype the path at the start of each session.
- **Agents amplify all of it.** Hand a coding agent a directory and it works in whatever checkout you gave it. Run several agents and the bookkeeping grows with them.

None of this is hard. It's repeated small decisions, which is exactly the kind of work worth handing to a tool.

## How Ferryx handles it

Ferryx is a desktop terminal workspace (MIT licensed, Rust and Tauri v2) that manages git worktrees for you. When you ask it for an isolated workspace, it creates a managed worktree at a fixed path inside the repository, `.orca-worktrees/wt-<slug>`, on a branch named `orca/<workspace-id>/<slug>`. You don't choose paths or branch names, so the convention can't drift. Ferryx creates the worktree, names it, and removes it when you're done, which covers the cleanup step most often forgotten in the manual flow.

What that means inside the workspace:

- **Jailed paths.** Managed worktree paths are constrained to the repository root, so a managed worktree can't escape the repo. Nothing lands in a sibling directory of your home folder unless you put it there yourself.
- **A terminal per worktree, side by side.** Ferryx provides split terminal panes with drag-and-drop rearrangement, and embedded browser tabs using native web views next to terminal panes. The layout you'd assemble by hand is the default shape of the workspace.
- **One convention across every repo.** Because the tool owns the path and branch scheme, `git worktree list` stays readable no matter how many worktrees you make.

If your goal is running several coding agents in parallel, each in its own worktree, that workflow is covered in [parallel AI agents](/use-cases/parallel-ai-agents/). Since sessions live in a daemon rather than a window, you can also check on them away from the desk; see [remote terminal access](/use-cases/remote-terminal-access/). For the workspace as a whole, start with the [introduction](/docs/introduction/).
