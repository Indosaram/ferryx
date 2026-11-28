// Browser mode's shell: everything the Activity Bar's Browser mode draws, in one place.
//
// `App.tsx` keeps the four-mode rail and the worktree surface; this module owns the Browser
// half of that split so the shell diff stays a handful of lines. Two exports matter:
//
//   - `useMahoBrowserShell`  one hook instance owns the drawer, the canvas, the Command Bar and
//                        the Ctrl+Tab switcher, and returns them as elements so the shell can
//                        place the drawer before the workspace and the canvas after it while
//                        sharing a single piece of state.
//   - `MahoBrowserMiniPane`  the Transient Mini Pane's right split slot for the modes the canvas
//                        does not draw (Supervisor / Worktrees), so Shift+click on a terminal
//                        link peeks beside the terminal too.
//   - `useMahoBrowserRuntime`  the app-lifetime half: global store hydration/live sync,
//                        auto-archive, and the mini link opener. It runs in every mode because
//                        the store and the archive policy are global.
//
// One `browserId` renders in exactly one place at a time: the canvas holds the mini while
// Browser mode is on screen, and `MahoBrowserMiniPane` holds it otherwise. Mounting both would
// make two `BrowserPane`s claim the same surface from `browserViewportCoordinator`, so the two
// slots are mutually exclusive by construction.
//
// IPC failures arrive as structured `{ code, message, details }`; nothing here matches on
// message text.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";

import { emit } from "@tauri-apps/api/event";
import { copyTextToClipboard } from "../../../lib/clipboard";
import {
  BROWSER_SHORTCUT_REQUESTED_EVENT,
  captureBrowserThumbnail,
  onBrowserStateChanged,
  type BrowserShortcutAction,
  closeBrowser,
  createBrowser,
  navigateBrowser,
  reloadBrowser,
  type BrowserReloadOptions,
} from "../../../lib/browserTauri";
import { browserTabNavigation, goBackInBrowserTab, goForwardInBrowserTab } from "../../../lib/browserTabNavigation";
import { registerMiniBrowserLinkOpener } from "../../../lib/linkRouting";
import {
  clampSidebarWidth,
  loadSidebarWidthForMode,
  saveSidebarWidthForMode,
  SIDEBAR_RANGE_BY_MODE,
} from "../../../lib/sidebarPersistence";
import type { MahoBrowserShortcutAction } from "../../../lib/shortcuts";
import type { BrowserState } from "../../../lib/types";
import { safeRandomUUID } from "../../../lib/uuid";
import { closeBrowserSessionFromSplit } from "../../../lib/worktreeBrowserBridge";
import {
  browserStore,
  selectActiveBrowserId,
  selectMruTabs,
  useBrowserStore,
  type BrowserStore,
} from "../../../state/browserStore";
import { useGlobalBrowserStoreSync } from "../../../state/browserSessionHydration";
import { toast } from "../../ui/sonner";
import { BrowserModeCanvas } from "./BrowserModeCanvas";
import {
  MahoAgentControlledBanner,
  MahoAgentTasksSection,
  MahoControlActivityChip,
  type BrowserAgentActivity,
} from "./MahoAgentControl";
import { MahoBrowserSidebar } from "./MahoBrowserSidebar";
import { MahoCommandBar, type MahoCommandBarMode, type MahoCommandBarTarget } from "./MahoCommandBar";
import { MahoLibraryMode } from "./MahoLibraryMode";
import { MahoMiniPane } from "./MahoMiniPane";
import { MahoNowPlayingCard } from "./MahoNowPlayingCard";
import { MahoTabSwitcher, type MahoTabSwitcherDirection } from "./MahoTabSwitcher";
import { useBrowserAutoArchive } from "./mahoAutoArchive";
import { useAppearanceSettings, useResolvedTheme } from "../../../lib/appearanceSettings";
import { spacePaletteStyle, type MahoScheme } from "./mahoSpacePalette";
import { tidyTodayTabs } from "./mahoTabTidy";

/**
 * A Browser-mode chord that arrived as a DOM event instead of a keydown: the shell's keydown
 * router lives in `App.tsx` (it owns the mode) while the surfaces live here, so the action
 * travels as an event. The detail shape is the contract, not the event name alone.
 */
