# A14 typed client composition

Status: implementation started; no acceptance claimed.

The binding plan is
`/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md`.
All writes remain in the isolated `herdr-resume-01a097f8` worktree.

## Topology and ownership

Two producers run in parallel: native typed HTTP operations and the desktop
TypeScript command adapter. Both require deep routing because their work is
authorization and asynchronous identity logic, not visual styling.
One native IPC integration node consumes both, then one aggregate verifier
checks the combined batch. No intermediate approval node is added.
A15 is not included: its A10/A12 runtime prerequisites are still unresolved.

- Native producer: new `paired_host/client.rs`, `paired_host/projects.rs`,
  their tests, module declarations, and minimal credential capture/adoption
  methods in `paired_host/service.rs`. No relay, terminal, metadata, or UI edits.
- UI producer: new `ui/src/lib/pairedDaemonProject.ts` and matching tests.
  Existing contracts and host store are read-only.
- Integration: paired-operation additions in daemon protocol/client/server,
  `ipc/paired_host.rs`, command registration in `lib.rs`, and private native
  integration fixtures. Preserve metadata and pairing blocks.
- Verifier: reports/logs only; defects return to the implementation owners.

## Shared command shape

Command: `paired_host_operation`, with `{ request }`.
Request: `{ hostId, generation, operation }`. Generation is a canonical u64
decimal string. Operation is a typed `kind` discriminated union, never an
arbitrary method, URL, credential, or raw JSON forwarding API.

Operation kinds are `capabilities`, `directories`, `projects`, `registerProject`,
`unregisterProject`, `worktrees`, `worktreeStatus`, `createWorktree`,
`deleteWorktree`, `sessions`, `session`, `createSession`, `closeSession`,
and `operation`. Reads carry their typed query fields. Mutations carry
`request` matching existing machine request DTOs. The `operation` read carries
`requestId` for journal reconciliation.

Response: `{ hostId, generation, result: { kind, data } }`, with result kind
matching the operation. Empty successful mutations use `data: null`.
Native project responses use canonical desktop IDs, `remoteWorkspaceId`,
`target: { kind: "pairedDaemon", hostId }`, and remote metadata; native Rust
alone creates IDs. Project-list unavailable IDs must use the same desktop
identity domain as its mapped rows.

The UI captures host ID and generation explicitly before invoking; active
selection is not request ownership. It validates response provenance and
current generation before adoption. Native credentials never cross this
command. Preserve structured machine errors; do not expose arbitrary relay
response bodies. Both producers must agree their exact exported Rust/TS
types in their reports before the integration node runs.

## Required evidence

Capture behavioral RED before fixes, not just missing-symbol compilation.
Use real loopback HTTP for wrong-machine response, redirect, body bound,
non-JSON errors, generation cancellation, and mutation reconciliation.
Integration must exercise native daemon IPC through the actual relay and
gateway for equal paths on separate hosts. No direct-path production fallback.
Fixtures may use an explicit test-only loopback origin policy.

Run focused Rust tests, the new UI Vitest target, existing related UI
regressions, UI build and headless CLI/relay check. Record exact commands,
exit codes, nonzero selected test counts, source hashes, owned process/socket
cleanup and any unmet A14 requirement under this evidence directory.
Keep paired proxy capability disabled. No canonical daemon, user PTY,
desktop automation, release, commit, deployment, or destructive git operation.
