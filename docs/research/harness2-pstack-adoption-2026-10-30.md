# Harness² and pstack: what Ferryx's supervisor should adopt

Date: 2026-10-30. Status: proposal, not approved, nothing implemented.
Base: the unified supervisor on `merge/sup-20261030` (not yet in `main`). Every file
reference below is on that branch unless it says `main`.

## 1. Where this came from

On 2026-10-30 madrobot posted to the 포모단 KakaoTalk room what he grafted onto his Dori agent
from two sources:

- **Harness²** (Google Research, Apache-2.0, v0.1.0, published 2026-10-08). It rewrites the
  harness around a fixed model: system prompt, guardrails, memory, tool guidance, subagents,
  skills, scripts, plugins. Two loops:
  - *Task level, Propose-Probe-Compose*: run the task once with the base harness, an editor
    model proposes two candidate edits (one for general agent failures, one for domain
    procedure), both are run on the same task, a composer keeps only edits the observed
    behaviour supports. No ground-truth labels: it uses the contrast between runs.
  - *Harnessing level*: after each task a reflector stores the adopted harness plus a
    lessons file; that library shapes later proposals. Every task still starts from the base
    harness.
  - Caveats from its own README and coverage: every step multiplies runs (API cost), executor
    network is unrestricted and Codex runs unsandboxed by default, model runs go through
    Vertex AI, results are first-party only.
- **pstack** (Lauren Tan, Cursor plugin, MIT; ports exist for Claude Code, Codex, OMP). About
  51 skills: ~24 `principle-*` rules, ~27 working skills, and `poteto-mode`, an orchestrator
  that picks one of ~23 playbooks and fans work out to subagents. Only a handful of pieces
  are runnable code (`watch-pr`, `show-me-your-work`'s `log.sh`, `orch`, `check-plan.mjs`,
  `worktree-audit.sh`); the strongest promises ("nothing merges without a clean verdict")
  are prose the main chat is trusted to follow.

madrobot's own adoption, which is the useful part:

- Harness²: **ideas only, not installed**. A lesson ledger (`dori lesson add/list/mark`): when
  a task closes, record "rule to change, evidence, effect"; the next task in the same folder
  gets at most **8 lines** of lessons that proved effective.
- pstack: 21 principles kept as is, 29 Cursor-specific skills rewritten, plus Dori-side tools:
  a file-backed report, `verify-pr` (PR has tests, commit author correct), restart recovery,
  a run supervisor (stall detection, time/token/cost caps that only alert, per-stage
  checkpoints so push/deploy is not repeated), and a cost meter.
- The reviewer's (잡도리) one concern: fifty skills at once means nobody knows which one fired;
  log skill use and drop the ones that never fire.

## 2. What Ferryx already has (so we do not rebuild it)

| Harness²/pstack idea | Ferryx today |
|---|---|
| Verify against evidence, not the agent's claim | `goals/prompts.rs` gap assessment uses only attested refs; `supervisor/completion.rs` requires each met requirement to cite a check Ferryx itself ran from the requirement's verifier |
| Independent final review | `goals/converge.rs` blind convergence check: fresh context, contract plus attested evidence only |
| Bounded self-correction | `completion.rs`: an unproven outcome becomes one corrective run within the turn budget (`DEFAULT_TURN_LIMIT`), then a visible hold |
| Decision log | turn ledger (`goals/turns.rs`), receipts (`supervisor/receipts.rs`), judge calls table (`judge_calls`) |
| Restart recovery | `dispatcher.rs` `reconcile_claims_after_restart` holds uncertain claims, never resends |
| Durable memory | `supervisor/memory.rs`: owner-authored teach/correct/forget through mhc, with task-only exceptions (`once:`, `이번만:`) never learned |
| Push/merge watching | covered by the shepherdr plan (`.omo/plans/shepherdr-learnings-20261030.md`) |

What is **missing** is exactly Harness²'s second loop: nothing carries "what went wrong on the
last goal in this project" into the next goal. `memory.rs` deliberately refuses worker and
supervisor text ("callers pass only what the authenticated owner wrote"), so owner memory
must not be used for it.