export const BROWSER_MODE_SHORTCUT_EVENT = "ferryx:browser-mode-shortcut";

export type BrowserModeShortcutDetail = { action: MahoBrowserShortcutAction };

/** The drawer's resize step, matching the other drawers' keyboard affordance. */
const KEYBOARD_STEP = 16;
/** The Mini Pane's column, matching the canvas overlay width. */
const MINI_PANE_WIDTH = 320;

const RANGE = SIDEBAR_RANGE_BY_MODE.browser;

function activeBrowserIdOf(state: {
  activeBrowserIdBySpace: Readonly<Record<string, string | null>>;
  activeSpaceId: string;
}): string | null {
  return state.activeBrowserIdBySpace[state.activeSpaceId] ?? null;
}

/** Registers a freshly created backend session as a store tab in the active Space. */
function registerNewTab(
  store: BrowserStore,
  browserId: string,
  url: string,
  created: BrowserState | undefined,
): boolean {
  const upserted = store.upsertTab({
    browserId,
    url: created?.url?.trim() || url,
    title: created?.title ?? "",
    spaceId: store.getState().activeSpaceId,
  });
  if (!upserted.ok) {
    void closeBrowser(browserId).catch(() => undefined);
    return false;
  }
  return true;
}

/**
 * Opens `url` in the Transient Mini Pane: a fresh transient session in the store's single mini
 * slot. Replacing a previous mini is the pane's own job (it destroys the session it was holding
 * when the slot changes), so this only ever creates and assigns.
 */
export async function openMiniBrowserTab(url: string, store: BrowserStore = browserStore): Promise<string | null> {
  const browserId = `mini:${safeRandomUUID()}`;
  let created: BrowserState | undefined;
  try {
    created = await createBrowser({ browserId, url, visible: false });
  } catch {
    // The host refused the session; the link still falls back through `routeHttpLink`.
    return null;
  }
  if (!registerNewTab(store, browserId, url, created)) return null;
  store.setMini(browserId);
  return browserId;
}

/** Ends a transient mini session: the store slot is already clear, only the page remains. */
export function destroyMiniBrowserSession(browserId: string): void {
  browserTabNavigation.forget(browserId);
  void closeBrowser(browserId).catch(() => undefined);
}

/**
 * Creates a Maho tab that belongs to the store only: Browser mode's `Cmd+T` and the Command
 * Bar's "new tab" never create a worktree layout tab.
 */
export async function createMahoBrowserTab(url: string, store: BrowserStore = browserStore): Promise<string | null> {
  const browserId = safeRandomUUID();
  let created: BrowserState | undefined;
  try {
    created = await createBrowser({ browserId, url, visible: false });
  } catch {
    return null;
  }
  if (!registerNewTab(store, browserId, url, created)) return null;
  store.touch(browserId);
  return browserId;
}

/**
 * App-lifetime Browser plumbing: the global store's startup migration + live sync, the
 * auto-archive policy, and the mini link opener. Runs in every Activity mode, because the
 * store and the policy are not scoped to the mode on screen.
 */
export function useMahoBrowserRuntime(enabled: boolean, store: BrowserStore = browserStore): void {
  useGlobalBrowserStoreSync(store, enabled);
  useBrowserAutoArchive(store);

  useEffect(() => {
    if (!enabled) return;
    return registerMiniBrowserLinkOpener(async (url) => {
      await openMiniBrowserTab(url, store);
    });
  }, [enabled, store]);
}

export type MahoBrowserShellOptions = {
  /** Browser mode is the mode on screen. */
  active: boolean;
  /** The shared drawer is open; `Cmd+B` collapses it without touching the rail. */
  drawerOpen: boolean;
  /** Agent tasks, handoffs and the badge, from `useBrowserAgentActivity` in the shell. */
  activity: BrowserAgentActivity;
  /** Hands a tab to the fullscreen Browser mode from anywhere (Now Playing, promoted mini). */
  onRevealBrowser?: (browserId: string) => void;
  store?: BrowserStore;
};

export type MahoBrowserShellElements = {
  /** The resizable Maho rail, placed where a drawer belongs (between rail and workspace). */
  drawer: ReactNode;
  /** The Maho canvas, placed where the Browser mode's main area belongs. */
  canvas: ReactNode;
  /** The Command Bar and Ctrl+Tab switcher: overlay surfaces, placed once near the root. */
  overlays: ReactNode;
};

