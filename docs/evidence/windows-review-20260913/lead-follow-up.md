# Windows review: integration conflict and QA harness follow-up

Date: 2026-09-13. Status: source review only; no production changes,
test execution, Windows GUI evidence, commit, or push in this follow-up.

## Integration boundary

The initial audit snapshot had 23 foreign modified tracked files. A later
`git status --short` and `git diff --stat` showed 30 modified tracked files,
391 insertions and 40 deletions. These totals describe concurrent work,
not this review's implementation.

New overlap with PR #3:

- `ui/src/App.tsx` now has a foreign change routing terminal leaf-root
  close through `handleClosePane`, adding active-agent confirmation, and
  forwarding confirmed pane/tab closure separately.
- `ui/src/App.test.tsx` has accompanying foreign tests and fixture changes.
- `ui/src/components/ConfirmCloseTabDialog.tsx` has foreign pane/tab and
  active-agent confirmation changes.
- The changed close handler still checks `activeTab?.kind === "terminal"`.
  PR #3's omitted-kind case is therefore distinct from the new leaf-root
  and confirmation behavior.

The foreign root-cause report
`docs/CMD_W_TAB_CLOSE_ROOT_CAUSE_2026-09-12.md` describes a user requirement
for pane-first closure regardless of root type. Its historical macOS
store/test evidence is not current Windows evidence for this review.

Before integration, inspect the latest branch and working diff again,
coordinate any overlapping work, and preserve the foreign changes.
Do not copy the current shared files into an isolated branch as if they
were authored or verified by this review. Worktree/branch creation and
removal permission remains pending.

## QA helper findings

Correct path: `script/qa/win-daemon-e2e.mjs`. The attempted plural
`scripts/qa/win-daemon-e2e.mjs` read returned ENOENT.

The complete helper was read, including its final cleanup and entry point.
Source-confirmed mechanisms:

1. `request` compares a pending entry's `resolve` against the original
   promise resolver when a timeout fires. The stored resolver is a wrapper,
   so this comparison cannot remove that request. A later response can
   consume the stale entry rather than the next request.
2. Every parsed message consumes the oldest pending request, even an
   unsolicited output message. The control/stream protocol must distinguish
   responses from events rather than rely on their arrival timing.
3. An attach failure rejects without closing the client acquired earlier.
   Stream close clears waiter timers without settling outstanding promises.
4. Cleanup suppresses close-request errors. This is not a receipt proving
   that an owned PTY was removed.

These findings affect the reliability of evidence. They do not establish
that production Windows PTY input or output is broken.

## Required harness regression packet

Proposed target, not yet created or executed:

```sh
bun test script/qa/win-daemon-e2e.test.mjs
```

The same assertions must fail against the old production helper seam and
pass after its repair. Use an owned loopback fixture peer and subscribe to
each exact event before triggering it. The test must use the real helper
client, not a copied implementation.

- A timed-out request is removed; the next valid request resolves to its
  own response. Inject the deadline callback deterministically rather than
  rely on a fixed sleep.
- An unsolicited output event reaches its listener while leaving an
  outstanding handshake/request pending until its matching response.
- Rejected handshake or attach closes the owned connection, observed by
  the fixture peer's close event.
- Closing a stream settles pending marker waits and removes their timers.
- A failed PTY close is reported as incomplete cleanup, not a PASS receipt.

No test result is recorded here. The synthesis worker received these
mechanisms and the proposed repair scope for inclusion in the per-finding
criteria before implementation.

## Current acceptance state

`omo-agent-toolkit ulw-loop status --session-id
01a0983f-c995-753d-afa9-593f6d118788 --json` returned exit code 0,
goal in progress, and C001, C002, C003 pending with no captured evidence.
The audit-synthesis task `st_01a09851` was running at the status read.
New overlap and harness findings were successfully delivered to that task.

Required implementation, RED/GREEN, current Windows debug GUI scenarios,
aggregate checks, cleanup receipts, final gate, PR dispositions, and
remote-SHA verification remain open. None is satisfied by the audit
reports or by the existence of the QA helper.
