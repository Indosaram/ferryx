# Main-tree WIP merge resolution

Base: `7c86fd156f686c1b2d43d272a8f4271464d716f2`. WIP: `51cd3e85a41b2931161f5b932a4c384a96000d47`.

## Conflict resolutions

- `src-tauri/Cargo.lock`: Keep main version 2026.1009.1 and main shared configuration; retain complementary additions.
- `src-tauri/Cargo.toml`: Keep main version 2026.1009.1 and main shared configuration; retain complementary additions.
- `src-tauri/src/daemon/client.rs`: Main split deadline, isolated transport, attach and tests; retained resource API and optional GUI-requested session ID. Removed duplicate competing WIP split implementations.
- `src-tauri/src/daemon/handover.rs`: Main runtime handover retained; WIP test-only legacy socket lives beside the fixture canonical socket rather than in the production runtime directory.
- `src-tauri/src/daemon/mod.rs`: Main QA producers plus WIP resource usage module.
- `src-tauri/src/daemon/protocol.rs`: Main durable split protocol plus WIP resources and optional session ID.
- `src-tauri/src/daemon/server.rs`: Main daemon and spawn lifecycle plus resource sampler and requested session ID adapter.
- `src-tauri/src/daemon/session_service.rs`: Main journal-backed admission remains authoritative; minimal requested session ID support layered on existing serialized spawn.
- `src-tauri/src/ipc/debug.rs`: Main bounded/scrubbed sink plus WIP diagnostic event prefixes; sent event assertion adapted to main persistence.
- `src-tauri/src/ipc/error.rs`: Combined distinct structured codes, keeping main definitions for shared codes.
- `src-tauri/src/ipc/file_link_tests.rs`: Main tests retained; Spawn fixture supplies the new optional session_id field.
- `src-tauri/src/ipc/native_terminal.rs`: Exact main native terminal IPC implementation retained.
- `src-tauri/src/ipc/terminal.rs`: Main prepared split path plus optional GUI session ID for ordinary local spawn.
- `src-tauri/src/ipc/tests.rs`: Keep main shared behavior; retain nonconflicting WIP additions and complementary DTO fixture fields.
- `src-tauri/src/native_terminal/platform/macos.rs`: Main compositor ownership and reveal/drop behavior retained; no WIP runtime override remains.
- `src-tauri/src/native_terminal/surface_host.rs`: Keep main shared behavior; retain nonconflicting WIP additions and complementary DTO fixture fields.
- `src-tauri/src/remote/server.rs`: Main initial agent-state emission once; WIP bounded machine grid transport and authoritative desktop epoch retained.
- `src-tauri/src/remote/workspace_api/worktree_authority_tests.rs`: Main authority tests retained; ordinary-spawn fixtures supply optional session_id without changing assertions.
- `src-tauri/src/terminal/pty.rs`: Main spawn and close implementation retained; WIP consumed-reader-handle check reports incomplete reader shutdown instead of returning false success.
- `src-tauri/src/terminal/service.rs`: Main prepared-spawn, suspension and service tests retained; no WIP runtime override remains.
- `src-tauri/src/terminal/shell.rs`: Exact main shell implementation retained.
- `src-tauri/src/worktree/git.rs`: Exact main Git preparation implementation retained.
- `src-tauri/tauri.conf.json`: Keep main version 2026.1009.1 and main shared configuration; retain complementary additions.
- `ui/src/lib/nativeTerminalInputQueue.test.ts`: Combined distinct main and WIP tests with properly separated cleanup scopes.
- `ui/src/lib/nativeTerminalInputQueue.ts`: Main FIFO run identity and independent input/preedit execution lanes plus per-lane WIP slow pending IPC diagnostics.
- `ui/src/lib/nativeTerminalLifecycle.test.ts`: Combined complementary lifecycle assertions.
- `ui/src/lib/switchDebug.ts`: Main bounded release sink plus WIP diagnostic events and exported run ID; removed duplicate sinks.
- `ui/src/lib/tauri.ts`: Main IPC payload and command wrappers retained; account enrollment helpers remain equivalent, with only declaration ordering changed.
- `ui/src/lib/types.ts`: Main shared DTO definitions retained; no competing WIP split types remain.
- `ui/src/state/workspaceStore.test.tsx`: Main split creation contract retained; complementary tests combined and duplicate unresolved creation signal repaired.
- `ui/src/state/workspaceStore.ts`: Main deferred split lifecycle retained; WIP-compatible incarnation metadata preserved.

