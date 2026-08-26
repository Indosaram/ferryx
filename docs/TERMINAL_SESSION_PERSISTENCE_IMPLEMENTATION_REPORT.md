# Terminal Session Persistence — Implementation Report

Date: 2026-08-24
Specification: [`markdown/TERMINAL_SESSION_PERSISTENCE_FUNDAMENTAL_SOLUTION.md`](../markdown/TERMINAL_SESSION_PERSISTENCE_FUNDAMENTAL_SOLUTION.md)

## Delivered architecture

Ferryx terminal processes are now owned by a user-session daemon, not the GUI:

- **Daemon (sole PTY owner)**: `src-tauri/src/daemon/` — protocol v2 (epoch, `RegisterWorkspace`, idempotent `Spawn { clientRequestId }`, `Attach { afterSequence }`, `DescribeSession`, typed replay-gap frames, dedicated per-attachment stream connections), secure runtime dir/socket/lock (symlink/UID/mode checks, stale-socket removal only after lock proof), event-driven readiness (`FERRYX_DAEMON_READY`), daemon-owned remote gateway (off by default), sequenced output ring buffer (`OutputChunk { sequence, bytes }`) with subscriber-before-snapshot attach.
- **Standalone binary**: `ferryx --daemon` headless entrypoint (`src-tauri/src/main.rs`); checked launchd bootstrap (`daemon/launchd.rs`, `com.rorca.daemon` compatibility identifier preserved).
- **Tauri GUI (client only)**: `lib.rs` manages only `DaemonClient`; every terminal command (spawn/attach/write/resize/signal/close/list/get_cwd) and every remote command proxies through the daemon; one managed attachment pump per session emits sequenced `terminal_output`/`terminal_lifecycle` events.
- **React UI**: StrictMode-safe restore coordinator (`state/workspaceRestore.ts`), persisted `daemonEpoch` + `lastOutputSequence` reconciliation (same epoch + live backend ID → attach; mismatch → mark exited, no auto-respawn), history-before-live rendering with `BigInt` sequence de-duplication and gap/epoch reset (`lib/terminalOutputScheduler.ts`, `terminalTransport/`).

## Phase gates (spec §10)

| Phase | Gate | Automated evidence |
|---|---|---|
| A: cold restore / HMR | StrictMode restore exactly once; HMR 0 spawn / 0 close; per-workspace isolation | `workspaceRestore.test.tsx` (7), `workspaceStore.hmrRetention.test.tsx` (2), `App.test.tsx` (46) |
| B: attach IPC | `cmd_terminal_attach` typed history + decimal sequences; history-before-live; reload re-attach | `ipc::tests` attach test; `terminalHostManager.output.test.ts` (12) |
| C: protocol v2 | Concurrent attach; control free during stream; lag → replay gap; idempotent spawn | `daemon:: --lib` 28/28 incl. dedicated-attach and idempotency tests |
| D: daemon-only routing | No GUI PTY ownership; all commands via `DaemonClient` | source audit + `ipc:: --lib` 24/24 |
| E: standalone daemon | `--daemon` headless; checked launchd; clean restart | `daemon_persistence_contract` CLI readiness test; `launchd` tests |
| F: no auto-respawn | Missing/epoch-mismatch sessions stay exited until explicit user action | `sessionPersistence.test.ts` reconcile tests |

## Verification matrix (re-run independently 2026-08-24 00:10)

| Command | Result |
|---|---|
| `cargo test --lib` | 194/194 |
| `cargo test --tests -- --test-threads=1` | all suites green (rorca_native 13, daemon_persistence 9, ipc_hardening 7, backend_hardening, worktree_safety, e2e_agent_workflow, session_persistence_integration) |
| `cargo check` / `cargo check --tests` | clean (pre-existing `WriterLeaseGuard` dead-code warnings only) |
| `bun run --cwd ui test` | 561/561 |
| `bun run --cwd ui build` | success |

## Real-surface scenario (no GUI)

`daemon_scenario_execution.log`: standalone `ferryx --daemon` (PID 53570), registered workspace, spawned session, verified shell PID 53608 with **PPID = daemon PID**, client disconnect → daemon+shell survive, client-2 reconnect → list/describe/attach → 1,858-byte replay containing pre-disconnect marker, resize verified, live write returned **same shell PID**, isolated close reaped the shell (ESRCH) while the daemon stayed healthy, graceful shutdown exit 0. Sequences 1..89 strictly monotonic. `cleanup_receipt.json` confirms zero residue.

## Manual desktop QA checklist (user-run; desktop automation is out of policy)

1. **HMR**: run `top` in a pane → edit a React component → verify tab/pane layout unchanged, backend session ID unchanged, `top` still running, zero terminal spawn/close in logs.
2. **Full webview reload**: run `while true; do date; sleep 1; done` → reload (Cmd+R) → scrollback replays, output continues without gap/duplicate, input works immediately.
3. **GUI restart**: record shell/agent PIDs (`ps` inside panes) → quit Ferryx → relaunch → same PIDs alive, layout and CWD restored, scrollback present.
4. **Isolated close**: close one pane of a split → only that pane's shell tree exits; other panes unaffected.

## Notes

- `ensureSessionBackends`/`REBIND_SESSION_BACKEND` exists in `workspaceStore.ts` as an exported, currently unwired utility for explicit user-initiated respawn; it is covered by green tests and does not participate in automatic restore.
- A post-settlement unauthorized "zero-tab" edit was reverted (last-tab close restores a replacement tab per the existing contract); orphaned `EmptyWorkspaceView` files removed.