## 3. Adopt

### A1. Project lesson ledger (core of this proposal)

A hub-owned store of supervisor-derived lessons, separate from owner memory.

**Data** (new migration, `supervisor_lessons`):

| column | meaning |
|---|---|
| `id` | uuid |
| `project` | repository identity, the same key `memory.rs` uses for project scope (never a host path, so it holds across worktrees and hosts) |
| `rule` | one imperative line, max 200 chars ("Run async tests 50 times before calling them stable") |
| `rationale` | why, one line |
| `source_goal`, `source_revision` | the goal and revision that produced it |
| `evidence_ref` | an attested ref (failed verifier check, divergence, owner correction), never worker prose |
| `state` | `candidate` -> `effective` / `ineffective` -> `retired` |
| `injected`, `helped`, `hurt` | counters (A2) |
| `created_at`, `updated_at` | |

**Writing lessons.** At goal close (converged, held at budget, or owner-cancelled after work),
one judge call (`Purpose::LessonReflect`, small model) receives: the contract, the turn ledger,
the divergence/correction history and the attested evidence. It returns 0-3 rules in strict
JSON. Inputs that can create a lesson:

- a requirement that diverged in `converge.rs` before it passed;
- a corrective run (`completion.rs`) and what fixed it;
- an owner correction (`request.rs` `corrected_instruction`): the *pattern*, not the text,
  because the text is owner-authored and already goes to mhc memory when it should.

A goal that converged first try with no correction writes nothing. Each lesson must cite an
`evidence_ref` that resolves; otherwise it is dropped (the same rule the gap assessment already
applies to agent claims).

**Using lessons.** `worker_instruction` (`admission.rs:1081`) and `corrected_instruction`
(`request.rs:1343`) append one fenced block:

    Lessons from earlier goals in this project (advisory; the grant and requirements above win):
    - <rule> (<rationale>)

- at most **8** lines (madrobot's cap; it stops the ledger turning into a second system
  prompt);
- `effective` first by `helped - hurt`, then newest `candidate`s to fill, at most 2 candidates;
- text passes through the existing `fence` so it cannot open or close a prompt block;
- the block is placed after the grant and before "Done when:", and its header says the grant
  and requirements win. A lesson can never widen `Allowed operations`.

### A2. Measure whether a lesson works (Harness² compose step, without extra runs)

Harness² needs probe runs because it has no ground truth. Ferryx has per-requirement verifiers,
so the outcome of the real goal is the probe. For every goal that received lesson L:

