import { useEffect, useId, useMemo, useRef, useState } from "react";
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
} from "lucide-react";
import {
  BROWSER_SHORTCUT_EVENT,
  extractBrowserErrorCode,
  focusBrowser,
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
  type BrowserDesignSnapshot,
  type BrowserReloadOptions,
  type BrowserShortcutAction,
  type BrowserShortcutDomEvent,
  type BrowserSnapshotCapability,
  type DesignFeedbackTarget,
} from "../lib/browserTauri";
import { BrowserDesignFeedbackPopover } from "./BrowserDesignFeedbackPopover";
import {
  BROWSER_HISTORY_EVENT,
  clearBrowserHistory,
  loadBrowserHistory,
  searchBrowserHistory,
  type BrowserHistoryEntry,
} from "../lib/browserHistory";
import { BROWSER_ZOOM_LEVELS, normalizeBrowserAddress, useBrowserSettings } from "../lib/browserSettings";
import { isHardReloadSupportedPlatform } from "../lib/shortcuts";
import type { BrowserTab } from "../lib/types";

// Keep the toolbar's zoom clamps in lockstep with the Settings-exposed zoom levels
// (BROWSER_ZOOM_LEVELS percentages), so a zoom restored by the backend stays reachable.
const BROWSER_ZOOM_MIN = Math.min(...BROWSER_ZOOM_LEVELS) / 100;
const BROWSER_ZOOM_MAX = Math.max(...BROWSER_ZOOM_LEVELS) / 100;

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
}: BrowserToolbarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const omniboxId = useId();
  const omniboxListId = `${omniboxId}-history`;
  const [inputUrl, setInputUrl] = useState(tab.url);
  // Set while the user has typed into the omnibox and not yet submitted, blurred or escaped, so
  // a live URL update from the page cannot overwrite half-typed text.
  const inputDirtyRef = useRef(false);
  const [historyQuery, setHistoryQuery] = useState("");
  const [zoomFactor, setZoomFactor] = useState(tab.zoomFactor ?? 1.0);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [omniboxOpen, setOmniboxOpen] = useState(false);
  const [selectedHistoryIndex, setSelectedHistoryIndex] = useState(-1);
  const [historyEntries, setHistoryEntries] = useState<BrowserHistoryEntry[]>(loadBrowserHistory);
  const [snapshotCapability, setSnapshotCapability] = useState<BrowserSnapshotCapability | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [designSnapshot, setDesignSnapshot] = useState<BrowserDesignSnapshot | null>(null);
  const [designSending, setDesignSending] = useState(false);
  const [designError, setDesignError] = useState<string | null>(null);
  // The toolbar owns the picker's armed state so the element-pick -> design-feedback flow works
  // without a parent; a parent that passes `elementPicking` still drives it.
  const [elementPicking, setElementPicking] = useState(elementPickingProp ?? false);
  const { settings, updateSettings } = useBrowserSettings();

  // Picking needs native snapshot capture AND this tab's isolated-world bridge, which the
  // backend reports as `pick`. An unknown capability (an older backend without the command)
  // keeps the control as-is; a reported capability enables picking only when `pick` is true.
  const elementPickSupported = snapshotCapability === null || snapshotCapability.pick === true;

  const omniboxEntries = useMemo(() => {
    if (!settings.rememberBrowsingHistory) return [];
    return searchBrowserHistory(historyEntries, historyQuery, 8);
  }, [historyEntries, historyQuery, settings.rememberBrowsingHistory]);

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
  }, [tab.url]);

  useEffect(() => {
    setZoomFactor(tab.zoomFactor ?? 1.0);
  }, [tab.browserId, tab.zoomFactor]);

  useEffect(() => {
    const syncHistory = () => setHistoryEntries(loadBrowserHistory());
    window.addEventListener(BROWSER_HISTORY_EVENT, syncHistory);
    window.addEventListener("storage", syncHistory);
    return () => {
      window.removeEventListener(BROWSER_HISTORY_EVENT, syncHistory);
      window.removeEventListener("storage", syncHistory);
    };
  }, []);

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
  const shortcutHandlersRef = useRef({ onReload, goBack: handleGoBack, goForward: handleGoForward });
  shortcutHandlersRef.current = { onReload, goBack: handleGoBack, goForward: handleGoForward };

  useEffect(() => {
    const runShortcut = (action: BrowserShortcutAction) => {
      const handlers = shortcutHandlersRef.current;
      switch (action) {
        case "focus-address":
          inputRef.current?.focus();
          inputRef.current?.select();
          setHistoryQuery("");
          setHistoryOpen(false);
          setSelectedHistoryIndex(-1);
          if (settings.rememberBrowsingHistory) setOmniboxOpen(true);
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
        default:
          // Tab navigation actions are routed at the App level; ignore here.
          break;
      }
    };

    const handleDomShortcut = (event: Event) => {
      const detail = (event as BrowserShortcutDomEvent).detail;
      if (detail?.browserId === tab.browserId) runShortcut(detail.action);
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

  const handleSubmit = (event: React.FormEvent) => {
    event.preventDefault();
    if (selectedHistoryIndex >= 0 && selectedHistoryIndex < omniboxEntries.length) {
      navigateFromAddress(omniboxEntries[selectedHistoryIndex].url);
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
    if (!settings.rememberBrowsingHistory || omniboxEntries.length === 0) return;
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

  const handleZoomIn = async () => {
    await applyZoom(Math.min(BROWSER_ZOOM_MAX, Math.round((zoomFactor + 0.1) * 10) / 10));
  };

  const handleZoomOut = async () => {
    await applyZoom(Math.max(BROWSER_ZOOM_MIN, Math.round((zoomFactor - 0.1) * 10) / 10));
  };

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
    setHistoryEntries(loadBrowserHistory());
    setOmniboxOpen(false);
    setHistoryOpen((open) => !open);
  };

  const navigateFromHistory = (url: string) => {
    setHistoryOpen(false);
    navigateFromAddress(url);
  };

  const setRememberHistory = (remember: boolean) => {
    if (!remember) {
      clearBrowserHistory();
      setHistoryQuery("");
      setOmniboxOpen(false);
    }
    updateSettings({ rememberBrowsingHistory: remember });
  };

  const hardReloadSupported = isHardReloadSupportedPlatform();
  const omniboxListOpen = settings.rememberBrowsingHistory && omniboxOpen && omniboxEntries.length > 0;
  const omniboxOptionId = (index: number) => `${omniboxId}-option-${index}`;

  const handleReload = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (hardReloadSupported && event.shiftKey) {
      onReload({ ignoreCache: true });
      return;
    }
    onReload();
  };

  return (
    <div className="relative z-10 shrink-0 bg-background border-b border-border text-xs text-foreground select-none">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <div className="flex items-center gap-1">
          <button
            type="button"
            disabled={!tab.canGoBack}
            onClick={() => void handleGoBack()}
            aria-label="Back"
            data-shortcut="browser.back"
            className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            disabled={!tab.canGoForward}
            onClick={() => void handleGoForward()}
            aria-label="Forward"
            data-shortcut="browser.forward"
            className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
          >
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={handleReload}
            aria-label="Reload"
            title={hardReloadSupported ? "Reload (Shift+Click bypasses the cache)" : "Reload"}
            data-shortcut="browser.reload"
            className="p-1 rounded hover:bg-muted transition-colors"
          >
            <RotateCw className={`w-3.5 h-3.5 ${tab.loading ? "animate-spin" : ""}`} />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="flex-1 flex items-center">
          <input
            ref={inputRef}
            type="text"
            value={inputUrl}
            onFocus={() => {
              setHistoryQuery("");
              setHistoryOpen(false);
              setSelectedHistoryIndex(-1);
              if (settings.rememberBrowsingHistory) setOmniboxOpen(true);
            }}
            onChange={(event) => {
              inputDirtyRef.current = true;
              setInputUrl(event.target.value);
              setHistoryQuery(event.target.value);
              setSelectedHistoryIndex(-1);
              if (settings.rememberBrowsingHistory) setOmniboxOpen(true);
            }}
            onBlur={() => {
              inputDirtyRef.current = false;
            }}
            onKeyDown={handleAddressKeyDown}
            placeholder="Search or enter URL"
            aria-label="URL address bar"
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
            className="w-full bg-background border border-border rounded px-2.5 py-1 text-xs text-foreground focus:outline-none focus:border-primary font-mono"
          />
        </form>

        <div className="flex items-center gap-0.5 border-l border-border/70 pl-1.5">
          <button type="button" onClick={handleZoomOut} title="Zoom out" aria-label="Zoom out" className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors">
            <ZoomOut className="w-3.5 h-3.5" />
          </button>
          <span className="font-mono text-[10px] text-muted-foreground w-8 text-center">{Math.round(zoomFactor * 100)}%</span>
          <button type="button" onClick={handleZoomIn} title="Zoom in" aria-label="Zoom in" className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors">
            <ZoomIn className="w-3.5 h-3.5" />
          </button>
        </div>

        <div className="flex items-center gap-0.5 border-l border-border/70 pl-1.5">
          <button type="button" onClick={toggleHistory} title="History" aria-label="History" aria-expanded={historyOpen} className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors">
            <HistoryIcon className="w-3.5 h-3.5" />
          </button>
          <button type="button" onClick={handleFocus} title="Focus webview" aria-label="Focus webview" className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors">
            <Target className="w-3.5 h-3.5" />
          </button>
          <button type="button" onClick={handleSyncState} title="Sync browser state" aria-label="Sync browser state" className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors">
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={() => {
              // A rejected devtools open is otherwise an unhandled rejection: the user clicks and
              // nothing happens, with no reason shown anywhere.
              void openBrowserDevtools(tab.browserId)
                .then(() => setActionError(null))
                .catch((error: unknown) => setActionError(actionFailureMessage("DevTools", error)));
            }}
            title="DevTools"
            aria-label="DevTools"
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <Bug className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            disabled={!elementPickSupported}
            onClick={() => {
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
            }}
            title={elementPickSupported ? "Select element" : "Select element (unavailable on this platform)"}
            aria-label="Select element"
            aria-pressed={elementPicking}
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors disabled:opacity-30"
          >
            <MousePointer2 className="w-3.5 h-3.5" />
          </button>
          <button type="button" onClick={handleExternalOpen} title="Open in default browser" aria-label="Open in external browser" className="p-1 rounded hover:bg-muted transition-colors">
            <ExternalLink className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>

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
        <div id={omniboxListId} role="listbox" aria-label="Address bar history" className="border-t border-border/70 bg-popover px-3 py-1">
          {omniboxEntries.map((entry, index) => (
            <button
              key={entry.id}
              id={omniboxOptionId(index)}
              type="button"
              role="option"
              aria-selected={selectedHistoryIndex === index}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => navigateFromHistory(entry.url)}
              className={`flex w-full items-center gap-3 rounded px-2 py-1.5 text-left ${selectedHistoryIndex === index ? "bg-accent" : "hover:bg-accent/60"}`}
            >
              <span className="min-w-0 flex-1 truncate text-xs">{entry.title || entry.url}</span>
              <span className="max-w-[45%] truncate font-mono text-[10px] text-muted-foreground">{entry.url}</span>
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
                <button key={entry.id} type="button" role="menuitem" onClick={() => navigateFromHistory(entry.url)} className="block w-full px-3 py-2 text-left hover:bg-accent">
                  <div className="truncate text-xs font-medium text-foreground">{entry.title || entry.url}</div>
                  <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">{entry.url}</div>
                </button>
              ))}
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
