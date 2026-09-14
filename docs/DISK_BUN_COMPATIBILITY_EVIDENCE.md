# Disk Bun compatibility evidence

Task: st_01a09af9. Scope: current C1 runner gap only.
Working directory for every command: `/Users/indo/code/project/orca-lite-wt/sa-worktree-disk`.

## Change and coverage

Test-only compatibility patch in `ui/src/components/WorktreeDeleteDialog.test.tsx`:
load the adjacent DOM setup and DOM matchers explicitly; replace Vitest-only
hoisting/module mocking with Tauri's public `mockIPC`/`clearMocks` boundary.
The existing native routing assertions still exercise the real dialog default
services, identity conversion, native wrappers and request serialization. No
injected replacement services are used in that integration case. All 11 tests
and their assertions remain. No module exports are mocked, avoiding shared Bun
module-cache contamination. Cleanup unmounts React before clearing IPC mocks.
Existing act/promise synchronization remains bounded by runner test timeouts;
no sleeps, polling, retries, skips or React private-state changes were added.
The existing disk suite's subprocess isolation is unchanged.

## Initial RED (before edits)

Command: `bun test --cwd ui src/components/WorktreeDeleteDialog.test.tsx`
Exit code: **1**. Full output:

```text
bun test v1.4.0 (34cbb9a40)

src/components/WorktreeDeleteDialog.test.tsx:

# Unhandled error between tests
-------------------------------
2 | import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
3 | 
4 | import type { BranchDeletionPreview, Worktree } from "../lib/types";
5 | import { WorktreeDeleteDialog, type WorktreeDeleteServices } from "./WorktreeDeleteDialog";
6 | 
7 | const native = vi.hoisted(() => ({
                      ^
TypeError: vi.hoisted is not a function. (In 'vi.hoisted(() => ({
  previewWorktreeDelete: vi.fn(),
  deleteWorktree: vi.fn(),
  deleteWorktreeDestructive: vi.fn()
}))', 'vi.hoisted' is undefined)
      at /Users/indo/code/project/orca-lite-wt/sa-worktree-disk/ui/src/components/WorktreeDeleteDialog.test.tsx:7:19
-------------------------------


 0 pass
 1 fail
 1 error
Ran 1 test across 1 file. [656.00ms]
```

## Intermediate validation

First single Bun run passed 11/11; first combined Bun and configured Vitest
runs each passed 31/31. Initial typecheck identified a patch-caused error in the
IPC handler; this was fixed by rejecting missing request envelopes (the API
accepts binary arguments too), not by casting or suppressing it.

Command: `ui/node_modules/.bin/tsc --project ui/tsconfig.json --noEmit`
Exit code: **2**. Full output:

```text
ui/src/components/WorktreeDeleteDialog.test.tsx(57,85): error TS2339: Property 'request' does not exist on type 'InvokeArgs'.
  Property 'request' does not exist on type 'number[]'.
ui/src/components/WorktreeDeleteDialog.test.tsx(58,70): error TS2339: Property 'request' does not exist on type 'InvokeArgs'.
  Property 'request' does not exist on type 'number[]'.
ui/src/components/WorktreeDeleteDialog.test.tsx(59,93): error TS2339: Property 'request' does not exist on type 'InvokeArgs'.
  Property 'request' does not exist on type 'number[]'.
```

## Final diagnostics

Tool: `lsp_diagnostics`
Arguments: `{"filePath":"/Users/indo/code/project/orca-lite-wt/sa-worktree-disk/ui/src/components/WorktreeDeleteDialog.test.tsx","severity":"all"}`
Full result: `No diagnostics found`. Tool has no process exit code.

## Final GREEN commands

Vitest caching is disabled so the existing node_modules symlink cannot cause
Vitest result-cache writes outside this worktree. Typecheck uses noEmit.

### `bun test --cwd ui src/components/WorktreeDeleteDialog.test.tsx`

Exit code: **0**. Full output:

