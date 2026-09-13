# A10 parent disposition and repair ownership

Status: NOT READY. Focused GREEN is not complete A10 acceptance.

The parent read the full A10-resume-implementation.md and the joint test
section in A10-resume-final-validation.log. The joint machine session/stream
command exited 0 with real original PID/CWD and cleanup evidence. The final
report nevertheless explicitly identifies unimplemented requirements:

- Cancellable saturated production PTY input.
- Routed legacy machine metadata and actual owner epoch.
- Per-consumer queued output byte bounds and progress-deadline proof.
- Idle ownership invalidation and full close/exit/epoch race coverage.

The earlier parent statement that focused tests were passing remains scoped
to those tests. The report prevents any downstream-ready or full A10 claim.
`terminalStreamV1` remains unadvertised.

## Parallel producer continuation

The existing Wave2 DAG automatically started A12 (`st_01a0985a`) and
A11-joint (`st_01a0985b`) when A10 returned. Both were explicitly steered to
read the NOT READY report, retain these blockers, avoid capability enablement,
and continue only their independent event and relay deliverables.

A10 worker `st_01a0983c` was continued with expanded but disjoint ownership:
`terminal/session.rs`, `terminal/pty.rs`, and new focused terminal-layer tests.
Its single deliverable is a bounded cancellable production PTY write seam
proved with a real saturated owned process and no post-cancellation writes.
It must not edit router, daemon session service, protocol, socket tests or
the current machine stream fixture while A12 owns that wiring.

The routed-owner and socket byte-budget repairs remain tracked separately for
the next coordinated write phase. No source mutation for these gaps has yet
been accepted. The terminal worker must report any additional file dependency
before touching another worker's files.

The aggregate verifier must include the completed repair composition, not run
against a moving source tree or infer success from DAG node status. If it
starts before the PTY repair settles, capture that outstanding producer and
defer its executable aggregate until the source is stable. Full acceptance
requires every listed gap to have actual evidence.

Linux readiness and Windows headless compiler repair tracks remain isolated
in their retained platform snapshots. Their results do not substitute for
final composed-source platform verification.
