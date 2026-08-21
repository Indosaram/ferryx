# Orca Architecture Findings — Layout, Panes, Tabs, Sidebar

Source of truth: read-only inspection of the shipped Orca bundle at `ui/original-dist/assets/*.js`
(unminified, original identifiers preserved), cross-referenced against the current rorca
implementation in `ui/src` and the reference screenshot at
`/var/folders/zh/.../clipboard-2026-08-21-141049-56982340.png`.

Every claim below cites the bundle file and line where the behavior is implemented. Nothing here is
inferred from the UI alone.

---

## 0. Executive summary — the one thing that matters

Orca has **two independent, nested binary layout trees**. rorca currently has neither.

| Level | Tree | Keyed by | Leaf payload | Owner scope | Implementation |
|---|---|---|---|---|---|
| **Outer** | Tab-group layout | `layoutByWorktree[worktreeId]` | `groupId` | one per **worktree** | React (`SplitNode`) |
| **Inner** | Terminal pane layout | `terminalLayoutsByTabId[tabId]` | `leafId` (UUID) | one per **tab** | imperative DOM (`.pane-split`) |

The outer tree splits the *window* into tab-group panels, each with its own tab strip. The inner
tree splits *one tab's body* into terminal panes, which share a single tab strip. They are separate
data models, separate resize handles, and separate drag systems.

**Yes — each tab has its own split tree.** `terminalLayoutsByTabId` is a record keyed by tab id
(`store-CgXrfmaH.js:3707, 17077, 23728, 29212, 48762`). Splitting panes in tab A has no effect on
tab B.

rorca's `ui/src/state/layout.ts` models a *single global* `{ primaryTabId, secondaryTabId, split,
nestedSplit }`. That is one shared split for the whole app, at most 2–3 panes, with no per-tab
ownership and no tree. It cannot represent the reference screenshot. This is the central refactor.

### Reference screenshot — measured, not eyeballed

Pixel analysis of the 1392×962 capture:

| Region | X range | Color | Maps to |
|---|---|---|---|
| Sidebar | 57–291 | `#2a2a2a`, selected rows `#353535` | `--worktree-sidebar` / `-accent` |
| Titlebar | y 39–104 | `#171717` | `--card` |
| Tab strip | y 105–160 | `#0a0a0a` + colored tab labels | `--terminal` |
| Left pane | 292–813 (w 522) | `#0a0a0a` | `--terminal` |
| **Divider** | **x 814 (1px)** | **`#1b1b1b` (27,27,27)** | pane divider |
| Right pane | 815–1335 (w 521) | `#0a0a0a` | `--terminal` |

Decisive detail: the divider spans **y 160→887 only** — the full terminal body, but it stops
*below* the tab strip (which ends at y 160) and does not cut through it. Both panes sit under **one
shared tab strip**. Ratio is exactly 522/1043 ≈ **0.5**.

That is an **inner (per-tab) pane split**, not an outer tab-group split. An outer split would carve
the tab strip in two as well, since each group panel renders its own strip. This screenshot is the
canonical target for rorca and it is squarely the per-tab pane tree.

---

## 1. Tabs, split panes, and layout trees

### 1.1 The outer tree: tab-group layout (per worktree)

Node shape (`store-CgXrfmaH.js:11925-11941`):

```ts
type TabGroupLayoutNode =
  | { type: "leaf"; groupId: string }
  | { type: "split";
      direction: "horizontal" | "vertical";
      first: TabGroupLayoutNode;
      second: TabGroupLayoutNode;
      ratio?: number }   // default 0.5, absent when 0.5
```

Stored as `layoutByWorktree: Record<WorktreeId, TabGroupLayoutNode>` (threaded through
`adoptGrouplessTabs` / `hydrateUnifiedFormat`, `store-CgXrfmaH.js:11180-11218`; empty sentinel
`EMPTY_LAYOUT_BY_WORKTREE` at `23411`).

A `TabGroup` is the unit that owns a tab strip:

```ts
type TabGroup = {
  id: string;              // createBrowserUuid()
  worktreeId: string;
  activeTabId: string | null;
  tabOrder: string[];      // authoritative ordering
  recentTabIds: string[];  // MRU, drives close-focus fallback
}
```

(shape visible in `adoptGrouplessTabs`, `store-CgXrfmaH.js:11190-11196`.) Companion records:
`groupsByWorktree`, `activeGroupIdByWorktree`, `unifiedTabsByWorktree` (`11181-11218`).

**Terminology note:** `direction: "horizontal"` means the split axis runs horizontally — children
are placed **side by side** (`flexDirection: row`). See `insertPaneNextTo`
(`terminal-appearance-D3oO-Ew5.js:7365-7371`): `isVertical = zone === "left" || "right"` →
`flexDirection = "row"`, class `is-vertical`. The DOM class and the data-model `direction` use
**opposite** conventions. Pick one convention in rorca and never mix them; the bundle's own
serializer bridges them at `store-CgXrfmaH.js:20399`.

Core tree operations, all pure and directly portable:

