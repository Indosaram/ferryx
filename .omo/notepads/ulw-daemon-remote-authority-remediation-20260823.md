# Ultrawork Notepad — Resolve Ferryx daemon and Remote authority review findings
Started: 2026-08-23T00:00:00Z

## Plan (exhaustively detailed)
1. Reconstruct the six reviewed P0/P1 failure paths from current source and test failures; identify existing daemon protocol commands and ownership boundaries before changing code.
2. Obtain a read-only architecture plan for the coupled daemon/GUI Remote authority migration, including safe implementation order and disjoint write scopes.
3. Add RED tests that prove GUI project registration fails to reach daemon terminal spawn, GUI/daemon Remote authority divergence, stale focused-terminal socket access, failed-enable persistence, and the terminal IPC hardening type mismatch.
4. Implement the smallest coherent daemon-authoritative Remote bridge: daemon workspace registration, daemon gateway/auth ownership, GUI Remote forwarding, and active-selection propagation.
5. Implement focus-generation or watch-driven Remote socket revocation, post-bind-only enabled-config persistence, and a daemon-backed hardening test harness.
6. Rerun focused RED→GREEN tests after each increment; then run native/UI build/type/LSP gates and a real daemon-backed Remote listener/terminal surface scenario with explicit cleanup.
7. Write one in-repo audit containing all reviewed findings, changed authority model, exact evidence, cleanup receipts, and manual desktop verification instructions.

## Success criteria + QA scenarios
- Tier: HEAVY — daemon/GUI authority, pairing auth, active-terminal authorization, persistent listener lifecycle, and Terminal IPC all cross process/security boundaries.
- Criterion 1 (workspace/terminal): RED→GREEN `project-register -> daemon terminal spawn` integration test. PASS iff the daemon recognizes the GUI-registered workspace and the resulting backend session is usable.
- Criterion 2 (Remote authority/auth): RED→GREEN test that GUI Remote commands and listener/session/pairing/revocation act on one daemon-owned state. PASS iff no second GUI listener/AuthManager can diverge.
- Criterion 3 (active-terminal access): RED→GREEN connected WebSocket scenario: focus A, connect mobile to A, switch focus to B. PASS iff A connection closes and does not accept further control/output.
- Criterion 4 (listener persistence): RED→GREEN occupied-port enable scenario. PASS iff bind failure leaves persisted config Off/unchanged; a successful enable restores after reopen; explicit Disable remains Off.
- Criterion 5 (hardening migration): RED→GREEN `cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract` with a daemon-backed harness. PASS iff it compiles and preserves terminal hardening assertions.
- Criterion 6 (real surface): start the daemon-authoritative persisted listener, use literal `curl -i http://127.0.0.1:<port>/api/v1/health`, and attach a valid test terminal through the Remote transport. PASS iff health is 200 and Remote exposes/attaches only the desktop-focused daemon PTY; capture transcript and tear everything down.
- Criterion 7 (regression): focused native tests, full affected native suite, Rust format/check, UI test/build/type if UI files change, and changed-file LSP diagnostics exit clean.
- I’ll stop right away when every reviewed P0/P1 failure path has direct RED→GREEN and real-surface evidence, all QA resources are cleaned up, and the in-repo audit is saved.

## Now
Running final fan-in gates after daemon-owned Remote transport proof and cleanup.

## Todo
- Discovery: map reviewed authority and IPC failure paths.
- Architecture: lock daemon authority migration sequence.
- Tests: capture reviewed failure-path RED proofs.
- Daemon: synchronize workspace registration before spawn.
- Remote: centralize listener and auth authority.
- Remote: revoke stale focused terminal sockets.
- Remote: commit enabled config after bind.
- Tests: migrate IPC hardening harness to daemon.
- Surface: capture daemon Remote terminal scenario.
- Verification: run full affected gates and diagnostics.
- Audit: save remediation evidence and manual checks.

