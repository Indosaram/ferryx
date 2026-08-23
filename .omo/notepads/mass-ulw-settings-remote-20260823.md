# Ultrawork Notepad — Settings and Remote reliability
Started: 2026-08-23T00:20:00+09:00

## Plan (exhaustively detailed)
1. Read the mass-ulw, debugging, frontend, programming, and visual-QA instructions; use mass-ulw for a read-only investigation DAG, debugging for persistence/lifecycle root cause, programming for TypeScript/Rust changes, frontend for Settings IA, and visual-QA for rendered evidence subject to the recorded no-desktop-control constraint.
2. Run a parallel investigation DAG for persistence, Remote lifecycle/pairing, Settings IA, workspace state, test seams, and a dependency cross-check.
3. Synthesize the returned evidence into disjoint implementation scopes and add the implementation DAG.
4. For every behavioral slice, capture a failing test or faithful scenario before code changes, then capture green proof after the smallest fix.
5. Run permitted real-surface proof. Do not operate the user's desktop; provide exact manual desktop verification steps where native-app interaction is required.
6. Run affected tests, production build, diagnostics, diff cleanup, persisted audit report, and a final self-review.

## Success criteria + QA scenarios
- Tier: HEAVY — persistence and Remote Access restart behavior cross frontend/native boundaries.
- C1 Settings persistence: `cd ui && npx vitest run <new persistence test>` must be RED then GREEN; desktop manual scenario: Appearance → Dark → quit/relaunch → Dark remains selected.
- C2 Pairing copy: `cd ui && npx vitest run <new PIN-copy test>` must be RED then GREEN; desktop manual scenario: Remote Access enable → click `[data-testid="remote-pairing-code"]` → copied feedback and clipboard receives the displayed PIN.
- C3 Remote restart: native lifecycle test must be RED then GREEN; desktop manual scenario: enable → quit/relaunch → Remote status matches explicit persisted behavior copy.
- C4 IA and workspace: Settings test must be RED then GREEN for Default Agent explanation, no Quick Commands, useful General page, and registered workspace display.
- C5 Regression: affected Vitest suite, `npm run build`, LSP errors 0; no temporary server/browser artifacts remain.
- Stop: I will stop right away when all seven behaviors pass their named proof and permitted surface evidence and the report is saved.

## Now
Run read-only investigation DAG and collect implementation facts.

## Todo
- Investigation DAG — in progress
- Synthesize safe implementation slices
- Capture RED proofs
- Run implementation DAG
- Capture GREEN and permitted surfaces
- Run final validation and write report