| Function | Line | Behavior |
|---|---|---|
| `buildSplitNode(existingId, newId, direction, position)` | `11925` | Wrap two leaves; `position` decides which side is new; `ratio: .5` |
| `replaceLeaf(root, targetGroupId, replacement)` | `11942` | Structural substitution |
| `updateSplitRatio(root, path, ratio)` | `11950` | Path is `("first"\|"second")[]` |
| `findFirstLeaf(root)` | `11967` | Leftmost descent |
| `findSiblingGroupId(root, targetGroupId)` | `11998` | Sibling lookup for merge |
| `removeLeaf(root, targetGroupId)` | `12004` | **Collapses parent into surviving sibling** |
| `collectLayoutGroupIds(node, set)` | `11158` | Leaf enumeration |
| `pruneTabGroupLayoutForGroups(root, validIds)` | `11219` | GC against live groups |
| `layoutSpanningGroups(groups, existing)` | `11166` | Append orphan groups as right-hand splits |

`removeLeaf` is the important one: closing a group does not leave a stub — the parent split is
replaced by the surviving sibling, so the tree self-normalizes and no degenerate one-child splits
can exist.

Node addressing uses a **dot path string**, not indices: `setTabGroupSplitRatio(worktreeId,
nodePath, ratio)` walks `nodePath.split(".")` over `first`/`second`
(`store-CgXrfmaH.js:13056-13066`). So the root is `""`, its right child is `"second"`, and that
child's left child is `"second.first"`. This makes ratios addressable without holding node
references, which is what allows persistence.

### 1.2 The inner tree: terminal pane layout (per tab)

Node shape (`serializePaneTree`, `store-CgXrfmaH.js:20373-20404`; `synthesizeDegenerateLayout`,
`23371-23390`):

```ts
type TerminalLayoutNode =
  | { type: "leaf"; leafId: string }          // leafId is a UUID
  | { type: "split";
      direction: "horizontal" | "vertical";
      first: TerminalLayoutNode;
      second: TerminalLayoutNode;
      ratio?: number }                        // omitted when within 0.005 of 0.5
```

Persisted snapshot per tab:

```ts
type TerminalLayoutSnapshot = {
  root: TerminalLayoutNode | null;
  activeLeafId: string | null;
  expandedLeafId: string | null;              // zoom/maximize one pane
  ptyIdsByLeafId: Record<string, string>;     // leafId -> PTY id
}
```

Stored as `terminalLayoutsByTabId: Record<TabId, TerminalLayoutSnapshot>`
(`store-CgXrfmaH.js:3707`; serializer `serializeTerminalLayout` at `20405-20416`;
`ptyIdsByLeafId` accessed at `17077, 23728, 29212, 31604, 33380, 48762, 48897`).

**Pane key** — the global address of a single terminal pane
(`makePaneKey` `store-CgXrfmaH.js:15043`, `parsePaneKey` `15048-15059`):

```ts
makePaneKey(tabId, leafId) => `${tabId}:${leafId}`
// tabId MUST NOT contain ":"; leafId MUST be a UUID
parsePaneKey(key) => { tabId, leafId, stablePaneId }
```

The `tabId` cannot contain `:` precisely so this key is unambiguously parseable
(`isValidTerminalTabId`, `store-CgXrfmaH.js:11152-11154`). Agent status is indexed by this key
(`agentStatusByPaneKey`), so per-pane agent state comes free once pane keys exist.

`leafId` is a stable UUID (`mintStablePaneId`), deliberately decoupled from the ephemeral numeric
`paneId` used by the DOM manager (`data-pane-id`). The stable id survives restart and replay; the
numeric id does not.

### 1.3 Layout repair and hydration

Persisted layouts are never trusted. `resolveTerminalLayoutRoot`
(`store-CgXrfmaH.js:23391-23395`) uses a three-tier fallback:

1. `authoritativeRoot` if `layoutCoversLeaves(root, leafIds)` — tree leaves are **exactly** the live
   leaf set, bijectively (`23365-23370`).
2. else `existingRoot` under the same test.
3. else `synthesizeDegenerateLayout(leafIds)` (`23342-23360`) — folds leaves into a left-leaning
   chain of horizontal splits.

Duplicate leaf ids are detected by counting and rewritten to fresh UUIDs
(`store-CgXrfmaH.js:20213, 20259-20268`), because a duplicated `leafId` would alias two panes onto
one PTY. Pane close re-maps retained leaves via `retainedLeafIdByRemovedLeafId`
(`20055-20125`) so scrollback buffers survive a neighbor closing.

**Port this.** Any tree that is persisted and restored alongside independently-lived PTYs will
desync; the coverage check plus degenerate fallback is the cheap, correct repair.

---

## 2. Per-tab split views — confirmed

Direct evidence that split state is per-tab, not global:

1. `terminalLayoutsByTabId` is keyed by tab id (`store-CgXrfmaH.js:3707` and ~12 call sites).
2. Focus is addressed by the `(tabId, leafId)` pair, not by a global pane index —
   `activateTabAndFocusPane(tabId, leafId, opts)` sets the active tab, then dispatches
   `FOCUS_TERMINAL_PANE_EVENT` with `{ tabId, leafId }` on the **next animation frame** so the
   target tab has mounted (`activate-tab-and-focus-pane-dvS5VCkm.js:12-30`).
