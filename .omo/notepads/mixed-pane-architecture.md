# Mixed Terminal / Browser Pane Architecture

## Selected model

Keep `PaneNode` as pure geometry and replace the terminal-only `sessionIdsByLeafId` projection with a per-tab discriminated content map:

```ts
type PaneContent =
  | { readonly kind: "terminal"; readonly sessionId: string }
  | { readonly kind: "browser"; readonly browser: BrowserPaneState };
```

`TabPaneLayout.contentsByLeafId` owns exactly one content record for every leaf in `root`.

## Invariants

1. Every leaf ID in `root` has exactly one `contentsByLeafId` record and no extra records exist.
2. A terminal local session ID or native browser ID is owned by at most one live pane leaf/top-level tab across active and parked layouts.
3. A browser tab merged into another tab's pane tree is removed from `layout.tabs` and its source tab group; its BrowserTab state moves into the target leaf content.
4. The target top-level tab remains the tab-bar identity. It may own terminal and browser leaves.
5. Closing/moving a pane transfers ownership before disposal. Native terminal/browser resources are closed only when absent from the post-transition graph.
6. Browser metadata updates locate content by browser ID across top-level browser tabs and browser pane leaves.
7. Native browser bounds are measured by the browser leaf's own viewport anchor. Hidden tabs, parked worktrees, expanded sibling panes, modal/drag suppression set native visibility false.
8. Existing persisted `sessionIdsByLeafId` records migrate to terminal `PaneContent`; existing browser top-level tabs migrate to one browser leaf.
9. When browser tab restoration is disabled, browser leaves are pruned from pane trees; an empty mixed tab is omitted rather than restored as a zombie browser.
10. `sessionIdsByLeafId` remains accepted only as a persisted compatibility input, not as live state.

## Rejected models

- Content inside `PaneNode`: pollutes pure tree transforms and breaks persisted paneTree compatibility.
- Global `paneContents`: creates orphan risks and breaks worktree layout parking.
- Keeping merged browser in `layout.tabs`: duplicates tab-bar/native webview ownership.

## Required RED seams

- Pure transition: browser source tab becomes a browser leaf in target tab, source group collapses, no group split appears.
- Renderer: one terminal leaf and one browser leaf render inside one tab and share one tab strip.
- Persistence: mixed pane round-trip preserves browser metadata and terminal session ownership; v2 terminal mapping migrates.
- Movement: browser pane can be moved to another leaf without duplicate browser ID, then closed without affecting terminal PTY.

## Implementation receipt

- Browser pane-targeted tab drops now transfer browser metadata into target `contentsByLeafId` and remove the source tab/group atomically.
- Mixed terminal/browser leaves render beneath one tab strip with one pane tree.
- Existing browser leaves move without native create/close and preserve terminal sessions.
- V2 terminal persistence migrates into terminal PaneContent; mixed content round-trips.
- Full UI verification: 76 files, 526 tests passed; TypeScript and Vite build passed.
- Final browser geometry: one tab/group, zero group splits, one pane split, terminal 720x868 and browser 719x868 with a 1px divider.
