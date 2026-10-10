# H-26 — per-task commit subjects: what the plan prescribed, what actually landed

**Plan clauses audited:** `/Volumes/T9-Mac/project/ferryx/.omo/plans/local-pane-liveness-completion-replan.md`
per-task `Commit:` lines — 121 (new1), 132 (new2), 142 (new3), 152 (new4), 161 (new5), 171 (new6),
182 (new7), 191 (new8 = N), 200 (new9 = N), 209 (new10) — plus the `## Commit strategy` section
("Commit one coherent verified increment, never entire dirty overlay… Footer
`Plan: .omo/plans/local-pane-liveness-completion-replan.md`").

**Hole closed:** F1 audit `H-26` — `E/F1-PLAN-TO-ARTIFACT-AUDIT.md`, HOLES / Holes B, row H-26 ("None
of the prescribed subjects was used; several tasks' work landed inside the mixed composition commit
`5464da0d` rather than per-lane verified increments"; stage "F2 (note)").

**This is a historical deviation and it is documented, not repaired.** No rebase, no amend, no
history rewrite, no reset was performed. The commit chain is exactly as found.

**Author:** host-independent F1-hole lane (`st_01a108d2`), 2026-10-05. Read-only git inspection
(`git log --reverse --oneline`, `git log --format=%B`, `git show --stat`, `git rev-list --count`).

## 1. The real chain

> **POSTSCRIPT — re-measured 2026-10-05, after this document's first draft.** A concurrent lane
> committed `14744c8a` (`fix(qa): search the window scope when focus cannot be resolved, and wait for
> exit-file content`) on top of `cfb4374b`, touching six `scripts/**` files. The chain is therefore now
> **37 commits** with **36 footers**; the only commit still lacking the `Plan:` footer is `cfb4374b`
> (now a middle commit, not the tip). Nothing else in this document changes: the new commit carries no
> prescribed subject either, and it is another `fix(qa):` measured-repair commit, which is the same
> deviation pattern section 3 describes. Every other measurement below is stated at the revision it
> was taken at.

- **Base:** `d82b35e4` — **reference revision for this document:** `cfb4374b` — **count at that revision:** 36 commits (`git rev-list --count d82b35e4..HEAD`).
- **Plan footer (measured at `cfb4374b`):** 35 of the 36 commits carry
  `Plan: .omo/plans/local-pane-liveness-completion-replan.md` in their body. **The exception is
  `cfb4374b` itself** (`fix(qa): raise the delegation retry sequence to outlast the measured 153s
  burst`), which has a full body but no footer line.
- **Shape:** one large composition commit, then a repair series, then a 21-commit QA-producer series.

| Phase | Commits | Subjects |
|---|---|---|
| Composition | `5464da0d` | `feat(terminal): compose pane-liveness completion candidate for consolidated verification` (**68 files, +15346/−241**) |
| Pass-1 repairs | `172baa87`, `21dea3c0` | `fix(ui): repair pane-liveness pass-1 findings …` (24 files); `fix(pane-liveness): land round-3 backend, native, frontend and adapter repairs` (31 files) |
| Pass-3/4 scoped repairs | `764934a4`, `7c0fed30`, `17295029`, `b0ed4bef`, `d97233c1`, `6c69715f`, `abd9e890`, `ad0ffb5a`, `11c3a46a`, `e57685ec`, `426d1b27`, `39e722ce`, `70eefafe` | per-defect `fix`/`test` subjects (3 files for `39e722ce`, 3 for `70eefafe`) |
| QA producer surface (the Task 9 blocker repair) | `dd9e6813`, `c34b90fc` … `cfb4374b` (21 commits) | `fix(qa): install the barrier channel on the GUI boot path …`, `feat(qa): author the native-scenario producer and consumer surface behind the local-split-qa gate`, then a measured-repair series |

## 2. Prescribed subject → what actually happened

Presence check (`git log --format=%s d82b35e4..HEAD | grep -cF "<prescribed subject>"`): **0 for every
one of the seven prescribed subjects.**

| New task | Prescribed subject (plan line) | Used? | Where its work is identifiable in the history | Why it deviated |
|---|---|---|---|---|
| 1 | `fix(terminal): preserve verified liveness diagnostics baseline` (121) | **No** | `5464da0d` — the composition commit carries the task-1 selected/excluded hunk set (27 files in `E/task-1/manifest-r3.json`); `E/task-1/*` is the evidence | The integration owner composed the whole candidate (tasks 1–7 authoring) into one commit before any verification ran, under the plan's readiness-gate amendment ("compile plus changed-path tests permits dependent implementation"). The plan's own `Commit: Y for selected verified code only` was not honored per lane. |
| 2 | `feat(terminal): define retained identity and bounded split contracts` (132) | **No** | `5464da0d` (`daemon/protocol.rs`, `ipc/pane_liveness_contract.rs`, `ui/src/lib/types.ts`, `ui/src/lib/localSplitContract.ts`), repaired in `21dea3c0`, one type fix in `426d1b27` | Same composition; the DTO increment was not split out as its own buildable commit. |
| 3 | `fix(terminal): complete reliable local split lifecycle` (142) | **No** | `5464da0d` + `21dea3c0`, then the fixture/portability repairs `b0ed4bef`, `d97233c1`, `6c69715f`, `ad0ffb5a`, `abd9e890`, `11c3a46a` | Same composition; the follow-on repairs each got their own commit but with defect-shaped subjects, not the lane subject. |
| 4 | `fix(terminal)`/`fix(daemon)` **per verified increment**, no single subject (152) | **No single subject prescribed; still not honored as increments** | `5464da0d` (identity/suspension/reader authoring), `39e722ce` (paired/ssh incarnation), `70eefafe` (handover adoption against the predecessor's authority) | Task 4's authoring is inside the composition commit; only the two defects found later landed as their own commits. The "not one mixed sweep" instruction was the one the composition violated. |
| 5 | `fix(terminal): fence native attachment and presentation identity` (161) | **No** | `5464da0d`, `21dea3c0`, `764934a4` (render-closure clone), `17295029` (presentation accessor test), `e57685ec` (deferred presentation path), `426d1b27` | Same composition; the native-side repairs were discovered by the verifier passes and committed individually. |
| 6 | `fix(ui): connect durable local split recovery and retained identity` (171) | **No** | `5464da0d`, `172baa87` (TS build errors, lifecycle assertions, caller fixtures, runner config root), `21dea3c0`, `7c0fed30` (transport fixture bound to the incarnation contract) | Same composition; `172baa87` is the pass-1 UI repair and is the closest thing to a task-6 increment, but under a pass-shaped subject. |
| 7 | `test(terminal): finish targeted native pane recovery scenarios` (182) | **No** | `5464da0d` + `21dea3c0` for the adapter authoring; the entire producer/consumer surface is `c34b90fc` … `cfb4374b` (21 commits) | Task 7's real work was not finished before verification: the GUI had no producer surface (audit `H-13`), so the lane kept working after the composition commit and produced its own measured-repair series. |
| 8 | `Commit: N except separately verified scoped repair with its own commit` (191) | **Honored in substance** | No commit claims to be task 8; the scoped repairs it discovered landed as `426d1b27`, `39e722ce`, `70eefafe`, `11c3a46a`, `e57685ec`, `ad0ffb5a`, `abd9e890`, `7c0fed30`, `764934a4`, `17295029`, `b0ed4bef`, `d97233c1`, `6c69715f` | — (the only task whose commit rule was followed) |
| 9 | `Commit: N; evidence only` (200) | **Honored** | No commit from task 9 | — |
| 10 | `chore(release): version verified pane liveness fixes` (209) | **No — and not applicable yet** | None: task 10 is unstarted (version metadata still `2026.928.7`) | Not a deviation: the commit does not exist because the task has not run. |

## 3. Why the deviation happened (as the run's own record shows)

1. **One composition commit instead of per-lane increments.** The replan's execution-order amendment
   (REPLAN header, "Execution-order amendment (user instruction, 2026-10-03)") moved all verification
   to a single consolidated final stage. The integration owner used that to land tasks 1–7's authoring
   as one commit (`5464da0d`, 68 files) and then let verification drive per-defect repairs. The
   prescribed subjects describe *lane deliverables*; the actual subjects describe *verification
   passes and defects*, which is what the post-amendment workflow produced.
2. **Lane batching under time pressure.** The composition commit is a single commit for what the plan
   enumerated as seven separate `Commit: Y` increments; the audit's own verdict rows record this for
   tasks 1/2/3/4/5/6/7 (`J1f`, `J2h`, `J3g`, `J4g`, `J5f`, `J6g`, `J7h`, all "No commit uses the
   prescribed subject").
3. **A shared worktree.** The candidate tree was edited by several lanes concurrently (the F1 audit
   documents a concurrent lane editing 7 files while the audit ran). Per-lane atomic commits from a
   shared tree are not achievable without per-lane staging discipline; the integration owner therefore
   committed by verification pass, which is also why the later series is defect-shaped.
4. **The one true violation of the commit strategy is the footer, not the subjects.** At the reference
   revision, 35/36 commits carry the required `Plan:` footer and `cfb4374b` does not; at the current
   tip (`14744c8a`) it is 36/37, still with `cfb4374b` as the single exception. That is the only
   mechanically checkable clause of the commit strategy that is not met.

## 4. Non-claims and non-actions

- **Nothing was rewritten.** No `rebase`, no `amend`, no `reset`, no history edit of any kind. This
  document is the deliverable, per the brief ("a historical deviation and cannot be undone, so the
  deliverable is an honest finding rather than a rewrite").
- The prescribed subjects are not "wrong" and the actual subjects are not renamed here. Both are
  recorded verbatim so a later reviewer can judge.
- **Not established:** whether any lane *tried* to commit separately and was blocked. No such attempt
  appears in the evidence this lane read; the claim is limited to what the history and the audit rows
  show.

## 5. Handoff

- If the plan's commit discipline is to bind future work, the two live levers are (a) require the
  `Plan:` footer mechanically at commit time (one commit already lacks it) and (b) state explicitly
  whether post-amendment commits may be pass-shaped rather than lane-shaped. Otherwise every future
  audit will re-open `H-26`.
