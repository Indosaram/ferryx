# Ultrawork Notepad — Correct sidebar worktree hierarchy and project naming
Started: 2026-08-22

## Plan (exhaustively detailed)
1. Audit the reviewed Sidebar, App, Rust registry, IPC, and relevant tests against the explicit hierarchy and naming criteria.
2. Obtain a typed startup-project architecture decision, then delegate disjoint UI and Rust implementation lanes.
3. Update frontend bootstrap and non-active-project completion routing after the typed backend contract exists.
4. Execute focused regression tests, UI build/type validation, Rust tests, and visual QA; record all evidence.
5. Persist final evidence and remove QA-only artifacts.

## Success criteria + QA scenarios
- `cd ui && bun x vitest run src/components/Sidebar.test.tsx src/components/WorktreeList.test.tsx`: project-row action exists and is passed the exact project; titlebar no longer contains Add worktree; Refresh status is absent. PASS means exit 0 and the exact assertions pass.
- `cd ui && bun x vitest run src/App.test.tsx`: clean bootstrap shows the backend canonical ID and non-active-project creation completes against its owner. PASS means exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml`: canonical startup-project command and no-alias registration tests pass. PASS means exit 0.
- `cd ui && bun run build`: PASS means TypeScript plus Vite exits 0.
- Visual QA: a real rendered sidebar screenshot contains titlebar Add project only, per-project worktree controls, and no Refresh status action.

## Now
Architecture decision and disjoint implementation DAG launch.

## Todo
- Complete architecture lane.
- Complete Sidebar/WorktreeList lane.
- Complete backend project-bootstrap lane.
- Complete App startup and owner-routing lane.
- Run tests, build, Rust validation, and visual QA.
- Persist evidence and clean runtime artifacts.

## Findings
- `Sidebar.tsx` currently renders global Add worktree in titlebar and no project-row worktree action.
- `WorktreeList.tsx` currently renders Refresh worktree status, which the user has now explicitly rejected.
- `App.tsx` seeds `DEFAULT_PROJECT` with workspace ID `default`; the backend folder-ID derivation cannot reach the frontend.
- `lib.rs` registers both derived ID and default alias for the same repository, creating duplicate workspace namespaces.
- `createTargetProject` already captures a selected project but its non-active completion must be validated against runtime hooks bound to activeProject.

## Learnings
- Green tests previously encoded the rejected global action; source-level review is required before treating tests as approval.

## Execution log
- 2026-08-22: User added an explicit requirement: remove the `Refresh worktree status` action entirely.
- 2026-08-22: Started DAG `dag_081e597f-0aa8-4a20-a826-4e3d045aacef`: architecture (read-only), disjoint Sidebar/WorktreeList UI implementation, Rust canonical-project IPC implementation, dependent App bootstrap/owner-routing implementation, then independent verification.
- 2026-08-22: QA success requires actual project-row actions, clean startup ID `orca-lite`, no default alias, no Refresh status action, and a rendered sidebar visual check.
- 2026-08-22: Rechecked active diffs. The UI lane had written RED expectations before source implementation; its production changes must remove the remaining global titlebar worktree action and add the project-row action. The Rust lane temporarily truncated the native-contract test while adding its test and was directed to repair it before integration.
- 2026-08-22: LSP independently confirmed App integration is still incomplete: `App.tsx` passes removed `onRefreshWorktreeStatus` to Sidebar. Spawned focused App integration lane `app-integration-fix` to adopt `cmd_project_initial`, migrate placeholder `default`, remove obsolete refresh callback, and test non-active-project creation routing.
- 2026-08-22: Reconstructed and inspected cancelled backend lane artifacts directly. `cmd_project_initial` is present and registered; it resolves the Git top level to `orca-lite`, no longer registers default alias, and native-contract tests assert those invariants. App still needs adoption; the App child has been explicitly steered to start edits after discovery stall.
- 2026-08-22: App integration was completed directly after two stalled child attempts. Added typed `getInitialProject`, native bootstrap gating, placeholder default migration, removed obsolete refresh-status App wiring, and owner-first non-active worktree completion. Focused UI tests (61), frontend build, and Rust full suite passed; the full Rust run additionally exposed an unrelated pre-existing version-2 session persistence test failure from a separately modified test file. Native project contract passed 13/13.
- 2026-08-22: Native visual QA cannot be performed by the agent because the user explicitly prohibits desktop/UI automation. Wrote `.omo/reviews/sidebar-project-hierarchy-manual-qa-2026-08-22.md` with exact user-run checks; awaiting `manual QA PASS` or an observed failure.
- 2026-08-22: Fresh completion audit re-ran feature-specific tests and the native project contract against the current dirty tree: 3 Sidebar, 1 WorktreeList, 4 App startup/owner-flow, 1 typed IPC wrapper, and 13 Rust contract tests passed. Full UI/build and Rust-suite gates are currently blocked by unrelated in-flight App performance/type-test changes and a new but unsupported session-v2 integration test; persisted detailed causality and commands in the review evidence. Native visual QA remains awaiting the user's manual observation.
- 2026-08-22: Concurrent lanes settled. The full focused UI suite now passes 72/72, `bun run build` exits 0, and the entire Rust suite exits 0 including the version-2 session integration test. The only remaining acceptance gate is native visual QA, blocked by the user's explicit prohibition on desktop automation; the manual checklist remains in the review directory.
