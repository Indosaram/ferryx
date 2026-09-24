---
title: "Leader and Worker Patterns for Coding Agents"
description: "Leader and worker agents split planning from execution without heavy queues. Set up the pattern with clear handoffs and a shared source of truth."
---

**Leader and Worker Patterns for Coding Agents.** The leader-worker pattern assigns one agent (the leader) to decompose a task and supervise, while worker agents execute the resulting subtasks in parallel — each in its own worktree — with communication flowing through a *shared source of truth* (files in the repo: a plan, task files, result notes) rather than direct chat — a shape that assumes the workspace (such as Ferryx's managed worktrees) has already given every worker an isolated tree. It scales coding agents without message-queue infrastructure precisely because the filesystem is the queue: plain text artifacts any agent can read and write, versioned by git, debuggable with `cat`.

![leader-worker-agent-orchestration cover](/images/blog/leader-worker-agent-orchestration/cover.png)

## Roles of leader and workers

**The leader plans, dispatches, and integrates — it does not execute subtasks itself.** Its inputs are the goal and the repo; its output is a decomposed plan: an ordered list of subtasks, each with scope ("touch only `billing/`"), acceptance conditions ("tests X pass"), and dependency edges ("after task 2"). While workers run, the leader monitors state files, answers escalations, and — at the end — owns integration: reviewing worker results, resolving the merge sequence, and declaring the goal met. The discipline that makes this work is the leader *resisting execution*: the moment a leader "just quickly fixes" a worker's output itself, the architecture degrades into an untracked side branch of work with no record of who did what.

**Workers are narrow and deep: one subtask, one worktree, one definition of done.** Each receives its task file — scope, acceptance, constraints — and works until its result note says done or blocked. Narrowness is load-bearing: a worker that finishes with a diff *plus* a structured note ("changed X, verified Y, uncertain about Z") hands the leader something reviewable; a worker that returns raw diff forces the leader to reconstruct intent by re-reading the whole change. Blocked is a first-class outcome: a worker hitting an ambiguity writes the question and parks, rather than guessing — guessing across three workers is how parallelism manufactures contradictory code.

The asymmetry in *context* matters too: the leader needs breadth (the goal, the plan, all workers' status), each worker needs depth (its task, its tree). This inverts typical "one big shared conversation" agent usage — broad context centralized in one supervised place, narrow context fanned out — and it is why the pattern maps cleanly onto workspace structures where sessions are separate objects with their own trees rather than one interleaved stream.

## Handoff contract that works

Direct agent-to-agent chat is the anti-pattern: unstructured, unversioned, and lost on crash. The contract that replaces it is file-based, with five artifacts:

1. **`plan.md` (leader → workers).** The decomposition: task IDs, scopes, dependencies, acceptance. Written once before dispatch, edited only by the leader — workers *read* it; a plan drifting mid-flight under multiple writers is how scopes silently overlap.
2. **`tasks/<id>.md` (leader → one worker).** The dispatch: everything that worker needs to start without asking — goal, file scope, acceptance check, allowed-to-touch list, and the explicit boundary ("do not modify shared config; escalate instead").
3. **`results/<id>.md` (worker → leader).** The return: summary of change, how acceptance was verified (commands + outcomes), open questions, and blast-radius notes ("touched `schema.sql`, needs migration review"). A worker that cannot fill this in has not finished — the template *is* the definition of done.
4. **Status transitions in a single register** (`status.yml`: `queued → running → done|blocked`, with timestamps). One file, one writer per row (each worker owns its row), readable by anyone — this is the leader's dashboard and the operator's dashboard simultaneously.
5. **Escalation protocol in both directions.** Workers signal `blocked` + question in their result file; the leader answers by *editing the task file* (append-only Q&A section), not by side channel — so the answer survives crash, context compaction, and handoff to a different operator.

Git gives the contract free durability: commit task dispatches and results as they happen, and the entire run reconstructs from history. The [spec-driven loop](/blog/spec-driven-agent-loop/) covers the same file-based discipline for single-agent iteration; leader-worker extends it across many.

## Failure and retry handling

Parallel workers fail in characteristic ways; the pattern pre-defines responses:

- **Worker dies mid-task** (crash, context exhaustion): status goes stale, leader notices via heartbeat/age, re-dispatches the task file to a fresh worker in a *fresh worktree* — the dead tree stays for autopsy, the new run starts clean. Idempotence lives in the acceptance criteria: "tests pass" is re-achievable; "apply 14 manual steps" is not, so task files forbid unrecorded manual state.
- **Worker returns garbage** (diff fails acceptance): the result file says `done`, the acceptance check says otherwise — leader re-dispatches with the *failure output appended* to the task file. Second attempts that incorporate the first attempt's error message converge far faster than blind retries; a third failure escalates to leader-decomposed-smaller (the task was mis-scoped, not mis-executed).
- **Two workers conflict** (overlapping scopes discovered at merge): scopes were wrong in `plan.md` — leader re-partitions, and the loser's work is rebased or discarded *by policy stated upfront* (merge order is plan order; later workers re-read earlier results before integrating). Stating the policy before dispatch prevents workers from "defensively" hedging into each other's territory.
- **The goal itself shifts mid-run:** leader freezes dispatch, updates `plan.md` with a changelog entry, and re-derives task files for unstarted work; running workers finish or are explicitly cancelled with reason. Silent plan drift — workers executing yesterday's goal — is the failure this freeze exists to prevent.

Escalation to a human follows the same channel: `blocked` entries in the register, oldest first, are the operator's queue — which is also the shape the [attention inbox](/blog/attention-inbox-design/) argument says status should present. Underneath all of it sits the isolation floor (one worktree per worker, ports and DBs fenced per tree — see [port partitioning](/blog/agent-port-conflicts-across-worktrees/) and [database isolation](/blog/parallel-agent-database-conflicts/)), and the [parallel agents use case](/use-cases/parallel-ai-agents/) documents the workspace mechanics these roles run on.

![Leader and Worker Patterns for Coding Agents illustration](/images/blog/leader-worker-agent-orchestration/body-1.png)

## FAQ

### Is this just multi-agent orchestration with extra steps?

It is multi-agent orchestration *with the specific simplification that no message broker exists*: files and git replace queues and topics. Compared to heavier frameworks, you trade delivery guarantees (no ack protocol — you poll status files) for total inspectability: every handoff is a diff you can read. For coding tasks — where artifacts are files anyway — the trade tends to favor the file-based version until you need hundreds of workers.

### How big should a worker subtask be?

Small enough that one worker can reach "verified done" within a single focused session, large enough that the handoff overhead (task file + result file) is proportionally cheap — in practice, a coherent unit with a runnable acceptance check (a module, a test suite, a migration), not a single function and not a whole feature. When a task's result file needs three paragraphs of "partially done," the plan was wrong to make it one task.

### What model or tool should leaders versus workers use?

Match capability to role shape: the leader needs broad reasoning over plan/integration decisions (often the strongest available model or a human-authored plan), workers need reliable execution within tight scope (often cheaper/faster — the model-lock pattern of routing worker nodes through one configured category and keeping orchestration elsewhere). The architecture does not depend on the choice; the *separation* of planning context from execution context is what keeps either choice tractable.

### Can one human supervise leader-worker runs directly?

That is the design intent: the human reads one register (statuses, blocked questions, oldest-first) instead of N raw streams, intervenes at *decision points* the leader escalates, and reviews integration rather than every intermediate diff. Supervision cost becomes the leader's escalation quality — which is why the handoff contract's Q&A channel matters: sloppy escalations drag the human back into micromanagement the pattern was built to remove.
