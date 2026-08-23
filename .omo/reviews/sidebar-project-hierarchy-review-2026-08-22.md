# Sidebar project hierarchy review

Date: 2026-08-22

## Verdict

**FAIL — the checked-out implementation does not satisfy the requested hierarchy or the user-visible initial project name.**

This is a read-only review. No production source was changed as part of it.

## Review contract

| Required behavior | Result | Evidence |
| --- | --- | --- |
| Only **Add Project** is top-level | FAIL | `ui/src/components/Sidebar.tsx:170` still renders a titlebar `Add worktree` button, alongside `Add project` at line 173. |
| **Add Worktree** appears to the right of every project name | FAIL | The project row in `ui/src/components/Sidebar.tsx:201-249` contains only the disclosure control and project-name button. No per-row worktree action is rendered. `WorktreeList` only exposes `Create the first worktree` when a project has zero worktrees (`ui/src/components/WorktreeList.tsx:39-54`). |
| Clicking a project-row worktree action targets that project | PARTIAL / unobservable | App plumbing correctly accepts an optional project and stores it as `createTargetProject` (`ui/src/App.tsx:610-613`); the dialog receives it (`ui/src/App.tsx:692-704`). The real Sidebar never renders the required row action, so this path is not reachable from the requested UI. |
| Fresh initial project displays the repository folder ID rather than `default` | FAIL | Frontend still seeds `DEFAULT_PROJECT` with `DEFAULT_WORKSPACE_ID` (`ui/src/App.tsx:52`), whose literal remains `"default"` (`ui/src/lib/tauri.ts:19`). `loadProjects()` falls back to that object (`ui/src/App.tsx:762-773`) and no project-list IPC exists to return the backend-derived ID. |

## Verified findings

### P1 — top-level Add Worktree violates the requested hierarchy

`ui/src/components/Sidebar.tsx:170-172` renders:

```tsx
<IconButton label="Add worktree" className="no-drag" size="sm" onClick={() => onCreateWorktree()}>
```

This is explicitly a top-level action. The adjacent `Add project` action at `:173-175` makes the violation visible in the same titlebar.

### P1 — there is no project-row Add Worktree affordance

The project row begins at `ui/src/components/Sidebar.tsx:201`. Its only controls are:

- disclosure toggle at `:213-223`;
- project-name/select button at `:224-248`.

The `onCreate={() => onCreateWorktree(project)}` callback at `:263` is passed to the nested `WorktreeList`, not to a button at the right of the project name. It is visible only for a project with no worktrees.

### P1 — folder-name derivation never reaches the initial frontend project

The Rust backend does derive and register a folder name at `src-tauri/src/lib.rs:127-150`. However, it also registers the same repository as `"default"` at `:152-154`.

The frontend independently persists and re-registers `DEFAULT_PROJECT` using `workspaceId: "default"`; it never asks the backend for the derived registry ID. Therefore a clean frontend storage state still renders `default`, not `orca-lite`.

### P2 — regression tests lock in the opposite behavior

`ui/src/components/Sidebar.test.tsx:81-100` describes the titlebar as containing the global actions and explicitly queries/clicks `Add worktree` at line 95. The same test file enumerates `Add worktree` as a titlebar action at lines 276-280.

The App mock has a synthetic `Add worktree to {project.workspaceId}` button (`ui/src/App.test.tsx:162-168`), but that is not the real Sidebar and no test clicks it. It proves the dialog state plumbing only, not the requested row-level control.

## Verification evidence

- `lsp_diagnostics` reported **no diagnostics** for `ui/src/components/Sidebar.tsx`, `ui/src/App.tsx`, and `src-tauri/src/lib.rs`.
- Focused source and test inspection establishes the requirement failures above.
- Independent QA completed these clean commands before this report was finalized:
  - `cd ui && bun x vitest run src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx src/components/WorktreeList.test.tsx` — exit 0, 25 tests passed.
  - `cd ui && bun x vitest run src/App.test.tsx` — exit 0, 18 tests passed.
  - `cd ui && bun x tsc --noEmit` — exit 0.
  - `cd ui && bun run build` — exit 0.
- Those green checks are **not approval evidence**: `Sidebar.test.tsx` currently asserts the global Add Worktree behavior that the user asked to remove. The QA reviewer confirmed no native Tauri-window automation was run, so this report does not claim desktop visual evidence.

## Security and architecture notes

- The Rust folder-name sanitizer only emits alphanumeric, `_`, and `-` characters before workspace registration, so this particular derivation does not introduce a path traversal primitive.
- Registering both `derived_id` and `default` aliases the same repository in `WorkspaceRegistry`. Besides failing the label requirement, it creates duplicate workspace identities for remote/project enumeration and makes cross-project state ambiguous.
- Security review verdict: no exploitable traversal, argument-injection, or cross-project write primitive was found. The duplicate alias remains a medium-severity integrity issue: it can create separate `orca/<derived-id>/...` and `orca/default/...` worktree namespaces for one repository and makes remote active-workspace selection order-dependent.
- The `createTargetProject` plumbing should be retained when the row action is added; it is the correct mechanism for opening a worktree dialog against a non-active project.

## Required remediation

1. Remove the global titlebar `Add worktree` button.
2. Add an `Add worktree to <project>` icon button as a sibling of the project-name button in every project row, invoking `onCreateWorktree(project)`.
3. Make the startup project identity a single folder-derived value visible to the frontend. The frontend must not seed/re-register `default` for a new workspace; either expose the backend-derived project through typed IPC or derive a matching value from the canonical registered root.
4. Remove the duplicate backend `default` registration once startup compatibility has a deliberate migration path.
5. Replace tests that assert the global worktree action with real-Sidebar tests scoped to a project row and assert that the callback receives that exact project. Add a clean-storage startup test proving `orca-lite` is displayed.

## Independent review synthesis

Five independent tracks reached the same result:

- Goal/spec review: 2/3 requirements missed; backend derivation is not user-visible.
- QA review: targeted tests, typecheck, and build are green but assert the wrong layout; no native desktop automation was performed.
- Quality review: confirmed the row action is unreachable and untested. It also identified a latent follow-on defect: creating in a non-active project currently refreshes and opens through runtime/store hooks bound to `activeProject`, so the completion path needs revalidation once the intended row action becomes reachable.
- Security review: no exploitable path-traversal, argument-injection, or cross-repository write route. Duplicate `default`/derived IDs are medium-severity state-integrity and remote-enumeration defects.
- History/context review: `docs/audits/workspace-sidebar-titlebar-layout.md` currently documents the inverse layout (global Add Worktree only). That documentation must be corrected or removed with the implementation, otherwise it will reintroduce the rejected design.

The review does not approve the current implementation. It recommends a fresh implementation pass followed by a real Sidebar DOM test, an App-level non-active-project creation test, a clean-storage startup-name test, and desktop visual confirmation by the user.
