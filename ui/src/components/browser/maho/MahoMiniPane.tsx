// Maho Mini, unified with Maho's Peek as an in-app Transient Mini Pane.
//
// Maho ships `MahoMiniWindow` (a 600x760 popup) and `MahoPeekController` (an in-window
// transient preview slot). A separate OS window per peek is exactly the cross-OS surface
// problem `BrowserViewportCoordinator` already solves for splits, so both collapse into one
// component: a right-split slot that draws Maho Mini's chrome and renders the live
// `miniBrowserId` through `BrowserPane chromeMode="maho-minimal"`.
//
// Peek's own presentation is kept: the pane is a rounded card with a shadow floating over the
// canvas edge, sized and placed by the user. That geometry is remembered across opens through
// `mahoMiniPanePrefs.ts` (`ferryx.maho.miniPane.prefs`), clamped to the viewport on read, and the
// card is dragged by its bar and resized from its corner handle. Clicking the page forwards
// keyboard focus to the native surface (`focusBrowser`), the way Peek hands typing straight to
// the previewed page.
//
// The slot is single by design. Opening a second mini URL replaces it: the store's
// `miniBrowserId` changes, and the previous transient session is destroyed through
// `onDestroyMiniSession`. `⌘O` / "Open in <Space>" is the other way out - it promotes the very
// same `browserId` into a Space's Today list, so the page keeps its state and the coordinator
// hands the native surface over without a reload.

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { Check, ChevronDown, ChevronUp, Globe, Link, X } from "lucide-react";

