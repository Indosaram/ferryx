# SSH directory picker code review

Date: 2026-09-08
Reviewer: omo-senpi-gate-reviewer (st_01a07fd8)

recommendation: APPROVE (corrected code delta only)
codeBlockers: []
commitReadiness: Prerequisite scope authorized and committed as c85f17e
validationStatus: Passed; final parent validation receipts are recorded below

This focused re-review supersedes the code rejection in `.omo/evidence/ssh-directory-picker-gate-review.md`. It does not approve a standalone picker commit, authorize staging foreign files, or certify pending validation results. Only this report was written during this re-review.

## originalIntent
Choose/add a saved SSH host, browse existing remote folders, and register the selected canonical folder like Zed. No editor at any product level, SSHFS, or picker-triggered remote writes.

## desiredOutcome
The Add Project and SSH settings entry points share a host-scoped picker. Navigation is separate from registration; pending, failed, edited, and stale selections cannot register. Valid remote paths retain their exact identity on POSIX and Windows.

## userOutcomeReview
The corrected delta resolves the concrete unsupported-name failure and the failed-navigation path-display ambiguity without widening product scope. No remaining reproducible code defect was found in this delta. Full feature context and previous reproduced validations are documented in the prior gate report; this turn intentionally did not repeat broad builds or live endpoint checks.

## Findings resolved

### C-PATH: unsupported byte names no longer hide valid siblings
Evidence: `src-tauri/src/ssh/browse.rs:189-236`.

The parser now separates raw NUL-delimited fields before UTF-8 decoding. Frame shape, sentinel, truncation flag, and entry hidden flags remain strict. Root and parent paths require valid UTF-8 and platform validation. Only undecodable entry names are omitted with `truncated=true`; no lossy replacement creates a different path. Hidden flags are validated before deciding to skip a name, so malformed metadata cannot hide behind an unsupported name.

The portable `skips_non_utf8_entry_names_but_rejects_invalid_metadata` regression asserts that a valid sibling survives, its exact path is preserved, omission is signaled, and invalid root/hidden metadata fails. The filesystem test `preserves_access_to_valid_folders_beside_non_utf8_names` is now Linux-only, avoiding the macOS filesystem's rejection of invalid-byte names during fixture setup. That platform restriction fixes an incompatible fixture rather than concealing a production failure; the portable parser regression remains enabled on macOS.

### C-NAV: failed child navigation displays the attempted path
Evidence: `ui/src/components/RemoteDirectoryPicker.tsx:27-49`; `ui/src/components/ProjectDialogs.test.tsx`, `shows the attempted child path when navigation fails and retries that path`.

Navigation updates the field immediately to the requested path (or `~` for Home), invalidates selection, and retains canonical response replacement on success. Existing generation checks still prevent stale success/error replies from changing the current selection. The failed-child test checks the attempted path, disabled registration, and Retry's exact requested path. The previous display ambiguity is resolved.

### C-HOST / C-VERIFY: stronger regression coverage
Evidence: `ui/src/components/ProjectDialogs.test.tsx`, `invalidates cached and pending selections when the same host ID changes configuration` and `filters hidden folders locally and refreshes cached directory data`; `src-tauri/tests/ssh_browse_live.rs:70-74`.

- Same-ID hostname replacement now has a deterministic deferred-response test: the old cached home cannot satisfy the new mount, registration is disabled while the replacement loads, and the old pending response cannot overwrite the new canonical selection.
- Refresh now returns different folder contents and asserts that the new folder appears while old entries disappear, rather than relying on call counts alone.
- Live parent navigation now compares the returned canonical path with the requested parent instead of accepting any nonempty path.

## remove-ai-slops / programming perspective
The prior review could not locate the named skill files in the available skill/config directories. Their documented prompt criteria were applied directly again to this delta.

- No excessive or useless new tests identified: each added assertion addresses a previously demonstrated failure or a specific coverage gap.
- No deletion-only, requested-removal-only, tautological, source-text, or implementation-mirroring tests added. Assertions consume real parser fields, rendered state, or actual IPC arguments.
- No sleeps, polling delays, or timing-luck dependencies added. Deferred promises explicitly control response order.
- The portable byte-frame regression and Linux filesystem regression test distinct boundaries: parser resilience and actual script output. Their overlap is justified.
- No unnecessary production extraction, generic parsing framework, normalization, or speculative defensive layer added. Byte-level framing is necessary at the untrusted transport boundary, and the UI change is a single state update in the existing navigation path.
- No new maintenance-burden, false-confidence, or scope-drift finding in the corrected delta.

This report explicitly supplies the skill-perspective code review; no independent second current review report was provided or assumed.

## Commit blocker

