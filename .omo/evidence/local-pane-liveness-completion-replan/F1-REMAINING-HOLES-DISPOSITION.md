# F1 remaining holes — disposition by category

**Written:** 2026-10-05, after the four landings of this session
(`af95b9a7`, `c43b3e5e`, `a13494e0`, `9be5eb3f`, `f50d74a8`, `238f7b3d`).
**Do not quote a single "holes closed" number** — a hand-maintained count drifted twice in one session
(13 → 15). Re-derive per row from the table below.

## Category 1 — closed by execution on a host (done)

| Hole | Closed by |
| --- | --- |
| H-17a | `pane_liveness_contract` filter, **10 passed, exit 0** |
| H-14 | five-site generation-compare mutation → baseline 3 passed → **0 passed; 3 failed** → restore 3 passed |
| H-15 | stale-generation fence removed → baseline 19 passed → **1 failed; 18 passed** → restore 19 passed |

Evidence: `E/final/H-14-H-15-H-17A-EVIDENCE.md`.

## Category 2 — closed by reading and by landing (done)

| Hole | Closed by |
| --- | --- |
| F1 audit itself | `E/F1-PLAN-TO-ARTIFACT-AUDIT.md` |
| H-18 | conflicting-fingerprint rejection: named test authored + harness wiring; the daemon half is delivered as an artifact |
| H-19 | "LSP unavailable" recorded explicitly in the Task 8 evidence |
| H-20 | synchronous-I/O audit — native surface **COMPLIANT**, seven gated QA-surface sites named |
| H-25 | old1-16 ↔ new-task mapping table with acceptance preservation |
| H-26 | per-task commit-subject deviations recorded **without rewriting history** |
| H-1…H-5, H-8…H-11 | `EVIDENCE-INVENTORY.md` |
| H-24, H-27 | final-finding artifacts; G9 disposition corrected, gate sweep in progress |
| **F3 capture gap** | `runSplitCancelScenario` wired + 9 call sites, **`9be5eb3f`**, mutation-proven |
| **H-16 / H-17b** | executed under their own names |

## Category 3 — NOT closed, and why (requires the native matrix to pass)

These four are **execution holes**, not code holes. Each needs a native run that reaches a PASS, and no native run
has ever passed (23 BLOCKED / 18 FAIL / **0 PASS** across 41).

| Hole | Requirement | What blocks it |
| --- | --- | --- |
| **H-21** | 12 native scenario runs with artifacts | 0 of 12 have artifacts; every run stopped upstream |
| **H-12** | `cmd_terminal_spawn_operation` executed with a frontend-shaped request | The split IPC never reached Rust in any run |
| **H-13** | QA live arm reachable on the GUI boot path (fixed; execution evidence needed) | Needs a native run to observe the arm |
| **H-23** | headless diagnostic-classifier smoke | Explicitly deferred, never executed |

**What changed for these this session**: the split path's **silent exits now name themselves**
(`split.pane.layout-noop`, `terminal.localSplit.earlyReturn`), so the next native run attributes the stall by
reading one line instead of inferring from an absence. That is the precondition for these runs producing usable
evidence; it is not the evidence itself.

**One claim re-confirmed so it is not doubted later**: the "zero `FERRYX_QA_SPLIT_*` tags" finding is **not** a
missing feature flag. The recorded build args carry `features: ['local-split-qa', 'qa_barrier']`, every
`app.stderr.log` carries the product's own `[cmd_terminal_spawn] request received` line (so the sink works), and
no `FERRYX_QA_*` tag appears in any app stderr — the tag names that do appear live in the harness's
**expected-marker lists**, not in product output.

## Category 4 — parked by the user's instruction (mac)

| Hole | Blocked on |
| --- | --- |
| **H-6** | `E/task-10/` absent; version still `2026.928.7` |
| **H-22** | mac has no measured result at `70eefafe` beyond `all-targets` |
| **H-28** | two Task 8 mac-host unknowns (whether pkill applied, whether disk was reclaimed) |
| mac scenarios | `diagnostic-classifier`, `retained-handover`, `handover-abort`, `stale-binding`, `suspension-ownership` — the last three fail honestly at fixture setup because fixture kinds `externally-stopped`/`adopted` are not configured |

## Summary

- **Categories 1 and 2**: closed, each with a cited artifact or executed measurement.
- **Category 3**: **open**; the code precondition is now in place, the runs are not.
- **Category 4**: **parked by instruction**.

No hole in Category 3 is reported as closed, and none is described as blocked by code — the code path is wired
and mutation-proven; what is missing is a native run that reaches a PASS.
