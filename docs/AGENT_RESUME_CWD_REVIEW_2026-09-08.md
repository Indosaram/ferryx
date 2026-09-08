# Final scoped gate review

recommendation: APPROVE

blockers: []

## Original intent and desired outcome

originalIntent: Review the final Ferryx/Senpi session restoration fixes once more before user-authorized commits, without implementing, deploying, restarting the live daemon, or touching foreign shared-tree work.

desiredOutcome: Ferryx resumes the exact agent-owned OMO session in its original nested project directory, repairs legacy ID-only state, preserves the workspace/worktree jail, and never implicitly forks or invents a provider ID. Explicit affirmative cross-project confirmation in Senpi forks; negative/default/EOF input cancels.

userOutcomeReview: The inspected changes satisfy the named source-level outcome. Approval is for the scoped source commit, not a claim that the running release Ferryx daemon has been updated. Recorded QA describes isolated real daemon/PTY execution; this review did not rerun tests, per the task instruction.

## Criteria and direct code review

Criterion identifiers below label the criteria stated in the task, not additional requirements.

- C1, original nested CWD and exact ID: `src-tauri/resources/agent-extensions/ferryx-agent-state.ts:29-42` retains the agent session ID and reports its actual transcript file. `src-tauri/src/terminal/resume_cwd.rs:70-106` reads the first bounded header, requires session type, matching ID, and absolute CWD. `src-tauri/src/daemon/server.rs:2036-2085` substitutes that directory before command execution. Existing `src-tauri/src/terminal/shell.rs:205-210` still supplies exactly `omo --session <validated ID>`; no new provider identity or fork argument is introduced.
- C2, legacy ID-only repair: `resume_cwd.rs:34-67,109-158` searches the configured/default/profile stores for the exact filename suffix and verifies the header ID. Missing and conflicting metadata fail rather than falling back to the pane root. `ui/src/lib/agentReconnect.integration.test.ts:20-73` exercises deserialize/reconnect/reducer/serialize with corrected daemon CWD while retaining the workspace root and transcript identity.
- C3, jail preserved: `server.rs:2050-2077` retains existence, directory, canonical workspace and resolved-worktree containment checks after the override. The added test at `server.rs:2995-3018` requests an outside-workspace transcript CWD and verifies no session is spawned.
- C4, new disk work off the async executor: `server.rs:2036-2045` wraps the new resolver in `ipc::run_blocking`. Existing CWD jail filesystem checks were not moved or newly introduced by this diff.
- C5, explicit confirmation semantics: Senpi `packages/coding-agent/src/main.ts:360-373` resolves the answer before synchronous readline close can settle false. Existing `main.ts:475-496` still gates cross-project forking on that result. The new regression uses real readline with substituted streams, not a mock that bypasses the close event.
- C6, final availability fixes: `resume_cwd.rs:52-58` skips regular files such as `.DS_Store` when enumerating profiles. `resume_cwd.rs:71-89` opens Unix transcripts nonblocking and checks the opened descriptor is a regular file, rejecting FIFOs before reading. The FIFO regression provides a valid header, so it can fail against the prior acceptance behavior; it is not merely an empty-file rejection test. The daemon QA includes a `.DS_Store` fixture for both transcript and legacy scenarios.
- C7, scope and operational restrictions: inspected explicit-path diffs only for tracked changes; read the new resolver, QA scripts, regression and documentation. No implementation edits, commits, test runs, provider calls, live restarts, desktop automation, or external writes were performed by this reviewer. Only this report was written.

## QA artifact inspection

The documentation matrix records 81 related UI tests, 10 resolver/jail tests, one existing spawn-validation test, frontend build/typecheck, default-feature debug build, and final isolated daemon success after the FIFO/profile fixes. The source receipt records nine confirmation regressions, TypeScript, CLI smoke 8/8, and the source/installed six-case PTY matrix.

I inspected the QA drivers rather than accepting counts as coverage:

- `scripts/qa/verify-ferryx-resume-cwd.mjs` launches an isolated daemon with temporary HOME/runtime/state and an actual PTY child reporting actual CWD/argv over TCP. It asserts `--session` with the exact requested ID, nested CWD, daemon description CWD and unchanged workspace for transcript-backed and legacy requests, with profile metadata present. It does not connect to the user's daemon.
- `scripts/qa/verify-omo-fork-confirmation.mjs` invokes real source/installed session-manager bootstrap under a PTY. It checks affirmative new identity/current-project CWD/two transcripts; same-project original identity/original CWD/one transcript; and negative/default/EOF no returned manager and one transcript. Inputs follow prompt observation, not fixed sleeps.
- `ui/src/lib/sshAgentState.test.ts` subscribes before the extension event and inspects parsed state over real loopback TCP.
- Senpi regression streams preserve the synchronous close behavior at issue. Cases cover y/yes and case variants, negative/default answers, and EOF without timing delays.
- Explicit-path `git diff --check` passed in both repositories during this review. No independent execution-pass claim is made for the recorded QA.

