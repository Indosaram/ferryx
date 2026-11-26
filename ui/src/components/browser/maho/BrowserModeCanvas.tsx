// Browser mode canvas: the full main area for the Activity Bar's Browser mode.
//
// One Maho canvas draws the active tab of the active Space through the real
// `BrowserPane` in `maho-minimal` chrome mode: the Ferryx toolbar, bookmark bar and
// vault bar stay out of the way because the Maho sidebar owns navigation, while the
// functional rows (find bar, downloads shelf, engine fallback notice, load-error
// retry) remain reachable. A Chromium (`engine === "chromium"`) tab keeps its own
// engine toolbar and still gets no Ferryx toolbar on top of it.
//
// The canvas also carries a 2-way in-browser split. Both panes are `BrowserPane`s
// with distinct `browserId`s, so each claims its own native surface from
// `browserViewportCoordinator`; the split ratio is clamped to 0.25-0.75 and
// persisted under `ferryx.browser.splitRatio`.
//
// Tab ownership stays in `browserStore`: this component never closes, opens or
// navigates a tab by itself. When the active tab goes away it asks the store for the
// next most-recently-used tab of the Space, and an empty Space renders the Maho
// empty state instead of a blank frame.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { ArrowLeftRight, Columns2, GripVertical, Plus, X } from "lucide-react";

