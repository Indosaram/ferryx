---
title: "Managing a Coding Agent Fleet Across Providers"
description: "A fleet that mixes Claude Code, Codex CLI, and Gemini CLI needs one status surface and per-agent isolation. Here is the operating model that holds up."
---

**Managing a Coding Agent Fleet Across Providers.** The operating model that holds: maintain a *fleet inventory* (every running agent with provider, task, worktree, and state on one register), route all status through *one surface* instead of each CLI's native UI, and define *escalation* — explicit rules for when a stalled agent gets unstuck, reassigned, or killed. Per-agent isolation (a worktree each) is the floor beneath all three; without it, no inventory stays accurate because agents overwrite each other's ground truth.

![cross-provider-agent-fleet cover](/images/blog/cross-provider-agent-fleet/cover.png)

## Fleet inventory and naming

A fleet becomes manageable when its membership is enumerable without SSH archaeology. The inventory answers, at any moment: what is running, for whom (task), where (worktree/branch), under which provider, and in what state.

**Names are the schema.** Adopt one naming grammar and apply it everywhere — session names, branch names, pane titles, worktree directories. A grammar like `<task>-<provider>-<seq>` (`auth-rerun-cc-01`) sounds pedantic until you try to answer "which run owns branch X" from memory. Ferryx's managed worktrees already enforce a branch scheme per session (`orca/<workspace>/<slug>`), which gives inventory a spine to hang on; layer the provider and task into the slug convention your team reads.

**States must be finite and observed, not guessed.** Four states cover real fleets: `running` (actively producing), `parked` (waiting — end of day or human pause), `blocked` (waiting on *you* — approval prompt or question), `done` (finished, awaiting review). The third is where fleets leak hours: a `blocked` agent is the most expensive state in the system because its throughput is your attention, not its tokens. Deriving state mechanically (prompt detection per CLI, heartbeat staleness, exit markers) beats renaming panes by hand the moment fleets pass two or three members.

**Inventory needs a home — not a person's memory.** A file in the repo (`fleet.yml` or similar), a dashboard backing store, or the workspace manager's session registry — all fine; what fails is "the inventory is whatever the team lead remembers." Registry entries should carry task description, owning human, worktree path, and last-observed state, so any member (or any future agent) can reconstruct the field without asking.

## One status surface design

Three CLIs, three native presentations, one rule: *aggregation beats inspection*. Design the surface around the questions operators actually ask, in order of urgency:

1. **Who is blocked right now?** Top of the surface, sorted by how long they have been parked. This is the only question that has a deadline.
2. **Who finished?** Done-but-unreviewed work is inventory rot; surface age so yesterday's green run gets merged or re-run before its context rots.
3. **Who is running healthily?** Needed to trust silence: active runs show progress signals (recent output, tool activity), not just "alive" — an agent wedged in a tool call is technically `running` and practically `blocked`.
4. **Who exists, wherever they run?** Laptop sessions, headless-host sessions, remote runs — one register. Fleet truth that depends on which machine you are sitting at is not fleet truth.

The surface itself can be a workspace dashboard, a terminal status line, a small TUI, or badges over panes — the architecture choice matters less than two properties: it reads *state* (not raw scrollback), and it is *the* surface (no second source of truth competing with it). If checking status means three CLI-specific commands, operators will pick one to trust and blind themselves to the rest; the aggregated view in [agent orchestration dashboards](/blog/agent-orchestration-dashboard/) shows one implementation of the pattern.

Crucially, the surface must ingest **provider-diverse signals**: Claude Code's permission dialogs, Codex CLI's approval modes, Gemini CLI's confirmations — each needs a detector, because a fleet surface that only understands one dialect silently mislabels the others as `running` when they are actually parked awaiting a human.

## Escalation when an agent stalls

Stalls are not anomalies in fleets; they are scheduled traffic. Codify the response as a ladder, so operators do not improvise differently every time:

- **Tier 0 — age check.** No output/activity for N minutes on a `running` agent: verify it is thinking (some phases are quiet) versus wedged (a tool call hung, a network wait spun). Distinguish via the provider's own signals — spinner/state line activity, or process CPU/file activity — before touching anything.
- **Tier 1 — nudge the park.** If `blocked` was the real state: answer the approval or question. Half of all "stalls" end here, which is why blocked-detection sits at the top of the status surface.
- **Tier 2 — interrupt and redirect.** A wedged-but-alive run gets an interrupt (per-CLI stop keybinding) and a corrective message describing what went wrong and what to do instead. Interrupting with intent beats killing: the agent keeps its context and corrects course.
- **Tier 3 — kill and hand off.** Unrecoverable (looping, corrupted its worktree, chasing a bad premise): kill it, record *why* in the inventory, and reassign the task — preferably to a fresh worktree so the damaged one can be inspected rather than repaired in place.
- **Tier 4 — post-mortem into prevention.** If the same stall class repeats across the fleet, the fix is upstream: a context-file correction, a tool-permission default, a task-shape change (too big to complete without review), or an infrastructure issue (disk, network) visible in [host observability signals](/blog/daemon-observability/).

Every tier writes back to the inventory — state changes and reasons are the fleet's memory. An operating model without recorded escalations re-teaches the same lessons every flight; with them, the fleet's history becomes the next operator's (and the next agent's) briefing. End-to-end mechanics of isolation underneath this model live in the [parallel agents use case](/use-cases/parallel-ai-agents/); provider-specific strengths for *assigning* tasks are mapped in [Claude Code vs Codex CLI vs Gemini CLI](/blog/compare-claude-code-codex-gemini-cli/).

![Managing a Coding Agent Fleet Across Providers illustration](/images/blog/cross-provider-agent-fleet/body-1.png)

## FAQ

### How many agents can one person realistically supervise?

With the model above — mechanical state detection, one surface, escalation ladder — supervision cost per agent drops to approval-and-review touchpoints, making 5–10 concurrent runs plausible for one focused operator; without it, even three cross attention budgets because every status check is manual. The binding constraint is blocked-agent latency: the faster you notice parked runs, the more runs you can carry. Related sizing discussion: [how many parallel Claude Code agents](/blog/when-not-to-parallelize/) covers when more runs stop paying.

### Do I need a special tool, or can a script maintain the fleet inventory?

A script works at small scale — periodically snapshot each CLI's state, process list, and worktree status into a registry file, and render it. What you build is the *information*, not the tooling: the moment the snapshot misses provider-specific prompt states, though, script-based detection silently degrades, which is why most fleets graduate to surfaces with native per-CLI detectors (or a workspace manager whose sessions carry state natively).

### Should stalled agents be killed immediately to free resources?

No — the ladder inverts that instinct: verify, answer (if merely blocked), interrupt-and-redirect, then kill. Killing first discards accumulated context that made the run valuable and often re-creates the stall on the next attempt because the underlying premise was never corrected. Kill is tier 3 for a reason; resource pressure is handled by admission control (limit concurrent runs), not by executing stallers early.

### How is fleet status different from just tailing each agent's output?

Tail shows *what* an agent emitted; status shows *what an agent needs*. Raw output buries a permission prompt inside a thousand lines of streaming thinking, while a state surface elevates it to `blocked, 4m`. Fleet management optimizes for the second view precisely because operator attention — not agent speed — is the scarce resource; the aggregation pattern is detailed in the [orchestration dashboard post](/blog/agent-orchestration-dashboard/).