3. Closing a tab disposes exactly that tab's PTYs:
   `for (const ptyId of Object.values(s.terminalLayoutsByTabId?.[tabId]?.ptyIdsByLeafId ?? {}))
   addDoomedPtyId(ptyId)` (`store-CgXrfmaH.js:17077`), and `omitByTabId` drops the entry
   (`17216`).
4. Per-tab side records are all keyed the same way: `paneTitlesByTabId`, `launchDraftByTabId`,
   `runtimePaneTitlesByTabId` (`23955`).

So: switching tabs swaps the entire pane tree. A tab with 4 panes and a tab with 1 pane coexist,
each retaining its own ratios, active pane, and expanded pane.

**Split entry points**

- Event-driven: `requestActiveTerminalPaneSplit(detail)` dispatches
  `"orca-request-active-terminal-pane-split"` (`request-active-terminal-pane-split-So9AiZw3.js`;
  event name at `terminal-C-BGupDh.js:5`). Decouples menu/keybinding/context-menu callers from the
  imperative pane manager.
- Feature telemetry marks `"terminal-pane-split"` on first use
  (`Terminal-qm6WvB4Q.js:1722`), and a contextual tour teaches it via `{terminal.splitRight}`
  (`store-CgXrfmaH.js:32476`).

**Pane expand/zoom** is first-class: `expandedLeafId` in the snapshot (`20405-20414`). One pane
fills the tab body while the tree is preserved intact.

---

## 3. Pane movement, reordering, drag handles, resize handles

There are **three distinct interaction systems**. Do not conflate them.

### 3.1 Inner pane divider — imperative, flex-based

`createDivider(isVertical, styleOptions, callbacks)`
(`terminal-appearance-D3oO-Ew5.js:376-391`; hit sizing `getDividerHitSize` at `373-375`):

```js
divider.className = `pane-divider ${isVertical ? "is-vertical" : "is-horizontal"}`;
const hitSize = (styleOptions.dividerThicknessPx ?? 4) + 6;   // visual + 6px grab margin
isVertical ? (divider.style.width = hitSize, cursor = "col-resize")
           : (divider.style.height = hitSize, cursor = "row-resize");
divider.style.flex = "none";
divider.style.position = "relative";
```

Note `getDividerHitSize` (`373-375`): the hit target is always **6px larger** than the painted
thickness. The visible line in the screenshot is 1px; the grab zone is not. Thickness is user
configurable, clamped 1..32 (`store-CgXrfmaH.js:1148`, `Settings-yKTVxZPa.js:4604`).

`attachDividerDrag` (`terminal-appearance-D3oO-Ew5.js:216-366`) — the full drag contract:

- **Pointer capture** on `pointerdown`, plus **capture-phase window listeners** for
  `pointermove`/`pointerup`/`pointercancel`/`blur` (`236-249`). The window listeners are the
  safety net: iframes, webviews, and xterm canvases otherwise swallow pointer events.
- **Measures once** at drag start: `prevSize` and `totalSize` from `getBoundingClientRect`
  (`296-303`). No per-frame layout reads.
- **rAF-coalesced writes** via `createDividerFlexFrameScheduler` (`181-215`) — at most one
  flex mutation per frame, with `flush()` on commit and `cancel()` on abort.
- **Clamping**: `MIN_PANE_SIZE = 50` px (`179`), and critically
  `effectiveMinPaneSize = Math.min(MIN_PANE_SIZE, totalSize / 2)` (`328`) so the clamp degrades
  gracefully instead of deadlocking in containers under 100px.
- **Sizing model**: `el.style.flex = `${size} 1 0%`` on both neighbors. Pixel sizes as flex-grow
  weights, so the ratio survives container resize for free.
- **Abort restores**: `pointercancel` / window `blur` call `finishActiveDrag(false)`, which
  cancels the scheduler and restores `prevInitialFlex` / `nextInitialFlex` (`262-268`).
- **Double-click resets to 50/50**: both neighbors set to `1 1 0%`, then refit
  (`340-348`).
- **PTY resize throttling**: `holdPtyResizesForPaneSubtrees([prevEl, nextEl])` suppresses PTY
  `SIGWINCH` churn during the drag and flushes once on commit (`309, 284-286`).
- Terminals are refit on commit via `refitPanesUnder` (`279-281`), never mid-drag.
- Cleanups are tracked in a `WeakMap` (`dividerDragCleanups`) and released by `disposeDivider`
  (`367-374, 392-394`).

### 3.2 Outer tab-group resize handle — React

`ResizeHandle({ direction, onResizeStart, onRatioChange })`
(`Terminal-qm6WvB4Q.js:2439-2511`; `SplitNode` follows at `2512`). Same physics, different
bookkeeping:

- Finds neighbors as `handle.previousElementSibling` / `nextElementSibling` (`2451-2453`).
- **Ratio-based, not pixel-based**: `ratio = (clientX - rect.left) / rect.width`, clamped to
  **`MIN_RATIO = .15` / `MAX_RATIO = .85`** (`2437-2438, 2465-2467`).
