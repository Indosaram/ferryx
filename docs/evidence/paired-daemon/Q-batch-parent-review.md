# Parent Q1/Q2/Q3 combined verification

## Scope

This batch checks the resumed inspection/rate proofs, non-HEAD/prune wire
proofs, rich prunable preview and their related worktree regressions. It is not
whole A06/A08, native/platform, or Wave2 acceptance. A09 remains independently
owned and in progress.

Exact first-run commands, monitor identifiers and seven source hashes are in
`Q-batch-parent-before.json`. Four `Q-batch-parent-*-attempt1.log` files preserve
the monitor output, including any display-level truncation.

## Initial results and diagnosis

- Filesystem: 12 passed, 1 failed, exit 101. The inspection-count and rate
  boundary tests passed. `a06_directory_http_home` failed on the old exact
  capability array: A09 now advertises `terminalCreateV1` alongside
  `directoryBrowseV1`. The parent reviewed the actual machine-Control,
  catalog-ready and session-journal-ready gating and changed only the exact
  expected array. Strict equality and all later revocation checks remain.
  Diagnostics have no errors or warnings; the Linux-only inactive-code hint
  remains expected on Darwin.
- Worktree authority/wire: 13 passed, exit 0, with the nested private-owner
  selector also passing. The count includes child-entry tests; it is not 14
  independent behaviors. Actual first non-HEAD HTTP 201, checkout commit,
  Local UDS prune failure and redacted legacy HTTP 409 were observed. Owner
  PID 15843 was reaped, listeners joined/refused, and fixture root removed.
- HTTP/safety group: rich preview passed, then machine worktrees had 1 pass and
  1 failure, exit 101. Cargo stopped before the safety target. The plain-folder
  scenario unexpectedly created a worktree with HTTP 201 instead of rejecting
  it with 422.
- Worktree manager: 42 passed, 2 failed, exit 101. Both failures treated separate
  plain folders as the enclosing resumed Git repository.

The three worktree failures were a parent harness mistake, not a demonstrated
production defect: TMPDIR was beneath this Git worktree's target directory.
Native Git discovery correctly resolved plain fixture folders to the enclosing
repository. The corrected runs retain isolated HOME/runtime/data/session/XDG
paths but place the private root under `/tmp`, outside every Git checkout.
No production Git code or worktree assertion was changed.

## Bounded cleanup of the accidental worktree

The initial HTTP fixture created exactly:

`/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8/.orca-worktrees/project-f76664532ee84a8aba7ceb5f5014a9fc/no-partial`

Its branch was `orca/project-f76664532ee84a8aba7ceb5f5014a9fc/no-partial`,
pointing to the existing `e1a00339` commit. The parent verified its worktree was
clean, removed that exact worktree with normal `git worktree remove`, then
deleted only its merged branch with `git branch -d`. Both exited 0. No force,
reset, restore, commit, or unrelated reference change was used.

The empty project parent directory was removed. Subsequent independent checks
confirmed the path absent, no worktree registration, and no branch reference.
All four first-run private roots were removed by their command traps. This
cleanup does not imply the first run passed.

## Corrected runs

Only the three failed groups were restarted; the passing authority/wire group
was not repeated. Monitors:

- Filesystem: `mon_GNRBT082343ATJKR`, `bash_7`.
- HTTP/safety: `mon_ZRQ0G7JGSJF2FTTY`, `bash_8`.
- Manager: `mon_Y24VQARXN22H7F1Q`, `bash_9`.

All three corrected commands exited 0:

- Filesystem: 13 passed, including the exact 9,999/10,000/10,001 inspection
  boundary, burst/refill/saturation, native HTTP and revocation/worker cleanup.
- HTTP/safety: live rich preview 1 passed, machine worktrees 2 passed, safety
  9 passed. Missing and locked-missing checkout previews retain null dirty
  inspection, actual branch metadata and live session IDs. Both owned preview
  sessions were explicitly closed and absent before listener/root cleanup.
- Worktree manager: 44 passed, including both formerly contaminated
  plain-folder tests and descendant containment scenarios.

All command roots were removed. Three `Q-batch-parent-*-GREEN.log` files retain
the actual output; `Q-batch-parent-after.json` records exact corrected commands
and source hashes. The seven scoped sources were unchanged during the corrected
runs. Relative to the first snapshot, only the parent's single capability
expectation changed.

Together with the passing authority/wire command and inspected RED/source
evidence, this closes Q1's budget proofs, Q3's rich preview, and the named
Darwin backend A08 followthrough gaps, including Q2's three real wire seams.
It does not promote in-process reload tests to original-process crash proofs,
promise automatic parent-death containment, or establish exceptional OS-failure,
Linux/Windows, native Tauri, external events or forced-relay exclusion.
Those boundaries remain documented in WAVE1-resume-acceptance-gaps.md and the
full packet/acceptance ledger. A09's separate source changes are outside this
scoped identity receipt; the final Wave2 gate needs its own composed snapshot.
