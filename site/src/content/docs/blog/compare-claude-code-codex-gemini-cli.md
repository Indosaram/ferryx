---
title: "Claude Code vs Codex CLI vs Antigravity for Parallel Work"
description: "Claude Code, Codex CLI, and Antigravity (agy) differ in permissions, isolation, and resume. An honest fit guide for running them side by side, and where Gemini CLI fits."
---

> **Note:** Gemini CLI runs in a Ferryx pane like any command but has no status detection or resume; Google's supported agent is Antigravity (agy).

**Claude Code vs Codex CLI vs Antigravity for Parallel Work.** The honest framing: Claude Code brings deep repository context and a mature tool/permission surface, Codex CLI emphasizes sandboxed execution and tight OpenAI-model iteration, and Antigravity (`agy`) is Google's agent CLI with its own permission prompt and resumable conversations. For *parallel* work specifically, the differences that matter are how each handles approvals, isolation assumptions, and session resumability, because those determine whether three concurrent runs multiply throughput or multiply babysitting. Whichever CLIs you run, a Ferryx workspace gives each its own worktree so provider choice never collides with isolation. No benchmark numbers appear here by design; claims are structural (documented features and behaviors), and each should be re-verified against current docs before you commit a workflow.

![compare-claude-code-codex-gemini-cli cover](/images/blog/compare-claude-code-codex-gemini-cli/cover.png)

## Where each CLI shines

**Claude Code** is strongest where *repo-wide understanding* drives the task: its context management (project memory files, plan modes, sub-agent patterns) is designed for an agent that reads a lot before it writes, and its permission model gives granular control over which tool classes run unattended. That property matters directly in parallel setups, because "what may this run do without asking" is the question every fleet operator is actually deciding.

**Codex CLI** is strongest where *execution isolation* is the concern: sandboxing-first behavior (network and filesystem boundaries around runs) makes unattended parallelism more comfortable, since a run you cannot fully supervise is exactly the run that benefits from a sandbox defaulting to safe. Its approval and mode surface reflects that posture: rather than trusting each action, constrain the environment and let the run proceed inside it.

**Antigravity** (`agy`) is the Google option for terminal agent work. It asks before acting through its own permission prompt, keeps each conversation under an id you can reopen, and exposes lifecycle hooks. For a fleet, those three properties are what count: a parked prompt you can see, a session you can resume after a restart, and state that arrives from the agent rather than from scraping its screen.

None of these is a "fastest" or "best" claim. They are differences in *design emphasis*, each documented by its vendor and each volatile across releases. Treat this section as a map of where to look, not a scorecard.

## What Ferryx adds per agent

All three ship a Ferryx status-detection manifest, so each pane reports working, waiting on you, or idle. All three resume: Ferryx reopens Claude Code with `--resume`, Codex with `codex resume`, and Antigravity with `agy --conversation <id>`. All three have native hooks installed by Ferryx and transcript readers for the remote client. Codex additionally supports Ferryx's managed chat. The full per-agent breakdown is in the [agent support matrix](/blog/agent-support-matrix/).

Gemini CLI is different. It runs in a pane like any shell command, but Ferryx ships no manifest, hook bridge, or resume builder for it, so its pane shows output without a status badge and does not reopen its session after a restart.

## Parallel behavior differences

Running one agent at a time hides fleet-relevant behaviors; running three exposes them:

- **Approval cadence under load.** The CLI with the most granular permissions will prompt most often: good for control, costly when prompts arrive while you are reading another pane. Codex-style sandboxing prompts less by constraining more. For *unattended* parallel runs, lower prompt cadence per run usually wins, which is why fleets often assign the approval-heavy CLI to the supervised slot and sandboxed modes to background slots.
- **Session state and resume.** How each persists conversation state across disconnects and restarts determines your reattach story: resumable sessions make a laptop close non-fatal. In parallel setups this compounds, because three fragile sessions are three daily recovery rituals.
- **Concurrent-run hygiene.** Each CLI's assumptions about working directories, lockfiles, config writes, and git usage decide how loudly they collide when pointed at overlapping trees. The universal equalizer is separate worktrees per run, which sidesteps most vendor-specific collision behaviors rather than cataloguing them; the [git lock fights post](/blog/agent-conflict-git-lock/) documents the canonical failure.
- **Context strategy under long runs.** Carry-everything versus managed compaction changes how mid-task interruptions behave. For parked-until-tomorrow fleet patterns, test *resume quality* explicitly.

## Choosing per task type

| Task shape | First pick | Why (structural) |
| --- | --- | --- |
| Multi-file refactor with repo conventions | Claude Code | Context depth + granular permission control |
| Unattended batch codegen in sensitive tree | Codex CLI (sandboxed) | Execution boundaries reduce supervision need |
| Google-stack work you want resumable and visible | Antigravity (agy) | Permission prompt, resumable conversations, hooks |
| Mixed fleet, one operator | All three, tiered | Supervised slot = granular; background slots = sandboxed |
| Anything sharing one checkout | None: split worktrees first | Vendor behavior is irrelevant to `index.lock` |

The last row is the load-bearing one: **isolation is not a provider feature.** Whichever CLI you pick, parallel runs on one repository meet the same git semantics, so the fleet's floor is worktree-per-run, above which provider choice becomes a matter of task fit rather than damage control.

For the *workspace* side of running these side by side (pane layout, per-provider context files, one status surface) see [one workspace for Claude, Codex, and Antigravity](/blog/claude-codex-gemini-one-workspace/).

![Claude Code vs Codex CLI vs Antigravity for Parallel Work illustration](/images/blog/compare-claude-code-codex-gemini-cli/body-1.png)

## FAQ

### Which of the three is best for running parallel agents?

Wrong question for parallel work: the binding constraints are supervision model (approval cadence per run) and isolation (worktrees per run), not which CLI tops a single-task comparison. Pick per task shape using the table above, isolate every run, and the parallelism question becomes about *your* operating model rather than vendor ranking.

### Can I still use Gemini CLI in Ferryx?

Yes, as an ordinary command. It runs in a pane and can have its own worktree, but Ferryx has no status detection or resume for it, so you read its pane yourself. If you want Google's agent with detection, resume, and hooks, run Antigravity (`agy`).

### Do all three support being run simultaneously on one machine?

Yes, structurally: they are independent CLIs with separate auth and config, and simultaneous execution is normal usage. The collisions to engineer around are shared *resources* (one repository's git state, port ranges, credentials you deliberately share), all solvable with per-run worktrees and env isolation.

### How do their permission/approval models differ?

Claude Code offers fine-grained, class-level permissions you tune (more control, more prompts when strict); Codex CLI leans sandbox-first (constrain the environment, prompt less inside it); Antigravity shows its own permission prompt before acting. Exact mechanics change frequently, so verify against each vendor's current docs before building fleet automation that assumes specific prompt behaviors.

### Should I standardize on one CLI to simplify fleet management?

Standardize the *workflow* (isolation, naming, status surface, escalation), not the CLI. One status surface that speaks all providers' dialects keeps task-fit routing available, so an approval-heavy refactor goes to the supervised slot while a sandboxed sweep runs in the background. The [cross-provider fleet model](/blog/cross-provider-agent-fleet/) details the single-surface operating pattern.
