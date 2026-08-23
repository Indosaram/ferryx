# Ultrawork Notepad — Fundamental terminal session persistence implementation
Started: 2026-08-23T11:45:18

## Plan (exhaustively detailed)
1. Read the solution and map current daemon, PTY, transport, restore, and test seams.
2. Run the discovery DAG, synthesize its outputs, and lock the exact implementation partition.
3. Run disjoint implementation lanes test-first: protocol/replay, independent daemon runtime, UI reconciliation, and client transport/renderer.
4. Integrate the lanes at the Tauri terminal IPC boundary.
5. Run focused tests, type/build checks, and a real standalone daemon/PTY reconnect scenario.
6. Persist evidence and a completion audit; leave desktop-only manual verification instructions rather than controlling the user's desktop.

## Success criteria + QA scenarios
Copied in full from `.omo/plans/terminal-session-persistence-fundamental-implementation.md` under “Binding success criteria and evidence.”

## Now
Complete daemon-only ownership and run deterministic standalone daemon/PTY persistence integration proofs.

## Todo
- Capture RED tests before each production contract change
- Integrate protocol-v2 daemon client/server and daemon-only Tauri IPC ownership
- Implement sequence-aware UI attachment, renderer replay, and reconnect behavior
- Add standalone daemon process reconnect/isolated-close contract coverage
- Run automated and real-PTY verification scenarios
- Persist audit and cleanup receipts

## Findings
- `src-tauri/src/lib.rs` constructs and manages `PtyManager` and `TerminalService`, violating daemon-only ownership.
- `src-tauri/src/daemon/protocol.rs` is protocol version 1 and exposes unsequenced attach history.
- `src-tauri/src/daemon/client.rs` retains `direct_server` and an in-process fallback, which cannot survive GUI death.
- `src-tauri/src/terminal/output_hub.rs` stores bytes without sequence ranges, so history/live boundaries cannot be verified.
- `ui/src/lib/terminalTransport/tauriTransport.ts` returns an attachment containing only `sessionId`; no initial history or sequence metadata exists.
- Skills in use: `mass-ulw` (staged DAG coordination), `programming` (Rust/TypeScript test-first implementation), `frontend` (React terminal restoration), and `visual-qa` (terminal/browser-facing QA rules).
- Tier decision: HEAVY because the requested change creates a persistent daemon ownership boundary, upgrades an inter-process protocol, and changes concurrent replay semantics.
- Discovery DAG started: `dag_27ac8f7d-3d0b-47b6-9769-4c717ffc3a5f`; wave 1 maps daemon lifecycle, PTY/replay, UI reconciliation, and verification; the dependency fan-in locks write scopes before implementation.
- Discovery DAG completed after retrying a transient task stream disconnect. The persisted node reports were read from `.omo/senpi-task/dag/results/dag_27ac8f7d-3d0b-47b6-9769-4c717ffc3a5f/` and cross-checked against the actual current sources.
- Locked producer partition: (1) terminal output sequencing/replay, (2) independent daemon CLI/launchd, (3) StrictMode/HMR restore coordinator; then (4) protocol-v2 server/client, (5) daemon-only Tauri IPC bridge, (6) UI attachment/replay transport, and (7) standalone PTY reconnection verification.
- Foundation producer DAG started: `dag_1d6efb32-30a6-4b4f-aa7c-50af8f27f9d3`; independent child write scopes are terminal replay, daemon entrypoint, and UI restoration. Their work is not trusted until the changed sources and reported commands are independently checked.
- Foundation DAG completed. Independent verification passed: `cargo test --manifest-path src-tauri/Cargo.toml terminal::output_hub --lib` (7/7); `cargo test --manifest-path src-tauri/Cargo.toml terminal::tests --lib` (13/13); `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1` (5/5); and the focused Vitest restore scope (61/61). `cargo check --bin ferryx` and `bun run --cwd ui build` also passed.
- The output-hub design passed source inspection: sequence metadata and replay-gap data are explicit, and it subscribes before snapshotting under the hub lock. The current direct PTY ownership in `lib.rs` and `ipc/terminal.rs` remains and is the next mandatory integration seam.
- Known pre-existing warnings from Rust verification: dead-code `WriterLeaseGuard` and `WorktreeManager::acquire_writer_lease` in `src-tauri/src/worktree/manager.rs`; not introduced by this work.
- Ownership-handoff DAG `dag_9f13d74c-591f-47ba-b488-f7bc774cee68` produced and source verification confirmed: protocol v2 (epoch, idempotent spawn, dedicated attachment streams, sequenced base64 frames); UI uses typed decimal-string sequences and history-before-live de-duplication; terminal IPC routes through `DaemonClient` with a managed attachment pump. Focused child claims remain subject to final direct test verification.
- Handoff audit found a root violation: `src-tauri/src/lib.rs:182-188` still creates `RemoteGatewayState` around `TerminalService::default()`. That secondary GUI-owned terminal service violates the requirement that the daemon be the only terminal and remote owner. Also found missing workspace registration, unsafe blind stale socket unlinking, default remote server start, and a fixed-sleep startup loop.
- Completion DAG started: `dag_994440a2-124a-46fe-a3b0-de6af1aff298`. It serializes daemon protocol/ownership hardening before the Tauri remote proxy and final standalone reconnect proof, while UI persistence reconciliation works independently. Completion-node outputs are claims only until sources, tests, and evidence artifacts are independently inspected.

## Learnings
- Terminal process persistence requires a process-owner boundary; layout persistence alone cannot recreate a live PTY.
- Desktop input automation is prohibited for this user, so desktop E2E must be supplied as an exact manual checklist while the daemon/PTY real-surface proof is executed directly.