- A **`ResizeObserver`** on the container re-reads `rect` during the drag (`2459-2462`) —
  handles sidebar toggles / window resize mid-gesture.
- Writes `flex` directly during drag for 60fps, then commits **once** on pointerup via
  `onRatioChange(draggedRatio)` → `setTabGroupSplitRatio` (`2472-2473, 2478`). Store is not
  touched per frame.
- Listens on the **handle itself** (it holds pointer capture), and additionally handles
  `lostpointercapture` (`2493-2496`).
- Guards re-entrancy with `activeResizeCleanupRef`, and cleans up on unmount (`2442-2447`).
- Class: `tab-group-split-resize-handle is-vertical|is-horizontal` + `is-dragging` (`2508`).

Note the **inverted class naming** vs. the data model: `isHorizontal ? "is-vertical" :
"is-horizontal"`. Same trap as §1.1.

### 3.3 Drag-to-move: tabs and panes (dnd-kit)

Tab drag is `@dnd-kit` based: `useTabDragSplit({ worktreeId, enabled })`
(`rename-file-JaIUF221.js:4309`), with `SortableTab`, `SortableContext`, `DragOverlay`,
`useDroppable` exported at `5272`. Robustness details worth copying:

- Custom `TabDragPointerSensor` with a distance activation constraint
  (`getTabDragActivationDistance`) so clicks aren't swallowed (`4322`).
- A **missed-`dragEnd` fallback**: `pointerup`/`pointercancel`/`blur`/`focus` listeners force
  `clearDragState` on a 0ms timeout if dnd-kit never fired end (`4332-4350`). Necessary because
  webview/native drags steal events.
- `preDragActivationSnapshotRef` restores the previously active tab if the drag is aborted.

**Drop zone geometry** — `resolveDropZone(rect, point)` (`rename-file-JaIUF221.js:4149-4159`):

```js
edgeWidthThreshold  = rect.width  * 0.1;
edgeHeightThreshold = rect.height * 0.1;
splitWidthThreshold = rect.width / 3;

// inset 10% on all sides => reorder within group, no split
if (inside inset box) return "center";
if (localX < w/3)      return "left";
if (localX > 2*(w/3))  return "right";
return localY < rect.height/2 ? "up" : "down";
```

So the middle 80%×80% is "move into this group"; the outer frame splits, with left/right thirds
winning over up/down. `"center"` is explicitly excluded from split handling
(`Terminal-qm6WvB4Q.js:2638, 2798`).

Separately, `resolvePaneColumnEdgeZone(panelRect, point, options)`
(`rename-file-JaIUF221.js:4160-4179`) uses a **20% edge band** and — importantly — returns `null`
when the pointer is over the tab strip (`tabStripHeightPx ?? 32`), so dragging across the strip
never triggers a body split.

Drop overlays are rendered into `document.body` via `createPortal` at `z-[10001]`, sized by
`getOverlayBounds(rect, zone)` to the exact half/edge that will be occupied
(`Terminal-qm6WvB4Q.js:2396-2436`).

**No-op suppression** — `isPaneColumnSplitDropNoOp` (`store-CgXrfmaH.js:11131-11136`):
a drop is rejected if (a) source === target and the source has ≤1 tab, or (b) the source is a
single-tab group already adjacent on that exact side. Adjacency uses
`findLayoutSiblingOnSplitSide` / `getDirectLayoutSiblingOnSplitSide` (`11113-11130`; direction
mapping at `11115-11122`), which maps
`direction: "horizontal"` + `"right"` → `second`, `"vertical"` + `"down"` → `second`, and so on.
This is what stops the tree from thrashing into an identical shape.

**Pane movement in the DOM** — `insertPaneNextTo(source, target, zone, callbacks)`
(`terminal-appearance-D3oO-Ew5.js:7359-7408`):

1. Snapshot target's `flex`/`minWidth`/`minHeight`.
2. Create `.pane-split`; inherit target's flex if the parent is a split, else `100%`/`100%`.
3. Create the divider.
4. **Dispose WebGL on both panes**, remembering whether each had it (`7385-7388`) — reparenting a
   canvas loses the GL context.
5. `parent.replaceChild(split, targetContainer)`; append `[source, divider, target]` or the
   reverse per `sourceFirst = zone === "left" || zone === "top"`.
6. On the **next animation frame**, re-attach WebGL (if it was on and not disabled after context
   loss) and refit both panes (`7400-7407`), guarded by `isDestroyed()`.

**Pane removal** — `detachPaneFromTree` (`7346-7358`) finds the sibling, removes the container,
removes dividers, then `promoteSibling` (`7409-7429`) hoists the sibling into the grandparent —
restoring `100%`/`100%` sizing if the grandparent is the root, or inheriting the dead parent's flex
if it is another split. Identical invariant to `removeLeaf` in the outer tree.

**Equalize** — `equalizePaneSplitSizes(root)` (`7458-7477`) recursively assigns
`flex: ${weight} 1 0%` where `getEqualizeWeight` (`7453-7457`) counts same-direction descendant
leaves.
This makes "equalize panes" produce genuinely equal panes across nesting, not just equal siblings.

