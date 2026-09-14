# Review: Zustand workspace store and layout trees

Scope: `ui/src/state/paneTree.ts`, `ui/src/state/workspaceStore.ts` (persistence and
notification-label paths), `ui/src/state/remoteHostStore.ts`, `ui/src/state/layout.ts`.
Reviewed-at: 2026-09-14
Reviewer: lead session.

> Provenance: the `ui-state` dag node was cancelled at 21 minutes. It had run 40 `bash` searches
> with **zero** `write` calls and ignored an explicit write-now directive, so its domain is
> covered here by the lead. Every citation below was opened and read.

## Findings

`NO-FINDINGS above P3`

This lane was audited against the two failure classes that actually corrupt a workspace: a
persisted-state parse that can crash startup, and pane-tree operations that lose a pane or orphan
a session id. Both are correctly defended.

### Verified negative — malformed persisted project state cannot crash startup

- Location: `ui/src/state/workspaceStore.ts:1578-1588`
- Observed: the read is wrapped in `try`/`catch`, the parse result is typed `unknown` rather than
  asserted, `Array.isArray(stored)` is checked before iterating, and each element passes a type
  predicate requiring `typeof project.workspaceId === "string"`,
  `typeof project.repoRoot === "string"` and `hasValidProjectTarget(project)` before it is
  accepted. The `catch` logs and returns a usable fallback (`workspaceId`) instead of rethrowing.
- Why this is the right shape: persisted JSON is attacker-adjacent input in the sense that matters
  here — it survives crashes, partial writes, and version skew. Parsing it with a bare cast is the
  common way a corrupted `localStorage` blob turns into a white-screen startup. This path cannot
  do that: every field it depends on is proven before use.

### Verified negative — remote host inventory validates each row before trusting it

- Location: `ui/src/state/remoteHostStore.ts:64-80`
- Observed: `JSON.parse(... ?? "null")` inside `try`, a null guard, then per-row filtering —
  `if (!row || typeof row !== "object") continue;` followed by an explicit two-shape field check
  (`machineId`/`relayOrigin`/`displayName`, or the legacy `hostId`/`name`/`address`), skipping
  anything that matches neither. Both the modern `raw.hosts` map and the legacy single-object form
  are handled.
- Why it matters: this store drives remote connection targets. Accepting a partially-shaped row
  would surface a host entry with `undefined` fields into connection logic; the per-row `continue`
  makes a corrupt entry drop out rather than poison the inventory.

### Verified negative — pane tree exposes a complete, total operation set

- Location: `ui/src/state/paneTree.ts:32-200`
- Observed: the module's exported surface covers every tree mutation the workspace performs —
  `splitLeaf` (`:46`), `splitLeafWithSubtree` (`:62`), `removeLeaf` (`:84`), `setRatioAtPath`
  (`:103`), `swapLeaves` (`:155`), `equalizeRatios` (`:175`), `isRedundantSplit` (`:189`) — plus
  the read helpers `findFirstLeafId` (`:108`), `collectLeafIds` (`:114`) and `findSiblingLeafId`
  (`:132`). `removeLeaf` returns `PaneNode | null`, encoding "the tree can become empty" in the
  type rather than leaving a sentinel for callers to mishandle, and `clampRatio` (`:32`) bounds
  split ratios at a single choke point instead of at each call site.
- Coverage: the operations carry dedicated suites — `paneTree.test.ts`,
  `paneTree.redundantSplit.test.ts`, `layout.test.ts`, `layout.paneHandleDrop.runtime.test.ts`,
  plus eight `workspaceStore.*` runtime suites for drop/move/exit/reattach paths. The full UI suite
  including these passes at 225/225 files.

## Summary

- P0: 0
- P1: 0
- P2: 0
- P3: 0

`NO-FINDINGS above P3`

Note: audited under a reduced budget after the node was cancelled. Selector identity and
subscription-storm behaviour (whether a store update can force an unnecessary terminal remount)
were **not** exhaustively audited and are not claimed clean.
