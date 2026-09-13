# P28 RC-03: published worktree target correction

Task `st_01a09972`, 2026-09-13. Local source/test evidence only. Windows cross-worktree browser, real socket attachment and sibling-session runtime acceptance remain pending. Parent combined verification/build is not performed by this child.

## Allocation and contract

Read root/UI AGENTS, remaining-remote-callers RC-03, gap-packet-addendum P28, and parent `01a0983f-c995-753d-afa9-593f6d118788` goals.json C002 (updatedAt `2026-09-13T06:26:15.033Z`, including this exact full-file invocation and matrix registration).

Only the returned `ui/src/remote/RemoteApp.tsx`, initially clean `ui/src/remote/RemoteAttention.test.tsx`, and this report were edited. The existing RC-01/02 request identity, whole-request deadline, stale completion guards and unmount invalidation were preserved; final diff was inspected. Other shared-tree modifications were not edited or restored. No commit, branch/worktree operation, build, runtime/SSH/desktop operation or preferences/wheel/browser/native edit was performed.

Mechanism traced through RemoteApp waiting/gesture callbacks -> selectContext POST; RemoteSessionList tab normalization and regular tab-button target construction; server.rs workspace selection validation. Server matches published tab ID and `tab.worktree_slug.or(selection.worktree_slug)` against the requested worktree, rejecting mismatches with HTTP 400. Existing RemoteTerminal mock exposes the component callback seam through touch events; this does not certify the real gesture recognizer or terminal WebSocket.

`RemoteSessionList.tabItem` passes slug through `safeContextText`, which drops null, undefined, empty and whitespace-only values, then omits an absent slug. Thus an explicit empty primary slug is NOT preserved by the current normalization contract. The scoped correction does not change that read-only contract or serialization; it uses `??` (not truthiness) for target fallback, matching existing direct-tab callers. Null/empty raw target semantics are not newly claimed as runtime-tested. A future requirement to distinguish explicit empty from omission must address normalization and server/serialization together, outside this allocation.

## Minimal correction

- Previous and next swipe targets prefer the published tab worktree slug and label, with current-context fallback.
- Waiting targets do the same, including use of the resolved slug in the deduplication key.
- No request lifecycle or server validation weakening.

## Regression matrix and synchronization

Added six parameterized cases named `uses the published target worktree for swipe and waiting selection`: previous/next/waiting, each with published `targetB` distinct from active `currentA`, and omitted-target fallback to `currentA`. All nine original cases remain unchanged.

Each case asserts the exact single parsed POST `{ workspaceId: "ferryx-ui", worktreeSlug: targetSlug, tabId: "target-tab" }`. The fixture checks method/workspace/tab/worktree, returns 400 for mismatched published pairs rather than unconditional acceptance, and only publishes the target state after acceptance. A matching event triggers the actual model refresh/confirmation path. Final assertions require exact target session, selected target tab, enabled former-current tab (pending released), and no second POST; optimistic session display alone cannot pass.

Initial state, socket construction, request receipt, response release and confirmation-state read signals are created before the relevant render/action/event. Async React work is flushed with act. A two-second failure-only deadline bounds missing signals; there are no new sleeps, polling, waitFor or findBy calls. The pre-existing nine tests retain their pre-existing waiting patterns as explicitly required. Cleanup in finally clears the deadline, unmounts, and settles the owned response gate; existing afterEach clears DOM/storage/socket reference and restores globals. No external sockets or processes are acquired by the test.

## Execution ledger

Exact command throughout:

```sh
bun run --cwd ui test src/remote/RemoteAttention.test.tsx
```

1. Initial fixture attempt: 6 failed / 9 passed, missing-boundary deadline. Incorrect state URL and render-inside-async-act prevented setup completion. This is a fixture failure, NOT behavioral RED. Corrected to actual `/api/v1/workspace/state` and synchronous render before awaiting subscribed signals, before product edits.
2. Behavioral RED on unchanged RC-03 product: exit 1, **3 failed / 12 passed**, 15 tests total. All published-target cases failed the identical POST equality assertion; all omitted-target controls and original cases passed. Output:

```text
FAIL ... ('previous', 'published')
FAIL ... ('next', 'published')
FAIL ... ('waiting', 'published')
Expected worktreeSlug: "targetB"
Received worktreeSlug: "currentA"
Tests  3 failed | 12 passed (15)
```

3. Minimal product fix: exit 0, 15/15 passed. Diagnostics then identified six TS2550 errors from Promise.withResolvers being outside the configured TypeScript library. Replaced only that deferred construction with a local typed Promise helper; assertions and synchronization remained unchanged. No target/library setting changed or error suppressed.
4. Final full-file GREEN: exit 0, **15/15 passed**, one file, no filter/retry within the run. Final output:

```text
$ vitest run --maxWorkers=1 src/remote/RemoteAttention.test.tsx
RUN v3.2.7 /Users/indo/code/project/orca-lite/ui
PASS src/remote/RemoteAttention.test.tsx (15 tests) 202ms
Test Files  1 passed (1)
Tests  15 passed (15)
Start at  15:31:27
Duration  1.37s
```

Final language-server diagnostics on both changed TSX files: **No diagnostics found**. `git diff --check -- ui/src/remote/RemoteApp.tsx ui/src/remote/RemoteAttention.test.tsx` emitted no errors. Lead read both complete raw logs, then moved them with apply_patch into `p28-target-worktree-red.log` and `p28-target-worktree-green.log` beside this report. The original `/tmp/st_01a09972-{red,green}.log` paths are absent.

Final source SHA256:

```text
4f890367fd3859f4454c82375df76c87f9b09d2afa2fd70c55fce40ddeb61966  ui/src/remote/RemoteApp.tsx
891577f551d5aa1b6ed78dfd2a26a4f8c291ef9b179a76bfc72352e7315cb397  ui/src/remote/RemoteAttention.test.tsx
```

## Handoff and remaining acceptance

Uncommitted shared-tree changes remain vulnerable to concurrent writers; files are returned to lead ownership. No temporary config or mutation loader was created. Actual Windows browser previous/next/waiting cross-worktree behavior, gateway acceptance, exact selected socket, and untouched sibling-session receipts remain required; this report does not close C002, P28 runtime acceptance or the parent goal.

## Lead combined selection verification

```sh
bun run --cwd ui test src/remote/RemoteAttention.test.tsx src/remote/RemoteUI.test.tsx src/remote/RemoteTerminal.contract.test.tsx src/remote/RemoteRouting.test.tsx src/remote/RemoteReconnect.test.tsx
```

`mon_CH8WDE9H1ZDMNWE5` / `bash_46`: exit 0, all five files and 130
tests passed, 4.91s, start 15:35:04. Full output read. Per-file counts:
Attention 15, UI 50, Terminal 58, Routing 6, Reconnect 1. Lead diagnostics
on RemoteApp and RemoteAttention are clean; whole-tree diff check exited 0.
The test uses mocked terminal callbacks and transport, not native gesture,
real gateway or actual socket attachment proof. Nine pre-existing attention
cases retain legacy polling; the six new cases use explicit signals.
The next P27 preference and P32 history repairs run in disjoint files;
the registered build is deferred to their combined verification, avoiding
a build against actively edited source. The previous 722-case/build result
is historical, not a build result for this new increment.
