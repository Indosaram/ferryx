# Wave2 resume aggregate gate: prerequisites missing

This is an early prerequisite notification, NOT a completed aggregate report,
code-review verdict, behavioral RED, or acceptance. The aggregate gate remains
pending. Historical reports are untouched. No production/test source was edited.

Observed in task st_01a0980c on 2026-09-12 in the resume worktree:

- All four current producer reports are absent: A09-resume-implementation.md,
  A10-resume-implementation.md, A12-resume-implementation.md, and
  A11-joint-resume-implementation.md.
- A09 session_api.rs exists with creation/close/read adapters: this is current
  partial implementation, not the historical missing-A09/ENOSPC condition.
- Current server.rs ws_terminal_handler rejects machine-only sessions with
  MACHINE_ACCESS_REQUIRED and explicitly leaves their stream to A10. It still
  calls attach_with_sequence with None in the mirror branch. This is source
  inspection, not a runtime denial measurement.
- Current ws_events_handler subscribes to event_tx and sends the legacy active
  selection snapshot. machine_events.rs, machine_terminal_stream.rs and the
  machine_events.rs integration test are absent. The required A10/A12 aggregate
  contracts therefore cannot be exercised from the current provided targets.
- RESUME-01a097f8.md explicitly says A09 production implementation is in progress
  and prescribes aggregate verification after all producers.

The full approved plan (all 833 lines), applicable root/backend instructions,
Wave1 handoff, and resume seed were read. WAVE2-resume-prerequisite-source.json
records current hashes against all 57 inherited seed entries; it is NOT an
immutable producer-completion snapshot. No assumption that HEAD is the seed.

Commands/exits observed:

```text
git --no-pager status --short --ignore-submodules=all       0
git --no-pager diff --stat --ignore-submodules=all          0
git --no-pager diff --check --ignore-submodules=all         0
test -e docs/evidence/paired-daemon/WAVE2-resume-verification.md  1
test -e src-tauri/src/remote/machine_events.rs              1
```

No Cargo test/build, runtime listener, daemon, PTY, desktop, or external host was
started by this verifier. There is no runtime PASS or RED and no owned runtime
resource requiring teardown. Full command logs, diagnostics, frozen before/after
source manifests, combined owner/relay HTTP+WS behavior, Local/SSH regressions,
and aggregate verdict remain owed after composition. Linux/Windows/native and
deployed-relay checks are not accepted.

Parent notification: the verifier was invoked before its real dependency
boundary. Keep the aggregate task pending rather than accepting this note as a
completed blocked verification. Finish A09, then A10, then A12/A11 composition;
the missing producers are not independent verifier-discovered repair lanes.
No duplicate implementation or per-node approval is requested.