```text
bun test v1.4.0 (34cbb9a40)

src/components/WorktreeDeleteDialog.test.tsx:
(pass) WorktreeDeleteDialog > gates destructive deletion after preview rejection [17.78ms]
(pass) WorktreeDeleteDialog > uses fresh dirty and unmerged preview instead of cached clean props [6.00ms]
(pass) WorktreeDeleteDialog > invalidates preview and refreshes current losses when safe deletion discovers new changes [9.17ms]
(pass) WorktreeDeleteDialog > shows branch safety metadata before safe deletion [6.43ms]
(pass) WorktreeDeleteDialog > contains an absolute hyphenated path and shows an explicit no-upstream divergence state [5.74ms]
(pass) WorktreeDeleteDialog > scopes native preview and safe deletion to the selected registered workspace [4.47ms]
(pass) WorktreeDeleteDialog > offers destructive deletion only for the UNMERGED_BRANCH error code [6.32ms]
(pass) WorktreeDeleteDialog > offers destructive deletion for the DIRTY_WORKTREE error code [6.43ms]
(pass) WorktreeDeleteDialog > names the files a destructive deletion will discard, with a count and a truncated remainder [7.18ms]
(pass) WorktreeDeleteDialog > omits the file listing when no dirty files were supplied [5.92ms]
(pass) WorktreeDeleteDialog > does not infer destructive deletion from an error message [3.60ms]

 11 pass
 0 fail
 36 expect() calls
Ran 11 tests across 1 file. [938.00ms]
```

### `bun test --cwd ui src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx src/components/WorktreeDeleteDialog.test.tsx`

Exit code: **0**. Full output:

```text
bun test v1.4.0 (34cbb9a40)

src/components/WorktreeDiskDialog.lifecycle.test.tsx:
(pass) WorktreeDiskDialog lifecycle > cancels its running scan and disposes the listener on unmount [11.02ms]
(pass) WorktreeDiskDialog lifecycle > disposes a listener registered after unmount without starting [2.50ms]
(pass) WorktreeDiskDialog lifecycle > cancels a start response arriving after unmount [2.39ms]
(pass) WorktreeDiskDialog lifecycle > keeps delayed old-workspace responses out of the replacement effect [5.17ms]
(pass) WorktreeDiskDialog lifecycle > serializes repeated refresh clicks while the start response is pending [12.88ms]
(pass) WorktreeDiskDialog lifecycle > preserves running events ahead of the start response [6.87ms]
(pass) WorktreeDiskDialog lifecycle > preserves completed events ahead of the start response [7.20ms]
(pass) WorktreeDiskDialog lifecycle > preserves cancelled events ahead of the start response [4.34ms]
(pass) WorktreeDiskDialog lifecycle > preserves failed events ahead of the start response [4.98ms]
(pass) WorktreeDiskDialog lifecycle > does not classify a cancelled response as failure [4.03ms]
(pass) WorktreeDiskDialog lifecycle > ignores regressive progress, terminal regressions and unrelated scan events [5.41ms]
(pass) WorktreeDiskDialog lifecycle > ignores superseded events while refreshing and after the new response [5.22ms]

src/components/WorktreeDiskDialog.test.tsx:
(pass) WorktreeDiskDialog > subscribes to scan progress BEFORE calling startScan [252.56ms]
(pass) WorktreeDiskDialog > displays scan progress while running and allows cancelling the scan [259.10ms]
(pass) WorktreeDiskDialog > displays every worktree row including size, last commit date, dirty state, and per-row error [262.64ms]
(pass) WorktreeDiskDialog > identifies cleanup candidates based on prunable status and unused days threshold [254.92ms]
(pass) WorktreeDiskDialog > surfaces structured errors including UNSUPPORTED for direct SSH projects [250.29ms]
(pass) WorktreeDiskDialog > wires cleanup flow using preview and warns before deleting a dirty worktree [278.06ms]
(pass) WorktreeDiskDialog > sorts worktree rows by apparent size descending by default [255.65ms]
(pass) WorktreeDiskDialog > removes the deleted worktree row from the list after deletion completes [275.78ms]

src/components/WorktreeDeleteDialog.test.tsx:
(pass) WorktreeDeleteDialog > gates destructive deletion after preview rejection [5.88ms]
(pass) WorktreeDeleteDialog > uses fresh dirty and unmerged preview instead of cached clean props [2.87ms]
(pass) WorktreeDeleteDialog > invalidates preview and refreshes current losses when safe deletion discovers new changes [4.12ms]
(pass) WorktreeDeleteDialog > shows branch safety metadata before safe deletion [3.29ms]
(pass) WorktreeDeleteDialog > contains an absolute hyphenated path and shows an explicit no-upstream divergence state [1.14ms]
(pass) WorktreeDeleteDialog > scopes native preview and safe deletion to the selected registered workspace [2.25ms]
(pass) WorktreeDeleteDialog > offers destructive deletion only for the UNMERGED_BRANCH error code [4.01ms]
(pass) WorktreeDeleteDialog > offers destructive deletion for the DIRTY_WORKTREE error code [5.49ms]
(pass) WorktreeDeleteDialog > names the files a destructive deletion will discard, with a count and a truncated remainder [3.34ms]
(pass) WorktreeDeleteDialog > omits the file listing when no dirty files were supplied [3.29ms]
(pass) WorktreeDeleteDialog > does not infer destructive deletion from an error message [6.31ms]

 31 pass
 0 fail
 78 expect() calls
Ran 31 tests across 3 files. [2.47s]
```

