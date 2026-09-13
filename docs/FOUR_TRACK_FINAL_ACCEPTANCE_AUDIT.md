# Four-track final acceptance audit

Date: 2026-09-13. This audit maps the original objective to preserved evidence.
It does not replace the objective or waive its historical conditions.

## Deliverable and verdict

The requested deliverable is four isolated worktrees under
`/Users/indo/code/project/orca-lite-wt`, implementing the disk-management,
agent-state watchdog, documentation, and regression-test tracks, with node and
supervisor execution evidence, cleanup receipts, no commits, and the original
live daemon PID preserved.

**Overall verdict: NOT ACHIEVED.** Current implementation and verification
results cannot retroactively satisfy the original PID, no-commit, or
pre-production RED requirements. The existing user decisions to keep those
requirements unmet and end the original run unsuccessfully remain recorded in
the main repository's
`docs/FERRYX_4TRACK_ACCEPTANCE_CORRECTION_2026-09-13.md`.

No automatic completion is authorized by an empty todo list. The goal API
returned `{"goal":null}` during this audit; there is no stored goal to mark
complete or blocked. No new goal or baseline was created to bypass that fact.

## C1: sa-worktree-disk

Worktree: `../sa-worktree-disk`.

- Locked sections A-F: scope audit and repairs are recorded in the completed
  task list and `docs/WORKTREE_DISK_EVIDENCE.md`,
  `docs/DISK_SCAN_LIFECYCLE_REPAIR_EVIDENCE.md`, and
  `docs/DISK_DELETION_REPAIR_EVIDENCE.md`.
- Portable traversal, no `du`, blocking offload, cancellation/progress,
  metadata, Tauri commands, dialog, and reuse of the preview/destructive
  deletion flow are covered by those implementation records. Desktop
  interaction is not established by source inspection.
- The backend and production-repair check is `docs/DISK_REPAIR_LEAD_VERIFICATION.md`.
  It records cargo check exit 0; three deletion-repair and three IPC disk tests
  passing; 31 related Vitest tests passing with TypeScript compilation exit 0.
- The earlier direct Bun run passed 20 disk/lifecycle tests but the existing deletion
  test module failed to initialize with `TypeError: vi.hoisted is not a
  function`. That combined command exited 1. The configured Vitest run passed
  all three suites; it is not a literal pass of the failing Bun command.
- The current runner gap is now repaired without production edits or weaker
  assertions. `docs/DISK_BUN_COMPATIBILITY_EVIDENCE.md` in the disk worktree
  records initial RED and replacement of Vitest-only module hoisting with the
  public Tauri IPC mock boundary. Independent supervisor monitor
  `mon_J9HWGBD4FF0A2YC2` / `bash_314` recorded 31 direct Bun tests passing,
  31 configured Vitest tests passing, TypeScript exit 0 and diff-check exit 0.
  All 11 deletion tests remain, including real default-service request routing.
  This closes the selected dialog suites' Bun failure, not unrelated UI failures.
- Full library tests were not rerun during repair because unrelated fixtures
  create commits. Filtered touched-module passes do not claim a full-suite pass.
- The initial implementation's pre-production RED remains unavailable.
  Later mutation and repair RED/GREEN evidence does not replace that history.

## C2: sa-watchdog

Worktree: `../sa-watchdog`.

- Process-evidence release, manual reset, release-reason logging, and the
  no-timeout HOLD contract are mapped in
  `docs/AGENT_STATE_WATCHDOG_EVIDENCE.md`.
- The phrase "absence of evidence must hold, never reset" is an assertion
  message, not a test name. Its containing test is
  `agent_detect::independent_probe::probe_working_then_unclassifiable_screen_holds_working`.
- That invariant and the named
  `agent_to_shell_transition_releases_state` and
  `quiet_agent_with_no_output_and_live_process_is_not_released` tests have
  recorded passing results. The evidence records 11 agent-state tests and
  12 independent-probe tests.
- Manual-reset failure handling was subsequently repaired. Independent
  supervisor execution of five related frontend suites passed 145 tests,
  exit 0, in `docs/WATCHDOG_RESET_OUTCOME_REPAIR_EVIDENCE.md`.
- The original implementation RED remains missing. Mutation evidence proves
  regression sensitivity, not that a test ran before the original change.

## C3: sa-docs

