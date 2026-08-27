import { useEffect, useMemo, useRef, useState } from "react";
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
} from "lucide-react";
import {
  BROWSER_SHORTCUT_EVENT,
  focusBrowser,
  getBrowserState,
  goBackBrowser,
  goForwardBrowser,
  onBrowserShortcutRequested,
  openExternalUrl,
  setBrowserZoom,
  type BrowserShortcutAction,
} from "../lib/browserTauri";
import {
  BROWSER_HISTORY_EVENT,
  clearBrowserHistory,
  loadBrowserHistory,
  type BrowserHistoryEntry,
} from "../lib/browserHistory";
import { normalizeBrowserAddress, useBrowserSettings } from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";

interface BrowserToolbarProps {
  tab: BrowserTab;
  onNavigate: (url: string) => void;
  onReload: () => void;
  onGoBack?: () => void;
  onGoForward?: () => void;
}

type BrowserShortcutDomEvent = CustomEvent<{ action: BrowserShortcutAction }>;

export function BrowserToolbar({
  tab,
  onNavigate,
  onReload,
  onGoBack,
  onGoForward,
}: BrowserToolbarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [inputUrl, setInputUrl] = useState(tab.url);
  const [historyQuery, setHistoryQuery] = useState("");
  const [zoomFactor, setZoomFactor] = useState(tab.zoomFactor ?? 1.0);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [omniboxOpen, setOmniboxOpen] = useState(false);
  const [selectedHistoryIndex, setSelectedHistoryIndex] = useState(-1);
  const [historyEntries, setHistoryEntries] = useState<BrowserHistoryEntry[]>(loadBrowserHistory);
  const { settings, updateSettings } = useBrowserSettings();

  const omniboxEntries = useMemo(() => {
    if (!settings.rememberBrowsingHistory) return [];
    const prefix = historyQuery.trim().toLocaleLowerCase();
    const entries = prefix
      ? historyEntries.filter((entry) => {
          const url = entry.url.toLocaleLowerCase();
          const title = (entry.title ?? "").toLocaleLowerCase();
          const compactUrl = url.replace(/^https?:\/\//, "");
          return url.startsWith(prefix) || compactUrl.startsWith(prefix) || title.startsWith(prefix);
        })
      : historyEntries;
    return entries.slice(0, 8);
  }, [historyEntries, historyQuery, settings.rememberBrowsingHistory]);

  useEffect(() => {
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

  const handleGoBack = async () => {
    try {
      await goBackBrowser(tab.browserId);
      onGoBack?.();
    } catch {
      // Native state remains authoritative; a later state sync can recover.
    }
  };

  const handleGoForward = async () => {
    try {
      await goForwardBrowser(tab.browserId);
      onGoForward?.();
    } catch {
      // Native state remains authoritative; a later state sync can recover.
    }
  };

  useEffect(() => {
    const runShortcut = (action: BrowserShortcutAction) => {
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
          onReload();
          break;
        case "back":
          void handleGoBack();
          break;
        case "forward":
          void handleGoForward();
          break;
        case "find":
          // BrowserPane owns the find overlay and receives the same event.
          break;
      }
    };

    const handleDomShortcut = (event: Event) => {
      const detail = (event as BrowserShortcutDomEvent).detail;
      if (detail?.action) runShortcut(detail.action);
    };
    window.addEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserShortcutRequested((payload) => {
      if (payload.browserId === tab.browserId) runShortcut(payload.action);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);
    };
    // The navigation handlers intentionally follow the current browser tab state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onReload, settings.rememberBrowsingHistory, tab.browserId, tab.canGoBack, tab.canGoForward]);

  const navigateFromAddress = (raw: string) => {
    const url = normalizeBrowserAddress(raw, settings);
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
    if (tab.url && tab.url !== "about:blank") {
      void openExternalUrl(tab.url);
    }
  };

  const handleZoomIn = async () => {
    const nextZoom = Math.min(2.0, Math.round((zoomFactor + 0.1) * 10) / 10);
    setZoomFactor(nextZoom);
    try {
      await setBrowserZoom(tab.browserId, nextZoom);
    } catch {
      // Native state will resynchronize on the next browser state event.
    }
  };

  const handleZoomOut = async () => {
    const nextZoom = Math.max(0.5, Math.round((zoomFactor - 0.1) * 10) / 10);
    setZoomFactor(nextZoom);
    try {
      await setBrowserZoom(tab.browserId, nextZoom);
    } catch {
      // Native state will resynchronize on the next browser state event.
    }
  };

  const handleFocus = async () => {
    try {
      await focusBrowser(tab.browserId);
    } catch {
      // The tab may have been closed while the command was queued.
    }
  };

  const handleSyncState = async () => {
    try {
      const state = await getBrowserState(tab.browserId);
      if (state.url && state.url !== inputUrl) {
        setInputUrl(state.url);
        setHistoryQuery("");
      }
      setZoomFactor(state.zoomFactor);
    } catch {
      // The tab may have been closed while the command was queued.
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

  return (
    <div className="relative z-10 shrink-0 bg-[#4b4b4b] border-b border-border text-xs text-foreground select-none">
      <div className="flex items-center gap-2 px-3 py-1.5">
        <div className="flex items-center gap-1">
          <button
            type="button"
            disabled={!tab.canGoBack}
            onClick={() => void handleGoBack()}
            aria-label="Back"
            className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            disabled={!tab.canGoForward}
            onClick={() => void handleGoForward()}
            aria-label="Forward"
            className="p-1 rounded hover:bg-muted disabled:opacity-30 transition-colors"
          >
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={onReload}
            aria-label="Reload"
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
              setInputUrl(event.target.value);
              setHistoryQuery(event.target.value);
              setSelectedHistoryIndex(-1);
              if (settings.rememberBrowsingHistory) setOmniboxOpen(true);
            }}
            onKeyDown={handleAddressKeyDown}
            placeholder="Search or enter URL"
            aria-label="URL address bar"
            aria-expanded={settings.rememberBrowsingHistory && omniboxOpen}
            aria-controls="browser-omnibox-history"
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
          <button type="button" onClick={handleExternalOpen} title="Open in default browser" aria-label="Open in external browser" className="p-1 rounded hover:bg-muted transition-colors">
            <ExternalLink className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>

      {settings.rememberBrowsingHistory && omniboxOpen && omniboxEntries.length > 0 ? (
        <div id="browser-omnibox-history" role="listbox" aria-label="Address bar history" className="border-t border-border/70 bg-popover px-3 py-1">
          {omniboxEntries.map((entry, index) => (
            <button
              key={entry.id}
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
          <div role="menu" aria-label="Recent browsing history" className="w-80 max-w-full overflow-hidden rounded-md border border-border bg-popover shadow-lg">
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
