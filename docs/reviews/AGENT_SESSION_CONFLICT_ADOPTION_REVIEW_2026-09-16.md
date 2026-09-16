# Gate Review: AGENT_SESSION_CONFLICT Recurrence Prevention (Conflict Adoption)

**Date:** 2026-09-16
**Reviewed change set:** Conflict adoption at the reconnect choke point
**Status:** COMPLETE — Gate reviewer verdict: APPROVE (confidence HIGH, zero blocking issues). Overall: REVIEW PASSED.

## Change summary

When an agent-resume spawn fails with `AGENT_SESSION_CONFLICT`, the UI no longer dead-ends with a raw JSON error. If the owning backend session is live and belongs to the same workspace, the GUI **adopts** it: the reconnecting leaf rebinds to the existing `backendSessionId` through the normal reconnect path (attach + persist + `REBIND_SESSION_BACKEND`). If another local leaf already binds that backend id, a friendly message is shown. Daemon claim semantics are unchanged; a new passthrough command `cmd_terminal_describe` exposes session details.

### Files

- NEW `ui/src/lib/agentConflictAdoption.ts` (+ test): `extractConflictingBackendSessionId`, `withAgentConflictAdoption`
- MOD `ui/src/lib/appReconnectDependencies.ts` (compose wrapper)
- MOD `ui/src/lib/tauri.ts` (`TerminalDescribeResult`, `describeTerminal`)
- MOD `ui/src/state/workspaceRuntime.ts` (friendly message for `AGENT_SESSION_CONFLICT`)
- MOD `ui/src/lib/agentReconnect.integration.test.ts` (adoption e2e)
- MOD `ui/src/App.test.tsx` (mock gains `describeTerminal`)
- Rust (landed inside concurrent commit `72904a03`): `cmd_terminal_describe` passthrough + registration

## QA matrix

| # | Criterion | Method | Result | Artifact |
|---|-----------|--------|--------|----------|
| 1 | Adoption precondition: owner live & describable on real daemon | UDS v4 probe of live owner `49376c02-…` (omo-bridge, running=true) | PASS | /tmp/review-qa/row1-probe.txt |
| 2 | Unit + integration: extraction, adoption mapping, double-bind, mismatch, failure passthrough, e2e rebind | `bun run test agentConflictAdoption agentReconnect.integration` | 14/14 PASS | /tmp/review-qa/row2-focused-tests.txt |
| 3 | Auto-resume regression | `bun run test App.test -t 'Automatic agent session resume'` | 2/2 PASS | /tmp/review-qa/row3-app-autoresume.txt |
| 4 | Full UI suite attribution | 4981 passed / 8 failed; all 8 identical on HEAD-baseline A/B (concurrent sessions' domains) | 0 failures attributable to this change | /tmp/ui-final.txt, /tmp/ab-baseline5.txt |
| 5 | tsc + vite build | exit 0 at our verification (19:50); current tree fails only in concurrent session's `ui/src/test/setup.ts` (3x TS2550, not ours) | PASS (ours) | /tmp/review-qa/row5-build.txt |
| 6 | cargo check | Finished dev profile, 0 errors | PASS | /tmp/review-qa/row6-cargo-check.txt |
| 7 | cargo test --lib | 1386 passed / 0 failed (includes a03 after fixture-binary rebuild + concurrent alignment) | PASS | /tmp/review-qa/row7-cargo-lib.txt |
| 8 | End-to-end GUI click-through (open conversation while zombie owner holds it) | Requires debug app + manual interaction; agent is barred from desktop UI automation | PENDING-USER (row 2 e2e is the automated proxy) | — |

## Known notes (pre-verdict)

- `workspaceRuntime` friendly-message branch has no direct unit test; the identical message string is asserted by the adoption double-bind unit test.
- Adoption covers the spawn path only; the re-attach/rebind path (`session_metadata_provider.rs bind_provider_session`) still returns `AGENT_SESSION_CONFLICT` — by design, spawn is the user-visible reopening path.
- Concurrency: Rust additions were swept into the concurrent session's commit `72904a03`; UI files remain uncommitted in the shared tree.

## Verdict

Gate review (omo-senpi-gate-reviewer, mahoquot/gemini-3.8-flash-high): **APPROVE / HIGH**.

- All 8 goal sub-requirements ACHIEVED with file:line evidence (structured extraction, describe bridge, liveness/workspace validation, rebind path, double-bind friendly error, toast + affordance reachability, remote/paired guard, daemon claim semantics untouched).
- QA rows 1-7 VERIFIED from artifacts; row 8 GUI click-through: automated proxy (integration e2e) verified; manual one-click check left to the user by design (no desktop automation by the agent).
- Blocking issues: none.
- Reviewer artifacts: `.omo/evidence/st_01a0aa1c-gate-review.md`.

Reviewer notes (non-blocking): raw-JSON defensive parse in extraction is well-scoped; `cmd_terminal_describe` is desktop-IPC-only and exposes standard session telemetry without credentials; adoption correctly targets only the spawn/resume path.
