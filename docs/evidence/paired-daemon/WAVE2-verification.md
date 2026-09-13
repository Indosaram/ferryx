# Wave2 aggregate verification - BLOCKED / NOT ACCEPTED

Independent review task: `st_01a09761`. Execution was confined to
`/Users/indo/code/project/orca-lite-wt/herdr-wave2` on macOS arm64.

## Aggregate verdict

The delivered tree is not an implemented A09-A12 composition. The required
aggregate test command exits 101 before compilation because
`machine_terminal_stream` is absent; `machine_events` is also absent from the
available targets. No test assertion executed. This is a missing-target
validation failure, **not behavioral RED**. Validation stopped at this failure;
there was no retry-to-green and no source fix.

All four implementation reports explicitly declare blocked/not implemented or
not jointly verified. Actual router, session service and event code independently
confirm their central limitations. No Wave2 capability, platform or full-plan
approval is warranted. Historical Wave1 acceptance is neither revoked nor
reclaimed as new evidence here.

## Inputs and source integrity

Read the entire approved 833-line plan, root/backend/daemon AGENTS, the authorized
Wave1 handoff boundaries, all A09/A10/A11-joint/A12 reports and the A10 codec
report. Read all three new Wave2 source/test files in full: terminal_wire.rs,
a10_terminal_wire_codec.rs and machine_sessions.rs. Inspected the actual router,
capability handler, terminal admission/output, event subscriber and shared
spawn/metadata/close path. This is a blocker review, not a completed exhaustive
review of all inherited Wave1 source/tests or relay implementation.

The seed is the parent's 32-source uncommitted composition, not HEAD e1a00339.
Current enumeration finds 35 modified/untracked sources versus HEAD: 18 tracked
modifications plus 17 untracked files. The three additional files match the
Wave2 reports above; no A09 production implementation was added. No independent
parent seed hash manifest was supplied in this worktree, so exact cryptographic
comparison to that seed is unavailable; do not treat the HEAD delta manifest as
that comparison. The inherited composition was never restored or rewritten.

Artifacts:

- `WAVE2-verification-commands.log`: full output, exact commands and exits for
  the persisted validation wave, including Cargo's complete target list.
- `WAVE2-verification-source-before.json` and `-source-after.json`: SHA-256
  manifests of 746 source/test/build/config files (including Cargo.lock).
- `WAVE2-verification-source-delta.json`: explicitly HEAD-relative 35-source
  delta, not a fabricated parent-seed manifest.

Before/after manifests are identical. Only WAVE2 verification artifacts were
written, using apply_patch. Dependencies, Cargo.lock and all source were left
unchanged. No commits, deployment, desktop launch or daemon management occurred.

## Precise repair boundaries

These are disjoint deliverables, not four independent acceptance gates. Shared
server.rs edits require composition; dependency ordering still applies.

### D1 - A09: missing machine session authority (blocking)

`remote/server.rs:2150` mounts POST sessions on
`machine_service_unavailable`; lines 2015 onward return 503
MACHINE_SERVICE_UNAVAILABLE for machine-Control. There is no detail/close route.
`daemon/session_service.rs:53` metadata lacks explicit machine ownership;
the existing spawn path calls spawn_in_worktree before recording its in-memory
request/metadata entries, and lifecycle cleanup removes metadata on exit.

Repair ownership: session service, durable journal integration, session HTTP
adapter and machine_sessions fixture. Implement preallocated raw-ID durable
intent through the existing validated spawn path, machine ownership, exact
owner epochs, retained exited/expired outcomes, typed shell/provider admission,
64-session admission and guarded journaled close. Validate queued revocation,
cwd/inheritance constraints, device-scoped retry/conflict, lost replies and
crash-unknown reconciliation. Do not duplicate spawn or use Local/SSH fallback.
The current test only expects creation status 201 in one root; it does not prove
PID/CWD/epoch, two roots, retry, guarded close or full bounded teardown.

### D2 - A10: absent machine attachment/controller branch (blocking)

`remote/server.rs:1113` still normalizes host-scoped IDs, requires active desktop
selection, and attaches with cursor None. Existing output sends no unconditional
machine attached boundary. There is no operational machine epoch/generation
admission or independent controller authority. The new codec is not registered
in remote/mod.rs and cannot establish those contracts by itself.

Repair ownership: machine socket branch, controller authority and stream tests;
consume D1 ownership/close rather than implementing a second session service.
Prove exact raw target/owner epoch, other-device conflict, same-device fencing,
15-second reservation, focus isolation, empty-history boundary, replay/gaps,
fresh tickets/revocation, View resize regression, bounded input/control/output
and write deadlines. Keep codec GREEN separate from live socket proof.

