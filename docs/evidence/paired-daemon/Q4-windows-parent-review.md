# Q4 Windows candidate parent review: composition pending

Parent read the complete fourteen-file candidate diff, reviewed the scope report,
and found two false-positive risks: EOF could satisfy the browser close loop,
and ConPTY cursor-query recognition depended on one PTY chunk. The candidate
worker corrected only those tests, preserving production bytes and prior logs.
The revised patch is Q4-windows-review-candidate.patch.

Parent read the retained review-run.ps1 and review-cleanup.ps1 and the changed
ConPTY fixture. It independently executed both review commands natively on
maho-win using the same isolated source/toolchain environment:

- Q4-windows-parent-review-relay.log: one passed, received Close observed=true.
- Q4-windows-parent-review-safety.log: nine passed, actual ConPTY query answered.
- Q4-windows-parent-review-exits.log: each current command exit0 and waited.
  Previous run entries remain in this appended receipt.

SSH exit0 alone is not the gate: review-run.ps1 overwrites its code variable and
does not propagate failure. Parent inspected actual logs and individual exits.
Initial parent invocations had PowerShell quoting/JS escape errors; those did
not run tests. Literal-path UTF16LE EncodedCommand executed correctly.

Before cleanup parent inspected Win32_Process for any executable or command
containing the exact owned root, without the script's broad name exclusions;
no matching process rows were returned. Then reviewed cleanup ran successfully.
Q4-windows-parent-review-cleanup.log records970 matching inputs, zero owned
runtime processes, and removal of all nine private runtime directories.
Source/build staging remains retained. Earlier review logs were copied to
parent-review-prior before rerun.

This is two corrected-test native verification, not parent verification of all
fourteen candidate files, the full172 remote/40 worktree suite, all integration
targets, or build. The candidate is NOT composed into the resumed worktree.
Next: inspect every final changed source artifact, match hashes against the
candidate, reconcile existing Linux marker changes and current producer edits,
compose only missing scoped deltas, then aggregate platform verification.

Dedicated adversarial Job, ACL and hidden-attribute evidence remains open.
No desktop, clipboard, canonical daemon, production host or release was touched.