### C-COMMIT: prerequisite ownership/authorization remains unresolved
- violatedCriterion: C-COMMIT (parent's explicit requirement not to stage foreign dirty prerequisite files without coordination/authorization).
- evidencePointer: `src-tauri/src/ssh/browse.rs` imports `runtime`; `src-tauri/src/ipc/project_remote.rs` uses the platform probe and `RemoteProject.platform`; mixed platform/picker diffs in `src-tauri/src/ssh/mod.rs`, `src-tauri/src/lib.rs`, and `ui/src/components/settings/SshSection.tsx`; prior gate report, Commit dependencies section.
- Observation: the picker is not a self-contained change against HEAD. Runtime detection relies on foreign transport behavior, and Windows registration/resolution relies on foreign platform prerequisites. Some shared files contain both features' hunks.
- Required resolution: coordinate the prerequisite commit with its owner or obtain explicit authorization for a clearly identified dependency commit. Do not treat code approval as permission to stage whole shared files, foreign platform changes, or unrelated release scripts.

This is an explicit blocker to committing, not a remaining picker-code defect. No isolated staged-tree build or commit was performed.

## Verification and exact evidence gaps
Directly performed during this focused re-review:
- Read the corrected parser and both new UTF-8 regressions.
- Read the navigation change and failed-child, same-ID configuration, and refresh regressions.
- Read the strengthened live parent assertion.
- Ran `git diff --check`: passed.

Pending, not claimed as passing for the corrected tree:
- Parent's final UI suite and UI build.
- Parent's final Rust SSH suite and cargo check.
- Parent's final Windows and Linux live SSH test receipts.
- Linux-only raw-byte filesystem regression execution: it cannot execute on the current macOS workstation merely by running the ordinary suite or connecting the macOS live test to a Linux endpoint.

Parent-reported RED-before-fix results were not independently rerun during this focused re-review. Earlier reviewer-reproduced 76 UI tests, 57 SSH tests, build, and both live endpoint passes precede this corrected delta and are not substituted for its pending final receipts. Physical-keyboard/desktop and real UNC checks remain unclaimed as before.

## Checked artifact paths
- `src-tauri/src/ssh/browse.rs` (corrected parser and new tests)
- `ui/src/components/RemoteDirectoryPicker.tsx` (navigation delta)
- `ui/src/components/ProjectDialogs.test.tsx` (corrected/new regressions)
- `src-tauri/tests/ssh_browse_live.rs` (parent equality delta)
- Prior baseline review: `.omo/evidence/ssh-directory-picker-gate-review.md`

No implementation files or foreign prerequisite files were edited.

## Final parent validation receipts

Recorded after the independent code approval:

- UI: 78 tests passed across the five related test files in one final invocation;
  `bun run build` passed. Combined monitor exit code: 0.
- Rust: 58 SSH library tests passed; `cargo check` passed.
- Live Windows `maho-win`: browse/home/child/exact-parent/registration test passed
  in 12.30 seconds.
- Live Linux `omarchy`: the same test passed in 6.30 seconds. The Rust tests, check,
  and both live invocations completed as one command chain with exit code 0.
- `git diff --check` passed.
- The portable non-UTF8 byte-frame test was observed failing on the original parser
  before the correction. The initial raw-name filesystem fixture failed during
  macOS filename creation, so that case is now Linux-only; it was not executed in
  these macOS runs. The portable parser regression passes on macOS.
- The failed-child-path UI regression was observed failing before the correction.
- A fresh headless browser rendered the actual RemoteDirectoryPicker using fixture
  responses at the IPC boundary. Clicking the denied child produced
  `pathInput=/home/test/denied`, the identical request path, and `selection=null`.
  At 390px width, document scroll width was also 390px. Screenshot:
  `docs/evidence/ssh-picker-reviewed-error.png`. The browser and isolated server
  were closed after verification.

The original desktop/physical-keyboard and LSP limitations still apply.
At that review checkpoint no prerequisite had been staged. The user subsequently
authorized reviewing and committing the necessary SSH platform changes. Those
changes received an independent APPROVE, passed isolated-tree and live tests, and
were committed separately as `c85f17e` (`feat(ssh): support Windows and POSIX remote
runtimes`). C-COMMIT is resolved. See `docs/SSH_PLATFORM_PREREQUISITE_CODE_REVIEW.md`.

## Final isolated picker commit verification

The second commit candidate was exported from a private index onto the verified
`c85f17e` foundation, excluding all unrelated working-tree changes.

- UI: 100 tests passed across seven related files; UI build passed (exit 0).
- Rust: 60 SSH library tests and 4 remote-project registration tests passed;
  `cargo check` passed.
- Actual Windows `maho-win` browse/home/child/exact-parent/registration test passed
  in 8.14 seconds.
- Actual Linux `omarchy` equivalent test passed in 1.04 seconds.
- The Rust/check/live command chain exited 0. These checks used the isolated tree,
  not the shared tree containing unrelated release and agent-exit changes.

The earlier receipt counts above describe prior review checkpoints. No additional
production code change was made after this final verification.
