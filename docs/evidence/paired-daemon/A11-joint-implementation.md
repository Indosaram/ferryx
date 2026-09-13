# A11 joint compatibility: blocked on actual A09/A10 runtime

Status: NOT VERIFIED. This is a blocked dependency handoff, not A11 joint
acceptance, behavioral RED/GREEN, or platform approval.

## Scope and delta versus supplied seed

Task `st_01a09760` inspected the supplied uncommitted composition in
`/Users/indo/code/project/orca-lite-wt/herdr-wave2`, not just HEAD. The full
833-line approved plan, applicable root/backend AGENTS and Wave 1 handoff
boundaries were read. Existing A11 route/query allowlists and transport tests
were retained unchanged. The only task-authored delta is this evidence file.
No production or test sources, dependency links, Cargo.lock, historical Wave 1
evidence, other worktrees, or A12-owned gateway/services were edited.

## Concrete dependency blockers

The actual router in `src-tauri/src/remote/server.rs` still mounts
`POST /api/v1/sessions` on `machine_service_unavailable` (line 2150 at
inspection). That handler returns 503 `MACHINE_SERVICE_UNAVAILABLE` for
machine-Control grants. There is no session detail/close route. Capabilities
advertise only `directoryBrowseV1` when machine services and authorization are
present, not terminal creation or machine streaming.

`A09-implementation.md` explicitly records no production implementation and a
linker ENOSPC failure before its creation assertion executed.
`A10-implementation.md` explicitly records NOT IMPLEMENTED: no A09 ownership
or guarded close authority and no operational `terminalStreamV1`. Inspection
of the current terminal handler confirms the legacy stream/focus watcher,
not the required machine attached/generation/epoch/cursor branch. A codec
file alone does not supply an operational stream.

These are gateway/session-service dependencies outside the relay-owned write
set. A synthetic exchange responder, legacy PTY stream, or direct service
spawn would not satisfy the requested actual A09/A10 joint proof. No such
substitution was made. Parent coordination must provide operational A09/A10
before the relay compatibility fixture can assert their real contract.

Disk capacity is also still exhausted: `df -h .` reported 123 MiB available,
100% capacity; `du -sh src-tauri/target` reported 6.2 GiB. Inherited target
artifacts were preserved. The known ENOSPC validator failure was not blindly
retried and no alternate target was populated on the same full volume.

## Commands and exits

All execution was confined to the assigned worktree. Explicit external paths
were read only for the authorized approved plan and handoff document.

- `pwd && git --no-pager status --short && git --no-pager diff --stat && find ...`:
  exit 128 at status because provisioned Ghostty is a symbolic link; subsequent
  chained commands did not execute.
- `git --no-pager -c diff.ignoreSubmodules=all status --short --ignore-submodules=all`
  and `git --no-pager -c diff.ignoreSubmodules=all diff --stat`: exit 0.
  Survey showed 18 inherited tracked modified files, 1599 insertions and 127
  deletions, plus supplied untracked implementation/evidence/dependency links.
- Applicable AGENTS discovery and read tools: successful. Full plan was read
  in two ranges because the first read reached the 50 KiB output limit.
- `git --no-pager -c diff.ignoreSubmodules=all diff -- src-tauri/src/remote/relay_server.rs`:
  exit 0; inherited A11 admission, forwarding and regression deltas preserved.
- Targeted `rg -n` inspection of relay server/client, gateway, session fixture,
  workspace tests and A09/A10 evidence: exit 0. These are source inspection,
  not runtime behavior tests.
- `df -h .`, `du -sh src-tauri/target`, `command -v apply_patch`, repeated safe
  status survey, and `git --no-pager diff --ignore-submodules=all --check`:
  exit 0.

No Cargo test/check/build or LSP validation was run: no source was changed,
the prerequisite implementation is absent, and known linker capacity failure
remains unresolved. In particular the required relay regression and new joint
fixture commands are unexecuted, not passing. No production repair was made
without the mandatory actual behavioral RED.

## Actual runtime proof and cleanup

None in this task. No listener, reverse tunnel, fixture daemon, shell, or PTY
was launched; no original PID/PWD/owner epoch exists to report. No canonical
daemon or PTY was contacted or signaled. There are no task-owned runtime
resources requiring teardown. No commit, deployment, or desktop launch occurred.

Actual session create/detail/close, lost-reply reconciliation, stream replay,
malformed query handling through the joint runtime, stale/replayed/cross-machine
tickets, and epoch/cursor preservation remain unverified here. Existing
parent-verified allowlists were not relabeled as new joint acceptance.
Client relay-only routing has not been proved against A09/A10; OS firewall
exclusion, deployed relay compatibility, native desktop, Linux/Windows runtime,
and full-plan acceptance remain separate external gates.