## remove-ai-slops and programming perspective

The named skill files were not found in the inspected user skill directories or repository skill locations. Applied the criteria supplied in the task directly to the diff, production code and tests.

- Excessive/useless tests: no blocker. Nine confirmation cases distinguish affirmative/default/EOF semantics with a small parameterized suite. The resolver cases target metadata identity, failure behavior, conflicts, file type and CWD; the daemon test checks enforcement at the real spawn boundary.
- Deletion-only/requested-removal tests: none introduced.
- Tautological or implementation-mirroring tests: no blocker. The UI mock supplies a daemon response intentionally, but exercises the real downstream state/persistence flow; the separate daemon QA covers the mocked boundary. The standalone `pwd` resolver test is weaker than the daemon QA and partly redundant, but not a product failure or material maintenance burden.
- Prose pinning: no new assertions pin documentation or ordinary prompt prose. The QA observes the actual machine interaction prompt marker to trigger input and checks resulting state/files.
- Unnecessary extraction/parsing/normalization: no blocker. Parsing the session header is necessary to recover CWD; no whole-transcript parser or additional identity layer was added. Exporting the existing confirmation helper exposes the real race to the regression without extracting a replacement abstraction.
- Error handling and scope drift: no new silent error swallowing or broad refactor. Resolver errors fail the resume explicitly. Existing extension transport behavior and existing jail logic are unchanged.

The supplied documentation's pre-commit re-review section explicitly describes the two reproduced availability failures and corrections, but does NOT explicitly document this same skill-perspective/overfit checklist. No separate scoped code-review report was supplied or found. Consequently equivalent prior-review skill coverage cannot be confirmed; this report supplies the direct check, not a claim that prior report coverage exists.

## Checked artifact paths

Ferryx root: `/Users/indo/code/project/orca-lite`

- `src-tauri/resources/agent-extensions/ferryx-agent-state.ts`
- `src-tauri/src/daemon/server.rs` (current 36-line diff and adjacent spawn/jail context)
- `src-tauri/src/terminal/mod.rs` (one-line module registration diff)
- `src-tauri/src/terminal/resume_cwd.rs`
- `src-tauri/src/terminal/shell.rs` (existing resume argument contract)
- `ui/src/lib/agentReconnect.integration.test.ts` (scoped diff)
- `ui/src/lib/sshAgentState.test.ts`
- `scripts/qa/verify-ferryx-resume-cwd.mjs`
- `scripts/qa/verify-omo-fork-confirmation.mjs`
- `docs/AGENT_RESUME_CWD_FIX_2026-09-08.md`

Senpi root: `/Users/indo/code/senpi-ferryx-confirm-fix`

- `packages/coding-agent/src/main.ts` (four-line diff and session bootstrap context)
- `packages/coding-agent/src/changes.md` (actual changed path corresponding to the task's abbreviated `src/changes.md`)
- `packages/coding-agent/test/suite/regressions/fork-confirmation.test.ts`
- `local-ignore/qa-evidence/20260908-fork-confirmation/receipt.md`

## Exact evidence gaps and nonblocking notes

1. The two receipts contain summarized results, not attached raw command logs or final-tree hashes. Test success is recorded evidence, not independently reproduced in this read-only review. The task explicitly says not to rerun unchanged QA; no stated product criterion requires a raw-log artifact.
2. Browser smoke is mentioned in the task but not recorded in the inspected source receipt or Ferryx matrix. It is not evidence for these CLI/daemon fixes and no browser behavior change is scoped; no browser-smoke pass is claimed here.
3. No separate scoped code-review report, standalone manual QA matrix beyond the documentation table, or notepad path was provided. Prior skill-check coverage cannot be confirmed. These are evidence-process limitations, not a demonstrated failure of a named product success criterion.
4. LSP and no-default-feature limitations are explicitly recorded in the documentation; Windows/Linux and the running release are not verified by this review.
5. `omo-agent-toolkit ulw-loop status --json` returned `ULW_LOOP_PLAN_MISSING` for this child session, so the report uses the required fallback `.omo/evidence/omo-resume-cwd-confirmation-gate-review.md`.

No specific scoped success criterion failure was found. APPROVE for the requested commit gate; deployment remains outside this verdict.
