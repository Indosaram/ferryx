# A10 implementation blocked at prerequisite inspection

Status: NOT IMPLEMENTED. No terminalStreamV1 or A10 acceptance approval.

## Scope and source delta versus seed

Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave2`.
Read the full approved 833-line plan, applicable root/backend/daemon AGENTS,
wave1 handoff boundaries, A09 implementation evidence, inherited A10 codec
evidence and codec source, actual terminal admission/stream code, session
metadata, and router. Preserved the supplied uncommitted composition rather
than restoring HEAD. This task adds only this evidence file. No production,
test, dependency, lockfile, other-worktree or historical evidence changes.

## Observed blockers

1. A09 is not implemented. Its evidence explicitly records a linker failure
   before behavioral RED and no production implementation. Actual source agrees:
   POST `/api/v1/sessions` routes to `machine_service_unavailable`; machine-Control
   receives 503 `MACHINE_SERVICE_UNAVAILABLE`. No session detail/close route is
   registered. `StoredSessionMeta` has workspace/cwd/provider/fingerprint fields
   but no authoritative machine ownership. Consequently there is no A09 guarded
   close authority to connect to A10, and no machine-owned created session on
   which to exercise the required socket runtime contract. Adding A09 CRUD and
   durable spawn is outside the assigned minimal session-metadata write scope.
2. Disk capacity remains exhausted. `df -h .` reports 125 MiB available, 100%
   capacity; `du -sh src-tauri/target` reports 6.2 GiB of inherited artifacts.
   Both prior A09 and codec validation attempts failed linking with errno 28.
   No inherited artifacts were deleted and no blind linker retry was made.

The inspected raw legacy resize branch already checks `can_control`; this task
does not claim to have fixed it or to have executed its socket regression.
The inherited codec remains unregistered in `remote/mod.rs` and unverified by
this task. It is not operational terminal stream capability evidence.

## Commands and exits

All commands ran in the assigned worktree unless a read used the explicit
approved plan/handoff path. Tool reads succeeded.

- Initial `git --no-pager status --short`: exit 128, provisioned Ghostty
  submodule is a symbolic link; the chained initial diff did not run.
- `git --no-pager -c diff.ignoreSubmodules=all status --short` and
  `git --no-pager diff --ignore-submodules=all --stat`: exit 0. Baseline:
  18 tracked modified files, 1599 insertions, 127 deletions, plus inherited
  untracked sources, evidence and dependency links.
- Applicable AGENTS path discovery: exit 0.
- `df -h .` and `du -sh src-tauri/target`: exit 0, values above.
- Targeted `rg -n` source inspection of remote server/state/protocol/module,
  session service and workspace APIs: exit 0; inspection only, not behavior.
- `git --no-pager diff --ignore-submodules=all -- src-tauri/src/daemon/session_service.rs src-tauri/src/remote/mod.rs`:
  exit 0; inherited session delta only adds workspace mutation gating.
- Evidence existence check and `command -v apply_patch`: exit 0.

No Cargo validator was run in this task: the known capacity blocker was
confirmed, not retried. No actual RED, GREEN, security socket execution,
headless check or LSP result is claimed. In particular no build failure is
presented as a failing behavioral assertion. The required combined machine
terminal/session command remains unexecuted, and machine_terminal_stream.rs
was not added merely to fabricate a dependency failure.

## Runtime proof, cleanup and limits

No runtime fixture, listener, child shell or PTY was launched. Original fixture
PID/CWD/owner epoch therefore does not exist for this task. No canonical daemon
or PTY was contacted or signaled. No runtime resources need teardown. There
is no proof of simultaneous streams, replay, controller fencing/reservation,
revocation, epoch expiry, explicit close or process survival. All remain open.
No commit, deployment or desktop launch occurred.

Parent coordination is required to provide operational A09 session ownership
and close contracts and sufficient build disk capacity (or explicit authority
to remove inherited build artifacts). This is a blocked handoff, not partial
platform, runtime, or full-plan approval.