/**
 * One instance owns every Browser-mode surface's state and returns the pieces as elements, so
 * the shell can keep its existing row order (rail, drawer, workspace, canvas) without either
 * duplicating this state or moving the always-mounted worktree surface.
 */
export function useMahoBrowserShell({
  active,
  drawerOpen,
  activity,
  onRevealBrowser,
  store = browserStore,
}: MahoBrowserShellOptions): MahoBrowserShellElements {
  const state = useBrowserStore((snapshot) => snapshot, store);
  const activeBrowserId = selectActiveBrowserId(state);
  const activeTab = activeBrowserId ? state.tabs[activeBrowserId] ?? null : null;
  const mruTabs = useMemo(() => selectMruTabs(state), [state]);
  const miniBrowserId = state.miniBrowserId;
  const { settings: appearance } = useAppearanceSettings();
  const scheme: MahoScheme = useResolvedTheme(appearance.theme) === "light" ? "light" : "dark";
  const spaceColor = state.spaces[state.activeSpaceId]?.color;

  const [width, setWidth] = useState(() => loadSidebarWidthForMode("browser"));
  const widthRef = useRef(width);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  widthRef.current = width;

  const [libraryOpen, setLibraryOpen] = useState(false);
  const [thumbnails, setThumbnails] = useState<Readonly<Record<string, string>>>({});

  useEffect(() => {
    let disposed = false;
    let unsupported = false;
    let previousId = selectActiveBrowserId(store.getState());
    const inFlight = new Set<string>();
    const recapture = new Set<string>();
    const refresh = async (browserId: string) => {
      if (disposed || unsupported || !store.getState().tabs[browserId]) return;
      if (inFlight.has(browserId)) { recapture.add(browserId); return; }
      inFlight.add(browserId);
      try {
        const data = await captureBrowserThumbnail(browserId);
        if (!disposed && store.getState().tabs[browserId]) {
          setThumbnails((current) => ({ ...current, [browserId]: data }));
        }
      } catch (error) {
        const code = error && typeof error === "object" && "code" in error ? error.code : null;
        if (code === "UNSUPPORTED") unsupported = true;
        console.warn("Browser thumbnail capture failed", browserId, error);
      } finally {
        inFlight.delete(browserId);
        if (recapture.delete(browserId)) void refresh(browserId);
      }
    };
    if (previousId) void refresh(previousId);
    const unsubscribe = store.subscribe((next) => {
      const nextId = selectActiveBrowserId(next);
      if (previousId && previousId !== nextId) void refresh(previousId);
      previousId = nextId;
      setThumbnails((current) => {
        const ids = Object.keys(current);
        return ids.some((id) => !next.tabs[id])
          ? Object.fromEntries(ids.filter((id) => next.tabs[id]).map((id) => [id, current[id]]))
          : current;
      });
    });
    const unlisten = onBrowserStateChanged((event) => {
      if (event.loading === false) void refresh(event.browserId);
    });
    void unlisten.catch((error) => console.warn("Browser thumbnail subscription failed", error));
    return () => {
      disposed = true;
      unsubscribe();
      void unlisten.then((stop) => stop(), () => undefined);
    };
  }, [store]);
  const [commandBar, setCommandBar] = useState<{
    open: boolean;
    mode: MahoCommandBarMode;
    initialQuery: string;
  }>({ open: false, mode: "navigate", initialQuery: "" });
  const [switcher, setSwitcher] = useState<{ open: boolean; direction: MahoTabSwitcherDirection; index: number }>({
    open: false,
    direction: "forward",
    index: 0,
  });

  const endResize = useCallback(() => {
    if (!dragRef.current) return;
    dragRef.current = null;
    saveSidebarWidthForMode("browser", widthRef.current);
    document.body.style.cursor = "";
  }, []);

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      setWidth(clampSidebarWidth(drag.startWidth + event.clientX - drag.startX, RANGE));
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", endResize);
      window.removeEventListener("pointercancel", endResize);
      document.body.style.cursor = "";
    };
  }, [endResize]);

  const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    document.body.style.cursor = "col-resize";
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // The pointer may already be gone; the window listeners still end the drag.
    }
  };

  const resizeWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const delta = event.key === "ArrowLeft" ? -KEYBOARD_STEP : KEYBOARD_STEP;
    const nextWidth = clampSidebarWidth(widthRef.current + delta, RANGE);
    widthRef.current = nextWidth;
    setWidth(nextWidth);
    saveSidebarWidthForMode("browser", nextWidth);
  };

  /** Landing on a tab is a use: it becomes the Space's active tab and the head of the MRU. */
  const activateTab = useCallback(
    (browserId: string) => {
      const tab = store.getState().tabs[browserId];
      if (!tab) return;
      store.touch(browserId);
      store.setActiveSpace(tab.spaceId);
    },
    [store],
  );

  const revealTab = useCallback(
    (browserId: string) => {
      activateTab(browserId);
      onRevealBrowser?.(browserId);
    },
    [activateTab, onRevealBrowser],
  );

  const openCommandBar = useCallback((mode: MahoCommandBarMode) => {
    const current = store.getState();
    const currentId = activeBrowserIdOf(current);
    const url = currentId ? current.tabs[currentId]?.url ?? "" : "";
    setCommandBar({
      open: true,
      mode,
      // `Cmd+L` seeds the live address; `Cmd+T` starts empty so a new tab is a fresh search.
      initialQuery: mode === "navigate" && url && url !== "about:blank" ? url : "",
    });
  }, [store]);

  const closeCommandBar = useCallback(() => {
    setCommandBar((current) => ({ ...current, open: false }));
  }, []);

  const handleNewTab = useCallback(() => {
    openCommandBar("new-tab");
  }, [openCommandBar]);

  const handleTidy = useCallback(() => {
    const result = tidyTodayTabs(store);
    if (result.movedCount === 0) {
      toast.info("Nothing to tidy", { description: "Today has no site with two or more tabs." });
      return;
    }
    const label = result.movedCount === 1 ? "1 tab" : `${result.movedCount} tabs`;
    toast.success(`Tidied ${label} into folders`, {
      description: "Grouped by site, kept in this Space.",
      action: { label: "Undo", onClick: () => result.undo() },
    });
  }, [store]);

  const handleCommandBarNavigate = useCallback(
    (url: string, target: MahoCommandBarTarget) => {
      const current = store.getState();
      const currentId = activeBrowserIdOf(current);
      if (target === "mini") {
        void openMiniBrowserTab(url, store);
        return;
      }
      if (target === "new-tab") {
        void createMahoBrowserTab(url, store);
        return;
      }
      // "current" with no tab in the Space has nothing to replace: open one instead of
      // navigating a session that does not exist.
      if (!currentId || !current.tabs[currentId]) {
        void createMahoBrowserTab(url, store);
        return;
      }
      store.upsertTab({ browserId: currentId, url, title: null });
      void navigateBrowser(currentId, url).catch(() => undefined);
    },
    [store],
  );

  const handleCanvasNavigate = useCallback(
    (browserId: string, url: string) => {
      store.upsertTab({ browserId, url, title: null });
      void navigateBrowser(browserId, url).catch(() => undefined);
    },
    [store],
  );

  const handleCanvasReload = useCallback((browserId: string, options?: BrowserReloadOptions) => {
    void reloadBrowser(browserId, options).catch(() => undefined);
  }, []);

  const closeActiveTab = useCallback(() => {
    const currentId = activeBrowserIdOf(store.getState());
    if (!currentId) return;
    closeBrowserSessionFromSplit(currentId, (id) => {
      browserTabNavigation.forget(id);
      void closeBrowser(id).catch(() => undefined);
    }, store);
  }, [store]);

  const cycleSwitcher = useCallback(
    (direction: MahoTabSwitcherDirection) => {
      const tabs = selectMruTabs(store.getState());
      if (tabs.length === 0) return;
      setSwitcher((current) => {
        if (!current.open) {
          // Opening on `next` starts at the previously used tab, the way browsers do.
          return {
            open: true,
            direction,
            index: direction === "forward" ? Math.min(1, tabs.length - 1) : tabs.length - 1,
          };
        }
        const step = direction === "forward" ? 1 : -1;
        return { open: true, direction, index: (current.index + step + tabs.length) % tabs.length };
      });
    },
    [store],
  );

  // The chord router in `App.tsx` owns the mode; the surfaces live here. One listener keeps
  // every Browser-mode chord in a single place instead of two half-handlers.
  useEffect(() => {
    const handle = (event: Event) => {
      const detail = (event as CustomEvent<BrowserModeShortcutDetail>).detail;
      const action = detail?.action;
      if (!action) return;
      switch (action) {
        case "command-bar":
          openCommandBar("navigate");
          return;
        case "new-tab":
          openCommandBar("new-tab");
          return;
        case "close-tab":
          closeActiveTab();
          return;
        case "switcher-next":
          cycleSwitcher("forward");
          return;
        case "switcher-prev":
          cycleSwitcher("backward");
          return;
      }
    };
    window.addEventListener(BROWSER_MODE_SHORTCUT_EVENT, handle);
    return () => window.removeEventListener(BROWSER_MODE_SHORTCUT_EVENT, handle);
  }, [closeActiveTab, cycleSwitcher, openCommandBar]);

  const copyUrl = useCallback((url: string) => {
    void copyTextToClipboard(url).catch(() => undefined);
  }, []);

  const dispatchBrowserAction = (action: BrowserShortcutAction) => {
    void emit(BROWSER_SHORTCUT_REQUESTED_EVENT, { action, browserId: activeBrowserId ?? "" })
      .catch((error) => toast.error(String(error)));
  };

  const drawer = active && drawerOpen ? (
    <aside
      data-testid="activity-drawer-browser"
      data-maho-space={state.activeSpaceId}
      data-maho-scheme={scheme}
      className="maho-drawer relative flex h-full shrink-0 flex-col overflow-hidden"
      style={{
        ...(spaceColor ? spacePaletteStyle(spaceColor, scheme) : {}),
        width: `${width}px`,
        minWidth: `${RANGE.min}px`,
        maxWidth: `${RANGE.max}px`,
      }}
    >
      <div data-tauri-drag-region className="h-titlebar shrink-0" />
      <div className="flex min-h-0 flex-1 flex-col">
        <MahoBrowserSidebar
          store={store}
          scheme={scheme}
          activeBrowserId={activeBrowserId}
          canGoBack={activeTab?.canGoBack === true}
          canGoForward={activeTab?.canGoForward === true}
          onBack={() => {
            if (activeBrowserId) void goBackInBrowserTab(activeBrowserId).catch(() => undefined);
          }}
          onForward={() => {
            if (activeBrowserId) void goForwardInBrowserTab(activeBrowserId).catch(() => undefined);
          }}
          onReload={() => {
            if (activeBrowserId) void reloadBrowser(activeBrowserId).catch(() => undefined);
          }}
          onActivateTab={activateTab}
          onOpenCommandBar={() => openCommandBar("navigate")}
          onNewTab={handleNewTab}
          onTidy={handleTidy}
          onCopyUrl={copyUrl}
          controlActivity={<MahoControlActivityChip activity={activity} />}
          agentSection={<MahoAgentTasksSection activity={activity} onSelectTab={activateTab} />}
          nowPlayingSlot={<MahoNowPlayingCard store={store} onSelectTab={revealTab} />}
          libraryOpen={libraryOpen}
          onToggleLibrary={() => setLibraryOpen((open) => !open)}
          libraryContent={
            <MahoLibraryMode
              store={store}
              onOpenTab={activateTab}
              onOpenUrl={(url) => void createMahoBrowserTab(url, store)}
              onClose={() => setLibraryOpen(false)}
            />
          }
        />
      </div>
      <div
        role="separator"
        aria-label="Resize browser sidebar"
        aria-orientation="vertical"
        aria-valuemin={RANGE.min}
        aria-valuemax={RANGE.max}
        aria-valuenow={width}
        tabIndex={0}
        onPointerDown={startResize}
        onLostPointerCapture={endResize}
        onKeyDown={resizeWithKeyboard}
        data-testid="browser-drawer-resize"
        className="no-drag absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none"
      />
    </aside>
  ) : null;

  const canvas = active ? (
    <section
      data-testid="activity-canvas-browser"
      className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background"
    >
      <BrowserModeCanvas
        store={store}
        active
        bannerSlot={
          activeBrowserId ? <MahoAgentControlledBanner activity={activity} browserId={activeBrowserId} /> : null
        }
        rightOverlayPane={
          miniBrowserId ? (
            <MahoMiniPane
              store={store}
              visible
              onDestroyMiniSession={destroyMiniBrowserSession}
              onPromoted={(browserId) => revealTab(browserId)}
            />
          ) : null
        }
        onNavigate={handleCanvasNavigate}
        onReload={handleCanvasReload}
        onNewTab={handleNewTab}
        onBack={(browserId) => void goBackInBrowserTab(browserId).catch(() => undefined)}
        onForward={(browserId) => void goForwardInBrowserTab(browserId).catch(() => undefined)}
        onOpenAddress={(browserId) => {
          if (browserId) activateTab(browserId);
          openCommandBar(browserId ? "navigate" : "new-tab");
        }}
        onCopyUrl={copyUrl}
        scheme={scheme}
      />
    </section>
  ) : null;

  const overlays = (
    <>
      <MahoCommandBar
        open={commandBar.open}
        initialQuery={commandBar.initialQuery}
        mode={commandBar.mode}
        store={store}
        commands={[
          { id: "new-tab", title: "New tab", run: handleNewTab },
          { id: "close-tab", title: "Close tab", run: closeActiveTab, disabled: !activeBrowserId },
          { id: "reload", title: "Reload tab", run: () => { if (activeBrowserId) handleCanvasReload(activeBrowserId); }, disabled: !activeBrowserId },
          { id: "copy-url", title: "Copy URL", run: () => { if (activeTab) copyUrl(activeTab.url); }, disabled: !activeTab },
          { id: "library", title: "Open library", run: () => { setLibraryOpen(true); if (!drawerOpen) dispatchBrowserAction("sidebar-toggle"); } },
          { id: "tidy", title: "Tidy tabs", run: handleTidy },
          { id: "sidebar", title: "Toggle sidebar", run: () => dispatchBrowserAction("sidebar-toggle") },
          { id: "split-right", title: "Split right", run: () => dispatchBrowserAction("split-right") },
          { id: "split-down", title: "Split down", run: () => dispatchBrowserAction("split-down") },
          { id: "back", title: "Go back", run: () => { if (activeBrowserId) void goBackInBrowserTab(activeBrowserId).catch((error) => toast.error(String(error))); }, disabled: !activeTab?.canGoBack },
          { id: "forward", title: "Go forward", run: () => { if (activeBrowserId) void goForwardInBrowserTab(activeBrowserId).catch((error) => toast.error(String(error))); }, disabled: !activeTab?.canGoForward },
        ]}
        onClose={closeCommandBar}
        onSelectOpenTab={activateTab}
        onNavigate={handleCommandBarNavigate}
      />
      <MahoTabSwitcher
        open={switcher.open}
        direction={switcher.direction}
        tabs={mruTabs}
        selectedIndex={switcher.index}
        thumbnails={thumbnails}
        onShowAllTabs={() => openCommandBar("all-tabs")}
        onSelect={activateTab}
        onClose={() => setSwitcher({ open: false, direction: "forward", index: 0 })}
        store={store}
      />
    </>
  );

  return { drawer, canvas, overlays };
}

export type MahoBrowserMiniPaneProps = {
  /** Whether this slot is the one on screen. A hidden slot must not claim the native page. */
  visible: boolean;
  store?: BrowserStore;
};

/**
 * The Transient Mini Pane's right split slot for the modes the canvas does not draw. It is
 * the only mini host outside Browser mode, which is what keeps one `browserId` on one surface:
 * Browser mode renders the mini inside its canvas overlay instead.
 */
export function MahoBrowserMiniPane({ visible, store = browserStore }: MahoBrowserMiniPaneProps) {
  const miniBrowserId = useBrowserStore((snapshot) => snapshot.miniBrowserId, store);
  if (!visible || !miniBrowserId) return null;
  return (
    <div
      data-testid="maho-mini-slot"
      className="flex h-full min-h-0 shrink-0 flex-col overflow-hidden border-l border-border bg-background"
      style={{ width: `${MINI_PANE_WIDTH}px` }}
    >
      <MahoMiniPane
        store={store}
        visible
        onDestroyMiniSession={destroyMiniBrowserSession}
        onPromoted={(browserId) => {
          store.touch(browserId);
        }}
      />
    </div>
  );
}
