# Terminal Session Persistence: Fundamental Implementation Plan

## Scope and tier

**Tier: HEAVY.** This changes the terminal process ownership model, Unix-domain-socket protocol, replay concurrency contract, app lifecycle, and React restoration flow. The implementation is correct only if a GUI process can disappear without terminating daemon-owned shells and a fresh UI can reconcile, attach, replay, and control those same sessions.

## Topology

### Discovery DAG: parallel read-only mapping

1. **Daemon protocol and lifecycle** (`unspecified-high`): map `daemon/{client,server,protocol,launchd}` plus `main.rs` and report the smallest production-daemon path and protocol-v2 deltas.
2. **PTY/replay contract** (`unspecified-high`): map `terminal/{pty,service,output_hub,session}` and specify a race-free sequence/replay implementation with focused tests.
3. **Frontend reconciliation** (`unspecified-high`): map `App.tsx`, `workspaceStore.ts`, `hmrWorkspaceState.ts`, terminal transport/host manager, and session persistence; specify local-versus-backend identity handling.
4. **Verification harness** (`unspecified-low`): map Rust integration tests, Vitest tests, current commands, and a non-desktop real PTY/daemon scenario.
5. **Architecture synthesis** (`ultrabrain`, depends on 1–4): reconcile the discovery facts into a dependency-safe implementation partition. It produces no source edits.

### Implementation DAG: staged, disjoint write scopes

1. **Protocol/replay lane**: protocol-v2 frames and sequence-aware `TerminalOutputHub`, including RED→GREEN Rust tests. Owns `daemon/protocol.rs`, `terminal/output_hub.rs`, and their tests.
2. **Daemon runtime lane**: independent daemon process start, safe socket/lock validation, no production direct fallback, lifecycle/epoch behavior, and Rust tests. Owns `main.rs`, `daemon/{server,client,launchd}.rs`, related integration tests, and Cargo configuration only when required.
3. **UI restore lane**: explicit restore coordinator, stable local session identity, `backendSessionId` reconciliation, and HMR isolation, with RED→GREEN Vitest tests. Owns `App.tsx`, `state/{workspaceStore,hmrWorkspaceState}.ts`, persistence tests, and types.
4. **Transport/renderer lane**: typed attach metadata and history-before-live, sequence de-duplication/gap behavior, plus tests. Owns `lib/{tauri,terminalHostManager}.ts`, `lib/terminalTransport/*`, and their tests.
5. **Integration lane** (after 1–4): route terminal IPC to `DaemonClient`, add attach/list APIs, reconcile client/server shapes, and add process-lifecycle integration coverage.
6. **Verification lane** (after all producers): run focused Rust/Vitest suites, typecheck/build as applicable, and launch an actual standalone daemon/PTY scenario that proves disconnect/reconnect retains the shell PID and output continuity. It writes only evidence under `.omo/evidence/`.

## Binding success criteria and evidence

1. **Daemon survives a GUI client loss.**
   - RED: an integration test starting a daemon-owned shell must fail because current direct GUI ownership kills or cannot reattach the shell.
   - GREEN: `cargo test --test daemon_persistence_contract daemon_process_survives_client_disconnect -- --exact` exits 0.
   - Surface: a bounded Rust driver launches the standalone `--daemon` process, spawns `sh -lc 'printf READY; exec sleep 30'`, disconnects/reconnects a `DaemonClient`, and observes the same backend session ID, shell PID, and `READY` replay. Evidence: `.omo/evidence/terminal-session-persistence/daemon-reconnect.txt`.

2. **Attach/replay has no ordering ambiguity.**
   - RED: a unit/integration test must show current unsequenced history/live delivery cannot distinguish duplicates or gaps.
   - GREEN: Rust tests cover monotonic sequence numbers, overflow range/gap response, and subscriber-before-snapshot ordering.
   - Surface: the bounded driver writes `before`, attaches after the recorded sequence, writes `after`, and receives each marker exactly once in increasing sequence order. Evidence: `.omo/evidence/terminal-session-persistence/replay-order.txt`.

3. **Reload/HMR restore reuses live backend sessions.**
   - RED: Vitest tests must fail under the old unconditional/misaligned restore behavior.
   - GREEN: `bun run --cwd ui test -- <focused restore and transport files>` exits 0 and proves existing IDs attach, missing IDs become explicit replacement candidates, history precedes live output, and HMR does not spawn/close.
   - Surface: UI-side transport scenario executes a remount/reconcile using a live daemon session with zero spawn/close calls. Evidence: `.omo/evidence/terminal-session-persistence/ui-reconcile.txt`.

4. **Intentional close is isolated and terminal.**
   - RED: integration test must fail before process-group closure is correctly routed through the daemon.
   - GREEN: the test proves one `Close` ends that shell/process group without harming an independent session.
   - Surface: driver closes one of two daemon-owned sessions and observes its lifecycle exit while the other continues to accept input. Evidence: `.omo/evidence/terminal-session-persistence/isolated-close.txt`.

5. **No unsafe fallback or silent recovery changes the ownership model.**
   - RED: daemon-connect failure test must demonstrate that the old direct fallback is reachable.
   - GREEN: production path returns a typed daemon-connection/protocol error; stale socket cleanup is restricted to verified safe conditions; protocol mismatch is visible.
   - Surface: daemon client sends an incompatible handshake and receives the explicit mismatch error, never an in-process terminal. Evidence: `.omo/evidence/terminal-session-persistence/protocol-mismatch.txt`.

## Manual desktop QA (user-run, because desktop input automation is explicitly prohibited)

After automated evidence is green, provide a precise checklist for the Ferryx desktop app: start long-running programs in split panes, record shell and daemon PIDs, trigger Vite HMR, reload the webview, kill/relaunch only the GUI process, verify PIDs/scrollback/input remain, then close one pane and verify only its process group exits. This is supplementary confirmation; automated daemon/transport evidence is the required non-GUI surface.

## Stop condition

Stop immediately when all five success criteria have RED→GREEN evidence, the daemon/PTY real-surface driver artifacts exist and pass, changed-code diagnostics/tests/build checks are clean or explicitly pre-existing, all QA resources are removed with receipts, and the completion audit is persisted.