## Findings
- Skills selected: mass-ulw (user-mandated DAG); debugging (settings persistence, incorrect workspace, Remote lifecycle); programming (TypeScript/Rust TDD); frontend (existing Settings information architecture); visual-qa (UI changes, with manual desktop verification because the user forbids desktop control).
- The user has an explicit standing constraint against direct desktop/app UI automation. Native-app manual QA must be requested, not driven by the agent.
- Current requested items: persistence; clickable pairing PIN; Remote restart semantics; Default Agent explanation; Quick Commands deletion; workspace false-empty state; non-empty General.
- Investigation DAG started: `dag_a89e9094-eacd-454b-83dc-3a56357bde5c`. Nodes: settings-persistence, remote-lifecycle, settings-information-architecture, workspace-settings, test-and-surface-inventory, then cross-check.
- The investigation DAG is not evidence: the `remote-lifecycle` worker was blocked twice by provider `400 Request Blocked`, so the cross-check node was skipped. Its output is not used for decisions.
- Direct source evidence: `SettingsDialog.tsx` retains an isolated Appearance localStorage flow; `GeneralSettings` is empty; Quick Commands remains implemented in the Settings module; the Remote PIN is visible but not a click target; Default Agent persists but `App.handleLaunchAgent` only receives explicit menu selection.
- Direct source evidence: `App.tsx` mounts `SettingsDialog` without `projects`, `activeProjectId`, `activeWorktree`, or workspace callbacks; `WorkspaceSettings` therefore receives defaults and falsely displays no workspace.
- Direct source evidence: `RemoteGatewayState` is rebuilt by `create_app` for each process; `cmd_remote_enable` changes state and starts a server only in that process. Existing code therefore turns Remote Access off on relaunch. This is the safe default policy to make explicit unless a tested startup restore is intentionally implemented.
- Implementation topology: Native Remote lifecycle owns Rust files/tests; Settings behavior owns SettingsDialog/UI tests; Workspace wiring owns App/App tests. These producer scopes are disjoint. Final verification depends on all three.
- Implementation DAG started: `dag_c750013e-821d-470d-9b44-4a42670dfd9b` with Native Remote lifecycle, Settings behavior, Workspace wiring, then integration verification.
- Direct post-DAG verification: `cargo test remote::state --manifest-path src-tauri/Cargo.toml` passed 2/2 (`stale_enabled_config_file_does_not_look_enabled`, `enabled_gateway_does_not_restore_listener_after_reopen`); it emitted three pre-existing dead-code warnings in `src/worktree/manager.rs`.
- Direct post-DAG verification: LSP returned 0 errors for `SettingsDialog.tsx`, `App.tsx`, and `remote/state.rs`; `git diff --check` passed for producer files.
- Direct post-DAG frontend sweep `npx vitest run src/components/SettingsDialog.test.tsx src/components/SettingsDialog.workspace.test.tsx src/App.test.tsx src/lib/settingsRuntime.test.ts` failed 2 tests. Root failures must be isolated before accepting the Settings producer.
- Isolated verification: pairing PIN test passed when run by its exact title. The initial broad failure did not invalidate that behavior. Default Agent test passed only because it asserts the non-functional current state; this does not meet the user-visible intent, so a new RED→GREEN increment will wire the preferred agent into the New Tab agent action list as explicit Default.
- RED cleanup evidence: focused PIN/Default Agent test command exercised the new PIN assertion but exited non-zero because the 1.5-second `setPinCopied(false)` callback fired after JSDOM teardown (`ReferenceError: window is not defined`). The pairing-copy timer must be cleared on component unmount before that criterion can pass.
- Direct functional review found `defaultAgentId` remained write-only after the producer update. New requirement-specific completion lane: the setting will have observable behavior in the New Tab agent launcher (Default badge and first position), with a distinct prop chain and test scope. This avoids shipping a misleading preference.
- Default Agent completion DAG started: `dag_173a99f9-8a8d-4770-801a-69a9b54adacb`.
- Direct lifecycle review found `RemoteGatewayState::new_persistent` was implemented but `create_app` still constructed `RemoteGatewayState::new`; without wiring, paired-device/auth persistence would not survive a real app restart. Completion DAG `dag_36f96bad-0f9d-45fe-89f4-dad57e05d411` will wire persistence while retaining session-only listener OFF semantics.
- RED terminal test capture: `cd ui && npx vitest run src/lib/terminalSettings.test.tsx --maxWorkers=1 --reporter=verbose` failed 2/6 because the test's dynamic `vi.mock(import("./tauri"), ...)` did not intercept `terminalSettings`' static `./tauri` import; actual browser fallback `#0a0a0a` appeared instead of mocked Ghostty `#282c34`. This is a test mock identity defect, not a product setting write change. The mutation is test-only: use the canonical static module-id mock and rerun the exact contract.
- GREEN terminal contract: after correcting the mock identity, `cd ui && npx vitest run src/lib/terminalSettings.test.tsx --maxWorkers=1 --reporter=verbose` passed 6/6. The test-only mock is narrowed further to the two runtime exports the hook reads so TypeScript diagnostics remain clean.
- GREEN Remote app construction: `cargo test app_remote_state_persists_pairing_but_starts_off --manifest-path src-tauri/Cargo.toml` passed 1/1. The app now uses `new_persistent`; paired devices survive a reconstructed app state while `mode` is Off, `is_running` false, and bound address absent. Cargo reported pre-existing dead-code warnings in `src/worktree/manager.rs`.
- Final validation started: UI full suite is running in `bash_140`. Native full suite ran 138 tests and failed only `daemon::server::tests::test_pump_stream_compact_framing_and_exit` at `src/daemon/server.rs:444`, where its expected compact JSON session ID no longer matched stream output. Remote/state tests and app construction test passed. Isolation of the daemon failure is in progress; do not attribute it to Settings/Remote changes without source evidence.
- UI full suite completed GREEN: `cd ui && npm test` passed 67 files / 488 tests. Native full suite result: 137 passed, 1 failed (unrelated daemon compact stream assertion); isolated failure reproduces at `src-tauri/src/daemon/server.rs:444` and lives in concurrent daemon protocol/server modifications, not in Settings or Remote persistence paths.
- Test hygiene: the new app-level Remote persistence test used `FERRYX_DATA_DIR`; add a scoped restoration guard plus mutex before final test evidence so parallel tests cannot observe leaked environment state.
- Final browser QA plan (local headless browser only; never the user's desktop): start Vite at `http://127.0.0.1:4173`, open with `agent-browser --session ferryx-settings-qa open http://127.0.0.1:4173`, enter Settings from app chrome, and capture snapshots/screenshots of General, Appearance, Workspace, Agents/New Tab Default badge, and Remote. PASS iff all named elements appear with no Quick Commands; cleanup by `agent-browser --session ferryx-settings-qa close` and killing Vite session `bash_157`.
- Browser QA result: Vite opened successfully, but its browser-only route rendered only the Remote pairing page (`6-digit PIN`, disabled Connect); there is no native Settings shell or Tauri command backend on this surface. It is not faithful evidence for desktop Settings and was not used as pass evidence. Browser session is closed; Vite listener cleanup follows. Manual native-app QA must be performed by the user under the no-desktop-control constraint.

## Final evidence
- RED mutation proof: the intentionally broken Appearance handler, pairing PIN test id, General overview test id, Default Agent eligibility, Quick Commands storage key, Workspace Settings props, and Remote restart reset made their named UI/Rust assertions fail. Each mutation was restored with `apply_patch` before GREEN verification.
- GREEN focused UI: `cd ui && npx vitest run src/components/SettingsDialog.test.tsx src/components/NewTabPopover.test.tsx src/lib/storageKeys.test.ts src/App.test.tsx src/lib/settingsRuntime.test.ts src/lib/terminalSettings.test.tsx src/components/SettingsDialog.workspace.test.tsx --maxWorkers=1` passed 7 files / 90 tests.
- GREEN UI full suite: `cd ui && npm test` passed 67 files / 489 tests.
- GREEN UI build: `cd ui && npm run build` passed.
- GREEN native Remote: `cargo test remote::state --manifest-path src-tauri/Cargo.toml`, `cargo test remote_status_after_reopen_is_disabled_session_only --manifest-path src-tauri/Cargo.toml`, and `cargo test app_remote_state_persists_pairing_but_starts_off --manifest-path src-tauri/Cargo.toml` all passed.
- GREEN native compiler gate: `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `cargo check --manifest-path src-tauri/Cargo.toml` passed. `ui/src` LSP diagnostics were clean; a late `src-tauri/src` LSP daemon request timed out, but `cargo check` completed cleanly.
- Cleanup receipt: agent-browser session `ferryx-settings-qa` closed; Vite `bash_157` killed; `lsof -nP -iTCP:4173 -sTCP:LISTEN` exited 1 (no listener).
- Full Rust suite residual: 137 passed / 1 failed. The isolated unrelated failure is `daemon::server::tests::test_pump_stream_compact_framing_and_exit` at `src/daemon/server.rs:444`, in shared daemon stream code; it remains unmodified.

## Self-review
- HEAVY tier held: persistence, native Remote lifecycle, and multi-surface Settings changes crossed UI/native boundaries. Direct diff reread confirmed all seven requested behaviors are implemented. The only incomplete faithful surface is native desktop visual QA, which I cannot operate under the user’s standing no-desktop-control instruction; exact manual scenarios are in `docs/audits/settings-remote-reliability-mass-ulw.md`.

## Learnings
- Record final state/behavior decisions in memory once confirmed by code and tests.

## Now
Await user confirmation of the six native desktop Settings QA scenarios; desktop automation is prohibited by the recorded user constraint.

## Todo
- Persist settings across application restarts — active investigation
- Enable clickable remote pairing PIN copying
- Define enabled remote restart lifecycle
- Clarify default agent launch behavior
- Remove Quick Commands settings flow
- Restore registered workspace settings display
- Replace empty General settings content
- Run targeted settings and remote regressions
- Build UI and inspect changed diagnostics
- Write audit and manual QA instructions
- User confirms six native Settings scenarios