## Retained contributions

Resource sampling and dialog are wired from App through registered Tauri command, daemon request and platform sampler. Plan settings and billing API are imported by SettingsDialog. Watchdog is started, managed and stopped by application lifecycle. Remote inventory projection modules are imported by RemoteApp and RemoteSessionList. Machine grid transport and snapshot/protocol modules are declared; protocol contract fixtures remain tested. Attach crypto diagnostic probe is included in its existing test scope.

## Unbuilt retained source

The snapshot codec and Rust/TypeScript terminal protocol contract helpers are declared/imported by their contract tests, but no new production snapshot-transfer consumer is invented. The existing remote grid path remains the runtime transport.

Desktop inventory scenarios in `src-tauri/src/daemon/wip_desktop_inventory_tests.rs` are included in machine tests and run only in a bounded child process with private HOME, USERPROFILE, FERRYX_DATA_DIR and FERRYX_RUNTIME_DIR. Machine-id assertions use main canonical identity lookup within that isolated child, preserving the original assertions. The parent verifies child exit and a completion sentinel.

`src-tauri/src/daemon/local_split_reliability_tests.rs` and `src-tauri/src/daemon/wip_admission_tests.rs` retain scenarios for the competing WIP in-memory admission design (`SpawnReservation`, `AdmissionClock`, `machine_capacity`, `split_probe`). They are not declared because main uses durable journal-backed admission; replacing main or inventing these internals would violate the resolution rule. Main capacity and split transport tests remain built.

## Verification

Pending complete receipts and plain-main comparisons.

Focused repaired frontend run: 139 passed, 0 failed; exit 0 (`/tmp/ferryx-maintree2-ui-repair.log`).
Queue lane repair: local tsc passed, 20 tests passed, exit 0 (`/tmp/ferryx-maintree2-queue-repair.log`).

After the T9 volume interruption, CLI prebuild passed again (exit 0; `/tmp/ferryx-maintree2-cli-recovered.log`). The recovered all-target check exited 101 (`/tmp/ferryx-maintree2-check-recovered.log`): WIP contention tests called main's zero-argument `journal_spawn_probe_handles` with an obsolete workspace argument, now repaired. Three `history_ranges` destructuring failures in `a10_output_budget` await the identical plain-main gate for classification.

Recovered focused frontend run: 41 passed, 3 failed, exit 1 (`/tmp/ferryx-maintree2-ui-recovered.log`). All 11 account-session tests and all 26 restore tests passed. Three WIP-only reducer expectations were stale relative to main: cross-epoch adoption requires matching incarnation, and absent/nonrunning inventory retains session identity with failed reconnect rather than deleting it. Fixtures and full-state assertions now follow those deliberate main contracts.

## Test adaptations

The debug sink sent-event assertion follows main persistence. The save failure fixture uses main accepted structured details and asserts main command/raw enrichment. Desktop inventory tests call main handle_spawn and owner epoch rather than removed WIP admit_spawn APIs. Main revocation synchronization and capacity tests remain authoritative; competing WIP capacity race assertions are preserved unbuilt. Billing test response fixtures use valid empty 204 bodies, await subscribed DOM transitions, and isolate the two account replacement scenarios without changing assertions.

Local inventory reconciliation now propagates main's incarnation evidence. The successful cross-epoch restore fixture supplies the same incarnation in persisted and live rows. WIP local-reconciliation tests assert main's preservation of backend identity and failed reconnect state for missing/nonrunning rows.