**DOM → model serialization** — `serializePaneTree` (`store-CgXrfmaH.js:20373-20404`) reads the
live DOM back into the persisted tree, deriving `ratio` from `flex` values and **omitting it when
within 0.005 of 0.5** (`20390-20394`), rounded to 3 decimals. Since the DOM is authoritative during
drags, this is how the store stays in sync without per-frame writes.

### 3.4 Group merge

`mergeGroupIntoSibling(worktreeId, groupId)` (`store-CgXrfmaH.js:13038-13054`): resolve sibling via
`findSiblingGroupId`, move every tab in `tabOrder` with
`moveUnifiedTabToGroup(..., { recordInteraction: false })`, then `closeEmptyGroup`. Preserves tab
order; the layout collapse falls out of `removeLeaf`.

### 3.5 Focus and fit helpers

`pane-helpers-9eOmrw__.js`:

- `focusActivePane(manager)` — **refuses to steal focus** if a tab-rename input is open
  (`[data-tab-rename-input="true"]`) or if the active element is a genuine editable
  (`shouldPreserveEditableFocus`). Crucially, elements inside `.xterm` or with class
  `xterm-helper-textarea` are **not** treated as protected editables, so terminal→terminal focus
  moves still work.
- `fitAndFocusPanes` = `fitPanes` then `focusActivePane`. Fit precedes focus so xterm measures
  against final geometry.

---

## 4. Projects and worktrees in the sidebar

### 4.1 Row model — a flat, virtualized row list

The sidebar is **not** a recursive React tree. A pure function flattens the hierarchy into a linear
array of typed rows, which is then virtualized. Row types
(`worktree-list-virtual-rows-B1jWEldM.js:9-18`):

```
host-header | header | lineage-group | imported-worktrees-card
| new-external-worktrees-inbox | pending-creation | folder-workspace | item
```

Stable keys per row (`getRenderRowKey`, `:9-18`) — `host:`, `hdr:`, `lineage-group:`,
`imported:`, `inbox:`, `pending:`,
`folder-workspace:`, `wt:` prefixes. Fixed row-height estimates drive the virtualizer
(`GROUP_HEADER_ROW_HEIGHT = 28`, `FOLDER_WORKSPACE_ROW_HEIGHT = 64`, item `116`,
`PENDING_CREATION_ROW_HEIGHT = 56`, gap `6`) (`:3-52`).

**Sticky headers** are index-based, with two independent tiers: host headers always stick;
group headers stick only at `projectGroupDepth === 0` (`getStickyHeaderIndexes`, `:66-74`). A
handoff algorithm (`getActiveStickyIndexesForScroll`, `:76-105`) offsets the group header by 36px
when a host header is pinned above it. Nested project groups deliberately **do not** stick — only
top-level ones do.

### 4.2 The hierarchy

`buildRows(...)` (`worktree-activation-BDsaiyMf.js:1366`) with `groupBy: "repo" |
"pr-status" | "workspace-status"`. In `"repo"` mode the nesting is:

```
projectGroup (recursive, parentGroupId)          depth 0
├─ folder-workspace rows                         groupDepth = depth + 1
├─ repo/project headers                          projectGroupDepth = depth + 1
│   ├─ imported-worktrees card
│   ├─ new-external-worktrees inbox
│   ├─ pending-creation rows
│   └─ worktree items                            depth = lineage depth
│       └─ nested child worktrees (lineage)      depth + 1
└─ child projectGroups (recurse)                 depth + 1
```

`appendProjectGroup(projectGroup, depth)` (`:1626-1650`) is the recursive driver;
`appendOrderedGroups(groups, projectGroupDepth)` (`:1503-1580`) emits repo headers and their
contents. Top-level entry: `for (const g of childGroupsByParentId.get(null) ?? [])
appendProjectGroup(g, 0)` (`:1652`). Repos whose `projectGroupId` is dangling are re-appended at
depth 0 (`:1653-1659`) — orphans are never dropped.

Project groups form their own tree via `parentGroupId`, indexed into `childGroupsByParentId`
(`:1608-1614`), sorted by `tabOrder` then name (`:1615`). Header counts are **subtree** counts:
`getProjectGroupSubtreeCount` sums direct repos + folder workspaces + all descendants recursively
(`:1621-1625`).

### 4.3 Independent open/close state — one flat Set

This is the key mechanism, and it is deliberately simple:

```ts
collapsedGroups: Set<string>
```

(`store-CgXrfmaH.js:34410`.) Collapse is **not** a boolean on each node. It is one flat set of
string keys, and every collapsible thing mints its own namespaced key:

| Key | Producer | Line |
|---|---|---|
| `project-group:<id>` | `getProjectGroupHeaderKey(groupId)` | `worktree-activation:1044-1046` |
| `repo:<repoId>` | repo section key | `:1477, 1505` |
| `lineage:<worktreeId>` | `getLineageGroupKey(worktreeId)` | `:1064-1066` |
| `pinned` | `PINNED_GROUP_KEY` | `:1047` |
| `all` | `ALL_GROUP_KEY` | `:1055` |
| `pr:<group>` / workspace-status keys | group key builders | `:1489-1495` |

