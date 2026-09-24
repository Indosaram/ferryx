---
title: "Run Claude Code, Codex, and Gemini in One Workspace"
description: "Mixing coding agents from different vendors in one repo asks for isolation per provider. Structure worktrees, panes, and status so fleets stay legible."
---

**Run Claude Code, Codex, and Gemini in One Workspace.** The workable structure is a workspace where each provider agent gets its own worktree and branch (so their edits never collide), panes are labeled by provider-and-task (so a glance answers "who is doing what"), and status flows into one place (so permission prompts from three different CLIs do not hide in three different scrollbacks). Ferryx's workspace model — daemon-owned sessions bound to managed worktrees — is built for exactly this: one client, many providers, zero shared-checkout races.

![claude-codex-gemini-one-workspace cover](/images/blog/claude-codex-gemini-one-workspace/cover.png)

## Provider quirks that collide first

Running a single agent style is habit; running three styles in one repository surfaces differences that a single-provider setup never exercises:

**Approval prompts differ.** Claude Code pauses on tool permissions with its own dialog phrasing, Codex has its sandbox/approval mode prompts, Gemini CLI its own confirmation flow. Three different visual patterns means your peripheral vision — tuned to one — misses the other two. A run that looks "busy" from across the room may have been parked on an approval for ten minutes. This is the single most common mixed-fleet time sink.

**Branch and checkout behavior diverges.** Some agents prefer creating branches, some work in place, some stash aggressively. When two of them share one checkout, you inherit the union of their assumptions — including the classic `.git/index.lock` contention that appears exactly when both decide to commit at once. Isolation per agent is not optimization here; it is correctness.

**Context and config live in different files.** Each CLI reads its own config (`CLAUDE.md`, `AGENTS.md`, `.gemini/`, dotfiles), and their defaults for auto-compaction, tool inclusion, and shell behavior differ. In one shared workspace you end up with three partially-overlapping instruction sets confusing all three agents; per-provider worktrees let each agent see exactly the context file(s) its authors intended, while shared policy lives in the repo where all will read it.

**Output cadence and TUI behavior differ.** One streams thinking blocks, another renders a dense status line, a third redraws aggressively. Panes sized for one look wrong for another — which sounds cosmetic until an agent's progress indicator sits below your pane fold and you stop monitoring it altogether.

## Layout for a mixed fleet

Start from identity, not aesthetics — every pane answers three questions on sight: which provider, which task, which worktree:

1. **One worktree per agent task.** The workspace creates (or you script) `wt-<task>` checkouts — Ferryx manages this under `.orca-worktrees/` with a branch per session — so provider A refactoring tests and provider B adding a feature touch disjoint trees. Merging happens through normal PR review, not through hope.
2. **Name panes for provider and task.** `[CC] fix-flaky-test`, `[CX] api-pagination`, `[GM] docs-pass` — a prefix convention you keep for all providers, so pane titles and any status strip reading them stay parseable. Do not encode only the provider: two Codex runs differing only by prefix still make you squint.
3. **Group by interaction pattern, not vendor.** Panes you *drive* (approval-heavy) sit in the primary workspace region; long unattended runs (doc sweeps, mechanical refactors) go to a background region or a second monitor/workspace. Vendor grouping sounds tidy and puts an approval-parked Gemini next to a sleeping Claude, which is the layout equivalent of noise.
4. **Centralize the shared context once.** Repo-level instructions (AGENTS.md or equivalent) live in the shared repository and get pulled into each worktree; provider-specific files stay provider-specific. The rule: shared facts written once in-repo, provider preferences in each provider's file, secrets nowhere in either.
5. **Keep a status surface that spans providers.** Whether it is Ferryx's session badges, a small script reading each CLI's state file, or disciplined `ps` habits — one view must answer "which of the three needs me." If each provider demands its own alt-tab to check, you are running three tools, not one workspace.

![Run Claude Code, Codex, and Gemini in One Workspace illustration](/images/blog/claude-codex-gemini-one-workspace/body-1.png)

## Status and handoff conventions

The fleet's throughput is gated by *your* attention, so conventions around status and handoff matter more than raw parallelism:

- **Park visibly.** When pausing an agent (end of day, waiting on review), make the parked state visible in the pane name or status badge — a parked and an actively-thinking pane must not look identical from a distance.
- **Hand off with a written note.** Each task's worktree carries a short state file or commit message describing where the agent stopped: "tests green, awaiting decision on API shape." Agents resume well from notes; humans resume better from them; and the next agent you point at the worktree inherits context instead of rediscovering it.
- **Sequence merges; parallelize edits.** Three agents may edit simultaneously *in separate trees*; the merges into main should be serial and reviewed. Conflict-resolution under three-way contention is where parallelism silently becomes rework.
- **Budget approvals in batches.** When several agents park on permissions, answer them in one sweep rather than as each notification arrives — context-switching cost per approval is what makes mixed fleets feel slower than they run.

Provider-comparison grounding for *which* agent gets *which* task is covered separately — [Claude vs Codex vs Gemini CLI](/blog/compare-claude-code-codex-gemini-cli/) maps strengths to task types; this post assumes you have chosen and now need the three to coexist. The workspace mechanics — managed worktrees, session badges, one client across machines — are detailed in the [parallel agents use case](/use-cases/parallel-ai-agents/) and the [architecture page](/docs/architecture/).

## FAQ

### Do I really need a separate worktree for each provider agent?

If they edit code, yes — provider choice does not change git's lock semantics, and two agents committing to one checkout will contend on `.git/index.lock` and interleave edits regardless of vendor. The worktree-per-agent rule is provider-agnostic precisely because the failure mode is provider-agnostic; see the [same-repo two-agent pattern](/blog/same-repo-two-agents/) for the mechanics.

### How do I keep three different context/config files from contradicting each other?

Split by audience: facts every agent needs (architecture, commands, constraints) live once in the repo's shared agent file; provider-specific preferences (tool policies, formatting quirks) stay in each provider's own config; secrets stay out of both. Each worktree carries the split naturally, so every agent reads shared truth plus its own dialect — no merged mega-config that satisfies none.

### What is the best way to notice when any of the agents needs approval?

Make it structural instead of vigilant: a status surface reading all three (badges, a dashboard, or a small watcher parsing each CLI's prompt state) gives you one place to look, and pane-naming discipline makes the parked agent obvious on sight. The attention-economics reasoning — why one aggregated view beats three polling habits — is covered in the [attention inbox design post](/blog/attention-inbox-design/).

### Can one person actually supervise three coding agents at once?

With the isolation and status machinery above, supervision load becomes mostly *approvals and merges* rather than babysitting: agents work in disjoint trees, status is aggregated, and you engage at decision points. Without that machinery, three agents multiply your coordination cost faster than they multiply throughput — which is why fleet structure, not raw count, decides whether mixing providers pays off.
