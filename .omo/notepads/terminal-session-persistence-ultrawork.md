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

- User directive (2026-08-23 23:0x): suspected model-fallback instability; ordered every lane re-routed to flash/ox-alpha. Evidence check: past failures were stream disconnects on both flash- and sol-served nodes, and `~/.omo/agent/omo.json` had `unspecified-high`→terra divergence. Both config layers (`~/.omo/omo.json`, `~/.omo/agent/omo.json`) now route ALL categories and agents to `quotio/gemini-3.7-flash-high` with `stealth/ox-alpha` fallback; backups saved (`*.bak-flash-oxroute-20260823`); no `omo.jsonc` shadow.
- Completion DAG `dag_994440a2` cancelled (preserving completed nodes) and the 3 unfinished nodes retried under new routing: `tauri-daemon-bridge-and-remote-proxy`, `daemon-persistence-integration`, `completion-verification`.
- Independent re-verification of the two pre-cancel completed nodes (claims → confirmed by my own runs): `cargo test daemon:: --lib` 28/28; `terminal:: --lib` 24/24; `remote:: --lib` 20/20 (claim said 14 — superset); UI persistence scope 69/69 via bun vitest.
- Repo state: HEAD moved to `fc2ef4d feat: consolidate Ferryx workspace updates` (accumulated implementation committed). Working tree delta on top: 28 files / +1237 −198 (current uncommitted completion work incl. epoch/sequence persistence and partial bridge edits). Pre-cancel bridge child had already removed GUI-side `create_remote_state`/`RemoteGatewayState::new_persistent` from lib.rs body (remains only in tests).
- Routing audit (retried run): `tauri-daemon-bridge-and-remote-proxy` task st_01a02f01 footer shows `model: quotio/gemini-3.7-flash-high` — new routing confirmed active. Downstream nodes (`daemon-persistence-integration`, `completion-verification`) pending scheduling behind the bridge node; they inherit the same config snapshot.

## 2026-08-23 23:3x — model-fallback root cause + bridge verification + amend
- Bridge node (st_01a02f01, flash-served) INDEPENDENTLY VERIFIED: source shows lib.rs manages only DaemonClient (no TerminalService/PtyManager/OutputHub/RemoteGatewayState in production path); ipc/remote.rs proxies all remote commands via RemoteGatewayManagerInner::Daemon; project.rs registers workspace before spawn; no fixed sleeps remain. My own re-runs: ipc:: 24/24, ipc_hardening 7/7, daemon_persistence_contract 5/5, cargo check clean.
- daemon-persistence-integration failed: task footer shows category `deep` resolved to gpt-5.6-sol (codex, 85h cooldown) — the LIVE host snapshots category→model at process start, so my config rewrite cannot affect this session (stale ModelRegistry, consistent with prior repro). Flash served only nodes whose categories the OLD user-layer config already mapped to flash (unspecified-high, quick).
- Fix without restart: `dag amend` on the SAME run changed daemon-persistence-integration category deep→unspecified-high (flash under both old snapshot and new config); its dependent completion-verification (quick→flash) re-runs with it; three verified nodes keep cached results. Amend initially rejected (node misplaced outside nodes[]); resubmitted with all 5 nodes embedded — accepted, run active.

## 2026-08-24 00:1x — final audit, rogue-edit repair, goal closure
- Post-settlement rogue edits ("zero-tab-store-inversion" + one more child, both flash) had inverted the last-tab-close contract (removed replacement spawn, added EmptyWorkspaceView, rewrote 3 tests). Neither zero-tab nor REBIND was in any authorized lane scope.
- Repaired via surgical edits (git restore was user-rejected): closeTab replacement logic restored (browser + terminal branches + deps); browserLifecycle.test + 2 rewritten tests restored to HEAD contract; rogue test titles/assertions reverted; EmptyWorkspaceView.{tsx,test.tsx} removed; App.tsx restored to direct TerminalSplitView render.
- REBIND_SESSION_BACKEND + "running" lifecycle kept: coherent, green, exported-but-unwired explicit-respawn utility (documented in report).
- rorca_native_contract.rs compile break (caused by our cmd_project_register daemon-state param): fixed with file-local TestDaemon (temp-socket DaemonServer) + managed DaemonClient across 5 affected tests. 13/13 green.
- FINAL INDEPENDENT VERIFICATION (my own runs): cargo --lib 194/194; cargo --tests all suites green; rorca_native 13/13; daemon_persistence 9/9; ui test 561/561; ui build clean. Real-surface daemon scenario evidence + cleanup receipt verified earlier (PID 53570/53608, PPID proof, replay, isolated close, ESRCH reap).
- Process hygiene: 5 leaked test daemons killed across session (13416,10536,5792,3542,57809 + final one); /tmp/rorca-501 sockets/locks removed. Final state: 0 ferryx --daemon processes.
- Report persisted: docs/TERMINAL_SESSION_PERSISTENCE_IMPLEMENTATION_REPORT.md (phase gates, verification matrix, manual desktop QA checklist per no-desktop-automation policy).
- Not committed (user rejected a git write earlier this session); work left staged in working tree for user review.
