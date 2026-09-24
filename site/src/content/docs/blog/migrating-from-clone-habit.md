---
title: "Migrating From Clone-Per-Agent to Worktrees"
description: "A step-by-step move from scattered clones to managed worktrees, including history and remote cleanup. Claims map to Ferryx docs and cited sources only."
---

**Migrating From Clone-Per-Agent to Worktrees.** A step-by-step move from scattered clones to managed worktrees, including history and remote cleanup. Claims map to Ferryx docs and cited sources only.

![migrating-from-clone-habit cover](/images/blog/migrating-from-clone-habit/cover.png)

## Inventory existing clones

The manual workaround for parallel agents is cloning the repository once per agent. Clones drift apart, remotes multiply, and upkeep is easy to forget. Worktrees keep one history while giving each agent an independent index and working tree, which ends both merge chaos and index.lock contention. In the specific case of “Migrating From Clone-Per-Agent to Worktrees”, the Ferryx angle is straightforward: a step-by-step move from scattered clones to managed worktrees, including history and remote cleanup. claims map to ferryx docs and cited sources only. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Convert in place

A Git worktree is a second working directory attached to the same repository. Ferryx manages them under `.orca-worktrees/wt-<slug>` with branches named `orca/<workspace-id>/<slug>` and jails every path to the repository root. Applied to “migrate clone to worktree”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Confirm the repository is clean enough to branch (committed or intentionally dirty state you understand).
2. Create a managed worktree from Ferryx (or `git worktree add` if you are doing it by hand) with a descriptive branch name.
3. Point the agent at that worktree path and start work; the shared `.git` history means fetches apply everywhere.
4. Diff and review from the main checkout with `git diff main...<agent-branch>` before merging.
5. Remove the worktree only after the branch is merged or explicitly discarded: `git worktree remove` plus branch cleanup.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![Migrating From Clone-Per-Agent to Worktrees illustration](/images/blog/migrating-from-clone-habit/body-1.png)

Related reading:
- [/use-cases/git-worktree-workflow/](/use-cases/git-worktree-workflow/)
- [/docs/facts/](/docs/facts/)

## FAQ

### How is a worktree different from a clone?

A clone copies the whole repository. A worktree adds a working directory that shares the same object database and refs, so it is cheaper and stays in sync.

### Where does Ferryx put managed worktrees?

Inside the repository under `.orca-worktrees/wt-<slug>`, jailed to the repository root so paths cannot escape the project.

### How do I clean one up safely?

Merge or discard the branch first, then remove the worktree through Ferryx or `git worktree remove`. Never `rm -rf` a directory that still has unmerged commits.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
