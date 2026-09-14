# Disk scan lifecycle repair evidence

Date: 2026-09-13. Task: st_01a09a6a.
Worktree: `/Users/indo/code/project/orca-lite-wt/sa-worktree-disk`.

## Scope and mechanism

Read root AGENTS.md, initial git status/diff, dialog, tests, App caller,
Tauri scan wrappers/types, and backend scan registry/start/cancel implementation.
Initial tracked diff was empty. Foreign untracked scope documents and
`ui/node_modules` were preserved. A later foreign change to
`src-tauri/src/ipc/worktree.rs` was observed and left untouched.
All writes used apply_patch and were confined to this document and
WorktreeDiskDialog.tsx / its two test files. No commits were made.

Each effect owns its listener, pending start, accepted scan and cancellation.
Late listener registration disposes immediately. A late running start response
is cancelled even after unmount or workspace/service replacement. The response
identifies the owned scan: events arriving first are buffered by scan ID, then
merged without regressing counters or an absorbing terminal state. Other scan
IDs and old-effect callbacks cannot replace the owned scan. Starts are serialized.
Cancelled snapshot errors do not become failure errors. Teardown failures are
logged rather than silently swallowed. No deletion production code, backend,
service signatures or DTO declarations were changed by this task.

Ownership assumption: the running scan returned by startScan is owned for the
dialog lifetime, including a reused running scan (the contract has no created-vs-
reused flag). Terminal responses are not cancelled. With no sequence number in
the contract, ordering uses terminal absorption and monotonic progress counters.

## RED before production edits

Command, cwd `ui`:

```sh
bun test src/components/WorktreeDiskDialog.lifecycle.test.tsx
```

First execution: exit 1, 0 pass / 10 fail. Nine lifecycle cases reproduced
failures; the refresh case hit Bun's unsupported `vi.mocked` helper. Replaced
that test helper with direct service mock assignment, with production still
unchanged, and executed the same command through Python subprocess.run with
capture_output=True (printing failure/summary lines to avoid giant DOM dumps).

Confirmed RED output excerpts (exit code 1):

```text
(fail) cancels its running scan and disposes the listener on unmount
Expected: [ "ws-1", "a" ]
(fail) disposes a listener registered after unmount without starting
(fail) cancels a start response arriving after unmount
(fail) preserves running events ahead of the start response
(fail) preserves completed events ahead of the start response
Expected: false
Received: true
(fail) preserves cancelled events ahead of the start response
(fail) preserves failed events ahead of the start response
(fail) does not classify a cancelled response as failure
(fail) ignores regressive progress, terminal regressions and unrelated scan events
(fail) ignores superseded events while refreshing and after the new response
 0 pass
 10 fail
 9 expect() calls
Ran 10 tests across 1 file. [1401.00ms]
EXIT_CODE=1
```

Only after this RED execution was WorktreeDiskDialog.tsx patched. Immediate
execution of the same test command: exit 0, 10 pass / 0 fail, 29 expectations.
Then added workspace-replacement and start-serialization coverage. Existing
dialog cases were converted from waitFor/findBy polling to async act around
their concrete immediately resolving service fixtures. Regression tests control
listener/start promises and progress callbacks explicitly; pending service-call
signals are bounded by the runner's test timeout. No sleeps or polling waits.

## GREEN affected tests

Command, cwd `ui`:

```sh
bun test src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx
```

Output summary, exit 0, one execution of the combined target:

```text
bun test v1.4.0 (34cbb9a40)
 20 pass
 0 fail
 42 expect() calls
Ran 20 tests across 2 files. [3.41s]
```

All 12 lifecycle cases and all eight existing dialog cases passed, including
cancellation interaction, row display/sorting, structured SSH error, preview /
dirty-delete confirmation and removal of the deleted row. The existing Bun
per-case process isolation wrapper was retained.

Package-script runner cross-check:

```sh
bun run test src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx
```

```text
$ vitest run --maxWorkers=1 src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx
Test Files  2 passed (2)
     Tests  20 passed (20)
Duration  1.57s
```

Exit 0.

## Diagnostics and build

Before build: LSP diagnostics (severity all) on production and initial lifecycle
test returned `No diagnostics found`. After final test changes, production and
existing-test diagnostics again returned `No diagnostics found`; refreshed
lifecycle-test diagnostics timed out after 3000ms (inconclusive, not a pass).
Full TypeScript validation then succeeded before bundling:

```sh
bun x tsc --noEmit
```

```text
TSC_EXIT_CODE=0
```

To respect the write-only scope, exercised Vite bundling in memory rather than
writing dist or running a release build:

```sh
bun --eval 'import { build } from "vite"; await build({ build: { write: false } });'
```

```text
vite v6.4.3 building for production...
transforming...
1893 modules transformed.
rendering chunks...
built in 3.43s
VITE_IN_MEMORY_BUILD_EXIT_CODE=0
```

`git diff --check`: no output, exit 0.
`rg -n 'waitFor|findBy|setTimeout|sleep' ui/src/components/WorktreeDiskDialog*.test.tsx`:
no matches, exit 1 (expected no-match status).

## Coordination and checks not run

The sole BranchDeletionPreview fixture now supplies
`dirtyState: { isDirty: true, files: [{ statusCode: " M", path: "dirty-file.ts" }] }`
and `missing: false`. It is typed as an intersection so the fixture remains
compatible before and after the required DTO extension. Tests/typecheck/build
above ran before the deletion worker's DTO/UI changes landed; cross-worker
integration after their final changes is not claimed here.

Not run: desktop manual QA (reserved for the user), GUI/browser automation,
native/Tauri release builds, full UI suite, backend tests, runtime desktop scan
against a real filesystem, or desktop daemon operations. No daemon signals,
restarts, or operations against PIDs 1010/21591 occurred. The real React dialog
surface was exercised through DOM rendering and clicks with controlled service
promises, not a desktop application launch. No packaged application validation
is claimed. Changes remain uncommitted in a concurrently used worktree.