Because keys are namespaced and independent, **collapsing a project does not touch its worktrees'
own lineage collapse state** — expanding it later restores exactly the prior sub-state. That is the
"independent folder open/close state" requirement, and it is achieved with a `Set<string>`, not a
recursive state tree.

Toggle + persistence (`store-CgXrfmaH.js:34410-34416`):

```js
collapsedGroups: new Set(),
toggle: (key) => {
  const next = new Set(s.collapsedGroups);
  /* add/delete key */
  window.api.ui.set({ collapsedGroups: [...next] }).catch(console.error);
  return { collapsedGroups: next };
}
```

Serialized as an **array** (`collapsedGroups: []`, `3661`, `34329`), rehydrated as a Set
(`new Set(ui.collapsedGroups ?? [])`, `34660`). Collapse is UI state, persisted separately from
session/layout state.

Rendering honors it by simply not emitting children: `const isCollapsed = collapsedGroups.has(key)`
then `if (!isCollapsed) { ...emit children... }` (`:1505, 1546`), and the same guard inline at the
project-group level (`:1639`). Collapsed subtrees cost zero rows — which is what keeps the
virtualizer cheap.

### 4.4 Worktrees nested under projects — lineage

`appendWorktreeRows(result, worktrees, repoMap, lineageById, worktreeMap, options)`
(`worktree-activation-BDsaiyMf.js:1167-1222`):

- If `nestLineage` is false: flat emit at `depth: 0` (`:1169-1182`).
- If true: build `childrenByParentId` from `getLineageRenderInfo`, keeping only edges whose parent
  is **visible in the current filter** (`lineage.state === "valid" && visibleIds.has(parent.id)`,
  `:1186-1193`). Filtered-out parents promote their children to roots rather than hiding them.
- `emit(worktree, depth, lineageTrail, isLastChild)` recurses, guarded by an `emitted` Set against
  cycles (`:1195-1216`), and stops descending when `collapsedGroups.has(lineage:<id>)` (`:1212`).
- Roots = worktrees that are nobody's child (`:1217`). If the graph is fully cyclic and yields no
  roots, everything is emitted flat as a fallback (`:1219-1221`) — no silent row loss.
- `cyclicLineageIds` is passed in so known cycles are pre-broken.

Each row carries the data needed to draw tree guides without measuring the DOM
(`buildWorktreeRow`, `:1150-1166`):

```ts
{ rowKey, sectionKey, depth, groupDepth,
  lineageTrail: boolean[],     // per ancestor level: does a vertical rail continue?
  isLastLineageChild: boolean, // draw elbow instead of tee
  lineageChildCount: number,
  lineageCollapsed: boolean,
  lineageGroupKey?: string,    // present only when lineageChildCount > 0
  hostContextLabel }
```

`lineageTrail` is the standard `tree`-style rail encoding: index `i` says whether an ancestor at
depth `i` has further siblings, i.e. whether to paint a continuing vertical line at that column.

Two independent depth axes coexist: `groupDepth` (project-group nesting) and `depth` (worktree
lineage). Indentation must be a function of both.

Ordering: `orderMainWorktreeFirst(group.items)` in repo mode (`:1556`), and pinned worktrees are
hoisted into a synthetic `pinned` group ahead of everything (`emitPinnedGroup`, `:1081-1126`) with
row keys `pinned:<worktreeId>` — the same worktree can therefore appear under two `sectionKey`s,
which is exactly why `rowKey` is `${sectionKey}:${id}` and not just the id.

---

## 5. Data models and component architecture for rorca

### 5.1 Gap analysis

| Concern | Orca | rorca today | Verdict |
|---|---|---|---|
| Pane layout | binary tree per tab | `{primary, secondary, split, nestedSplit}` global | **replace** |
| Split scope | per tab | one global split | **replace** |
| Pane count | unbounded | 2 (+1 nested) | **replace** |
| Pane identity | UUID `leafId` + `tabId:leafId` | `Pane = {id, tabId}` unused | **replace** |
| Resize | — | none found in `layout.ts` | **add** |
| Pane drag/move | dnd-kit + zones | none | **add** |
| Tab groups | per-worktree tree | none | **defer** |
| Sidebar collapse | flat `Set<string>` | project-level only, derived from active project | **replace** |
| Worktree nesting | lineage tree + rails | flat list (`WorktreeList`) | **add** |

`ui/src/state/layout.ts` cannot be incrementally extended into this. `normalizeLayout`'s invariants
(single `primaryTabId`, single `secondaryTabId`, mirror special-case) are structurally incompatible
with an n-ary tree. Rewrite the module; keep the reducer *shape* (pure `(state, action) => state`)
since it is already well-tested.

### 5.2 Target data model

