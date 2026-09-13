# A12 blocked at prerequisite inspection

Status: NOT IMPLEMENTED. No machineEventsV1 or A12 acceptance approval.

## Source delta versus supplied seed

This task adds only this evidence file. The inherited uncommitted composition
was inspected and preserved, not replaced with git HEAD. No production source,
test, dependency link, lockfile, other worktree, or historical evidence changed.
Neither remote/machine_events.rs nor tests/machine_events.rs was created: a
synthetic broadcaster test would not establish the requested runtime contract.
No commit, deployment, desktop launch, daemon contact or signal occurred.

## Confirmed blockers

- A09 and A10 reports both say NOT IMPLEMENTED. Current source independently
  confirms POST /api/v1/sessions routes to machine_service_unavailable and
  returns 503 MACHINE_SERVICE_UNAVAILABLE for machine-Control. StoredSessionMeta
  has no authoritative machine ownership and cleanup removes metadata on exit.
  The terminal wire module is not registered in remote/mod.rs. Required machine
  session lifecycle, retained exit, ownership and stream contracts therefore
  are not available to consume. Completing those packets is not a necessary
  event-wiring adjustment; it is prerequisite implementation outside this lane.
- df reports only 124 MiB available, 100% capacity. The inherited private target
  occupies 6.2 GiB; target-events does not exist. A09/A10 evidence records prior
  errno=28 linker failures before behavioral RED. No inherited artifacts were
  deleted and no blind compile retry was made. Parent coordination must supply
  build capacity or authorize removal of inherited artifacts.

Current events code uses one legacy event_tx for all grants. It subscribes
before the active-selection snapshot, but has no machine inventory snapshot,
independent projection or explicit lag reconciliation. This is source inspection,
not a behavioral RED or evidence of path disclosure at runtime. Capabilities
currently advertise directoryBrowseV1 only for eligible shared-service grants;
this task leaves that gate unchanged.

## Commands and exits

Working directory for commands:
`/Users/indo/code/project/orca-lite-wt/herdr-wave2`.

- `pwd && git --no-pager status --short && git --no-pager diff --stat && find .. -name AGENTS.md -not -path '*/node_modules/*' -not -path '*/target*/*' -not -path '*/vendor/*'`:
  exit 128 at status because the provisioned Ghostty submodule is a symlink;
  subsequent chained commands did not run.
- `git -c diff.ignoreSubmodules=all --no-pager status --short --ignore-submodules=all`
  and `git --no-pager diff --ignore-submodules=all --stat`: exit 0.
  Initial tracked baseline: 18 modified files, 1599 insertions, 127 deletions.
- Applicable AGENTS discovery with explicit ancestor paths and
  `find src-tauri docs -name AGENTS.md -not -path '*/node_modules/*' -not -path '*/target*/*' -not -path '*/vendor/*'`:
  exit 0. Read full approved plan (833 lines), all applicable AGENTS, wave1
  handoff, A09/A10 reports and cited current sources using file-read tools.
- `df -h .`: exit 0, 124 MiB available. `du -sh src-tauri/target src-tauri/target-events 2>/dev/null`:
  exit 1 because target-events is absent; existing target is 6.2 GiB.
- `rg -n 'api/v1/sessions|machine_service_unavailable|ws_events|events_handler|machineEventsV1|terminalStreamV1' src-tauri/src/remote/server.rs`
  and `rg -n 'StoredSessionMeta|machine|session_meta' src-tauri/src/daemon/session_service.rs`:
  exit 0, source inspection only. `command -v apply_patch`: exit 0.
- Repeated scoped status and
  `git --no-pager diff --ignore-submodules=all -- src-tauri/src/daemon/session_service.rs src-tauri/src/remote/state.rs src-tauri/src/remote/mod.rs`:
  exit 0. `test ! -e docs/evidence/paired-daemon/A12-implementation.md`: exit 0 before creation.

No Cargo test/check or LSP validator was run: production/test source is unchanged
and the known compile-capacity blocker remains. The requested machine_events,
session/event regressions and headless check remain unexecuted, not passing.

## Runtime proof, cleanup and limits

No fixture, listener, child or PTY was launched, so there are no owned runtime
resources requiring teardown and no original PID/CWD/owner epoch to report.
There is no behavioral RED, GREEN, authorized WS snapshot mutation, tiny-buffer
lag, second-client worktree mutation, session metadata/exit, mirror-redaction,
Git watcher, or reconnect process-survival proof. All A12 contracts remain open.
No platform or full-plan approval is implied. Parent must resolve prerequisite
implementation and disk capacity before this lane can establish actual RED and
make production changes under the requested test discipline.
