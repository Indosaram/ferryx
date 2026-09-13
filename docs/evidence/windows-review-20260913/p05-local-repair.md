# P05 PR #3 bounded local repair

Task st_01a0994d, 2026-09-13. Local source/wiring repair only; C002 is not a full-target GREEN or Windows runtime acceptance receipt.

## Scope and source

Read root/UI instructions, original loop brief at `.omo/ulw-loop/01a0983f-c995-753d-afa9-593f6d118788/brief.md`, `pr-review.md`, and P05 in `repair-packets.md`. App files were clean at entry. Changed only App.tsx, App.test.tsx and this receipt. Preserved newer confirmation-aware handleClosePane/handleCloseTab; no shortcut, production store/type, browser capability WIN-UI-13, or unrelated close policy change.

Production diff at App.tsx:1814:

```diff
-    if (activeTab?.kind === "terminal" && activeLayout) {
+    if (activeTab && activeTab.kind !== "browser" && activeLayout) {
```

Both the real App shortcut handler and App-registered onCloseTabMenu callback reach handleCloseActiveSurface. Its explicit-terminal equality incorrectly sent omitted-kind tabs to whole-tab close. The corrected existing-tab/non-browser discriminator forwards them to the unchanged confirmation-aware pane handler, including leaf-root layouts and fallback focus. Browser and nonexistent-tab guards remain intact.

Read-only store trace: closePane delegates leaf-root close to closeTab (which protects pinned tabs); split close dispatches CLOSE_PANE and disposes an unreferenced selected session. The test below does NOT execute those store behaviors.

## Mounted production App coverage

Added 28 cases, retaining all 100 existing cases:

- Both routes: Windows Ctrl+W DOM KeyboardEvent through real shortcut registration, and invocation of the actual callback supplied by mounted App to the native subscription mock.
- Each route has eight tagged/untagged x pinned/unpinned x split/unsplit cases. Focus is leaf-2 rather than first leaf for splits; unsplit uses null activeLeafId to exercise fallback.
- Each route checks working/waiting focused-agent confirmation, cancellation, then confirmed pane dispatch; busy sibling alone does not cause confirmation.
- Each route checks pinned/unpinned browser whole-tab behavior even with a leftover terminal layout, and nonexistent active tab with leftover layout.

The Windows fixture sets navigator.platform/userAgent and process.platform because production detection has a Darwin process fallback. All descriptors are restored after each case and App is unmounted before restoration. No shortcut implementation is mocked. Ctrl events must be defaultPrevented.

Readiness is subscribed before render using onCloseTabMenu registration; await its actual handler with a 2000ms failure deadline, cleared in finally. No polling, waitFor, or sleeps were added. Existing unrelated tests retain their original waits and assertions.

This is explicitly mocked-store wiring coverage. A closePane spy proves neither sibling PTY survival nor actual pinned leaf-root retention. No store allocation was requested or edited, and no real-store coverage is claimed.

## Exact RED and post-fix execution

Registered identical command, run once before and once after production correction:

```sh
bun run --cwd ui test src/App.test.tsx
```

Captured stdout/stderr in task-owned `/tmp/st_01a0994d-red.log` and `/tmp/st_01a0994d-green.log` respectively. The shell wrapper redirected output, printed its tail and propagated the command exit status.

RED, start 14:48:30, duration 4.52s, exit 1:

```text
$ vitest run --maxWorkers=1 src/App.test.tsx
Test Files  1 failed (1)
Tests  15 failed | 113 passed (128)
```

Fourteen intended new failures: eight untagged routing cases and two busy-sibling cases reported:

```text
AssertionError: expected "spy" to be called once with arguments: [ 'tab-1', 'leaf-2' ]
Number of calls: 0
```

The four focused-agent cases reported unexpected whole-tab dispatch:

```text
AssertionError: expected "spy" to not be called at all, but actually been called 1 times
Number of calls: 1
```

Post-fix, start 14:49:07, duration 4.29s, exit 1:

```text
$ vitest run --maxWorkers=1 src/App.test.tsx
Test Files  1 failed (1)
Tests  1 failed | 127 passed (128)
```

All 28 P05 cases passed with unchanged assertions. This is regression GREEN only, NOT an overall command GREEN. Both runs also failed the existing unrelated test:

```text
FAIL src/App.test.tsx > App project workspace flow > checks for a signed update when the native app starts
AssertionError: expected "spy" to be called once, but got 0 times
await waitFor(() => expect(updater.checkForUpdate).toHaveBeenCalledOnce());
error: script "test" exited with code 1
```

Read-only diagnosis: App imports startUpdatePolling; the test partial-mocks exported checkForUpdate, while the actual startUpdatePolling calls its module-local checkForUpdate. No updater/test repair attempted within this close-routing scope. Lead must resolve the existing target failure before claiming full C002 GREEN.

After execution, a diagnostics-only fixture correction replaced an asserted inline tab array with an inferred local tab object, avoiding excess-property inference for pinned. Runtime values and assertions are identical; this final type-only adjustment was not followed by another test run.

## Diagnostics, cleanup and pending acceptance

- App.tsx language server: no errors/warnings, two existing async-conversion hints at 1102 and 1109.
- App.test.tsx language server: initially flagged the fixture pinned inference, then no diagnostics after the local-variable correction.
- `git diff --check -- ui/src/App.tsx ui/src/App.test.tsx`: exit 0.
- Source/test diff: 139 insertions, 1 deletion (138 test lines, one production line replaced).
- No broad tests/build, daemon, desktop, SSH, runtime launch, branch/worktree or commit. No user processes touched. Temporary output logs retained for lead inspection; readiness deadline cleared, mounts cleaned and platform descriptors restored.
- Changes are uncommitted in the shared tree; foreign changes preserved.

Lead ownership update: the lead exclusively owns `ui/src/state/workspaceStore.test.tsx` and reports preregistration of eight tagged/pinned/first-second survivor cases using a deferred close service. This child did not edit that file or execute/verify those cases; they remain separate from the mounted App wiring evidence above.

Pending: full-target updater failure resolution and lead combined validation; independent original-behavior mutation by lead; real Windows debug GUI selected-PTY exit and fresh SAME sibling PID/backend-session output, active-agent confirmation runtime evidence; final PR #3 disposition. Focused browser capability WIN-UI-13 is a separate increment, not implemented here.
