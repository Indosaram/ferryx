import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  ArrowRight,
  RotateCw,
  ExternalLink,
  ZoomIn,
  ZoomOut,
  Target,
  RefreshCw,
  History as HistoryIcon,
  Bug,
  MousePointer2,
  Star,
  Search as SearchIcon,
  AppWindow,
  Bookmark as BookmarkIcon,
  Library as LibraryIcon,
  Download as DownloadIcon,
  Printer,
  FileDown,
  MoreHorizontal,
  Puzzle,
  type LucideIcon,
} from "lucide-react";
import {
  BROWSER_SHORTCUT_EVENT,
  extractBrowserErrorCode,
  focusBrowser,
  getBrowserEngineStatus,
  getBrowserState,
  getBrowserSnapshotCapability,
  goBackBrowser,
  goForwardBrowser,
  deliverDesignFeedback,
  finishBrowserElementPick,
  injectBrowserElementPicker,
  onBrowserElementPicked,
  onBrowserShortcutRequested,
  openBrowserDevtools,
  removeBrowserElementPicker,
  openExternalUrl,
  setBrowserZoom,
  type BrowserEngineStatus,
  type BrowserDesignSnapshot,
  type BrowserReloadOptions,
  type BrowserShortcutAction,
  type BrowserShortcutDomEvent,
  type BrowserSnapshotCapability,
  type DesignFeedbackTarget,
} from "../lib/browserTauri";
import { BrowserDesignFeedbackPopover } from "./BrowserDesignFeedbackPopover";
import { openBrowserLibraryView } from "./BrowserLibraryPanel";
import { printBrowser, saveBrowserPdf } from "../lib/browserSystem";
import {
  addBookmark,
  clearAllBrowserHistory,
  deleteBookmark,
  fetchSearchSuggestions,
  findBookmarkByUrl,
  listHostHistory,
  onBrowserLibraryChanged,
  openBrowserLibrary,
  rankOmniboxSuggestions,
  useBookmarks,
  type HostHistoryEntry,
  type OmniboxSuggestion,
} from "../lib/browserLibrary";
import { requestSwitchToBrowserTab } from "../lib/browserLibrarySync";
import { BROWSER_ZOOM_LEVELS, normalizeBrowserAddress, searchUrlFor, useBrowserSettings } from "../lib/browserSettings";
import { openSettingsSection } from "../lib/browserExtensions";
import { isMacHost } from "../lib/platform";
import { isHardReloadSupportedPlatform } from "../lib/shortcuts";
import type { BrowserTab } from "../lib/types";

// Keep the toolbar's zoom clamps in lockstep with the Settings-exposed zoom levels
// (BROWSER_ZOOM_LEVELS percentages), so a zoom restored by the backend stays reachable.
const BROWSER_ZOOM_MIN = Math.min(...BROWSER_ZOOM_LEVELS) / 100;
const BROWSER_ZOOM_MAX = Math.max(...BROWSER_ZOOM_LEVELS) / 100;

/** Below this toolbar width the secondary actions move into the "More" row. */
export const BROWSER_TOOLBAR_COMPACT_WIDTH = 720;

const TOOLBAR_BUTTON_BASE =
  "inline-flex shrink-0 items-center rounded text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted-foreground";
/** The one class set every icon-only toolbar button shares. */
export const TOOLBAR_ICON_BUTTON = `${TOOLBAR_BUTTON_BASE} size-7 justify-center`;
const TOOLBAR_ROW_BUTTON = `${TOOLBAR_BUTTON_BASE} h-7 gap-1.5 px-2 text-[11px]`;

type ToolbarButtonProps = {
  icon: LucideIcon;
  label: string;
  title: string;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  variant?: "icon" | "row";
  disabled?: boolean;
  pressed?: boolean;
  expanded?: boolean;
  hasPopup?: boolean;
  controls?: string;
  testId?: string;
  shortcut?: string;
  iconClassName?: string;
};

/** Icon button (toolbar) or icon + text button (inline rows): same label, tooltip and focus ring. */
function ToolbarButton({
  icon: Icon,
  label,
  title,
  onClick,
  variant = "icon",
  disabled,
  pressed,
  expanded,
  hasPopup,
  controls,
  testId,
  shortcut,
  iconClassName = "",
}: ToolbarButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={label}
      aria-pressed={pressed}
      aria-expanded={expanded}
      aria-haspopup={hasPopup ? "true" : undefined}
      aria-controls={controls}
      data-testid={testId}
      data-shortcut={shortcut}
      className={`${variant === "row" ? TOOLBAR_ROW_BUTTON : TOOLBAR_ICON_BUTTON} ${pressed ? "bg-muted text-foreground" : ""}`}
    >
      <Icon className={`size-3.5 ${iconClassName}`} />
      {variant === "row" ? <span>{label}</span> : null}
    </button>
  );
}

type ExtensionsStatus = "loading" | "error" | BrowserEngineStatus;

/** Every toolbar failure names the action the user took plus the backend's structured code, so a
 * failed action can never read as a successful one. The code comes from the IPC error contract,
 * never from matching message prose. */