```ts
// ---- Terminal pane tree (per tab) ----
export type PaneDirection = "horizontal" | "vertical"; // horizontal = children side by side

export type PaneNode =
  | { type: "leaf"; leafId: string }
  | { type: "split"; direction: PaneDirection; first: PaneNode; second: PaneNode; ratio: number };

export type TabPaneLayout = {
  root: PaneNode;
  activeLeafId: string | null;
  expandedLeafId: string | null;
  sessionIdsByLeafId: Record<string, string>;   // rorca's analogue of ptyIdsByLeafId
};

export type PaneKey = string;                    // `${tabId}:${leafId}`
export const makePaneKey = (tabId: string, leafId: string) => `${tabId}:${leafId}`;

// ---- Workspace layout ----
export type LayoutState = {
  tabs: TerminalTab[];
  tabOrder: string[];
  activeTabId: string | null;
  layoutsByTabId: Record<string, TabPaneLayout>;
};
```

Deliberate simplifications vs. Orca, justified:

- **`ratio` required, always written.** Orca omits it at 0.5 to shrink persisted JSON; rorca has no
  such pressure and an always-present number removes a `?? 0.5` from every consumer.
- **Drop tab groups initially.** The reference screenshot is a per-tab pane split under a single
  shared tab strip; the outer tree buys nothing yet. Keep `LayoutState` a record keyed by tab so a
  `groupsByWorktree` layer can wrap it later without touching `PaneNode`.
- **`sessionIdsByLeafId`** replaces `ptyIdsByLeafId`, matching rorca's existing
  `TerminalSession.id`.

Keep `SplitMode`/`NestedSplit` in `lib/types.ts` only as long as `TerminalSplitView.tsx` and its
tests still reference them; delete in the same change that ports the view.

### 5.3 Reducer actions

```ts
type LayoutAction =
  | { type: "ADD_TAB"; tab: TerminalTab; sessionId: string; activate?: boolean }
  | { type: "CLOSE_TAB"; tabId: string }
  | { type: "ACTIVATE_TAB"; tabId: string }
  | { type: "REORDER_TABS"; from: number; to: number }
  | { type: "SPLIT_PANE"; tabId: string; leafId: string;
      direction: PaneDirection; position: "first" | "second";
      newLeafId: string; sessionId: string }
  | { type: "CLOSE_PANE"; tabId: string; leafId: string }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | { type: "SET_PANE_RATIO"; tabId: string; path: string; ratio: number }  // "" | "first.second"
  | { type: "TOGGLE_PANE_EXPANDED"; tabId: string; leafId: string }
  | { type: "EQUALIZE_PANES"; tabId: string };
```

Pure helpers, ported near-verbatim from §1.1 (they are already pure and dependency-free):

```
splitLeaf(root, leafId, newLeafId, direction, position) -> PaneNode
removeLeaf(root, leafId) -> PaneNode | null        // collapses parent into sibling
setRatioAtPath(root, path, ratio) -> PaneNode
collectLeafIds(root) -> string[]                   // in order
findFirstLeafId(root) -> string
siblingLeafId(root, leafId) -> string | null
equalizeRatios(root) -> PaneNode                   // weight = same-direction leaf count
resolveRoot({authoritative, existing, leafIds}) -> PaneNode | null   // 3-tier repair
```

Invariants the reducer must hold (each maps to a test):

1. No `split` node ever has fewer than 2 children — guaranteed by `removeLeaf` collapsing.
2. `leafId` set is bijective with `sessionIdsByLeafId` keys.
3. `activeLeafId` always references a live leaf; on close, fall back to the sibling
   (`siblingLeafId`), then first leaf.
4. Closing the last pane of a tab closes the tab.
5. `ratio ∈ [0.15, 0.85]` when committed from a drag (clamped at the handle).
6. `expandedLeafId` is cleared when that leaf dies.

### 5.4 Component architecture

```
App
└─ WorkspaceHeader
└─ Sidebar
│   ├─ SectionHeader ("Projects")
│   └─ SidebarTree                     NEW  — flat row list, keyed rows
│       ├─ ProjectGroupRow             NEW  — aria-expanded, subtree count
│       ├─ ProjectRow                  (from Sidebar.tsx)
│       └─ WorktreeRow                 (from WorktreeList.tsx) + LineageRail NEW
└─ TabBar                              — + reorder (dnd-kit) + drop zones
└─ PaneTreeView                        REPLACES TerminalSplitView
    ├─ PaneSplit  (recursive)          NEW  — flex container, direction class
    │   ├─ PaneTreeView (first)
    │   ├─ PaneDivider                 NEW  — pointer capture + rAF
    │   └─ PaneTreeView (second)
    └─ PaneLeaf                        — wraps existing TerminalPane
```

**`PaneDivider`** — port §3.1 exactly, React-flavored:

- `onPointerDown`: `setPointerCapture`, measure `rect` once, record initial flex.
- Window capture-phase listeners for move/up/cancel/blur.
- rAF-coalesced flex writes; **no store dispatch during drag**.
- Commit once on pointerup → `SET_PANE_RATIO` with the node path.
- Clamp `Math.min(50, total/2)`; hit size = thickness + 6px; `col-resize`/`row-resize`.
- Double-click → reset to 0.5.
- `pointercancel` / `blur` → restore initial flex, no dispatch.
- After commit, refit xterm for the two affected subtrees only.