Worktree: `../sa-docs`.

- Deliverables exist at `docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md` and
  `site/src/content/docs/privacy.md`.
- `docs/DOCUMENTATION_CLAIM_COVERAGE.md` maps source-read evidence for 51 guide
  and 36 privacy claim clusters. `docs/GUIDE_FINAL_CORRECTION.md` and
  `docs/PRIVACY_FINAL_CORRECTION.md` map their dispositions.
- Final supervisor evidence is `docs/DOCUMENTATION_FINAL_LEAD_CHECK.md`:
  32 verifier regression tests pass, 354 structural references and 16 explicit
  token assertions pass, site build exits 0, and diff check exits 0.
- The automatic verifier explicitly does not check prose truth. Semantic
  coverage comes from the source-read audit and corrections, not its green
  status. External deployment recommendations remain labeled unexecuted.
- The site build retains a duplicate `privacy` identifier warning. Neither
  Linux deployment nor live external relay connectivity was verified.

## C4: sa-tests

Worktree: this worktree.

- The three tests exist and have execution evidence: multi-host input latency,
  selection preservation, and process-level duplicate prevention.
- `ui/src/remote/input-latency-soak/REPORT.md` records a four-host, 60-second
  baseline with 4,011,582 echoes. This measures production client transport
  against simulated loopback echo gateways, not SSH/PTY/Rust gateway/rendering
  latency. It records three harness tests passing and percentile mutation
  RED followed by restored GREEN.
- The same report records the selection mutation failing with the whole row
  replacing the selected word. Original per-track supervisor execution is
  preserved in the main repository's
  `.omo/notepads/mass-ulw-ferryx-4track-20260913.md`.
- The original duplicate mutation's exact diagnostic and cleanup were not
  recoverable, as documented in `docs/TEST_EVIDENCE_RECOVERY.md`.
- New supervisor-run evidence now fills the current regression-proof gap:
  `docs/DUPLICATE_DAEMON_PROSPECTIVE_PROOF.md` and
  `docs/duplicate-daemon-prospective-logs/{baseline,red,green}.log`.
  Baseline exits 0; LOCK_EX-to-LOCK_SH mutation exits 101 specifically because
  the duplicate reached readiness; exact source restoration is hash-verified;
  the restored test exits 0. Both mutation children are reaped before the
  assertion. All six fixture PIDs across the three runs were reaped and all
  fixture directories plus their temporary parent were removed.
- Child environment is cleared and all runtime/data/session/home/temp paths
  are fixture-local. Cleanup uses retained PID-checked protocol connections,
  not OS signals. These are configuration and runtime receipts, not a sandbox
  or proof of every possible failed-start cleanup branch.

## C5 and constraints

- Independent per-worktree commands and later repair checks have been run by
  the supervisor; evidence paths above distinguish their snapshots and scope.
- Original cleanup receipts are in the main notepad. Latest verifier fixtures
  and duplicate-daemon fixture processes/directories were independently checked
  and removed. No tmux or persistent listener was created by those latest checks.
  These receipts do not certify unrelated sessions' processes or directories.
- Original live daemon PID: 36170. Later application daemon: 1010. Current
  direct process observations show application daemon 1010 and separate
  worktree daemon 21591. Their stability during the new checks cannot satisfy
  whole-run preservation of 36170.
- Past commit objects were created. No history was rewritten to hide them,
  and no new commit was made by these repairs or prospective proof.
- Cross-platform design and source checks are not Windows/Linux runtime
  certification. New tests use controlled events or state, not fixed sleeps.
- Desktop GUI E2E is not claimed. User-run procedures remain in the main
  repository's `.omo/FERRYX_MANUAL_GUI_QA.md`, using `bun tauri dev` and no
  live-daemon termination. That document describes historical snapshots;
  current repair behavior must be considered when following its older details.

## Stop-condition decision

All actionable items tracked in this continuation are completed; the original
PID and unavailable historical evidence items remain dropped as unmet, not
passed. The current todo tool reports no open tasks. The latest child tasks and
command monitors have completed.

The declared successful stop condition does not hold. Another green run cannot
change the original PID, erase commit creation, or manufacture pre-production
execution history. This audit therefore records an unsuccessful original run,
with working-tree corrections and stronger current evidence preserved, rather
than claiming full acceptance or starting an unrequested replacement run.
