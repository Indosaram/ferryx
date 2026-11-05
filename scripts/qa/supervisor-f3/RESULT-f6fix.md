# F6 fix evidence limits

Source review: `../.evidence/supv-f6-review-20261104/review-delta-242d78539.md`
(relative to the worktree root), plus the DSR code in `desktop-lane.mjs` and `session.mjs`.

This change has no live desktop acceptance evidence yet. Unit/contract checks
cannot substitute for live macOS and Windows lane evidence. The integration has
not run tests or builds; runtime behavior remains unverified.

## Known gap: m6

For Windows `.cmd` fixtures, the harness answers an observed ConPTY `ESC[6n`
startup DSR with `ESC[1;1R` (the fixture startup loops in `desktop-lane.mjs` and
`session.mjs`).
Thus F3-D6 proves row focus and typed delivery after harness-assisted startup,
NOT unattended Windows cmd pane startup. Repeated `attach` calls in the startup
loop may leave multiple attach subscriptions open; this remains an unmeasured
potential gap, not a demonstrated leak.

## Remaining limits

- **n1:** OCR uses `-c tessedit_create_tsv=1`; the tesseract setting and any
  alternative tessdata selected by `F3_TERMINAL_TESSDATA` are not recorded in
  evidence.
- **n4:** Obsolete short-path restrictions for the former Windows Run-dialog
  typing remain.
- **n5:** `qaProjectSeed` applies terminal font size **24** through localStorage
  (`ferryx.terminal.settings`). This is a harness-applied QA setting.
