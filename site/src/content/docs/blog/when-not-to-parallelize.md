---
title: "When Not to Parallelize Coding Agents"
description: "Parallel agents are not free: lock contention, review load, and shared config files. A decision guide before you fan out."
---

**When Not to Parallelize Coding Agents.** Parallel agents are not free: lock contention, review load, and shared config files. A decision guide before you fan out.

![when-not-to-parallelize cover](/images/blog/when-not-to-parallelize/cover.png)

## Costs of naive fan-out

Running more than one coding agent against a single checkout is how teams lose work: both agents see the same files, both commit into the same history, and a refactor from one invalidates an edit the other is halfway through. Ferryx turns the clone-per-agent workaround into a managed feature so parallel runs stay visible in one window without stepping on each other. In the specific case of “When Not to Parallelize Coding Agents”, the Ferryx angle is straightforward: parallel agents are not free: lock contention, review load, and shared config files. a decision guide before you fan out. Teams evaluating this often arrive from a shared-checkout pain story or from tooling that treats the terminal as a disposable window. Ferryx takes the opposite stance—sessions are daemon-owned, isolation is a first-class action, and everything you need to verify sits behind documented surfaces.

## Serial-only tasks

Ferryx is a desktop terminal workspace for running several CLI coding agents at the same time. Each agent runs in its own terminal pane and, when isolation is requested, its own managed Git worktree under `.orca-worktrees/wt-<slug>` on a branch named `orca/<workspace-id>/<slug>`. Applied to “when not to parallelize agents”, that context matters because the failure modes people search for are almost always failures of ownership or visibility rather than of raw rendering. When the process outlives the window, when the buffer can prove what it missed, and when each agent's files live in a jailed worktree, the same task that used to require three clones and a spreadsheet becomes a pane layout you can read at a glance.

## Practical steps

1. Open the project as a Ferryx workspace so tabs, panes, and sessions share one sidebar.
2. Request a managed worktree for each agent that needs isolation; Ferryx creates it inside the repository root and names the branch for you.
3. Start each agent (Claude Code, Codex, Gemini CLI, or any shell command) in its own pane with ⌘T and ⌘D splits.
4. Watch status badges and the attention counter instead of polling every pane; open a browser tab with ⌘⇧B for CI or PR checks.
5. When an agent finishes, review its diff from that worktree and merge in a deliberate order before cleaning the branch up.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer “who needs me?” without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![When Not to Parallelize Coding Agents illustration](/images/blog/when-not-to-parallelize/body-1.png)

Related reading:
- [/use-cases/parallel-ai-agents/](/use-cases/parallel-ai-agents/)
- [/docs/introduction/](/docs/introduction/)

## FAQ

### Do I need a separate clone for every agent?

No. Managed worktrees share one repository object database while keeping independent checkouts, so disk cost stays low and history stays unified.

### Will agents corrupt each other's edits?

Not when each agent has its own worktree. Shared-checkout collisions (including index.lock fights) are the failure mode isolation removes.

### Which agents are supported?

Anything you can run in a shell works. Ferryx additionally ships status-detection manifests for eleven named agents so panes report working, waiting, or idle.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