## Findings
- Web review scope `cd87c3ba-dc4c-427d-8ba1-85b94a6d4367` independently reported six substantiated findings: P0 daemon workspace registration gap; GUI/daemon Remote authority split; split `AuthManager` persistence; stale active-terminal WebSocket control; pre-bind persistence of enabled listener; and current TerminalService-to-DaemonClient migration breaking `ipc_hardening_contract` compilation.
- The reviewer reproduced daemon workspace failure through `tauri_mock_terminal_events_use_registered_workspace` and `E0308` in the hardening contract. It retracted the prior claim that the hardening failure was unrelated.
- Applicable skills: `programming` (mandatory Rust/TypeScript typed TDD), `debugging` (multi-hypothesis daemon lifecycle diagnosis), `memory-discipline` (durable authority decision), and `frontend` only if GUI/Tauri TypeScript bridge changes are required by the finalized architecture.
- User requested implementation after the review. New goal registered at full reviewed scope; no code change is accepted until a direct test proves the reviewed failure path.
- Discovery directly reproduces the P0 workspace gap and current terminal IPC hardening failure. Existing daemon protocol already exposes `RegisterWorkspace`, `RemoteGetStatus`, `RemoteConfigure`, pairing/device commands, and active-selection set/get. The gap is current GUI forwarding and ownership, not absence of a daemon transport vocabulary.
- Architecture worker `st_01a02e27` is read-only and must return dependency order, tests, authority model, and safe disjoint Gemini write scopes before implementation begins.
- The architecture worker remained silent after two explicit delivery requests and was cancelled as inconclusive. Independent code facts resolve the ownership decision: GUI must forward Remote commands through existing daemon client APIs and stop constructing/restoring its own gateway; daemon owns workspace registration, listener handle, terminal service, auth manager, and active selection. This tightly coupled native protocol migration requires one atomic writer, not parallel overlapping patches.
- The first broad Gemini remediation lane (`st_01a02e2b`) was cancelled after repeated checkpoint requests produced neither a changed RED test nor a precise blocker. The implementation is now split along non-overlapping files: Lane A owns daemon workspace registration and terminal hardening harness; Lane B owns Remote daemon authority/auth/socket/persistence. They both retain direct RED→GREEN requirements.
- Started Lane A `st_01a02e2f` (workspace registration and `ipc_hardening_contract`) and Lane B `st_01a02e30` (daemon-only Remote/Auth, stale socket revocation, safe persistence). Their write scopes are explicitly disjoint; fan-in validation cannot begin until both return terminal evidence.
- Lane A independently passed P0 workspace registration and all seven daemon-backed hardening tests. Lane B captured valid RED tests for stale WebSocket focus revocation and occupied-port persistence but incorrectly stopped before implementation. It was revived in the exact same session to finish its original authority/auth/socket/persistence assignment with mandatory GREEN gates.
- Independent result: `test_occupied_port_enable_does_not_persist_enabled_intent` is now GREEN. The stale-focus test currently cannot compile because the in-progress authority refactor removed manager lifecycle methods still used by startup/tests and creates non-Send future/type inference errors. Lane B was sent exact corrective requirements; no Remote finding is complete until these compile errors and the stale socket assertion are green.
- Lane B correction is accepted provisionally pending final fan-in: GUI `create_app` now uses a daemon-backed manager, GUI Remote IPC forwards through `DaemonClient`, the daemon retains persistent Remote/Auth/PTY state, active-session watch cancellation ends stale terminal sockets, and enabled config persists only after bind succeeds. Independent Remote suite, formatter, compiler, and daemon-owned chain test are green.
- Live QA: an ephemeral daemon on port 43892 registered a temp workspace, spawned a real PTY, marked it active, configured Remote, and paired a Control client. Literal `curl -i http://127.0.0.1:43892/api/v1/health` returned `HTTP/1.1 200 OK`, JSON content type, and `{"status":"ok","version":"0.1.0"}`. Ctrl-C produced `STOPPED`; port closure was confirmed; the temporary example and health transcript were removed (`QA_CLEANUP_COMPLETE`).
- Final fan-in is clean: `cargo test --lib` passed 192 tests; daemon-backed `ipc_hardening_contract` passed 7 tests; `cargo fmt --check` and `cargo check` passed; UI full `npm test` passed 76 files / 553 tests after the test-only `daemonEpoch: null` fixture correction; UI build and TypeScript check passed. Changed native/UI files have zero LSP errors; `git diff --check` is clean; audit and QA cleanup were independently confirmed.
- Saved final audit: `docs/audits/ferryx-daemon-remote-authority-remediation.md`. It records all six findings, RED→GREEN proof, daemon-owned PTY/listener/pairing transport evidence, live curl response, cleanup receipt, and user-run Desktop validation steps.

## Learnings
- Remote cannot safely be split between GUI-local and daemon-local state when daemon owns live PTYs; terminal, listener, pairing, and revocation must share one authority.
