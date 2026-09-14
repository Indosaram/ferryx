# Review: terminal library modules

Scope: `ui/src/lib/terminalEvents.ts` (the global PTY multiplexer and 512 KiB replay backlog),
`ui/src/lib/terminalOutput.ts`, `ui/src/lib/nativeTerminalLifecycle.ts`,
`ui/src/lib/nativeTerminalAttachPolicy.ts`, `ui/src/lib/terminalTransport/`.
Reviewed-at: 2026-09-14
Reviewer: lead session.

> Provenance: the `ui-lib-terminal` dag node failed with `"Model returned an empty response
> twice"` (transient — gateway re-probed 200/3.0s, zero stream-budget hits, 16 tool calls). Its
> domain is covered here by the lead. Every citation below was opened and read.

> Stale-map note: this lane's original prompt named `ui/src/lib/terminalOutputScheduler.ts` and
> `ui/src/lib/terminalHostManager.ts`, both of which the repo's root `AGENTS.md` lists in its CODE
> MAP as high-reference symbols. **Neither file exists** (`rg` → `No such file or directory`).
> The knowledge base is out of date; a reviewer trusting it would have reported on files that are
> not there. Recorded as a P3 documentation defect below.

## Findings

### [P1] Replay backlog is never released when the last output listener unsubscribes — FIXED

- Location: `ui/src/lib/terminalEvents.ts:150-154` (the `subscribeOutput` teardown closure),
  against the retention guard at `:313-315`
- Observed: the retention rule is stated by the code itself at `:313` — *"Only retain a replay
  backlog while an output listener is attached to this session. Unconsumed native sessions must
  not accumulate bytes"* — and enforced on the write side at `:315`
  (`if (data.byteLength > 0 && (this.outputListeners.get(sessionId)?.size ?? 0) > 0)`).
  The unsubscribe path did not honour the other half of it:

  ```ts
  const current = this.outputListeners.get(sessionId);
  current?.delete(listener);
  if (current?.size === 0) this.outputListeners.delete(sessionId);
  // backlog left intact
  ```
- Why it is wrong: once the last listener detaches, the retained chunks can never be replayed to
  anyone — nothing reads them again — yet up to `MAX_BACKLOG_BYTES` (`:14`, 512 KiB) stays
  resident **per session** for the lifetime of the app. `clearSession` (`:189`) does delete the
  backlog, but it only runs on real session teardown; closing a pane, switching tabs, or
  unmounting a terminal host hits the unsubscribe path instead, which is the common case. A long
  session that opens and closes many panes accumulates dead buffers with no ceiling across
  sessions and no way for the user to reclaim them short of restarting.
- **RED:** `bun run --cwd ui test src/lib/terminalEvents.bus.test.ts` →
  `AssertionError: expected { sessions: 1, chunks: 1, chars: 4096 } to deeply equal { sessions: +0, chunks: +0, chars: +0 }`,
  `Tests 1 failed | 8 passed`. Log: `/tmp/ulw-massreview/backlog-red.log`. Test added first:
  `releases the replay backlog when the last output listener unsubscribes`.
- **Fix:** delete the backlog entry inside the same `size === 0` branch that already drops the
  listener set, so the write-side guard and the teardown agree.
- **GREEN:** `bun run --cwd ui test src/lib` — see `/tmp/ulw-massreview/backlog-green.log`.

### Verified negative — the backlog trim loop cannot spin or over-trim

- Location: `ui/src/lib/terminalEvents.ts:325-337`
- Observed: `while (entry.totalBytes > MAX_BACKLOG_BYTES && entry.chunks.length > 0)` with two
  exits — a whole-chunk drop (`shift()`, decrement) when `oldest.byteLength <= overflow`, and a
  partial-chunk trim (`entry.chunks[0] = oldest.subarray(overflow)`) that **always** `break`s.
- Why it is sound: the partial branch is the one that could loop forever if it failed to make
  progress or forgot to exit; it does both correctly. The `oldest === undefined` guard plus the
  `chunks.length > 0` conjunct make the empty case unreachable. Byte accounting is decremented on
  both paths, so `totalBytes` cannot drift above the cap.

### Verified negative — listener maps are pruned on unsubscribe

- Location: `ui/src/lib/terminalEvents.ts:150-154`, `:157-165`
- Observed: both `subscribeOutput` and `subscribeReplayGap` delete their per-session `Set` once
  it empties, rather than leaving an empty `Set` keyed by a dead session id. `clearSession`
  (`:188-194`) additionally clears the decoder registry, title carry, and last-title maps.
- Note: this correct pruning is exactly what made the backlog omission stand out — every
  sibling map is released on the same path, and only `backlog` was missed.

### [P3] Root `AGENTS.md` CODE MAP cites two files that do not exist

- Location: repository root `AGENTS.md`, CODE MAP rows for `terminalOutputScheduler`
  (`ui/src/lib/terminalOutputScheduler.ts`) and `terminalHostManager`
  (`ui/src/lib/terminalHostManager.ts`), both marked "High" reference count
- Observed: `rg`/`wc` on both paths returns `No such file or directory`. The actual terminal
  library surface is `terminalEvents.ts`, `terminalOutput.ts`, `terminalThroughputMetrics.ts`,
  `nativeTerminal*.ts`, and `terminalTransport/`.
- Why it matters: the knowledge base is the first thing an agent or new contributor reads. Citing
  removed files sends reviewers to dead paths and invites fabricated findings about code that is
  not in the tree.
- Minimal fix: refresh the CODE MAP rows to the modules listed above. Not done in this pass —
  it is a docs change outside this run's defect scope, recorded so it is not lost.

## Summary

- P0: 0
- P1: 1 (fixed, RED→GREEN captured)
- P2: 0
- P3: 1 (recorded, not fixed)

Note: audited under a reduced budget after the node failed. The transport layer
(`terminalTransport/tauriTransport.ts`, `remoteTransport.ts`) and the attach-policy state machine
were **not** exhaustively audited and are not claimed clean.
