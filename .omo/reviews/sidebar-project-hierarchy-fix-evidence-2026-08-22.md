# Sidebar project hierarchy fix — verification evidence

Date: 2026-08-22

## Acceptance checklist

| Requirement | Evidence | Status |
| --- | --- | --- |
| Titlebar retains Add Project but not Add Worktree | `ui/src/components/Sidebar.tsx` renders `Add project` in the titlebar and has no titlebar `Add worktree`; `Sidebar.test.tsx` asserts the absence. | PASS |
| Every project row owns its worktree action | `Sidebar.tsx` renders `label={\`Add worktree to ${project.workspaceId}\`}` and invokes `onCreateWorktree(project)`. `Sidebar.test.tsx` clicks both project rows and asserts the exact object passed. | PASS |
| Refresh status action is removed | `WorktreeList.tsx` no longer imports `RefreshCcw`, accepts `onRefreshStatus`, or renders `Refresh worktree status`; `WorktreeList.test.tsx` asserts absence while delete and dirty display remain. | PASS |
| Initial project is canonical folder ID | Rust `cmd_project_initial` returns the Git-root-derived project. `App.tsx` calls typed `getInitialProject()` during native bootstrap and migrates only the `default`/`.` placeholder. `App.test.tsx` proves fresh and migrated startup display `orca-lite`. | PASS |
| No duplicate default alias | `src-tauri/src/lib.rs` no longer registers `default`; `cmd_project_register` enforces one ID per canonical root. `rorca_native_contract.rs` proves initial registry contains only `orca-lite`. | PASS |
| Non-active project worktree creation uses owner | The row action supplies the selected project; App uses `createTargetProject`, switches to its owner, stores pending path, and opens it after the owner runtime receives the worktree. `App.test.tsx` drives beta creation and asserts beta creation, active-project switch, and tab opening. | PASS |

## Commands run

1. `cd ui && bun x vitest run src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx src/components/WorktreeList.test.tsx src/App.test.tsx src/lib/tauri.test.ts`
   - Exit 0: 61 tests passed.
2. `cd ui && bun run build`
   - Exit 0.
3. `cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract`
   - Exit 0: 13 tests passed, including canonical `orca-lite`, no `default` alias, and one-root/one-workspace-ID contracts.
4. `cargo test --manifest-path src-tauri/Cargo.toml`
   - Not clean due to a pre-existing unrelated failure in `src-tauri/tests/session_persistence_integration.rs::test_version_2_session_envelope_restore_preserves_multiple_workspaces`: it expects loader support for version 2 while `src-tauri/src/session/mod.rs` intentionally accepts only version 1. The test file was already modified outside this scope; this sidebar/project change does not touch session persistence.

## Fresh completion audit

This audit was run again after subsequent concurrent working-tree changes. It does not rely on the earlier green output above.

| Objective deliverable | Current direct evidence | Verdict |
| --- | --- | --- |
| Add project is the only titlebar management action | `Sidebar.tsx` titlebar renders `Add project`; the focused `Sidebar.test.tsx` hierarchy run passed and asserts no titlebar Add worktree. | PASS |
| Row action is accessible and targets its exact project | `IconButton` supplies `aria-label`/`title`; `Sidebar.tsx` labels each action `Add worktree to <workspaceId>` and passes `project`. Focused Sidebar tests passed 3/3, including the collapsed non-active row. | PASS |
| Refresh status action is absent | `WorktreeList.tsx` has no refresh prop or icon; focused WorktreeList behavior test passed 1/1. | PASS |
| Clean native startup exposes `orca-lite`, not `default` | App calls `getInitialProject`, canonicalizes only the legacy placeholder, and the four focused App behavior tests passed 4/4, including fresh startup and migration. | PASS |
| Exactly one canonical workspace root registration | `cmd_project_initial` plus `register_canonical_project` enforce canonical-root uniqueness; native project contract passed 13/13. | PASS |
| Non-active owner handles creation and opens result | App's creation handler retains `createTargetProject`, switches to `owner`, then opens the pending worktree when that owner's runtime has listed it. Focused App owner-flow tests passed. | PASS |
| Persisted implementation evidence | This evidence file, the implementation plan, notepad, and manual native QA checklist are stored under `.omo/`. | PASS |
| Full requested UI/build/Rust gates | See **Current shared-tree blockers** below. | BLOCKED outside feature scope |
| Native visual QA surface | See **Manual native QA blocker** below. | BLOCKED on user-run observation |

### Fresh focused commands and exits

