# Manual agent reset outcome repair evidence

Task: st_01a09a6b. Worktree: `/Users/indo/code/project/orca-lite-wt/sa-watchdog`.
Verified on 2026-09-13, Darwin arm64, Vitest 3.2.7.

## Contract and scope

- A single reset keeps frontend activity until `tauriIpc.resetAgentState` fulfills.
  IPC rejection propagates unchanged; rejection does not dispatch `RESET_AGENT_STATE`.
- Worktree resets attempt every matching session and await all settlements. Only
  successful sessions clear activity. Any failure rejects with an Error whose
  message reports failed/total counts and whose `results` carries each frontend
  `sessionId`, settlement `status`, and rejection `reason` or fulfillment `value`.
- Tab resets likewise await all sessions, then emit exactly one error notification
  if any failed, or one success notification if all succeeded. Worktree handlers
  catch the aggregate rejection and route it to error, never unconditional success.
- Backend session IDs remain the IPC target; frontend IDs remain the reducer target.
  No backend, process lifecycle, manual release, or HOLD semantics were changed.
  Backend fulfillment is the authority; backend-internal events/fallbacks are not
  reinterpreted by this frontend repair.

Read the worktree instructions and clean tracked diff before edits. Existing
untracked scope documents and the node_modules symlink were left untouched.
All authored edits used apply_patch, limited to the store, App, its existing
notification test file, and this evidence file. No commit was created.

## Deterministic behavioral proof

Tests live in `ui/src/App.notifications.test.tsx`, named `manual reset outcome`.
They mount the real store with restored sessions, inject controlled IPC promises,
and pass its real reset methods into a rendered App. The captured TerminalSplitView
and Sidebar callbacks are the actual App outcome boundary. Unrelated child rendering
and native subscriptions are mocked; the reset store/reducer path is not mocked.

The two store cases assert actual rejection for single and grouped calls, pending
activity retention, failure retention, and clearing successful peers. The grouped
case additionally checks machine-readable per-session outcomes and original reasons.
Ten App cases cover tab/worktree surfaces with single success/failure and grouped
success/partial failure/total failure. Assertions check notification routing/counts,
backend calls, activity presence, and stable backend identity, not notification prose.
Controlled promises are created before triggering resets; tests await the returned
operation, bounded by Vitest's test timeout. No sleeps or polling helpers were added.

### RED before production changes

Working directory for all commands below: the worktree's `ui` directory.

```sh
bun run test --no-cache src/App.notifications.test.tsx -t 'manual reset outcome'
```

Behavioral RED at 20:01:09: **exit 1**, 8 failed, 4 passed, 29 filtered out.
Production files had not yet been edited.

```text
store rejection propagates (grouped=false/true):
  expected 'fulfilled' to be 'rejected'
six tab/worktree single/partial/total failure notification cases:
  expected "spy" to be called +0 times, but got 1 times
Tests  8 failed | 4 passed | 29 skipped (41)
RED_EXIT_CODE=1
```

Two prior executions of that exact command exited 1 during fixture setup, not
behavioral proof: first the pre-existing App IPC mock lacked native title event
exports; then newly added vi.fn subscription implementations were reset by test
setup. Providing plain async no-op subscription functions fixed the harness before
the behavioral RED above. No production edits were made during those corrections.

### GREEN after production changes

The identical focused command at 20:02:10: **exit 0**.

```text
Tests  12 passed | 29 skipped (41)
GREEN_EXIT_CODE=0
```

The final fixture was then strengthened with a valid split tree and exact
machine-readable aggregate-result assertions. The full related command below
passed both before and after that strengthening, without production changes:

```sh
bun run test --no-cache src/App.notifications.test.tsx src/state/workspaceStore.test.tsx src/state/workspaceActivity.test.tsx src/components/TabBar.test.tsx src/components/WorktreeList.test.tsx
```

Final run at 20:03:01: **exit 0**.

```text
workspaceStore.test.tsx       50 passed
App.notifications.test.tsx   41 passed
workspaceActivity.test.tsx   16 passed
TabBar.test.tsx              20 passed
WorktreeList.test.tsx        18 passed
Test Files  5 passed (5)
Tests  145 passed (145)
FINAL_GREEN_EXIT_CODE=0
```

```sh
bun x --no-install tsc --noEmit --incremental false
```

**Exit 0**, including after the final test strengthening; no compiler diagnostics.
LSP diagnostics initially reported no diagnostics on all three changed TS/TSX
files. A fresh test-file LSP request after strengthening timed out at 3000 ms;
the full TypeScript compiler check above verified the final source instead.
`git diff --check` also exited 0 before writing this evidence.

## Verification boundary

## Supervising-session independent verification

The supervising session inspected the production diff and independently executed:

```sh
cd /Users/indo/code/project/orca-lite-wt/sa-watchdog/ui
bun run test --no-cache src/App.notifications.test.tsx src/state/workspaceStore.test.tsx src/state/workspaceActivity.test.tsx src/components/TabBar.test.tsx src/components/WorktreeList.test.tsx
```

Captured monitor `mon_YWCANZR6NAWPKNYQ`, session `bash_300`:

```text
Test Files  5 passed (5)
Tests  145 passed (145)
WATCHDOG_LEAD_TEST_EXIT=0
watcher completed (exit code 0)
```

The supervisor's subsequent App.tsx LSP error check returned
`No diagnostics found`. This is frontend regression evidence, not desktop
manual QA or a repair of the historical acceptance failures.

## Verification boundary (continued)

App was executed in jsdom through its real callback boundary, not through native
menus or a live daemon. No GUI automation, daemon actions, release builds, or
backend tests were run. Vitest used `--no-cache` because the existing node_modules
symlink points outside this worktree. Results establish frontend outcome propagation
for backend fulfillment/rejection, not new guarantees about backend HOLD behavior.
