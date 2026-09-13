# Lead review of the first audit synthesis

Date: 2026-09-13. Verdict: qualified source-review deliverables accepted;
comprehensive coverage and product acceptance remain incomplete.

## Reconstructed scope

The `audit-synthesis` node in
`dag_948b2c08-1b90-42dc-abca-48c6b0136c78` owed exactly three reports:
`coverage.md`, `findings.md`, and `repair-packets.md`. It had to read eight
producer reports, reopen retained mechanisms and PR diffs, reject unsupported
claims, expose missing coverage, and map each confirmed defect to one
file-scoped repair packet. Source, test, configuration, Git ref and runtime
changes were outside its scope.

## Independent checks

- Read the node's original prompt from the workflow snapshot rather than
  infer obligations from its completion message.
- Read all three report bodies, including their final policy and QA-helper
  additions. Earlier lead reads covered all eight producer reports and both
  PR diffs.
- The final reports contain 122, 110 and 154 newline-delimited lines,
  respectively. All are below the node's stricter 220-line report limit.
  An in-memory scan found no trailing spaces or tabs.
- Counted 52 retained finding rows, including foreign-owner findings and
  three QA-helper rows. Every row references a packet ID present in the
  implementation map. This is a mapping check, not 52 passing regressions.
- Confirmed that the primary input-boundary, engine, visibility and browser
  bridge test files named by the map exist. The frontend package's `test`
  script is `vitest run --maxWorkers=1`; `build` is `tsc && vite build`.
  No test or build was executed in this review.
- Reopened `src-tauri/src/daemon/protocol.rs`: its protocol constant is 3.
  The previously read helper's control and attach requests use version 2.
  The helper's parser-only self-test cannot detect this incompatibility.
- Verified persisted P04 now repairs only duplicate workspace/tab digit
  bindings. It no longer authorizes changing established Ctrl+W close,
  Ctrl+V paste or Ctrl+click link behavior.
- Verified the helper packet does not simply remove a timed-out FIFO entry:
  it requires invalidating the ambiguous connection so a late response
  cannot be assigned to a newer request. Failure/EOF must settle waiters
  and close acquired sockets.
- The observed HEAD remains
  `b7ad45163e6d90f2c6ae4410a821e57f7198f5e0`. The modified tracked-path list
  remains the 30 foreign paths recorded in the follow-up. No source changes
  from this synthesis were identified. This is not a frozen-tree guarantee
  against future concurrent writes.

## Acceptance registration

The loop CLI accepted a strengthening of C002 before implementation:

- Literal frontend input, Rust boundary and helper self-test commands.
- Actual wheel coordinates/modifiers, zero/horizontal input, delta units,
  fractional accumulation, bounded rows, Shift tracking and sibling isolation.
- Real Windows HWND wheel actions over numbered scrollback in both directions.
- Owned TCP transport regressions covering unsolicited responses, expired
  requests, late replies, EOF, attachment failure, and protocol version.
- Identical pre-fix/post-fix assertions, current-source runtime evidence and
  owned-resource cleanup.

The accepted criterion also binds the current repair-packet snapshot by
SHA-256. New findings from the gap audit require registration before edits.
No evidence status was changed to PASS.

## Still open

The synthesis explicitly lacks complete historical-ID and branch-level
coverage. The separate gap audit
`dag_f6568880-26de-4341-848f-d36a13e64f78` owns five missing-scope reports and
their cross-verification. A completed first DAG therefore does not mean the
requested exhaustive audit is complete.

Worktree/branch permission remains pending. Production fixes, same-assertion
RED/GREEN, real Windows debug GUI scenarios, aggregate builds/tests, cleanup,
final gate, PR dispositions and pushed remote SHA are all unproven.
The reports are uncommitted and must be protected from concurrent changes.
