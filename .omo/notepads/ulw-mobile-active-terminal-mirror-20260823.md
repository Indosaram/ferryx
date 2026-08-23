# Ultrawork Notepad — Ferryx mobile active-terminal desktop mirror
Started: 2026-08-23T00:00:00Z

## Plan (exhaustively detailed)
1. Reconcile the prior blocked Web delegation attempt, inspect current native Remote, desktop focus, and mobile Remote source, and record the HEAVY-tier evidence contract.
2. Run two read-only architecture lanes: native Remote authorization/selection flow and desktop/mobile focus and selector flow; incorporate their facts before execution.
3. Start the mass-ulw DAG with isolated producer scopes: native active-context/selection contract, desktop focused-context publisher/selection consumer, and mobile singular-mirror selector; fan-in to read-only verification.
4. Require each producer to capture an appropriate RED test before production code, its GREEN rerun, and only files in its assigned ownership boundary.
5. Verify the joined behavior with focused tests, formatting, compiler/type checks, LSP diagnostics, a browser-rendered Remote UI scenario, resource cleanup, and a source-level cross-check.
6. Save an in-repo audit with exact evidence, blocked/unsupported desktop GUI limitation, and user-run native desktop QA actions. Do not manipulate the user's desktop.

## Success criteria + QA scenarios
- Tier: HEAVY — Remote session exposure/attach authorization plus native-to-desktop-to-mobile state integration crosses a security/session boundary.
- Criterion 1 (focused-only Remote session): RED→GREEN native tests that a focused session is returned while a live background PTY is omitted and a background `/api/v1/terminal/:id` WebSocket attach is rejected. Surface scenario: authenticated `curl -i http://127.0.0.1:<port>/api/v1/workspace/state` with a fixture/live server containing focused `focus-1` and background `background-2`; PASS iff only `focus-1` occurs, captured HTTP transcript artifact.
- Criterion 2 (desktop control bridge): RED→GREEN tests that active tab/leaf maps to its backend terminal ID, browser/no-terminal focus clears publication, and a safe `{ workspaceId, worktreeSlug }` request invokes the existing desktop selection behavior. Native desktop verification is a user-run manual scenario: select a workspace/worktree from an authenticated mobile Remote page and observe the desktop activate that worktree and its focused terminal; PASS iff the mobile breadcrumb/terminal updates to the desktop result. No desktop automation under the user's constraint.
- Criterion 3 (mobile mirror UX and privacy): RED→GREEN Remote UI tests using state that includes focused and background sessions; PASS iff exactly the declared active terminal renders, context selector lists only safe workspace/worktree labels, selecting a row presents pending then confirmed state, no-focus state is useful, and `/Users/example/private` never appears. Surface scenario: run the project browser Remote-QA harness against mocked `/api/v1/workspace/state` and `/api/v1/workspace/select`; PASS iff its screenshot shows one terminal and selector context, captured screenshot and action log.
- Criterion 4 (regressions): `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, focused Rust remote tests, `cargo check --manifest-path src-tauri/Cargo.toml`, `cd ui && npx vitest run src/remote src/App.test.tsx --maxWorkers=1`, `cd ui && npm run build`, and LSP diagnostics on all changed files must exit clean. Evidence is captured command output in this notepad/audit.
- I'll stop right away when all criteria pass with captured artifacts, all QA resources have cleanup receipts, the in-repo audit has exact manual desktop checks, and the mass-ulw DAG has no unresolved node.

## Now
All implementation, regression, browser QA, cleanup, and audit evidence is captured; preparing final outcome.

## Todo
- Discovery evidence and plan: inspect current state and close architecture lanes.
- Native contract: RED→GREEN active Remote context plus safe selection bridge.
- Desktop publisher: RED→GREEN focused context and remote selection handling.
- Mobile selector: RED→GREEN singular Remote mirror interface.
- Fan-in tests: verify joined native and UI regressions.
- Browser QA: render and capture mobile Remote behavior.
- Audit: save evidence and exact manual desktop checks.

## Findings
- The previous `delegate_to_chatgpt_web` batch made no source changes because `gpt2omo` at `127.0.0.1:18800` refused its preflight health check; it created no retained scope. This invocation now uses the user-requested mass-ulw native DAG instead.
- Existing plan and direct source inspection identify the frontend layout as focused-terminal authority: active tab, active leaf, leaf-local session mapping, then backend session ID.
- Applicable skills: `mass-ulw` (required native DAG orchestration); `programming` (Rust/TypeScript TDD and typed integration); `frontend` (Remote mobile UX and browser visual QA); `debugging` (prior failure/root-cause discipline); `memory-discipline` (durable decisions and constraints). `delegate-web-dag` is not used for this run because its sole required Web helper is authoritatively unavailable and this turn explicitly requests `mass-ulw`.
- Goal registration: the existing goal remains active in the goal tool, so creating a duplicate goal was rejected. Its stated objective already matches this task and is the binding goal.
- Native architecture lane (`st_01a02c88`) verified `src-tauri/src/remote/server.rs:248`, `:272`, and `:446`: list/state enumerate every `TerminalService::list_sessions()` entry, and attach accepts any extant PTY. `RemoteGatewayState` and its Tauri manager are the native bridge seam. Its initially suggested `worktree_path` DTO is rejected because Remote must not receive a local path.
- UI architecture lane (`st_01a02c89`) verified the desktop authority chain: `LayoutState.focusedGroupId` -> group active tab -> `TabPaneLayout.activeLeafId` -> `sessionIdsByLeafId` -> `TerminalSession.backendSessionId`; browser tabs resolve to no terminal. Existing actions are `ACTIVATE_TAB`, `FOCUS_PANE`, and `SELECT_WORKTREE`.
- Current `RemoteSessionList` is an accordion grouping all server sessions. It must not become a mobile local session picker: the product contract is one server-declared focused terminal, while the selector requests a safe desktop workspace/worktree transition.
- Current Remote project DTO contains `repo_root`; this field must not be rendered and new active/selection DTOs must contain only safe workspace IDs plus worktree labels/slugs, never `repo_root`, `cwd`, or worktree paths.
- Browser QA harness from the ultrawork directive is absent at `script/qa/web-terminal-visual-qa.mjs`; no direct desktop automation is permitted. The QA lane must use an available real browser driver against a browser-served Remote fixture or record its concrete availability block.
- Started mass-ulw DAG `dag_8095bb90-0e3c-44fb-8ff8-489370f63043` with topology locked above: three isolated producer nodes in Wave 1; read-only integration verification and browser QA in Wave 2; one audit writer in Wave 3. The `start` response reported six nodes and no definition warning.
- DAG completion is untrusted until independently verified. Its own `integration-verify` correctly found a concrete defect: `ui/src/lib/tauri.ts:456` calls `cmd_remote_set_active_selection` with `{ payload: ... }`, while `src-tauri/src/ipc/remote.rs` names the command parameter `request`; Tauri argument deserialization therefore fails at runtime. The `desktop-focus-publisher` node was revived with a test-first exact-key correction.
- Current `npm run build` is red on unowned runtime-test type errors: `ui/src/state/workspaceStore.browserPaneLeafMove.runtime.test.tsx:159` declares unused `browserTabsWithId`; `ui/src/components/TerminalSplitView.mixedPaneLeaf.runtime.test.tsx:124` constructs a browser leaf missing required `browser` content. This must be distinguished from the focused-mirror changes; do not modify those shared concurrent files unless their owning agent resolves them.
- Independent browser evidence is present under `/tmp/ferryx-remote-qa-st_01a02c9b`: five Chrome screenshots, action/request/cleanup logs, and no listener on its fixture port after cleanup. Visual inspection confirms one active terminal, context selector safe labels, disabled pending row, confirmed no-focus state, and independent no-focus state; no background title/path appears.
- Independent rerun after the IPC correction is green: `cargo test --manifest-path src-tauri/Cargo.toml remote` (13 passed); `cargo fmt --check`; `cargo check`; targeted App/Tauri/Remote Vitest; full `cd ui && npm test` (76 files, 527 tests); `cd ui && npm run build`; and `cd ui && npx tsc --noEmit` all exit 0. Changed-file LSP error diagnostics are clean.
- The first final-gate build failure was transient shared-workspace state; the final rerun is authoritative. The audit must not retain it as a current blocker, so its DAG writer was revived with exact fresh gate evidence.
- The revived audit writer updated `docs/audits/ferryx-mobile-active-terminal-mirror.md`. Independent read confirms it records no stale FAIL/type-error/IPC-key claim, includes the green 76-file/527-test suite, production build, typecheck, native tests, privacy and authorization guarantees, browser evidence, cleanup, and an explicit manual desktop QA section.
- Final audit checks independently passed: DAG has 6/6 completed nodes; all five `/tmp/ferryx-remote-qa-st_01a02c9b/*.png` browser screenshots exist; cleanup log says the browser closed and fixture PID terminated; port 51942 has no listener; and LSP error diagnostics are clean across changed Remote Rust, remote IPC, `App.tsx`, `lib/tauri.ts`, and `ui/src/remote`.

## DAG topology lock
- Wave 1 (parallel, disjoint writes): `native-active-contract` owns only Rust Remote/IPC state, HTTP routes, attach authorization, and native tests; `desktop-focus-publisher` owns only App/Tauri bridge plus App tests; `mobile-singular-selector` owns only `ui/src/remote/**` plus Remote tests. This prevents shared-file edits while each lane provides its own RED→GREEN proof.
- Wave 2 (parallel, read-only): `integration-verify` reruns typed native/UI/build/LSP gates and falsifies source-level invariants; `browser-remote-qa` renders the real browser-facing Remote fixture and captures one-terminal/context-switcher/no-focus evidence without desktop control.
- Wave 3: `write-audit` owns only `docs/audits/` and records evidence, cleanup, and exact user-run desktop instructions after it independently confirms outputs exist. `integration-verify` and `browser-remote-qa` are prerequisites.
- Routing: native is `unspecified-high` because it changes Remote session authorization and native IPC across several modules; desktop is `unspecified-low` because it is a contained but non-mechanical App/store integration; mobile and browser QA use `visual-engineering` because their deliverable is browser/mobile interaction quality; final integration verification is `unspecified-low`; the audit is `writing`.
- Safety invariants: public text says Ferryx (never old Orca branding); retain internal compatibility identifiers only; selection requires Control permission; View remains read-only; inactive attach is always forbidden; no path-bearing field crosses or renders in the Remote active/selection contract.

## Learnings

## Learnings
- Do not treat a green suite that predates this feature as evidence of focused-only mobile mirroring; the contract must be directly tested and rendered.
- A workspace/worktree selector is a desktop-context control request, not permission to browse or attach arbitrary PTYs from a mobile list.