- `helped` += 1 when the goal converged with zero divergences and zero corrective runs;
- `hurt` += 1 when it diverged or needed a correction on a requirement in the area L names
  (the reflector tags each lesson with requirement keywords; a match on the failing
  requirement's text counts);
- otherwise no change.

Promotion: `candidate` -> `effective` after `helped >= 2` and `helped > hurt`;
-> `ineffective` after `hurt >= 2` and `hurt >= helped`; anything not injected in 30 days or
injected 10 times with no `helped` is `retired`. All thresholds are constants with tests.

This is correlation, not proof; the doc and the UI call it "observed", never "proven".

### A3. Owner control on every surface

Through the one supervisor service that already serves MCP, daemon, desktop, CLI and remote
HTTP (`supervisor/request.rs`), add `lessons_list(project)`, `lesson_retire(id)` and
`lesson_pin(id)` (pinned lessons are always injected and count toward the 8). Slack gets
`/omo-lessons`, next to the existing `/omo-memory`. No surface edits lesson text: the owner
retires it and, if they want a rule, teaches it to memory as an owner-authored item.

### A4. Two verifier rules taken from pstack

These tighten what Ferryx already enforces instead of adding skills:

1. **Proof ladder (from `blast-radius`).** A requirement verifier must be at least level 4: a
   command that calls the shipped code and fails when the claim is false. "Asserted" or
   "file:line" verifiers (levels 1-2) are refused at admission with a typed error, the same
   way a verifier Ferryx cannot run is held today.
2. **Seen-fail rule (from Productize's addition to `show-me-your-work`).** A verifier that
   has never been observed failing on this goal is reported as "unproven" in the completion
   provenance. Ferryx does not mutate code itself; it records whether the run history contains
   a failing result of that verifier before the passing one. This is shown, not enforced, in
   the first version.

### A5. Cost and stall caps per goal (from madrobot's run supervisor)

The turn budget limits turns, not spend or wall time. Add per-goal `token_cap` and
`wall_cap` (defaults from config, owner-lowerable, never model-raisable, like the turn budget
in `admission.rs`). Crossing a cap **alerts only** (owner DM via the existing owner-alert path),
as madrobot did; it does not stop the worker. Token spend comes from the engine's usage
records; if a worker kind reports no usage, the cap shows "not measured" instead of zero.

## 4. Do not adopt

- **Installing Harness².** A benchmark research tool: Linux plus Python 3.12 plus uv plus
  Vertex AI, unsandboxed executor by default, cost multiplied per refinement step. It also
  breaks our cross-platform rule and the "easy for anyone to install" rule.
- **Automatic harness rewriting (Propose-Probe-Compose on live goals).** Probing means
  running the same goal twice in parallel worktrees, doubling spend and pushing twice. If we
  ever want it, the safe place is cron remediation (`cron_remediate`), which already has a
  verified rerun; that is a separate proposal.
- **Lessons editing guardrails, grants or system prompts.** Lessons are advisory text inside
  one block. Harness² lets the agent change its own guardrails; this design does not.
- **`poteto-mode` or the full pstack bundle.** It is an orchestrator, and the supervisor
  already owns dispatch, review and merge decisions. Two managers collide.
- **Bulk-installing ~50 skills.** Per 잡도리's concern: without usage data nobody knows what
  fired. If skills are ever bundled for workers, log each skill load per goal first and keep
  only the ones that fire.

## 5. Order of work

All of it lands on top of the unified supervisor, so `merge/sup-20261030` reaches `main`
first.

1. A1 storage plus injection with an empty reflector (owner can pin lessons by hand). Small,
   immediately useful, no model cost.
2. A1 reflector call plus A2 counters.
3. A3 surfaces (service plus Slack command; desktop and phone show the list read-only first).
4. A4.1 admission refusal of low-level verifiers, then A4.2 provenance flag.
5. A5 caps.

## 6. How to verify

- Unit (hub): injection never exceeds 8 lines; pinned lessons count toward the cap; lesson
  text is fenced; the grant line is byte-identical with and without lessons; a lesson from
  project X never reaches project Y; a reflector reply citing an unresolvable evidence ref
  writes nothing; promotion and retirement thresholds, each seen failing first by breaking
  the threshold.
- Boundary: a lesson is never written through `memory.rs` teach (assert no
  `memory_receipts` call during reflection).
- Scenario (existing supervisor QA harness): goal 1 on a fixture repo fails a verifier once and
  is corrected; goal 2 on the same project shows the lesson in its first instruction; goal 3 on
  a different project does not.
- Cost: the reflector runs once per closed goal that had a divergence or correction, never per
  turn; assert the `judge_calls` count in the scenario.

## 7. Open questions for the owner

1. Should a new lesson need owner approval before injection, or start as `candidate`
   automatically (as proposed, capped at 2 candidates per instruction)?
2. Default `token_cap` and `wall_cap`, or start alert-only with no default?
3. Should A4.1 refuse low-level verifiers immediately, or warn for a release first?

## Sources

- madrobot's post and 잡도리's replies, 포모단 room, 2026-10-30 12:17-12:21 KST (PirateTalk
  backend, room 18491246151592761).
- Techstrong.ai, "Google Research Releases Harness² to Improve AI Agents Without Retraining
  Models", 2026-10-20.
- Productize, "Cursor pstack review: 51 skills with a built-in manager, and the 5 pieces we
  took".

