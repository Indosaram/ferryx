# V06 cleanup follow-up review

**Approve the cleanup refinement. The prior startup-cleanup finding is resolved; no new blockers found.**

Reviewed only `resize_security_tests.rs`, with SHA-256 confirmed as:
`5d9d773045ef88a1ceeaad6f76730499054e2ef6c68e1e991a47faebabb99986`.

- Pairing now occurs before PTY spawn. After successful spawn, setup and assertions run inside the bounded `catch_unwind` boundary.
- Server, pump-task, and session owners remain outside that boundary, so setup panics reach explicit cleanup.
- Cleanup awaits PTY close and pump completion, shuts down remaining pump tasks on join timeout, and attempts hub/root removal before asserting collected cleanup results.
- Structured receipts report PID/root, close/reap status, registry removal, pump completion, and root removal.
- Resize assertions, ordered barriers, and positive-Control-before-View-denial ordering remain intact.

This resolves the identified **post-successful-spawn setup-panic** gap; it does not claim cleanup guarantees for process aborts or failures internal to `pty.spawn`.

**Verification boundary:** source-only review, no edits or reruns. The current focused run (`mon_CHJ2WP6WYSCHQQNG`) remains pending in the supplied status. Prior focused/headless/default/check results are parent-reported and were not revalidated in this turn.
