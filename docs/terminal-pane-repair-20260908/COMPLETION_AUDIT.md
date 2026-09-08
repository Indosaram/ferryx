# Terminal repair acceptance audit

This is an incomplete-goal audit, not a completion claim.

## Requirement to artifact mapping

- Parallel root-cause diagnosis: `dag_a1bc6899-f931-4467-bac9-db4f861076d3`
  completed three nodes. `DIAGNOSIS.md` records source-verified findings.
- Separate verification/review phase: `dag_7307b428-d03d-4b28-be09-67c36f5dbfca`
  completed compilation, UI build and review. Original review requested changes.
- C1, error does not cover terminal: `evidence/c1-red.log` failed specifically
  on the opaque backing. `evidence/ui-integrated-green.log` now passes the
  full NativeTerminalPane and lifecycle suites, including retry/exactly-once
  input and uncovered bounds errors. Five suites total: 204 passed, zero failed.
  A real desktop failure/Retry/printf screenshot and action log are missing.
- C2, stale deferred resize: commit `0d19660`, original `evidence/c2-red.log`
  and `evidence/c2-green.log` establish 640-versus-900 RED and native GREEN.
  The expanded regression now queues B640 and C900 during the same DEC2026
  interval and tests both completion orders, including the intermediate width
  when the older waiter finishes first. `evidence/c2-both-pending-mutation-red.log`
  fails with the old replay behavior; `evidence/c2-both-pending-green.log` has
  158 passed, zero failed/ignored after restoring the fix. Assertions cover
  session/host bounds, VT dimensions and final queued PTY size. The injected
  frame target and PTY queue observation are not desktop pixels or a kernel
  PTY acknowledgement. Desktop wide/narrow/divider scenarios remain missing.
- C3, retained macOS terminal behind overlays: implementation landed from the
  concurrent session in `851b763`. See `docs/TERMINAL_OVERLAY_RETENTION_2026-09-09.md`
  for that owner's evidence and native acceptance limitations. The shared
  boolean visibility wrapper still let BrowserPane child views remain visible.
  `evidence/c3-browser-red-corrected.log` establishes three correct-condition
  failures for dialog/search/owner occlusion. The wrapper now returns the
  interaction/yielding state while native terminals consume the full state.
  `evidence/ui-integrated-green.log` passes visibility and BrowserPane suites.
  Modal desktop focus, native child masking and visible terminal pixels remain
  unverified. Initial `c3-browser-red.log` had one malformed fixture; use the
  corrected RED log, not that initial case, for owner occlusion evidence.
  Additional mutation proof for the already-landed terminal retention:
  `evidence/c3-terminal-retention-mutation-red.log` restores old universal
  hiding and fails both macOS dialog/search cases specifically on visible=false
  versus visible=true while interaction remains false. After restoring the
  committed code, `evidence/c3-terminal-retention-green.log` passes all 14
  visibility tests. No production mutation remains in the working tree.
- C4, regression coverage: the 204 UI tests include native lifecycle and input
  recovery; the 158 native tests include synchronized output, detach, focus,
  surface recovery and scroll-up. `evidence/native-check.log` records the
  earlier cargo check exit zero. Production Rust is unchanged since then;
  subsequent native test compilations passed after test-only expansion.
- UI build: **PASS** in `evidence/ui-final-build.log`, exit 0, after the other
  owner reconciled the worktree hook contract. Earlier
  `evidence/ui-integrated-build.log` exits 2 at the concurrent,
  foreign `ui/src/state/inactiveProjectWorktrees.test.tsx:64` (TS2554, five
  arguments supplied to a function accepting two through four). The previous
  TerminalPane missing-export error has been removed by its owner, but this
  was superseded by the successful final build. Foreign work was not edited.
- LSP: daemon socket remains unavailable; no clean LSP verdict is claimed.
- Code review: original report
  `.omo/evidence/terminal-pane-repair-20260908-code-review.md` identified browser
  masking and the narrower resize test. Both deltas now have test evidence.
  Re-review completed as `dag_607dc65c-0d3d-4b73-862e-260be05d2ab9`:
  APPROVE, no blockers in the reviewed delta. `REVIEW.md` records the actual
  verdict and nonblocking coverage limits.
  The prior reviewer task was evicted and could not be revived; this is a
  replacement review of the same delta, not multiple simultaneous reviewers.
- Desktop channel: `osascript -e 'tell application "System Events" to tell
  process "ferryx" to get {name, position, size} of every window'` fails with
  `osascript is not allowed assistive access. (-1728)`. No alternative channel
  has demonstrated a native GUI pass. No release or debug app was launched;
  existing GUI and daemon processes remain untouched.
  The separate Peekaboo permission query also reports both Accessibility and
  Screen Recording denied; see `evidence/desktop-permissions.md`.
- Cleanup: all test/build monitors finished (expected RED failures or GREEN
  exit zero). No persistent app/server/browser/container/socket was launched.
  Test logs and reports are retained evidence, not orphaned QA runtime state.
- Commits: `0d19660` contains this session's original native repair;
  `851b763` is the concurrent owner's overlay retention commit. Browser masking
  is committed in `8311967`. The expanded ordering test is committed in `57980ce`.
  No push was performed.

## Current verdict

Not achieved. Passing component/native tests do not fulfill the required native
desktop scenarios. Desktop permission, actual native interaction evidence,
final cleanup and final evidence handoff remain open.