### `bun run --cwd ui test --cache=false src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx src/components/WorktreeDeleteDialog.test.tsx`

Exit code: **0**. Full output:

```text
$ vitest run --maxWorkers=1 "--cache=false" src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx src/components/WorktreeDeleteDialog.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite-wt/sa-worktree-disk/ui

 ✓ src/components/WorktreeDiskDialog.test.tsx (8 tests) 136ms
 ✓ src/components/WorktreeDeleteDialog.test.tsx (11 tests) 110ms
 ✓ src/components/WorktreeDiskDialog.lifecycle.test.tsx (12 tests) 113ms

 Test Files  3 passed (3)
      Tests  31 passed (31)
   Start at  22:37:11
   Duration  2.12s (transform 91ms, setup 178ms, collect 734ms, tests 360ms, environment 413ms, prepare 75ms)

```

### `ui/node_modules/.bin/tsc --project ui/tsconfig.json --noEmit`

Exit code: **0**. Full output:

```text
```

### `git diff --check`

Exit code: **0**. Full output:

```text
```

## Scope and cleanup receipt

- Only this test file and this evidence document were written by this task.
- No new helper, dependency/config/production edits, commits, desktop automation,
  daemon contact or process signals. No background checks remain running.
- All pre-existing dirty changes were preserved; the authorized earlier deletion
  test repairs remain underneath this compatibility patch.
- No manual application build was run: scope is a single test-domain repair;
  its real runnable surfaces were exercised with direct Bun and configured Vitest.
- No native monitor or delegation tool was exposed. Short checks ran via bash
  to completion without a polling runner.
- Relevant programming/Bun/debugging skills were not found in the exposed local
  skill directories; root AGENTS.md and local test setup/config were read.
- Historical overall PID/no-commit/preproduction RED failures remain unmet.
  This document does not claim those broader gates are closed.
- Changes remain uncommitted and vulnerable to concurrent worktree edits.

## Independent supervisor verification

The supervisor reviewed the current test and diff, including the real
default-service workspace request assertions, then independently executed the
three suites once per runner. Fresh LSP output: `No diagnostics found`.
Monitor: `mon_J9HWGBD4FF0A2YC2`; command session: `bash_314`.

Commands from the worktree root:

```sh
bun test --cwd ui src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx src/components/WorktreeDeleteDialog.test.tsx
bun run --cwd ui test --cache=false src/components/WorktreeDiskDialog.test.tsx src/components/WorktreeDiskDialog.lifecycle.test.tsx src/components/WorktreeDeleteDialog.test.tsx
ui/node_modules/.bin/tsc --project ui/tsconfig.json --noEmit --incremental false
git diff --check
```

Verbatim final summaries (intermediate Vitest progress rows omitted):

```text
 31 pass
 0 fail
 78 expect() calls
Ran 31 tests across 3 files. [3.50s]
C1_LEAD_BUN_EXIT=0

 Test Files  3 passed (3)
      Tests  31 passed (31)
   Start at  22:39:01
   Duration  2.22s (transform 92ms, setup 180ms, collect 738ms, tests 365ms, environment 423ms, prepare 77ms)

C1_LEAD_VITEST_EXIT=0
C1_LEAD_TSC_EXIT=0
C1_LEAD_DIFF_EXIT=0
```

The monitor completed with exit code 0. Unfiltered session output names all 31
Bun tests, including default-service workspace routing, and all three Vitest
files. These are filtered file runs, not a claim that all UI tests pass.

Cleanup: the command session exited, and a subsequent process query found no
matching dialog Bun/Vitest/typecheck processes. No server, tmux session or
temporary fixture directory was created by these commands. Cache was disabled
and typecheck emitted no files. Direct before/after process observations were:

```text
1010  Sun Sep 13 07:55:43 2026  /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon
21591 Sun Sep 13 19:27:52 2026  /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/src-tauri/target/debug/Ferryx.app/Contents/MacOS/ferryx --daemon
```

Neither daemon was contacted or signalled by the supervisor. This limited
before/after stability does not repair the original PID 36170 acceptance failure.