import {
  browserStore,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreState,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { BROWSER_SPLIT_RATIO_STORAGE_KEY } from "../../../lib/storageKeys";
import type { BrowserReloadOptions } from "../../../lib/browserTauri";
import type { BrowserTab } from "../../../lib/types";
import { BrowserPane } from "../../BrowserPane";

/** Divider limits: neither pane may shrink below a quarter of the canvas. */
export const MIN_SPLIT_RATIO = 0.25;
export const MAX_SPLIT_RATIO = 0.75;
export const DEFAULT_SPLIT_RATIO = 0.5;
/** One arrow-key step on the divider. */
export const SPLIT_RATIO_STEP = 0.02;

/** Keeps a ratio inside the legal band; a non-finite value falls back to the even split. */
export function clampSplitRatio(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_SPLIT_RATIO;
  return Math.min(MAX_SPLIT_RATIO, Math.max(MIN_SPLIT_RATIO, value));
}

function defaultStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Reads the persisted split ratio; an unreadable, absent or out-of-band value means the default. */
export function readStoredSplitRatio(storage: Storage | null = defaultStorage()): number {
  if (!storage) return DEFAULT_SPLIT_RATIO;
  try {
    const raw = storage.getItem(BROWSER_SPLIT_RATIO_STORAGE_KEY);
    if (raw === null) return DEFAULT_SPLIT_RATIO;
    const parsed = Number.parseFloat(raw);
    if (!Number.isFinite(parsed)) return DEFAULT_SPLIT_RATIO;
    return clampSplitRatio(parsed);
  } catch {
    return DEFAULT_SPLIT_RATIO;
  }
}

export function storeSplitRatio(ratio: number, storage: Storage | null = defaultStorage()): number {
  const clamped = clampSplitRatio(ratio);
  if (!storage) return clamped;
  try {
    storage.setItem(BROWSER_SPLIT_RATIO_STORAGE_KEY, String(clamped));
  } catch {
    // A disabled or full storage keeps the in-memory ratio; the split still works.
  }
  return clamped;
}

/** The pane model `BrowserPane` expects, derived from the store's tab record. */
export function toBrowserPaneTab(tab: BrowserStoreTab): BrowserTab {
  const pane: BrowserTab = {
    kind: "browser",
    id: tab.browserId,
    label: tab.title.trim() || tab.url,
    browserId: tab.browserId,
    url: tab.url,
    title: tab.title || null,
  };
  if (tab.engine) pane.engine = tab.engine;
  return pane;
}

/** The Space's tabs, most recently used first, then any tab the MRU does not cover. */
function tabsInMruOrder(state: BrowserStoreState, spaceId: string): BrowserStoreTab[] {
  const ordered: BrowserStoreTab[] = [];
  const seen = new Set<string>();
  for (const browserId of state.mru) {
    const tab = state.tabs[browserId];
    if (!tab || tab.spaceId !== spaceId || seen.has(browserId)) continue;
    seen.add(browserId);
    ordered.push(tab);
  }
  for (const tab of Object.values(state.tabs)) {
    if (tab.spaceId !== spaceId || seen.has(tab.browserId)) continue;
    seen.add(tab.browserId);
    ordered.push(tab);
  }
  return ordered;
}

export type BrowserModeCanvasProps = {
  store?: BrowserStore;
  /** Space to draw; defaults to the store's active Space. */
  spaceId?: string;
  /** Whether Browser mode is on screen. Panes get `visible={active}` so a hidden mode
   * never reveals a native webview. */
  active: boolean;
  /** Todo 14: controlled banner / human-handoff help bar above the panes. */
  bannerSlot?: ReactNode;
  /** Todo 15: the Transient Mini Pane column beside the canvas. */
  rightOverlayPane?: ReactNode;
  onNavigate?: (browserId: string, url: string) => void;
  onReload?: (browserId: string, options?: BrowserReloadOptions) => void;
  onNewTab?: () => void;
};

type CanvasPane = {
  role: "primary" | "secondary";
  tab: BrowserStoreTab;
};

export function BrowserModeCanvas({
  store = browserStore,
  spaceId,
  active,
  bannerSlot,
  rightOverlayPane,
  onNavigate,
  onReload,
  onNewTab,
}: BrowserModeCanvasProps) {
  const state = useBrowserStore((snapshot) => snapshot, store);
  const targetSpaceId = spaceId ?? state.activeSpaceId;
  const activeBrowserId = state.activeBrowserIdBySpace[targetSpaceId] ?? null;
  const activeTab = activeBrowserId ? state.tabs[activeBrowserId] ?? null : null;
  const spaceTabs = useMemo(
    () => Object.values(state.tabs).filter((tab) => tab.spaceId === targetSpaceId),
    [state.tabs, targetSpaceId],
  );
  const mruTabs = useMemo(() => tabsInMruOrder(state, targetSpaceId), [state, targetSpaceId]);

  const [secondaryBrowserId, setSecondaryBrowserId] = useState<string | null>(null);
  const [ratio, setRatio] = useState<number>(() => readStoredSplitRatio());
  const splitRowRef = useRef<HTMLDivElement | null>(null);
  const draggingRef = useRef(false);

  const secondaryTab = secondaryBrowserId ? state.tabs[secondaryBrowserId] ?? null : null;
  const secondaryPane = secondaryTab && secondaryTab.spaceId === targetSpaceId && secondaryTab.browserId !== activeTab?.browserId
    ? secondaryTab
    : null;

  // A closed or moved secondary tab (or the active tab taking its place) collapses the split:
  // a pane pointing at a tab that is gone would claim a surface nothing renders.
  useEffect(() => {
    if (secondaryBrowserId === null) return;
    if (!secondaryPane) setSecondaryBrowserId(null);
  }, [secondaryBrowserId, secondaryPane]);

  // Closing the active tab leaves the Space without one; claim the next most-recently-used tab
  // instead of leaving the canvas blank. An empty Space is handled by the empty state below.
  useEffect(() => {
    if (!active || activeTab) return;
    const next = mruTabs[0];
    if (!next) return;
    store.touch(next.browserId);
  }, [active, activeTab, mruTabs, store]);

  const splitCandidate = useMemo(
    () => mruTabs.find((tab) => tab.browserId !== activeTab?.browserId && tab.browserId !== secondaryBrowserId) ?? null,
    [mruTabs, activeTab, secondaryBrowserId],
  );

  const applyRatio = useCallback((next: number) => {
    setRatio(storeSplitRatio(next));
  }, []);

  const handleDividerPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    draggingRef.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handleDividerPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    const rect = splitRowRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return;
    applyRatio((event.clientX - rect.left) / rect.width);
  };

  const endDividerDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    draggingRef.current = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const handleDividerKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
      event.preventDefault();
      applyRatio(ratio - SPLIT_RATIO_STEP);
    } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
      event.preventDefault();
      applyRatio(ratio + SPLIT_RATIO_STEP);
    } else if (event.key === "Home") {
      event.preventDefault();
      applyRatio(MIN_SPLIT_RATIO);
    } else if (event.key === "End") {
      event.preventDefault();
      applyRatio(MAX_SPLIT_RATIO);
    }
  };

  const startSplit = () => {
    if (!splitCandidate) return;
    setSecondaryBrowserId(splitCandidate.browserId);
  };

  const swapPanes = () => {
    if (!secondaryPane || !activeTab) return;
    setSecondaryBrowserId(activeTab.browserId);
    store.touch(secondaryPane.browserId);
  };

  const banner = bannerSlot ? (
    <div className="shrink-0" data-testid="browser-canvas-banner">
      {bannerSlot}
    </div>
  ) : null;

  // While the mode is off screen nothing here may touch a native surface. A mounted
  // `BrowserPane` claims its browser from `browserViewportCoordinator` on mount, so a hidden
  // canvas would take the surface away from a live worktree split (or the mini pane) showing the
  // same `browserId` and hide it. The shell stays in the tree so the mode's slot does not jump;
  // the panes come back - and re-claim - the moment the mode is shown again.
  if (!active) {
    return (
      <section
        data-testid="browser-mode-canvas"
        data-active="false"
        aria-hidden="true"
        className="hidden h-full min-h-0 w-full"
      />
    );
  }

  if (!activeTab && spaceTabs.length === 0) {
    return (
      <section
        data-testid="browser-mode-canvas"
        data-empty="true"
        data-active="true"
        aria-label="Browser"
        className="flex h-full min-h-0 w-full flex-col bg-background"
      >
        {banner}
        <div className="flex min-h-0 flex-1">
          <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
            <p className="text-sm font-medium text-foreground">No tabs in this Space</p>
            <p className="max-w-xs text-xs text-muted-foreground">
              Open a tab to start browsing. Your Spaces keep their own tabs and history.
            </p>
            <button
              type="button"
              onClick={onNewTab}
              className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Plus className="size-3.5" aria-hidden="true" />
              New Tab
            </button>
          </div>
          {rightOverlayPane ? (
            <div className="flex min-h-0 w-80 shrink-0 flex-col border-l border-border" data-testid="browser-canvas-right-overlay">
              {rightOverlayPane}
            </div>
          ) : null}
        </div>
      </section>
    );
  }

  const panes: CanvasPane[] = [];
  if (activeTab) panes.push({ role: "primary", tab: activeTab });
  if (secondaryPane) panes.push({ role: "secondary", tab: secondaryPane });

  const paneStyle = (index: number): CSSProperties =>
    panes.length === 1 || index > 0
      ? { flex: "1 1 0%" }
      : { flex: `0 0 ${(ratio * 100).toFixed(4)}%` };

  return (
    <section
      data-testid="browser-mode-canvas"
      data-active="true"
      data-split={secondaryPane ? "true" : "false"}
      aria-label="Browser"
      className="flex h-full min-h-0 w-full flex-col bg-background"
    >
      {banner}
      <div
        className="flex shrink-0 items-center justify-end gap-1 border-b border-border bg-card px-2 py-1"
        data-testid="browser-canvas-controls"
        role="toolbar"
        aria-label="Browser canvas"
      >
        <button
          type="button"
          onClick={onNewTab}
          aria-label="New Tab"
          title="New Tab"
          className="rounded p-1 hover:bg-accent"
        >
          <Plus className="size-3.5" aria-hidden="true" />
        </button>
        <button
          type="button"
          onClick={startSplit}
          disabled={!splitCandidate}
          aria-label="Split view"
          title={splitCandidate ? "Split view" : "Open another tab to split"}
          aria-pressed={secondaryPane ? "true" : "false"}
          className="rounded p-1 hover:bg-accent disabled:opacity-40"
        >
          <Columns2 className="size-3.5" aria-hidden="true" />
        </button>
        {secondaryPane ? (
          <>
            <button
              type="button"
              onClick={swapPanes}
              aria-label="Swap split panes"
              title="Swap panes"
              className="rounded p-1 hover:bg-accent"
            >
              <ArrowLeftRight className="size-3.5" aria-hidden="true" />
            </button>
            <button
              type="button"
              onClick={() => setSecondaryBrowserId(null)}
              aria-label="Close split pane"
              title="Close split pane"
              className="rounded p-1 hover:bg-accent"
            >
              <X className="size-3.5" aria-hidden="true" />
            </button>
          </>
        ) : null}
      </div>
      <div className="flex min-h-0 flex-1">
        <div ref={splitRowRef} className="flex min-h-0 min-w-0 flex-1" data-testid="browser-canvas-panes">
          {panes.map((pane, index) => (
            <div
              // Keyed by the browser it draws, never by the pane slot: swapping the panes then
              // moves two mounted `BrowserPane`s instead of unmounting both, and an unmount
              // would release (hide) the surfaces the swap is supposed to keep on screen.
              key={pane.tab.browserId}
              data-testid="browser-canvas-pane"
              data-role={pane.role}
              data-browser-id={pane.tab.browserId}
              data-engine={pane.tab.engine ?? "system"}
              data-visible="true"
              className="flex h-full min-h-0 min-w-0 flex-col"
              style={paneStyle(index)}
            >
              <BrowserPane
                tab={toBrowserPaneTab(pane.tab)}
                visible
                chromeMode="maho-minimal"
                surfacePriority={pane.role === "primary" ? 1 : 0}
                onNavigate={(url) => onNavigate?.(pane.tab.browserId, url)}
                onReload={(options) => onReload?.(pane.tab.browserId, options)}
              />
            </div>
          ))}
          {panes.length === 2 ? (
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize split"
              aria-valuemin={Math.round(MIN_SPLIT_RATIO * 100)}
              aria-valuemax={Math.round(MAX_SPLIT_RATIO * 100)}
              aria-valuenow={Math.round(ratio * 100)}
              tabIndex={0}
              data-testid="browser-canvas-divider"
              className="group relative flex w-1.5 shrink-0 cursor-col-resize items-center justify-center bg-border hover:bg-primary/60 focus-visible:bg-primary/60 focus-visible:outline-none"
              onPointerDown={handleDividerPointerDown}
              onPointerMove={handleDividerPointerMove}
              onPointerUp={endDividerDrag}
              onPointerCancel={endDividerDrag}
              onKeyDown={handleDividerKeyDown}
            >
              <GripVertical className="size-3 text-muted-foreground opacity-0 group-hover:opacity-100" aria-hidden="true" />
            </div>
          ) : null}
        </div>
        {rightOverlayPane ? (
          <div className="flex min-h-0 w-80 shrink-0 flex-col border-l border-border" data-testid="browser-canvas-right-overlay">
            {rightOverlayPane}
          </div>
        ) : null}
      </div>
    </section>
  );
}
