# App notification integration follow-up

Task: `st_01a0777f`
Date: 2026-09-06
Status: Fixed in the notification integration fixture; ready for final reviewer.

## Reproduction and classification

Exact focused reproduction:

```sh
CI=1 bun run --cwd ui test src/App.notifications.test.tsx --reporter=dot
```

Before the fix: **10 passed, 1 failed, 1 unhandled rejection**. The closed-target test timed out at 5000 ms; it did not fail a navigation assertion. The unhandled rejection was the missing `getSystemPermissionsStatus` export in the test's complete `./lib/tauri` mock, reached by App's existing 1200 ms onboarding callback. Capture: `runtime-notifications-red.log`.

For comparison, temporary sibling files contained HEAD versions of App, workspaceRestore, and App.notifications.test, with only imports redirected to those temporary modules. Other dependencies remained in the working tree. No working file was replaced. The same CI-enabled notification suite passed **11/11** against those HEAD files; all temporary files were removed. Capture: `runtime-notifications-baseline.log`.

The extra failure is not independently pre-existing and is not a closed-target navigation regression. The new reactive restore-status subscription exposes a pre-existing fixture contract mismatch:

1. The real workspace store memoizes `restoreWorkspace` with `useCallback`.
2. The notification fixture instead returned a newly created `vi.fn()` for it on every render.
3. Restore-status updates now render App. The changing fixture callback invalidates App's restoration callback and the restoration effect, repeatedly cancelling/restarting restore.
4. The closed-target test cannot finish mounting. Once it exceeds 1200 ms, the separate, already-missing permissions mock export throws from onboarding.

The actual closed-session checks in App remain unchanged. The HEAD baseline passes because its restore-status map does not trigger the render that exposes the unstable mock.

## Minimal fix

Only `ui/src/App.notifications.test.tsx` was edited in this follow-up:

- Hoist/reset one stable `restoreWorkspace` mock, matching the real store's callback identity.
- Supply the missing typed `getSystemPermissionsStatus` IPC response for an all-granted test environment. The onboarding timer and error reporting are not disabled or intercepted.
- Keep all original closed-target assertions, including the two activation drains, no project switch to `other`, and no focus dispatch. Add an explicit assertion that the active project stays `default`.
- Control session loading and the activation queue with deferred promises. Await the exact load/drain-start signals, check restoration transitions from `loading` to `restored`, and require exactly one session load. No sleeps, polling, timeout extensions, skipped tests, or production notification refactors.

## GREEN verification

The exact focused command above now exits 0 with **11/11 tests passing and no unhandled errors**. Capture: `runtime-notifications-green.log`.

Related integrated check:

```sh
CI=1 bun run --cwd ui test \
  src/App.notifications.test.tsx \
  src/App.remote.test.tsx \
  src/state/workspaceRestore.test.tsx \
  src/lib/notificationActivation.test.ts \
  --reporter=dot
```

Result: **4 files, 47 tests passed**, exit 0, no unhandled errors. Capture: `runtime-notifications-related.log`.

LSP returned no diagnostics after the fixture changes. After the final one-line adjustment moving the restore-start await outside the render `act` block, fresh LSP requests twice timed out at the tool's 3000 ms limit; fresh final diagnostics are therefore unavailable, not claimed clean. Both final test commands above include that adjustment. `git diff --check` passed.

## Scope

No App/runtime production change was needed in this follow-up. The full UI suite was not rerun here. The known seven baseline failures (App updater, three tauri tests, three push tests) were neither modified nor suppressed. No unrelated files or tests were changed, and no commits were created.
