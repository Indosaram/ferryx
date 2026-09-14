# Parent review of missing-checkout preview

The parent inspected the implementation and actual child logs after
st_01a0980b completed. Source review is complete; independent runtime acceptance
is pending the combined batch. No separate per-node verifier was started.

## Scope checked

The three prior source files in herdr-wave2 still match their frozen seed
hashes. Comparing those files with the continuation tree showed only:

- worktrees.rs: the preview read path and removal of its unsupported response.
  The additional wire_proof_tests module declaration belongs to st_01a09810.
  Mutation implementations are unchanged.
- manager.rs: shared branch-ref preview extraction, including an explicit
  refs/heads commit lookup. Existing checkout validation remains at its caller.
- worktree_authority_tests.rs: only the old prunable assertion now expects
  truthful unavailable metadata. Catalog/journal fault and replay assertions
  remain intact.
- machine_prunable_preview.rs: the complete new 117-line HTTP fixture was read.

The parent inspected the path-component walk, listed identity matching,
unavailable dirty fields, actual branch-ref inspection, live-session lookup,
normal preview path and unchanged DELETE path. Missing dirty data is not
represented as clean. Symlinks are refused rather than followed. No additional
source defect was established by this scoped review.

The explicit constructor config/auth paths also isolate catalog, journal,
remote-session and SSH-store paths. The fixture does not run the daemon server
or bind its canonical socket. It launches only its loopback gateway and two
owned shell sessions, with readiness subscribed through the spawn output
receiver. Its main assertion block is unwind-caught before session/listener/root
cleanup.

## Actual child artifacts read

- A08-prunable-resume-RED.log: actual HTTP422 versus expected200, failed
  assertion and cleanup; not a compilation failure.
- A08-prunable-resume-live-GREEN.log: final expanded fixture, 1 passed.
  Both missing and locked-missing targets return unavailable dirty fields and
  unmerged branch metadata; actual live session IDs are asserted. Missing
  DELETE returns404; six symlink preview cases return400; removed refs return
  explicit unavailable branch metadata.
- A08-prunable-resume-GREEN.log: initial preview1, machine worktrees2,
  worktree safety9 passed.
- A08-prunable-resume-authority-GREEN.log: authority10 passed.
- A08-prunable-resume-manager-GREEN.log: worktree44 passed.
- A08-prunable-resume-build.log: headless build finished successfully.
- A08-prunable-resume-cleanup.log: both actual session IDs closed/absent,
  listener joined/refused, owner dropped and private root removed.

These are inspected child receipts, not commands independently rerun by the
parent. Final composed runtime verification must include the expanded preview,
existing machine-worktree/safety/authority/worktree targets and headless build.
Do not run a full remote aggregate during the A06 mutation-test window.

## Source identity at review

- worktrees.rs: 702488a1da896b284c050952980650bd1aaff85e30678c159ea44d36f6aa08d4
- manager.rs: 838bb13869c8bce7e704bbecbad224dd720452c3b9d4f53121af4d58970bea52
- worktree_authority_tests.rs: 7231b3ce6150caf0689a827ec3c2922cd95f757e367f217d9ce6edc699308dbb
- machine_prunable_preview.rs: 0dab956b0304cdff8827ae9a2ebabe548741f41d02fdd5775975122c3bc7dbea

## Remaining acceptance

The later A14 client must parse null dirty fields as unavailable, not zero;
branchDeletion may also be null with branchDeletionError. Native deletion
controls must not infer eligibility from HTTP200. Linux/Windows, actual Tauri
and native UI, events/capabilities and final combined verification remain open.
This report does not close whole A08 or the full thread goal.