### D3 - A12: absent independent machine event projection (blocking)

`remote/server.rs:1047-1111` subscribes all grants to the same event_tx and sends
an active-selection snapshot, not a revisioned machine inventory. Lag terminates
the while-let receive loop without explicit inventoryInvalidated reconciliation.
`remote/state.rs` has the existing legacy broadcaster, not machine inventory
authority. This is source evidence of missing behavior, not runtime evidence
that a machine path actually leaked.

Repair ownership: independent events/boundary model and publication hooks in
committed project/worktree/session mutations. Consume D1/D2 lifecycle; prove
subscribe-before-snapshot mutation, bounded-buffer lag, revisions/owner epochs,
metadata/availability/Git invalidation, mirror redaction and reconnect without
Create. Add the real machine_events fixture, not prose/source-string tests.

### D4 - A11 joint integration proof missing (acceptance blocker)

Retain the inherited relay allowlist/data-channel implementation. The report
correctly declines to call synthetic responders actual A09/A10 compatibility.
After D1/D2, exercise real owner HTTP plus WS through relay data channels, not
direct service spawn: two registered roots, all required methods/DELETE bodies,
encoded queries, epoch/cursor forwarding, forbidden routes, cross-host/replayed
tickets and safe close. No independent relay-code defect is asserted from this
limited review. This repair owns transport integration evidence, not D1-D3 logic.

### E1 - build capacity and seed provenance (independent prerequisite)

Current df: 133 MiB available, volume 100%; private target: 6.2 GiB.
Prior A09/codec reports record linker ENOSPC, not behavioral failures. Provide
capacity through parent-authorized cleanup without deleting inherited artifacts
in this task, and provide the original 32-source hash manifest for exact seed
comparison. No known linker failure was retried here.

## Owed aggregate runtime coverage

| Contract | Evidence in this verification |
| --- | --- |
| Two owner roots, original PID/CWD/shell and owner epoch | Not run; creation unavailable |
| CRUD, durable intent, same-target retry, lost reply, crash unknown | Not run; D1 |
| Safe close, termination known, controller conflict and no detach kill | Not run; D1/D2 |
| Input/resize/interrupt, fencing, replay/gap, empty attach, focus isolation | Not run; D2 |
| Auth/revocation/redaction, mirror behavior, epoch and ticket rejection | Not run; inherited protections are not new runtime proof |
| Actual relay HTTP/WS joint sessions, method/body/query preservation | Not run; D4 |
| Snapshot race, lag/resync, lifecycle/provider metadata, Git invalidation | Not run; D3 |
| 64 sessions, 8 mutations, 64 KiB input, 16 KiB controls, 1 MiB output, deadlines | Not exercised on Wave2 paths |
| No Local/SSH fallback or duplicate spawn | Stub fails closed; missing implementation cannot prove future routing |
| Resource teardown on success/failure | No fixture was launched; no task-owned listener, child or PTY exists |

Capabilities currently include only directoryBrowseV1 for eligible shared-service
grants (`server.rs:1997`); terminalCreateV1, terminalStreamV1 and machineEventsV1
are not falsely advertised. Keep them gated until operational aggregate proof.

## Validation ledger and limits

Initial plain git status exited 128 due to the provisioned Ghostty symlink.
Submodule-ignoring status/diff surveys and diff --check exited 0. Full output of
the repeated persisted survey is in the command log. Tool-based LSP diagnostics
on all three added Wave2 files returned no diagnostics before the Cargo command.
No Markdown prose tests were added.

The exact first required Cargo command is in the log: CARGO_BUILD_JOBS=4,
RUSTC_WRAPPER empty, CARGO_TARGET_DIR set to the private src-tauri/target,
--locked, --no-default-features, all five requested integration targets.
It exited 101 on missing machine_terminal_stream. No runtime assertion or
compilation ran. In accordance with stop-on-validation-failure, the remaining
sequential commands were NOT RUN:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test worktree_safety --test daemon_persistence_contract --test daemon_handover_contract --test remote_project_public_contract --test relay_pairing_generation_regression
cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay
```

Those commands would require the same private-target/four-job environment; they
have no exits or passing claims in this session. SSH live credentials were not
used. Native desktop, Linux/Windows execution, deployed relay compatibility,
OS-level direct-path exclusion, logging audit and rollout/rollback remain
external/unexecuted gates. No original runtime PID/CWD/owner epoch is available
because no runtime fixture executed. No canonical daemon/PTYS were contacted,
signaled or changed. All implementation remains uncommitted.
