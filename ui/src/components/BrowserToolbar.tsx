import { useState, useEffect } from "react";
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
  focusBrowser,
  getBrowserState,
  goBackBrowser,
  goForwardBrowser,
  navigateBrowser,
  openExternalUrl,
  setBrowserZoom,
} from "../lib/browserTauri";
import {
  BROWSER_HISTORY_EVENT,
  clearBrowserHistory,
  loadBrowserHistory,
  type BrowserHistoryEntry,
} from "../lib/browserHistory";
import { useBrowserSettings } from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";

interface BrowserToolbarProps {
  tab: BrowserTab;
  onNavigate: (url: string) => void;
  onReload: () => void;
  onGoBack?: () => void;
  onGoForward?: () => void;
}

export function BrowserToolbar({
  tab,
  onNavigate,
  onReload,
  onGoBack,
  onGoForward,
}: BrowserToolbarProps) {
  const [inputUrl, setInputUrl] = useState(tab.url);
  const [zoomFactor, setZoomFactor] = useState(1.0);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historyEntries, setHistoryEntries] = useState<BrowserHistoryEntry[]>(loadBrowserHistory);
  const { settings, updateSettings } = useBrowserSettings();

  useEffect(() => {
    setInputUrl(tab.url);
  }, [tab.url]);

  useEffect(() => {
    const syncHistory = () => setHistoryEntries(loadBrowserHistory());
    window.addEventListener(BROWSER_HISTORY_EVENT, syncHistory);
    window.addEventListener("storage", syncHistory);
    return () => {
      window.removeEventListener(BROWSER_HISTORY_EVENT, syncHistory);
      window.removeEventListener("storage", syncHistory);
    };
  }, []);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    let url = inputUrl.trim();
    if (!url) return;
    if (!url.startsWith("http://") && !url.startsWith("https://") && url !== "about:blank") {
      if (url.startsWith("localhost") || url.startsWith("127.0.0.1")) {
        url = `http://${url}`;
      } else {
        url = `https://${url}`;
      }
    }
    onNavigate(url);
  };

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
      // ignore
    }
  };

  const handleZoomOut = async () => {
    const nextZoom = Math.max(0.5, Math.round((zoomFactor - 0.1) * 10) / 10);
    setZoomFactor(nextZoom);
    try {
      await setBrowserZoom(tab.browserId, nextZoom);
    } catch {
      // ignore
    }
  };

  const handleFocus = async () => {
    try {
      await focusBrowser(tab.browserId);
    } catch {
      // ignore
    }
  };

  const handleSyncState = async () => {
    try {
      const state = await getBrowserState(tab.browserId);
      if (state.url && state.url !== inputUrl) {
        setInputUrl(state.url);
      }
    } catch {
      // ignore
    }
  };

  const toggleHistory = () => {
    setHistoryEntries(loadBrowserHistory());
    setHistoryOpen((open) => !open);
  };

  const navigateFromHistory = async (url: string) => {
    setHistoryOpen(false);
    try {
      await navigateBrowser(tab.browserId, url);
    } catch {
      // Native browser state remains authoritative when navigation is rejected.
    }
  };

  const setRememberHistory = (remember: boolean) => {
    if (!remember) clearBrowserHistory();
    updateSettings({ rememberBrowsingHistory: remember });
  };

  return (
    <div className="relative z-10 shrink-0 bg-sidebar border-b border-border text-xs text-foreground select-none">
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
            type="text"
            value={inputUrl}
            onChange={(e) => setInputUrl(e.target.value)}
            placeholder="Search or enter URL"
            aria-label="URL address bar"
            className="w-full bg-background border border-border rounded px-2.5 py-1 text-xs text-foreground focus:outline-none focus:border-primary font-mono"
          />
        </form>

        <div className="flex items-center gap-0.5 border-l border-border/70 pl-1.5">
          <button
            type="button"
            onClick={handleZoomOut}
            title="Zoom out"
            aria-label="Zoom out"
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <ZoomOut className="w-3.5 h-3.5" />
          </button>
          <span className="font-mono text-[10px] text-muted-foreground w-8 text-center">
            {Math.round(zoomFactor * 100)}%
          </span>
          <button
            type="button"
            onClick={handleZoomIn}
            title="Zoom in"
            aria-label="Zoom in"
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <ZoomIn className="w-3.5 h-3.5" />
          </button>
        </div>

        <div className="flex items-center gap-0.5 border-l border-border/70 pl-1.5">
          <button
            type="button"
            onClick={toggleHistory}
            title="History"
            aria-label="History"
            aria-expanded={historyOpen}
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <HistoryIcon className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={handleFocus}
            title="Focus webview"
            aria-label="Focus webview"
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <Target className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={handleSyncState}
            title="Sync browser state"
            aria-label="Sync browser state"
            className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground transition-colors"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={handleExternalOpen}
            title="Open in default browser"
            aria-label="Open in external browser"
            className="p-1 rounded hover:bg-muted transition-colors"
          >
            <ExternalLink className="w-3.5 h-3.5 text-muted-foreground" />
          </button>
        </div>
      </div>

      {historyOpen ? (
        <div className="flex justify-end border-t border-border/70 px-3 py-1.5">
          <div
            role="menu"
            aria-label="Recent browsing history"
            className="w-80 max-w-full overflow-hidden rounded-md border border-border bg-popover shadow-lg"
          >
            <div className="border-b border-border px-3 py-2 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              Recent History
            </div>
            <div className="max-h-64 overflow-y-auto py-1">
              {historyEntries.length === 0 ? (
                <div className="px-3 py-4 text-center text-[11px] text-muted-foreground">No browsing history yet.</div>
              ) : historyEntries.slice(0, 20).map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  role="menuitem"
                  onClick={() => void navigateFromHistory(entry.url)}
                  className="block w-full px-3 py-2 text-left hover:bg-accent"
                >
                  <div className="truncate text-xs font-medium text-foreground">{entry.title || entry.url}</div>
                  <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">{entry.url}</div>
                </button>
              ))}
            </div>
            <label className="flex items-center justify-between gap-3 border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
              <span>Remember browsing history</span>
              <input
                type="checkbox"
                aria-label="Remember browsing history"
                checked={settings.rememberBrowsingHistory}
                onChange={(event) => setRememberHistory(event.target.checked)}
                className="size-3.5 accent-foreground"
              />
            </label>
          </div>
        </div>
      ) : null}
    </div>
  );
}
