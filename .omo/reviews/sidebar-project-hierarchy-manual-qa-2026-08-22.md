# Manual native UI QA — sidebar project hierarchy

Run this in Ferryx after restarting the desktop app so the new native command handler is loaded. Do not preserve a screenshot of unrelated paths or terminal contents.

## Expected state

1. Start with a clean project-storage state, or remove the existing `default` placeholder through the app's normal project controls. The initial sidebar project label must be **`orca-lite`**, not `default`.
2. Inspect the sidebar titlebar: it has **Add project** (`+`) and does **not** have a global folder-plus / **Add worktree** action.
3. Inspect every project row: a dim folder-plus action appears at the right edge and becomes fully visible on hover or keyboard focus.
4. Click the action for a non-active project. The modal title must read **`Add Worktree · <that project>`**; it must not use the previously active project.
5. Inspect a normal worktree card: there is no refresh/status action. Delete remains available for a non-primary worktree.

## Result to report

Reply with one of:

- `manual QA PASS` if all five observations match; or
- `manual QA FAIL: <step number> — <what appeared>` if any does not.

The implementation has automated coverage for every listed behavior; this check solely confirms native desktop rendering and interaction under the actual app profile.
