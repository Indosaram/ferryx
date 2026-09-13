# Linux readiness candidate parent composition

Status: candidate composed; final composed-runtime verification pending.

The parent inspected the exact raw child stdout capture, original failure
description, six-emission patch, actual result JSON, GREEN log summaries and
cleanup receipt. With `RUST_TEST_THREADS=1`, each captured first marker follows
libtest's unterminated `test ...` prefix. Prepending a newline repairs framing
without relaxing strict parent parsers or crash/restart assertions.

The private Linux candidate executed remote tests (254 passed), catalog tests
(4 passed) and headless build, all exit 0. These are private frozen-baseline
results, not a rerun of the newer composed implementation.

The parent applied the six marker changes and three explanatory comments to
the current resumed files using apply_patch, preserving existing changes.
No timeout, parser, assertion, production journal or process-cleanup logic
changed. All three files have no LSP error diagnostics. The current whole-tree
`git -c diff.ignoreSubmodules=all diff --check` exited 0.

## Source identity

- `src-tauri/tests/machine_catalog_persistence.rs`:
  `4d492f1010c3bc3b68b959fab94eb4a2cfece4be18fdb8359e570ed2a7ed9322`;
  exact match to Linux candidate.
- `src-tauri/src/remote/workspace_api_tests.rs`:
  `0c30c7da1b1460bdbe555f3442c0919ca9e0361926a71ac4d1ee1b4548a84857`;
  exact match to Linux candidate.
- `src-tauri/src/remote/workspace_api/worktree_authority_tests.rs`:
  `7b59dd041008806465e776521df9916e29bad2bba3ec2d403bc3feb9f10f2175`;
  differs from candidate because the resumed tree already contains the
  independently recorded rich prunable-preview assertions. Parent diff
  inspection confirms that preserved block plus the two marker changes.
  The file was not replaced with the older frozen copy.

The cleanup log reports no owned live processes and explicit private-runtime
root removal. Its literal LOGGED_PIDS_ABSENT list contains 29 IDs; the child
summary's claim of 30 is not used as a verified count here.

Combined Wave2 verification must execute the affected remote and catalog
suites with these merged inputs once the active producer writes settle.
No whole-platform or full-plan acceptance is inferred. Changes are uncommitted.