Reference-matched styling: 1px painted line at `#1b1b1b`, 7px hit area, `bg-terminal` panes.

**`PaneLeaf`** must key on `leafId` (not array index) so React never remounts a terminal on a
sibling's structural change — remounting an xterm loses scrollback and the PTY binding.

**Focus** — keep it event-driven like Orca: dispatch `FOCUS_TERMINAL_PANE` with `{tabId, leafId}`
on a `requestAnimationFrame` after activating the tab, so the target pane is mounted. Reuse the
`shouldPreserveEditableFocus` rule verbatim, including the `.xterm` exemption.

**`SidebarTree`** — build rows with a pure `buildSidebarRows(...)` returning a typed row array;
render that array. Collapse state is a single `Set<string>` of namespaced keys
(`project-group:`, `project:`, `lineage:`), persisted to `localStorage` as an array under
`orca.sidebar.collapsedGroups`. Preserve DESIGN.md §3: `aria-expanded` on disclosure rows,
`aria-current="true"` for active, guide rails via `border-worktree-sidebar-border` brightening to
`-ring`, `pl-3` per depth step. Compute indentation from **both** `groupDepth` and lineage `depth`.

Virtualization is not needed yet — Orca virtualizes because it renders 116px cards for potentially
hundreds of worktrees. rorca's rows are `h-7`. Add it only if a project exceeds a few hundred
worktrees (consistent with DESIGN.md §7's stance on accepted debt).

### 5.5 Persistence

```
orca.layout.v1              -> { tabs, tabOrder, activeTabId, layoutsByTabId }
orca.sidebar.collapsedGroups-> string[]
orca.sidebar.width          -> number   (exists)
```

On hydrate, run the three-tier `resolveRoot` repair (§1.3) against the live session list, drop
leaves with no live session, mint fresh UUIDs for any duplicate `leafId`, and re-derive
`activeLeafId`. Never render a persisted tree unvalidated.

### 5.6 Recommended sequence

1. `state/paneTree.ts` — pure node helpers + unit tests (no React). Highest value, zero risk.
2. `state/layout.ts` — rewrite reducer over `layoutsByTabId`; port existing tests to the new shape.
3. `PaneDivider` + `PaneSplit` + `PaneTreeView`; delete `TerminalSplitView`.
4. Persistence + hydration repair.
5. `SidebarTree` with `Set<string>` collapse + lineage rails.
6. Tab reorder / drag-to-split (dnd-kit) — last; it depends on everything above and is the only
   piece the reference screenshot does not evidence.

---

## 6. Verbatim constants

| Constant | Value | Source |
|---|---|---|
| `MIN_PANE_SIZE` | `50` px | `terminal-appearance-D3oO-Ew5.js:179` |
| divider thickness | `4` px default, clamp `1..32` | `:374`, `store:1148` |
| divider hit size | `thickness + 6` px | `:374` |
| `MIN_RATIO` / `MAX_RATIO` (tab groups) | `0.15` / `0.85` | `Terminal-qm6WvB4Q.js:2437-2438` |
| default split ratio | `0.5` | `store:11939` |
| ratio serialize epsilon | `0.005`, 3 decimals | `store:20390-20394` |
| drop zone edge inset | `10%` w/h | `rename-file:4152-4153` |
| drop zone split threshold | `width / 3` | `rename-file:4154` |
| pane column edge band | `20%` | `rename-file:4162, 4176` |
| assumed tab strip height | `32` px | `rename-file:4165` |
| group header row height | `28` px | `worktree-list-virtual-rows:3`, applied `:44` |
| host header row height | `32` px | `:39` |
| sidebar virtual row gap | `6` px | `:4` |
| folder workspace row height | `64` px | `:8`, applied `:52` |
| worktree item row height | `116` px | `:53` |
| sticky group header offset | `36` px | `:103` |
| drop overlay z-index | `10001` | `Terminal-qm6WvB4Q.js:2428` |

## 7. Primary source index

| File | Contains |
|---|---|
| `store-CgXrfmaH.js` | Both layout models, all tree ops, pane keys, hydration, collapse state |
| `terminal-appearance-D3oO-Ew5.js` | Imperative pane manager: divider drag, split/detach/promote, equalize |
| `Terminal-qm6WvB4Q.js` | React `SplitNode`, `ResizeHandle`, drop overlays, `TabGroupPanel` |
| `rename-file-JaIUF221.js` | dnd-kit tab drag, `resolveDropZone`, `resolvePaneColumnEdgeZone` |
| `worktree-activation-BDsaiyMf.js` | `buildRows`, project-group recursion, lineage nesting, group keys |
| `worktree-list-virtual-rows-B1jWEldM.js` | Row types, heights, sticky header algorithm |
| `pane-helpers-9eOmrw__.js` | `fitPanes`, `focusActivePane`, `shouldPreserveEditableFocus` |
| `activate-tab-and-focus-pane-dvS5VCkm.js` | `(tabId, leafId)` focus handoff via rAF + CustomEvent |
| `request-active-terminal-pane-split-So9AiZw3.js` | Split request event decoupling |