- `bun x vitest run src/components/Sidebar.test.tsx -t "gives every project row its own add-worktree action bound to that exact project|drops the per-worktree status refresh control|hooks worktree creation and deletion into the owning project group"` — exit 0; 3 passed.
- `bun x vitest run src/components/WorktreeList.test.tsx -t "renders worktree dirty status without a manual refresh control"` — exit 0; 1 passed.
- `bun x vitest run src/App.test.tsx -t "uses the canonical folder-derived project on an empty native startup|migrates the legacy default placeholder to the canonical native project|opens worktree creation for the clicked non-active project|switches to the non-active project before opening its created worktree"` — exit 0; 4 passed.
- `bun x vitest run src/lib/tauri.test.ts -t "gets the native initial project without a request"` — exit 0; 1 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --test rorca_native_contract` — exit 0; 13 passed.
- `git diff --check` over the feature sources/tests and all review artifacts — exit 0. The separately changed `App.test.tsx` has unrelated trailing-whitespace findings.

### Current shared-tree blockers

These are not inferred from a proxy result; each was reproduced in the current working tree and traced to a separate, uncommitted change. They must not be silently changed as part of the sidebar task.

1. The complete focused UI command runs 68 tests but fails four unrelated App performance assertions. `ui/src/App.test.tsx` now asserts a specific lazy-import source spelling and no inline callback props, while the concurrently edited `ui/src/App.tsx` uses a valid `lazy(...).then(...)` named-export adapter and contains inline callbacks. These assertions do not exercise any sidebar hierarchy, startup-project, or worktree-owner behavior.
2. `cd ui && bun run build` currently exits 2 because the concurrently rewritten `ui/src/App.test.tsx` imports `jsdom` without declarations, has an unused `error` binding, and `ui/src/state/workspaceStore.test.tsx` also lacks the `jsdom` declaration. The same source directory otherwise has zero LSP errors; these test-file type errors are not caused by the sidebar/project implementation.
3. The full Rust suite reaches `src-tauri/tests/session_persistence_integration.rs::test_version_2_session_envelope_restore_preserves_multiple_workspaces` and exits 101. That newly added, uncommitted test writes a frontend version-2 envelope, but unchanged `src-tauri/src/session/mod.rs` deserializes required version-1 `sessionId`/`worktreePath` fields and rejects any version other than 1. A correct repair is a separate native session-v2 migration, not a sidebar change.

## Settled full validation

After concurrent work settled, the full required gates were rerun rather than inferred from earlier partial output:

- `cd ui && bun x vitest run src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx src/components/WorktreeList.test.tsx src/App.test.tsx src/lib/tauri.test.ts` — exit 0; **72 passed**.
- `cd ui && bun run build` — exit 0; TypeScript and Vite production build completed.
- `cargo test --manifest-path src-tauri/Cargo.toml` — exit 0; all unit, integration, native-contract, session-persistence, worktree-safety, and doc-test targets passed.

The Rust build emitted four pre-existing warnings in terminal/worktree internals (`unused Path`, unused `WriterLeaseGuard` and `acquire_writer_lease`); no warning comes from the sidebar/project-hierarchy files.

Final scoped `git diff --check` encountered one unrelated existing trailing blank line at `ui/src/App.test.tsx:1102`. No feature source, feature-specific test, Rust project contract, or persisted evidence file has a whitespace diagnostic. It was left untouched because that shared test file contains concurrent work outside this task.

## Manual native QA blocker

Native visual verification remains deliberately unperformed because the user prohibits the agent from driving desktop applications or injecting mouse/keyboard input. The exact five-step check and response format are stored in `.omo/reviews/sidebar-project-hierarchy-manual-qa-2026-08-22.md`. A user response of `manual QA PASS` completes this remaining visual gate; a failure report identifies the next product fix.

## Static diagnostics

- No diagnostics: `Sidebar.tsx`, `WorktreeList.tsx`, `src-tauri/src/ipc/project.rs`, `src-tauri/src/lib.rs`.
- The language server timed out before returning a fresh response for `App.tsx` and some UI test files. `bun run build` completed successfully after their final changes, providing the TypeScript validation for those files.

## Visual QA limitation and manual check

No native Tauri window was driven: the user explicitly prohibits desktop/UI automation. Please launch Ferryx from a clean local-storage profile and visually confirm:

1. Sidebar titlebar shows **Add project** and no global folder-plus/Add worktree action.
2. Each project row shows its dim folder-plus action at the right edge; hover/focus makes it fully visible.
3. Clicking a non-active project's action opens `Add Worktree · <that project>`.
4. Worktree cards have no refresh icon; delete remains available for non-primary worktrees.
5. The initial sidebar project is `orca-lite`, not `default`.

No QA server, browser context, terminal session, port, or temporary directory was started by this verification, so no runtime cleanup was required.