function actionFailureMessage(action: string, error: unknown): string {
  return `${action} failed: ${extractBrowserErrorCode(error)}`;
}

interface BrowserToolbarProps {
  tab: BrowserTab;
  onNavigate: (url: string) => void;
  // Reload carries an optional cache-bypass request: the backend reaches the platform binding
  // (macOS `WKWebView.reloadFromOrigin`, Linux WebKitGTK `reload_bypass_cache`) and answers a
  // structured error where this build has no such binding (Windows carries no WebView2 binding),
  // so the flag is never silently dropped on the way to the engine.
  onReload: (options?: BrowserReloadOptions) => void;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onToggleElementPick?: () => void;
  elementPicking?: boolean;
  designFeedbackTargets?: DesignFeedbackTarget[];
  /** Other open browser tabs, offered by the omnibox as "switch to tab". */
  openTabs?: Array<{ browserId: string; url: string; title: string | null }>;
}

export function BrowserToolbar({
  tab,
  onNavigate,
  onReload,
  onGoBack,
  onGoForward,
  onToggleElementPick,
  elementPicking: elementPickingProp,
  designFeedbackTargets = [],
  openTabs = [],
}: BrowserToolbarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const omniboxId = useId();
  const omniboxListId = `${omniboxId}-history`;
  const moreRowId = `${omniboxId}-more`;
  const extensionsRowId = `${omniboxId}-extensions`;
  // 0 means "not measured yet" (first paint, jsdom): treated as wide so nothing is hidden blindly.
  const [toolbarWidth, setToolbarWidth] = useState(0);
  const [moreOpen, setMoreOpen] = useState(false);
  const [extensionsOpen, setExtensionsOpen] = useState(false);
  const [extensionsStatus, setExtensionsStatus] = useState<ExtensionsStatus>("loading");
  const [extensionsErrorCode, setExtensionsErrorCode] = useState<string | null>(null);
  const [inputUrl, setInputUrl] = useState(tab.url);
  // Set while the user has typed into the omnibox and not yet submitted, blurred or escaped, so
  // a live URL update from the page cannot overwrite half-typed text.
  const inputDirtyRef = useRef(false);
  const [historyQuery, setHistoryQuery] = useState("");
  const [zoomFactor, setZoomFactor] = useState(tab.zoomFactor ?? 1.0);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [omniboxOpen, setOmniboxOpen] = useState(false);
  const [selectedHistoryIndex, setSelectedHistoryIndex] = useState(-1);
  // Recent pages (host store) for the History menu and for the omnibox before typing.
  const [historyEntries, setHistoryEntries] = useState<HostHistoryEntry[]>([]);
  // Host matches for the typed text, ranked by frecency.
  const [historyMatches, setHistoryMatches] = useState<HostHistoryEntry[]>([]);
  const [searchSuggestions, setSearchSuggestions] = useState<string[]>([]);
  const { tree: bookmarkTree } = useBookmarks();
  const [snapshotCapability, setSnapshotCapability] = useState<BrowserSnapshotCapability | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [designSnapshot, setDesignSnapshot] = useState<BrowserDesignSnapshot | null>(null);
  const [designSending, setDesignSending] = useState(false);
  const [designError, setDesignError] = useState<string | null>(null);
  // The toolbar owns the picker's armed state so the element-pick -> design-feedback flow works
  // without a parent; a parent that passes `elementPicking` still drives it.
  const [elementPicking, setElementPicking] = useState(elementPickingProp ?? false);
  const { settings, updateSettings } = useBrowserSettings();

  // The toolbar measures its own width (not the window's): panes can be split narrower than the
  // viewport, and the right cluster must yield to the address bar at the pane's width.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const measure = () => setToolbarWidth(el.getBoundingClientRect().width);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const compact = toolbarWidth > 0 && toolbarWidth < BROWSER_TOOLBAR_COMPACT_WIDTH;
  const moreRowOpen = compact && moreOpen;

  // Engine status is only needed once the user opens the row, so it is fetched lazily then.
  useEffect(() => {
    if (!extensionsOpen) return;
    let disposed = false;
    setExtensionsStatus("loading");
    setExtensionsErrorCode(null);
    void Promise.resolve()
      .then(() => getBrowserEngineStatus())
      .then((status) => {
        if (!disposed) setExtensionsStatus(status);
      })
      .catch((error: unknown) => {
        if (disposed) return;
        setExtensionsErrorCode(extractBrowserErrorCode(error));
        setExtensionsStatus("error");
      });
    return () => {
      disposed = true;
    };
  }, [extensionsOpen]);

  // Picking needs native snapshot capture AND this tab's isolated-world bridge, which the
  // backend reports as `pick`. An unknown capability (an older backend without the command)
  // keeps the control as-is; a reported capability enables picking only when `pick` is true.
  const elementPickSupported = snapshotCapability === null || snapshotCapability.pick === true;

  const omniboxEntries: OmniboxSuggestion[] = useMemo(() => {
    const typed = historyQuery.trim();
    const history = settings.rememberBrowsingHistory ? (typed ? historyMatches : historyEntries) : [];
    if (!typed && history.length === 0) return [];
    return rankOmniboxSuggestions(historyQuery, {
      history,
      bookmarks: bookmarkTree.bookmarks,
      tabs: openTabs,
      searchSuggestions: typed && settings.searchSuggestions ? searchSuggestions : [],
      currentBrowserId: tab.browserId,
    }, 8);
  }, [bookmarkTree.bookmarks, historyEntries, historyMatches, historyQuery, openTabs, searchSuggestions, settings.rememberBrowsingHistory, settings.searchSuggestions, tab.browserId]);
  const currentBookmark = useMemo(() => findBookmarkByUrl(bookmarkTree, tab.url), [bookmarkTree, tab.url]);

  useEffect(() => {
    if (elementPickingProp !== undefined) setElementPicking(elementPickingProp);
  }, [elementPickingProp]);

  const pickBrowserIdRef = useRef(tab.browserId);
  useEffect(() => {
    // A picker armed for one browser must not stay pressed for another reusing this toolbar.
    if (pickBrowserIdRef.current === tab.browserId) return;
    pickBrowserIdRef.current = tab.browserId;
    setElementPicking(false);
    setDesignSnapshot(null);
    setDesignError(null);
  }, [tab.browserId]);

  useEffect(() => {
    if (inputDirtyRef.current) return;
    setInputUrl(tab.url);
    setHistoryQuery("");
    // A committed navigation ends the omnibox session: its suggestions describe the old page.
    setOmniboxOpen(false);
    setSelectedHistoryIndex(-1);
  }, [tab.url]);

  useEffect(() => {
    if (tab.loading && !inputDirtyRef.current) {
      setOmniboxOpen(false);
      setSelectedHistoryIndex(-1);
    }
  }, [tab.loading]);

  // Clicking into the page moves focus to the native page view, which the toolbar sees only as
  // the app window losing focus; the suggestion list must not stay drawn over the page then.
  useEffect(() => {
    const closeOnWindowBlur = () => {
      setOmniboxOpen(false);
      setSelectedHistoryIndex(-1);
    };
    window.addEventListener("blur", closeOnWindowBlur);
    return () => window.removeEventListener("blur", closeOnWindowBlur);
  }, []);

  useEffect(() => {
    setZoomFactor(tab.zoomFactor ?? 1.0);
  }, [tab.browserId, tab.zoomFactor]);

  // History lives in the host store; any window's visit refreshes the recent list.
  const refreshRecentHistory = useRef(() => undefined as void);
  refreshRecentHistory.current = () => {
    if (!settings.rememberBrowsingHistory) {
      setHistoryEntries([]);
      return;
    }
    void Promise.resolve()
      .then(() => listHostHistory("", undefined, 50))
      .then((entries) => setHistoryEntries(entries ?? []))
      .catch(() => setHistoryEntries([]));
  };
  useEffect(() => {
    refreshRecentHistory.current();
  }, [settings.rememberBrowsingHistory]);
  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void Promise.resolve()
      .then(() => onBrowserLibraryChanged((kind) => {
        if (kind === "history") refreshRecentHistory.current();
      }))
      .then((cleanup) => {
        if (disposed) cleanup?.();
        else unlisten = cleanup;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Typed text: host history matches, and (opt-in) search-engine suggestions. The latest query
  // wins; a failed or offline suggest call leaves local suggestions only.
  useEffect(() => {
    const query = historyQuery.trim();
    if (!query) {
      setHistoryMatches([]);
      setSearchSuggestions([]);
      return;
    }
    let stale = false;
    if (settings.rememberBrowsingHistory) {
      void Promise.resolve()
        .then(() => listHostHistory(query, undefined, 50))
        .then((entries) => { if (!stale) setHistoryMatches(entries ?? []); })
        .catch(() => { if (!stale) setHistoryMatches([]); });
    }
    if (settings.searchSuggestions) {
      void Promise.resolve()
        .then(() => fetchSearchSuggestions(settings.searchEngine, query))
        .then((items) => { if (!stale) setSearchSuggestions(items ?? []); })
        .catch(() => { if (!stale) setSearchSuggestions([]); });
    }
    return () => {
      stale = true;
    };
  }, [historyQuery, settings.rememberBrowsingHistory, settings.searchEngine, settings.searchSuggestions]);

  const toggleBookmark = async () => {
    if (!/^(https?|file):/i.test(tab.url)) return;
    try {
      if (currentBookmark) await deleteBookmark(currentBookmark.id);
      else await addBookmark(tab.title?.trim() || tab.url, tab.url);
      setActionError(null);
    } catch (error) {
      setActionError(actionFailureMessage("Bookmark", error));
    }
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let lastTrustedBridge: unknown;
    // The IPC binding may be unavailable or partial (missing export, non-function, sync throw),
    // and toolbar rendering must never break over it. Deferring the call into the promise chain
    // routes any such failure into the catch below. The capability only decides whether one
    // control is offered, so an unknown capability must leave the control as it is today.
    const refreshCapability = () => {
      void Promise.resolve()
        .then(() => getBrowserSnapshotCapability(tab.browserId))
        .then((capability) => {
          // A backend that answers without a capability object is as unknown as one without the command.
          if (!disposed) setSnapshotCapability(capability ?? null);
        })
        .catch(() => {
          // Older backends do not expose the capability command; leave the control untouched.
        });
    };
    refreshCapability();
    // `pick` depends on the tab's trusted bridge, which the backend records after creation
    // (and on Windows upgrades later still). Re-read the capability when it changes.
    void Promise.resolve()
      .then(() =>
        listen<{ browserId?: string; trustedBridge?: unknown }>("browser_state_changed", (event) => {
          const { browserId, trustedBridge } = event.payload ?? {};
          if (browserId !== tab.browserId || trustedBridge === undefined) return;
          if (trustedBridge === lastTrustedBridge) return;
          lastTrustedBridge = trustedBridge;
          refreshCapability();
        }),
      )
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {
        // Without the event the capability stays as first read; the control still renders.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tab.browserId]);

  const handleGoBack = async () => {
    try {
      await goBackBrowser(tab.browserId);
      setActionError(null);
      onGoBack?.();
    } catch (error) {
      // Native state remains authoritative, but the user's click must not vanish as a no-op.
      setActionError(actionFailureMessage("Back", error));
    }
  };

  const handleGoForward = async () => {
    try {
      await goForwardBrowser(tab.browserId);
      setActionError(null);
      onGoForward?.();
    } catch (error) {
      setActionError(actionFailureMessage("Forward", error));
    }
  };

  // The shortcut listener must not re-subscribe on every render (parents pass inline callbacks),
  // so it reads the latest handlers through a ref instead of depending on them.
  // The zoom handlers are function declarations hoisted from below, read at shortcut time.
  const shortcutHandlersRef = useRef({ onReload, goBack: handleGoBack, goForward: handleGoForward, toggleBookmark, zoomIn: handleZoomIn, zoomOut: handleZoomOut, zoomReset: handleZoomReset });
  shortcutHandlersRef.current = { onReload, goBack: handleGoBack, goForward: handleGoForward, toggleBookmark, zoomIn: handleZoomIn, zoomOut: handleZoomOut, zoomReset: handleZoomReset };

  useEffect(() => {
    /** Returns true when this toolbar handled the action itself. */
    const runShortcut = (action: BrowserShortcutAction): boolean => {
      const handlers = shortcutHandlersRef.current;
      // Every shortcut but focus-address acts on the page (find, reload, history), so the
      // suggestion list from an earlier omnibox session closes first.
      if (action !== "focus-address") {
        setOmniboxOpen(false);
        setSelectedHistoryIndex(-1);
      }
      switch (action) {
        case "focus-address":
          inputRef.current?.focus();
          inputRef.current?.select();
          setHistoryQuery("");
          setHistoryOpen(false);
          setSelectedHistoryIndex(-1);
          setOmniboxOpen(true);
          break;
        case "bookmark-page":
          void handlers.toggleBookmark();
          break;
        case "reload":
          handlers.onReload();
          break;
        case "reload-hard":
          handlers.onReload({ ignoreCache: true });
          break;
        case "back":
          void handlers.goBack();
          break;
        case "forward":
          void handlers.goForward();
          break;
        case "find":
          // BrowserPane owns the find overlay and receives the same event.
          break;
        case "zoom-in":
          void handlers.zoomIn();
          return true;
        case "zoom-out":
          void handlers.zoomOut();
          return true;
        case "zoom-reset":
          void handlers.zoomReset();
          return true;
        default:
          // Tab navigation actions are routed at the App level; ignore here.
          break;
      }
      return false;
    };

    const handleDomShortcut = (event: Event) => {
      const detail = (event as BrowserShortcutDomEvent).detail;
      // Claiming the event tells App this tab applied the zoom, so App does not step it again.
      if (detail?.browserId === tab.browserId && runShortcut(detail.action)) event.preventDefault();
    };
    window.addEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserShortcutRequested((payload) => {
      if (payload.browserId === tab.browserId) runShortcut(payload.action);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch((error: unknown) => {
      // Without this subscription the native shortcut bridge is inert for this tab, which the
      // user experiences as shortcuts that simply do nothing.
      if (!disposed) setActionError(actionFailureMessage("Shortcut listener", error));
    });

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);
    };
  }, [settings.rememberBrowsingHistory, tab.browserId]);

  useEffect(() => {
    if (!elementPicking) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserElementPicked((payload) => {
      if (payload.browserId !== tab.browserId) return;
      // The backend disarms the picker as part of finishing the pick.
      setElementPicking(false);
      // A completion that cannot snapshot must not vanish as an unhandled rejection: the
      // capability gate keeps the picker from being armed off macOS, and anything that still
      // slips through surfaces below the toolbar instead of leaving the pick silently dead.
      void finishBrowserElementPick(tab.browserId)
        .then((snapshot) => {
          setActionError(null);
          setDesignSnapshot(snapshot);
        })
        .catch((error) => {
          setActionError(actionFailureMessage("Element pick", error));
        });
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch((error: unknown) => {
      // A missing subscription means a pick could never complete; say so instead of leaving the
      // picker armed and permanently silent.
      if (!disposed) setActionError(actionFailureMessage("Element pick listener", error));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [elementPicking, tab.browserId]);

  const navigateFromAddress = (raw: string) => {
    const url = normalizeBrowserAddress(raw, settings);
    inputDirtyRef.current = false;
    setInputUrl(url);
    setHistoryQuery("");
    setOmniboxOpen(false);
    setSelectedHistoryIndex(-1);
    onNavigate(url);
  };

  const acceptSuggestion = (suggestion: OmniboxSuggestion) => {
    if (suggestion.kind === "tab" && suggestion.browserId) {
      inputDirtyRef.current = false;
      setInputUrl(tab.url);
      setHistoryQuery("");
      setOmniboxOpen(false);
      setSelectedHistoryIndex(-1);
      requestSwitchToBrowserTab(suggestion.browserId);
      return;
    }
    if (suggestion.kind === "search" && suggestion.query) {
      navigateFromAddress(searchUrlFor(settings.searchEngine, suggestion.query));
      return;
    }
    navigateFromAddress(suggestion.url);
  };

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    if (selectedHistoryIndex >= 0 && selectedHistoryIndex < omniboxEntries.length) {
      acceptSuggestion(omniboxEntries[selectedHistoryIndex]);
      return;
    }
    if (!inputUrl.trim()) return;
    navigateFromAddress(inputUrl);
  };

  const handleAddressKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      inputDirtyRef.current = false;
      setOmniboxOpen(false);
      setSelectedHistoryIndex(-1);
      return;
    }
    if (omniboxEntries.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setOmniboxOpen(true);
      setSelectedHistoryIndex((current) => Math.min(current + 1, omniboxEntries.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setOmniboxOpen(true);
      setSelectedHistoryIndex((current) => current <= 0 ? omniboxEntries.length - 1 : current - 1);
    }
  };

  const handleExternalOpen = () => {
    if (!tab.url || tab.url === "about:blank") return;
    void openExternalUrl(tab.url)
      .then(() => setActionError(null))
      .catch((error: unknown) => setActionError(actionFailureMessage("Open in system browser", error)));
  };

  const applyZoom = async (nextZoom: number) => {
    // The optimistic value must not outlive a rejected IPC: showing 110% while the page stays at
    // 100% is exactly the lie the zoom readout must never tell.
    const previousZoom = zoomFactor;
    setOmniboxOpen(false);
    setZoomFactor(nextZoom);
    try {
      const applied = await setBrowserZoom(tab.browserId, nextZoom);
      // The backend answers the zoom it actually applied; show that rather than the request.
      if (typeof applied === "number" && Number.isFinite(applied)) setZoomFactor(applied);
      setActionError(null);
    } catch (error) {
      setZoomFactor(previousZoom);
      setActionError(actionFailureMessage("Zoom", error));
    }
  };

  async function handleZoomIn() {
    await applyZoom(Math.min(BROWSER_ZOOM_MAX, Math.round((zoomFactor + 0.1) * 10) / 10));
  }

  async function handleZoomOut() {
    await applyZoom(Math.max(BROWSER_ZOOM_MIN, Math.round((zoomFactor - 0.1) * 10) / 10));
  }

  async function handleZoomReset() {
    await applyZoom(1.0);
  }

  const handleFocus = async () => {
    try {
      await focusBrowser(tab.browserId);
      setActionError(null);
    } catch (error) {
      setActionError(actionFailureMessage("Focus", error));
    }
  };

  const handleSyncState = async () => {
    try {
      const state = await getBrowserState(tab.browserId);
      if (state.url && state.url !== inputUrl) {
        inputDirtyRef.current = false;
        setInputUrl(state.url);
        setHistoryQuery("");
      }
      setZoomFactor(state.zoomFactor);
      setActionError(null);
    } catch (error) {
      setActionError(actionFailureMessage("Sync state", error));
    }
  };

  const toggleHistory = () => {
    refreshRecentHistory.current();
    setOmniboxOpen(false);
    setHistoryOpen((open) => !open);
  };

  const navigateFromHistory = (url: string) => {
    setHistoryOpen(false);
    navigateFromAddress(url);
  };

  const setRememberHistory = (remember: boolean) => {
    if (!remember) {
      void clearAllBrowserHistory().catch((error: unknown) => setActionError(actionFailureMessage("Clear history", error)));
      setHistoryQuery("");
      setOmniboxOpen(false);
    }
    updateSettings({ rememberBrowsingHistory: remember });
  };

  const hardReloadSupported = isHardReloadSupportedPlatform();
  const modKey = isMacHost() ? "Cmd" : "Ctrl";
  const hasOpenableUrl = Boolean(tab.url) && tab.url !== "about:blank";
  const omniboxListOpen = omniboxOpen && omniboxEntries.length > 0;
  const omniboxOptionId = (index: number) => `${omniboxId}-option-${index}`;

  const handleReload = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (hardReloadSupported && event.shiftKey) {
      onReload({ ignoreCache: true });
      return;
    }
    onReload();
  };

  const handleDevtools = () => {
    // A rejected devtools open is otherwise an unhandled rejection: the user clicks and
    // nothing happens, with no reason shown anywhere.
    void openBrowserDevtools(tab.browserId)
      .then(() => setActionError(null))
      .catch((error: unknown) => setActionError(actionFailureMessage("DevTools", error)));
  };

  const handleTogglePicker = () => {
    // Arming the picker is the user's action; a rejection here would otherwise leave the
    // button pressed with nothing listening for a pick.
    const nextPicking = !elementPicking;
    const togglePicker = elementPicking
      ? removeBrowserElementPicker(tab.browserId)
      : injectBrowserElementPicker(tab.browserId);
    setElementPicking(nextPicking);
    void togglePicker
      .then(() => setActionError(null))
      .catch((error: unknown) => {
        // The page kept its previous picker state, so the button does too.
        setElementPicking(!nextPicking);
        setActionError(actionFailureMessage("Element picker", error));
      });
    onToggleElementPick?.();
  };

  const openChromiumTab = () => {
    window.dispatchEvent(
      new CustomEvent("ferryx:open-browser-url", { detail: { url: tab.url, profileId: tab.profileId } }),
    );
  };

  // The same four actions render as icon buttons (wide) or icon + text buttons (More row).
  const secondaryActionButtons = (variant: "icon" | "row") => (
    <>
      <ToolbarButton
        icon={Target}
        label="Focus webview"
        title="Focus webview: send keyboard input to the page"
        variant={variant}
        onClick={() => void handleFocus()}
      />
      <ToolbarButton
        icon={RefreshCw}
        label="Sync browser state"
        title="Sync browser state: re-read the page URL and zoom"
        variant={variant}
        onClick={() => void handleSyncState()}
      />
      <ToolbarButton icon={Bug} label="DevTools" title="Open DevTools for this page" variant={variant} onClick={handleDevtools} />
      <ToolbarButton
        icon={MousePointer2}
        label="Select element"
        title={elementPickSupported ? "Select element on the page" : "Element picking is unavailable for this tab"}
        variant={variant}
        disabled={!elementPickSupported}
        pressed={elementPicking}
        onClick={handleTogglePicker}
      />
      {/* Print and Save as PDF (the shortcut is Cmd/Ctrl+P). */}
      <ToolbarButton
        icon={Printer}
        label="Print page"
        title={`Print (${modKey}+P)`}
        variant={variant}
        onClick={() => {
          void printBrowser(tab.browserId)
            .then(() => setActionError(null))
            .catch((error: unknown) => setActionError(actionFailureMessage("Print", error)));
        }}
      />
      <ToolbarButton
        icon={FileDown}
        label="Save page as PDF"
        title="Save as PDF"
        variant={variant}
        onClick={() => {
          void saveBrowserPdf(tab.browserId)
            .then(() => setActionError(null))
            .catch((error: unknown) => setActionError(actionFailureMessage("Save as PDF", error)));
        }}
      />
    </>
  );

  const zoomControls = (variant: "icon" | "row") => (
    <>
      <ToolbarButton icon={ZoomOut} label="Zoom out" title="Zoom out" variant={variant} onClick={() => void handleZoomOut()} />
      <span className="w-9 text-center font-mono text-[10px] text-muted-foreground" data-testid="browser-zoom-readout">
        {Math.round(zoomFactor * 100)}%
      </span>
      <ToolbarButton icon={ZoomIn} label="Zoom in" title="Zoom in" variant={variant} onClick={() => void handleZoomIn()} />
    </>
  );

  return (
    <div ref={rootRef} data-testid="browser-toolbar-root" data-compact={compact ? "true" : "false"} className="relative z-10 shrink-0 border-b border-border bg-card text-xs text-foreground select-none">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <div className="flex shrink-0 items-center gap-0.5">
          <ToolbarButton
            icon={ArrowLeft}
            label="Back"
            title={tab.canGoBack ? `Back (${modKey}+[)` : "No previous page"}
            disabled={!tab.canGoBack}
            onClick={() => void handleGoBack()}
            shortcut="browser.back"
          />
          <ToolbarButton
            icon={ArrowRight}
            label="Forward"
            title={tab.canGoForward ? `Forward (${modKey}+])` : "No next page"}
            disabled={!tab.canGoForward}
            onClick={() => void handleGoForward()}
            shortcut="browser.forward"
          />
          <ToolbarButton
            icon={RotateCw}
            label="Reload"
            title={`Reload (${modKey}+R${hardReloadSupported ? ", Shift+Click bypasses the cache" : ""})`}
            onClick={handleReload}
            shortcut="browser.reload"
            iconClassName={tab.loading ? "animate-spin" : ""}
          />
        </div>

        <form onSubmit={handleSubmit} className="flex min-w-0 flex-1 items-center">
          <input
            ref={inputRef}
            type="text"
            value={inputUrl}
            onFocus={() => {
              setHistoryQuery("");
              setHistoryOpen(false);
              setSelectedHistoryIndex(-1);
              setOmniboxOpen(true);
            }}
            onChange={(event) => {
              inputDirtyRef.current = true;
              setInputUrl(event.target.value);
              setHistoryQuery(event.target.value);
              setSelectedHistoryIndex(-1);
              setOmniboxOpen(true);
            }}
            onBlur={() => {
              inputDirtyRef.current = false;
              // Suggestion rows keep focus on mousedown, so a real blur means focus left the
              // omnibox (the page, the find bar, another control).
              setOmniboxOpen(false);
              setSelectedHistoryIndex(-1);
            }}
            onKeyDown={handleAddressKeyDown}
            placeholder="Search or enter URL"
            aria-label="URL address bar"
            title={`Address bar (${modKey}+L)`}
            data-shortcut="browser.focusAddress"
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={omniboxListOpen}
            aria-controls={omniboxListOpen ? omniboxListId : undefined}
            aria-activedescendant={
              omniboxListOpen && selectedHistoryIndex >= 0 && selectedHistoryIndex < omniboxEntries.length
                ? omniboxOptionId(selectedHistoryIndex)
                : undefined
            }
            className="h-7 w-full min-w-0 rounded border border-border bg-background px-2.5 font-mono text-xs text-foreground focus:border-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          />
          <button
            type="button"
            onClick={() => void toggleBookmark()}
            disabled={!/^(https?|file):/i.test(tab.url)}
            title={currentBookmark ? "Remove bookmark" : "Bookmark this page"}
            aria-label={currentBookmark ? "Remove bookmark" : "Bookmark this page"}
            aria-pressed={Boolean(currentBookmark)}
            data-shortcut="browser.bookmarkPage"
            className="ml-1 p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground disabled:opacity-30 transition-colors"
          >
            <Star className={`w-3.5 h-3.5 ${currentBookmark ? "fill-current text-amber-400" : ""}`} />
          </button>
        </form>

        {compact ? null : (
          <div className="flex shrink-0 items-center gap-0.5 border-l border-border/70 pl-1.5" data-testid="browser-toolbar-zoom">
            {zoomControls("icon")}
          </div>
        )}

        <div className="flex shrink-0 items-center gap-0.5 border-l border-border/70 pl-1.5">
          <ToolbarButton
            icon={HistoryIcon}
            label="History"
            title="Browsing history"
            expanded={historyOpen}
            onClick={toggleHistory}
          />
          {compact ? null : secondaryActionButtons("icon")}
          <ToolbarButton
            icon={Puzzle}
            label="Extensions"
            title="Extensions"
            expanded={extensionsOpen}
            controls={extensionsOpen ? extensionsRowId : undefined}
            testId="browser-toolbar-extensions"
            onClick={() => setExtensionsOpen((open) => !open)}
          />
          <ToolbarButton
            icon={ExternalLink}
            label="Open in external browser"
            title={hasOpenableUrl ? "Open in default browser" : "Nothing to open yet: this tab has no page"}
            disabled={!hasOpenableUrl}
            onClick={handleExternalOpen}
          />
          {compact ? (
            <ToolbarButton
              icon={MoreHorizontal}
              label="More"
              title="More actions: zoom, focus, sync, DevTools, select element, print, save as PDF"
              hasPopup
              expanded={moreRowOpen}
              controls={moreRowOpen ? moreRowId : undefined}
              testId="browser-toolbar-more"
              onClick={() => setMoreOpen((open) => !open)}
            />
          ) : null}
        </div>
      </div>

      {moreRowOpen ? (
        <div id={moreRowId} data-testid="browser-toolbar-more-row" className="flex flex-wrap items-center gap-1 border-t border-border/70 px-3 py-1.5">
          {zoomControls("row")}
          <span aria-hidden="true" className="mx-1 h-4 w-px bg-border/70" />
          {secondaryActionButtons("row")}
        </div>
      ) : null}

      {extensionsOpen ? (
        <div id={extensionsRowId} data-testid="browser-extensions-row" className="flex flex-wrap items-center gap-2 border-t border-border/70 px-3 py-1.5 text-[11px] text-muted-foreground">
          <Puzzle className="size-3.5 shrink-0" />
          {extensionsStatus === "loading" ? (
            <span role="status">Checking the browser engine…</span>
          ) : extensionsStatus === "error" ? (
            <span role="alert" className="text-destructive">Couldn't check the browser engine: {extensionsErrorCode}</span>
          ) : extensionsStatus.state === "installed" ? (
            <>
              <span>This tab uses the system webview. New tabs open in Chromium with extensions.</span>
              <button type="button" onClick={openChromiumTab} className={TOOLBAR_ROW_BUTTON}>Open new Chromium tab</button>
            </>
          ) : extensionsStatus.state === "unsupported" ? (
            <span>Extensions need the Chromium engine, which is not available on this platform yet.</span>
          ) : (
            <>
              <span>Extensions need the full browser engine.</span>
              <button type="button" onClick={() => openSettingsSection("browser", "browser-engine")} className={TOOLBAR_ROW_BUTTON}>Install engine</button>
            </>
          )}
        </div>
      ) : null}

      {actionError ? (
        <div role="alert" data-testid="browser-toolbar-error" className="border-t border-border/70 px-3 py-1 text-[10px] text-destructive">
          {actionError}
        </div>
      ) : null}
      {designSnapshot ? (
        <BrowserDesignFeedbackPopover
          snapshot={designSnapshot}
          targets={designFeedbackTargets}
          sending={designSending}
          error={designError}
          onSend={(request) => {
            const snapshot = designSnapshot;
            if (!snapshot) return;
            setDesignSending(true);
            setDesignError(null);
            void deliverDesignFeedback({ sessionId: request.sessionId, memo: request.memo, snapshot, workspaceId: request.workspaceId })
              .then(() => { setDesignSnapshot(null); setDesignSending(false); })
              .catch((error) => {
                setDesignError(actionFailureMessage("Design feedback", error));
                setDesignSending(false);
              });
          }}
          onCancel={() => { setDesignSnapshot(null); setDesignError(null); setElementPicking(false); }}
        />
      ) : null}

      {omniboxListOpen ? (
        <div id={omniboxListId} role="listbox" aria-label="Address bar suggestions" className="border-t border-border/70 bg-popover px-3 py-1">
          {omniboxEntries.map((entry, index) => (
            <button
              key={entry.id}
              id={omniboxOptionId(index)}
              type="button"
              role="option"
              aria-selected={selectedHistoryIndex === index}
              data-suggestion-kind={entry.kind}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => acceptSuggestion(entry)}
              className={`flex w-full items-center gap-3 rounded px-2 py-1.5 text-left ${selectedHistoryIndex === index ? "bg-accent" : "hover:bg-accent/60"}`}
            >
              {entry.kind === "search" ? (
                <SearchIcon className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
              ) : entry.kind === "tab" ? (
                <AppWindow className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
              ) : entry.bookmarked ? (
                <BookmarkIcon className="size-3 shrink-0 text-amber-400" aria-hidden="true" />
              ) : (
                <HistoryIcon className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
              )}
              <span className="min-w-0 flex-1 truncate text-xs">{entry.title || entry.url}</span>
              <span className="max-w-[45%] truncate font-mono text-[10px] text-muted-foreground">
                {entry.kind === "tab" ? `Switch to tab · ${entry.url}` : entry.url}
              </span>
            </button>
          ))}
        </div>
      ) : null}

      {historyOpen ? (
        <div className="flex justify-end border-t border-border/70 px-3 py-1.5">
          <div role="menu" aria-label="Recent browsing history" data-native-terminal-yield="off" className="w-80 max-w-full overflow-hidden rounded-md border border-border bg-popover shadow-lg">
            <div className="border-b border-border px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">Recent History</div>
            <div className="max-h-64 overflow-y-auto py-1">
              {historyEntries.length === 0 ? (
                <div className="px-3 py-4 text-center text-[11px] text-muted-foreground">No browsing history yet.</div>
              ) : historyEntries.slice(0, 20).map((entry) => (
                <button key={entry.url} type="button" role="menuitem" onClick={() => navigateFromHistory(entry.url)} className="block w-full px-3 py-2 text-left hover:bg-accent">
                  <div className="truncate text-xs font-medium text-foreground">{entry.title || entry.url}</div>
                  <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">{entry.url}</div>
                </button>
              ))}
            </div>
            <div className="flex border-t border-border">
              <button type="button" role="menuitem" onClick={() => { setHistoryOpen(false); openBrowserLibrary({ browserId: tab.browserId, view: "history" }); }} className="flex flex-1 items-center gap-1.5 px-3 py-2 text-left text-[11px] hover:bg-accent">
                <HistoryIcon className="size-3" aria-hidden="true" /> Show all history
              </button>
              <button type="button" role="menuitem" onClick={() => { setHistoryOpen(false); openBrowserLibrary({ browserId: tab.browserId, view: "bookmarks" }); }} className="flex flex-1 items-center gap-1.5 px-3 py-2 text-left text-[11px] hover:bg-accent">
                <LibraryIcon className="size-3" aria-hidden="true" /> Bookmarks manager
              </button>
              <button type="button" role="menuitem" onClick={() => { setHistoryOpen(false); openBrowserLibraryView({ browserId: tab.browserId, view: "downloads" }); }} className="flex flex-1 items-center gap-1.5 px-3 py-2 text-left text-[11px] hover:bg-accent">
                <DownloadIcon className="size-3" aria-hidden="true" /> Downloads
              </button>
            </div>
            <label className="flex items-center justify-between gap-3 border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
              <span>Remember browsing history</span>
              <input type="checkbox" aria-label="Remember browsing history" checked={settings.rememberBrowsingHistory} onChange={(event) => setRememberHistory(event.target.checked)} className="size-3.5 accent-foreground" />
            </label>
          </div>
        </div>
      ) : null}
    </div>
  );
}
