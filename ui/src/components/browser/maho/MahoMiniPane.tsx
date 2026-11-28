// Maho Mini, unified with Maho's Peek as an in-app Transient Mini Pane.
//
// Maho ships `MahoMiniWindow` (a 600x760 popup) and `MahoPeekController` (an in-window
// transient preview slot). A separate OS window per peek is exactly the cross-OS surface
// problem `BrowserViewportCoordinator` already solves for splits, so both collapse into one
// component: a right-split slot that draws Maho Mini's chrome and renders the live
// `miniBrowserId` through `BrowserPane chromeMode="maho-minimal"`.
//
// The slot is single by design. Opening a second mini URL replaces it: the store's
// `miniBrowserId` changes, and the previous transient session is destroyed through
// `onDestroyMiniSession`. `⌘O` / "Open in <Space>" is the other way out - it promotes the very
// same `browserId` into a Space's Today list, so the page keeps its state and the coordinator
// hands the native surface over without a reload.

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Check, ChevronDown, ChevronUp, Globe, Link, X } from "lucide-react";

import {
  browserStore,
  selectSpaceList,
  useBrowserStore,
  type BrowserStore,
  type BrowserStoreTab,
} from "../../../state/browserStore";
import { navigateBrowser, reloadBrowser } from "../../../lib/browserTauri";
import { normalizeBrowserAddress, type BrowserSettingsState, loadBrowserSettings } from "../../../lib/browserSettings";
import { isMacShortcutPlatform } from "../../../lib/shortcuts";
import type { BrowserTab } from "../../../lib/types";
import { BrowserPane } from "../../BrowserPane";
import { displayHost } from "./mahoBrowserTabActions";
import { spacePaletteStyle } from "./mahoSpacePalette";
import "./maho.css";

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
  /** Called after the slot was cleared, so the host can collapse its split. */
  onClose?: () => void;
};

function miniTabFor(tab: BrowserStoreTab): BrowserTab {
  return {
    kind: "browser",
    id: `maho-mini-${tab.browserId}`,
    label: tab.title || displayHost(tab.url),
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
    setDraftUrl(miniUrl && miniUrl !== "about:blank" ? displayHost(miniUrl) : "");
    setCopied(false);
    setCompact(false);
  }, [miniBrowserId, miniUrl]);

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

  const host = displayHost(miniTab.url);
  const paletteStyle = spacePaletteStyle((chosenSpace ?? spaces[0])?.color ?? "#6C8CFF", "dark");

  return (
    <section
      className="maho-mini-pane flex h-full min-h-0 w-full flex-col overflow-hidden border-l border-border bg-background"
      data-maho-space={chosenSpace?.id ?? ""}
      data-testid="maho-mini-pane"
      data-visible={visible ? "true" : "false"}
      data-compact={compact ? "true" : "false"}
      aria-label="Maho Mini"
      aria-hidden={visible ? undefined : true}
      style={paletteStyle as unknown as CSSProperties}
    >
      {/* One row, Maho Mini's own order: `[Space chip | centered host | copy] [Open in <Space>
          ⌘O | Space selector] [x]`. */}
      <div
        className="maho-mini-bar"
        role="toolbar"
        aria-label="Maho Mini bar"
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
      <div className="maho-mini-page min-h-0 flex-1">
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
    </section>
  );
}