import {
  browserStore,
  selectSpaceList,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { focusBrowser, navigateBrowser, reloadBrowser } from "../../../lib/browserTauri";
import { normalizeBrowserAddress, type BrowserSettingsState, loadBrowserSettings } from "../../../lib/browserSettings";
import {
  MINI_PANE_VIEWPORT_MARGIN,
  clampMiniPaneGeometry,
  isDockedGeometry,
  loadMahoMiniPaneGeometry,
  saveMahoMiniPaneGeometry,
  type MahoMiniPaneGeometry,
} from "../../../lib/mahoMiniPanePrefs";
import { shortHost } from "../../../lib/registrableDomain";
import { isMacShortcutPlatform } from "../../../lib/shortcuts";
import type { BrowserTab } from "../../../lib/types";
import { BrowserPane } from "../../BrowserPane";
import { displayHost } from "./mahoBrowserTabActions";
import { spacePaletteStyle } from "./mahoSpacePalette";
import "./maho.css";

/** Gap the docked card keeps below the window's titlebar/toolbar row. */
const DOCKED_TOP_INSET = 64;

export type MahoMiniPaneProps = {
  store?: BrowserStore;
  /** Host visibility; a hidden host keeps the session alive but hides the native page. */
  visible?: boolean;
  /** Space the promote button targets. The dropdown overrides it until the user picks again. */
  targetSpaceId?: string;
  /** Hands the promoted browserId to the host, which mounts it in the Space it landed in. */
  onPromoted?: (browserId: string, spaceId: string) => void;
  /** Terminates a transient session: replaced by a newer mini, or closed by Esc / x. */
  onDestroyMiniSession: (browserId: string) => void;
  /** Overridable navigation, so tests and hosts can observe it without a backend. */
  onNavigate?: (browserId: string, url: string) => void;
  onReload?: (browserId: string) => void;
  /** Overridable clipboard, for hosts that own one and for tests. */
  onCopyUrl?: (url: string) => void;
  /** Overridable focus forwarding, for hosts that own the native surface and for tests. */
  onFocusPage?: (browserId: string) => void;
  /** Restored card geometry; the stored value when the host does not supply one. */
  initialGeometry?: MahoMiniPaneGeometry;
  /** Called after the slot was cleared, so the host can collapse its split. */
  onClose?: () => void;
};

function miniTabFor(tab: BrowserStoreTab): BrowserTab {
  return {
    kind: "browser",
    id: `maho-mini-${tab.browserId}`,
    label: tab.title || shortHost(tab.url, displayHost(tab.url)),
    browserId: tab.browserId,
    url: tab.url,
    title: tab.title,
    canGoBack: false,
    canGoForward: false,
    loading: false,
    ...(tab.engine ? { engine: tab.engine } : {}),
    ...(tab.private ? { profileId: "private" } : {}),
    ...(tab.worktreeTag ? { worktreePath: tab.worktreeTag.worktreePath } : {}),
  };
}

export function MahoMiniPane({
  store = browserStore,
  visible = true,
  targetSpaceId,
  onPromoted,
  onDestroyMiniSession,
  onNavigate,
  onReload,
  onCopyUrl,
  onFocusPage,
  initialGeometry,
  onClose,
}: MahoMiniPaneProps) {
  const state = useBrowserStore((snapshot) => snapshot, store);
  const miniBrowserId = state.miniBrowserId;
  const miniTab = miniBrowserId ? state.tabs[miniBrowserId] ?? null : null;
  const miniUrl = miniTab?.url ?? null;
  const spaces = useMemo(() => selectSpaceList(state), [state]);
  const activeSpaceId = state.activeSpaceId;

  const [selectedSpaceId, setSelectedSpaceId] = useState<string | null>(targetSpaceId ?? null);
  const [draftUrl, setDraftUrl] = useState("");
  const [copied, setCopied] = useState(false);
  const [compact, setCompact] = useState(false);
  // Peek's card geometry: restored on mount, written back when a drag or a resize ends.
  const [geometry, setGeometry] = useState<MahoMiniPaneGeometry>(() => (
    initialGeometry ?? loadMahoMiniPaneGeometry(undefined, viewportSize())
  ));
  const geometryRef = useRef(geometry);
  geometryRef.current = geometry;
  const dragRef = useRef<null | { kind: "move" | "resize"; startX: number; startY: number; base: MahoMiniPaneGeometry }>(null);
  const settingsRef = useRef<BrowserSettingsState>(loadBrowserSettings());
  settingsRef.current = loadBrowserSettings();

  /** The transient session this pane is currently holding. */
  const liveIdRef = useRef<string | null>(null);
  /** A browserId that was promoted, not replaced: its session must survive the slot clearing. */
  const promotedRef = useRef<string | null>(null);

  // A target Space can be deleted while the mini is open: fall back to the active Space, then to
  // whatever Space still exists, so the selector and the promote button never point at a ghost.
  const requestedSpaceId = selectedSpaceId ?? targetSpaceId ?? activeSpaceId;
  const chosenSpace =
    spaces.find((space) => space.id === requestedSpaceId)
    ?? spaces.find((space) => space.id === activeSpaceId)
    ?? spaces[0]
    ?? null;
  const chosenSpaceId = chosenSpace?.id ?? "";
  const shortcutLabel = isMacShortcutPlatform() ? "⌘O" : "Ctrl+O";

  useEffect(() => {
    setDraftUrl(miniUrl && miniUrl !== "about:blank" ? shortHost(miniUrl, displayHost(miniUrl)) : "");
    setCopied(false);
    setCompact(false);
  }, [miniBrowserId, miniUrl]);

  // Window resizes can strand a dragged card off-screen; re-clamp and re-persist.
  useEffect(() => {
    if (typeof window === "undefined") return;
    const onResize = () => {
      setGeometry((current) => clampMiniPaneGeometry(current, viewportSize()));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const beginDrag = useCallback((kind: "move" | "resize", event: ReactPointerEvent<HTMLElement>) => {
    // A drag that starts on a control is that control's, not the card's.
    if (kind === "move" && event.target instanceof Element && event.target.closest("button, input, select, label, a")) {
      return;
    }
    event.preventDefault();
    dragRef.current = { kind, startX: event.clientX, startY: event.clientY, base: geometryRef.current };
    let latest = geometryRef.current;
    const onMove = (moveEvent: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const dx = moveEvent.clientX - drag.startX;
      const dy = moveEvent.clientY - drag.startY;
      const next = drag.kind === "move"
        ? { ...drag.base, x: drag.base.x + dx, y: drag.base.y + dy }
        : { ...drag.base, width: drag.base.width + dx, height: drag.base.height + dy };
      latest = clampMiniPaneGeometry(next, viewportSize());
      setGeometry(latest);
    };
    const onUp = () => {
      dragRef.current = null;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      // The gesture is a user preference: persist the clamped result once, not per frame. The
      // value comes from this closure, not from a render that may not have run yet.
      saveMahoMiniPaneGeometry(latest);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  }, []);

  /** Hands keyboard focus to the native page so typing lands in the previewed site. */
  const focusPage = useCallback(() => {
    if (!miniTab) return;
    if (onFocusPage) {
      onFocusPage(miniTab.browserId);
      return;
    }
    try {
      void focusBrowser(miniTab.browserId).catch(() => undefined);
    } catch {
      // A host without the focus bridge keeps the app webview's focus.
    }
  }, [miniTab, onFocusPage]);

  // A second mini replaces the slot: the session the pane was holding is transient, so it ends
  // here. A promoted id is exempt - `promoteMini` clears the slot precisely to keep it.
  useEffect(() => {
    const previous = liveIdRef.current;
    liveIdRef.current = miniBrowserId;
    if (!previous || previous === miniBrowserId) {
      return;
    }
    if (promotedRef.current === previous) {
      promotedRef.current = null;
      return;
    }
    onDestroyMiniSession(previous);
  }, [miniBrowserId, onDestroyMiniSession]);

  const close = useCallback(() => {
    const browserId = store.getState().miniBrowserId;
    if (!browserId) return;
    // Clearing the slot is this call's own destroy, so the replace-watcher must not repeat it.
    liveIdRef.current = null;
    store.setMini(null);
    onDestroyMiniSession(browserId);
    onClose?.();
  }, [onClose, onDestroyMiniSession, store]);

  const promote = useCallback(() => {
    const current = store.getState();
    const browserId = current.miniBrowserId;
    if (!browserId || !chosenSpaceId) return;
    promotedRef.current = browserId;
    const result = store.promoteMini(chosenSpaceId);
    if (!result.ok) {
      promotedRef.current = null;
      return;
    }
    onPromoted?.(browserId, chosenSpaceId);
  }, [chosenSpaceId, onPromoted, store]);

  useEffect(() => {
    if (!visible || !miniBrowserId) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        return;
      }
      // Cmd+O on macOS, Ctrl+O elsewhere: promote, never a reload.
      if (event.key.toLowerCase() === "o" && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey) {
        event.preventDefault();
        promote();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [close, miniBrowserId, promote, visible]);

  const navigateTo = useCallback(
    (browserId: string, input: string) => {
      const url = normalizeBrowserAddress(input, settingsRef.current);
      if (!url || url === "about:blank") return;
      if (onNavigate) {
        onNavigate(browserId, url);
        return;
      }
      void navigateBrowser(browserId, url).catch(() => undefined);
    },
    [onNavigate],
  );

  const reload = useCallback(
    (browserId: string) => {
      if (onReload) {
        onReload(browserId);
        return;
      }
      void reloadBrowser(browserId).catch(() => undefined);
    },
    [onReload],
  );

  const copyUrl = useCallback(() => {
    const url = miniTab?.url;
    if (!url || url === "about:blank") return;
    if (onCopyUrl) {
      onCopyUrl(url);
      setCopied(true);
      return;
    }
    const clipboard = typeof navigator === "undefined" ? undefined : navigator.clipboard;
    if (!clipboard) return;
    void clipboard.writeText(url).then(
      () => setCopied(true),
      () => undefined,
    );
  }, [miniTab?.url, onCopyUrl]);

  if (!miniBrowserId || !miniTab) return null;

  const host = shortHost(miniTab.url, displayHost(miniTab.url));
  const paletteStyle = spacePaletteStyle((chosenSpace ?? spaces[0])?.color ?? "#6C8CFF", "dark");
  // Peek's card presentation: docked to the canvas edge until the user drags it elsewhere. The
  // dock offsets are inline (not CSS) so the card's place is readable from the DOM, which is what
  // the card's own tests and the floating-overlay host both rely on. Physical `right`/`left` are
  // used because the app draws no RTL surface, and they are the properties every engine renders.
  const docked = isDockedGeometry(geometry);
  const cardStyle: CSSProperties = {
    ...(paletteStyle as unknown as CSSProperties),
    position: "fixed",
    width: `${geometry.width}px`,
    height: `${geometry.height}px`,
    maxWidth: `calc(100vw - ${MINI_PANE_VIEWPORT_MARGIN * 2}px)`,
    maxHeight: `calc(100vh - ${MINI_PANE_VIEWPORT_MARGIN * 2}px)`,
    ...(docked
      ? { right: `${MINI_PANE_VIEWPORT_MARGIN}px`, top: `${DOCKED_TOP_INSET}px` }
      : { left: `${geometry.x}px`, top: `${geometry.y}px` }),
  };

  return (
    <section
      className="maho-mini-pane maho-mini-card flex min-h-0 flex-col overflow-hidden"
      data-maho-space={chosenSpace?.id ?? ""}
      data-testid="maho-mini-pane"
      data-presentation="peek"
      data-docked={docked ? "true" : "false"}
      data-visible={visible ? "true" : "false"}
      data-compact={compact ? "true" : "false"}
      aria-label="Maho Mini"
      aria-hidden={visible ? undefined : true}
      style={cardStyle}
    >
      {/* One row, Maho Mini's own order: `[Space chip | centered host | copy] [Open in <Space>
          ⌘O | Space selector] [x]`. The row is the card's drag handle. */}
      <div
        className="maho-mini-bar"
        role="toolbar"
        aria-label="Maho Mini bar"
        data-maho-mini-drag="move"
        onPointerDown={(event) => beginDrag("move", event)}
      >
        <div className="maho-mini-integrated">
        <span
          className="maho-mini-space"
          data-testid="maho-mini-space-chip"
          title={`Space: ${chosenSpace?.name ?? ""}`}
        >
          <Globe className="size-3.5 shrink-0" style={{ color: chosenSpace?.color }} aria-hidden="true" />
          <span className="truncate">{chosenSpace?.name ?? "Space"}</span>
        </span>
        <input
          className="maho-mini-address"
          aria-label="Address"
          value={draftUrl}
          placeholder="Search or enter address"
          title={host}
          onChange={(event) => setDraftUrl(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            navigateTo(miniTab.browserId, draftUrl);
          }}
        />
        <button
          type="button"
          className="maho-mini-copy maho-top-bar__button"
          aria-label="Copy link"
          title="Copy link"
          onClick={copyUrl}
        >
          {copied ? <Check aria-hidden="true" /> : <Link aria-hidden="true" />}
        </button>
        </div>
        <div className="maho-mini-promote-control">
        <button
          type="button"
          className="maho-mini-promote"
          aria-label={`Open in ${chosenSpace?.name ?? "Space"}`}
          title={`Open in ${chosenSpace?.name ?? "Space"} (${shortcutLabel})`}
          onClick={promote}
        >
          <span className="max-w-28 truncate">Open in {chosenSpace?.name ?? "Space"}</span>
          <kbd>{shortcutLabel}</kbd>
        </button>
        <label className="maho-mini-choose">
        <ChevronDown aria-hidden="true" />
        <select
          aria-label="Choose Space"
          value={chosenSpaceId}
          onChange={(event) => setSelectedSpaceId(event.target.value)}
        >
          {spaces.map((space) => (
            <option key={space.id} value={space.id}>
              {space.name}
            </option>
          ))}
        </select>
        </label>
        </div>
        <button type="button" className="maho-mini-toggle maho-top-bar__button" aria-label={compact ? "Expand Mini window" : "Compact Mini window"} onClick={() => setCompact((value) => !value)}>
          {compact ? <ChevronUp aria-hidden="true" /> : <ChevronDown aria-hidden="true" />}
        </button>
        <button
          type="button"
          className="maho-top-bar__button"
          aria-label="Close mini"
          title="Close mini (Esc)"
          onClick={close}
        >
          <X aria-hidden="true" />
        </button>
      </div>
      <div
        className="maho-mini-page min-h-0 flex-1"
        data-testid="maho-mini-page"
        onPointerDown={() => focusPage()}
      >
        {/* A hidden mini keeps its transient session in the store but must not touch the native
            surface: `BrowserPane` claims `browserViewportCoordinator` on mount and hides the
            surface whenever it is not visible, which would blank a live worktree split or
            Browser-mode pane showing this same `browserId`. */}
        {visible && !compact ? (
          <BrowserPane
            tab={miniTabFor(miniTab)}
            visible
            chromeMode="maho-minimal"
            onNavigate={(url) => navigateTo(miniTab.browserId, url)}
            onReload={() => reload(miniTab.browserId)}
          />
        ) : null}
      </div>
      {/* Peek's corner grip: the card is the user's, so it resizes from the bottom-inline edge. */}
      <button
        type="button"
        className="maho-mini-resize"
        aria-label="Resize Mini window"
        title="Resize Mini window"
        data-maho-mini-drag="resize"
        onPointerDown={(event) => beginDrag("resize", event)}
      />
    </section>
  );
}

/** The window box a geometry is clamped against; zero when there is no window (tests, SSR). */
function viewportSize(): { width: number; height: number } | null {
  if (typeof window === "undefined") return null;
  return { width: window.innerWidth, height: window.innerHeight };
}
