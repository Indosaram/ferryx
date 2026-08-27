import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronUp, Download, ExternalLink, RefreshCw, X } from "lucide-react";

import {
  BROWSER_SHORTCUT_EVENT,
  clearBrowserFind,
  downloadBrowserUrl,
  findBrowser,
  onBrowserDownloadRequested,
  onBrowserShortcutRequested,
  openExternalUrl,
  setBrowserBounds,
  setBrowserVisible,
  type BrowserFindResult,
  type BrowserShortcutAction,
} from "../lib/browserTauri";
import { recordBrowserHistory } from "../lib/browserHistory";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

interface BrowserPaneProps {
  tab: BrowserTab;
  visible?: boolean;
  onNavigate: (url: string) => void;
  onReload: () => void;
}

type BrowserStateChangedPayload = {
  browserId: string;
  generation: number;
  url: string;
  title?: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  loadError?: string | null;
};

type BrowserShortcutDomEvent = CustomEvent<{ action: BrowserShortcutAction }>;

function suggestedDownloadName(url: string): string {
  try {
    const parsed = new URL(url);
    const name = parsed.pathname.split("/").filter(Boolean).pop();
    return name || "download";
  } catch {
    return "download";
  }
}

function extractDroppedHttpUrl(dataTransfer: DataTransfer): string | null {
  const raw = dataTransfer.getData("text/uri-list") || dataTransfer.getData("text/plain");
  const candidate = raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line && !line.startsWith("#"));
  if (!candidate) return null;
  try {
    const parsed = new URL(candidate);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.href : null;
  } catch {
    return null;
  }
}

export function BrowserPane({ tab, visible = true, onNavigate, onReload }: BrowserPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const [liveTab, setLiveTab] = useState<BrowserTab>(tab);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findResult, setFindResult] = useState<BrowserFindResult>({ matchCount: 0, found: false });
  const [findError, setFindError] = useState<string | null>(null);
  const [downloadUrl, setDownloadUrl] = useState<string | null>(null);
  const [downloadStatus, setDownloadStatus] = useState<string | null>(null);

  useEffect(() => {
    setLiveTab((current) => ({ ...current, ...tab }));
  }, [tab]);

  useEffect(() => {
    if (findOpen) findInputRef.current?.focus();
  }, [findOpen]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    const applyState = (payload: BrowserStateChangedPayload) => {
      if (payload.browserId !== tab.browserId) return;
      setLiveTab((current) => ({
        ...current,
        url: payload.url || current.url,
        title: payload.title ?? current.title,
        loading: payload.loading,
        canGoBack: payload.canGoBack,
        canGoForward: payload.canGoForward,
        zoomFactor: payload.zoomFactor,
        loadError: payload.loadError ?? null,
      }));
      if (!payload.loading && !payload.loadError && payload.url) {
        recordBrowserHistory({
          browserId: payload.browserId,
          url: payload.url,
          title: payload.title ?? null,
        });
      }
    };

    void listen<BrowserStateChangedPayload>("browser_state_changed", (event) => {
      applyState(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tab.browserId]);

  useEffect(() => {
    const openFind = (action: BrowserShortcutAction) => {
      if (action !== "find") return;
      setFindOpen(true);
      setFindError(null);
    };
    const handleDomShortcut = (event: Event) => {
      const detail = (event as BrowserShortcutDomEvent).detail;
      if (detail?.action) openFind(detail.action);
    };
    window.addEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);

    let disposed = false;
    let shortcutCleanup: (() => void) | undefined;
    let downloadCleanup: (() => void) | undefined;
    void onBrowserShortcutRequested((payload) => {
      if (payload.browserId === tab.browserId) openFind(payload.action);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else shortcutCleanup = cleanup;
    }).catch(() => undefined);
    void onBrowserDownloadRequested((payload) => {
      if (payload.browserId !== tab.browserId) return;
      setDownloadUrl(payload.targetUrl);
      setDownloadStatus(null);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else downloadCleanup = cleanup;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      shortcutCleanup?.();
      downloadCleanup?.();
      window.removeEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);
    };
  }, [tab.browserId]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateVisibility = (nextVisible: boolean) => {
      void setBrowserVisible(tab.browserId, nextVisible).catch(() => undefined);
    };

    const updateBounds = () => {
      if (!visible || liveTab.loadError) {
        updateVisibility(false);
        return;
      }

      const rect = el.getBoundingClientRect();
      const toolbarRect = toolbarRef.current?.getBoundingClientRect();
      const minTop = toolbarRect ? toolbarRect.bottom : rect.y;
      const clampedY = Math.max(rect.y, minTop);
      const heightReduction = clampedY - rect.y;
      const clampedHeight = Math.max(0, rect.height - heightReduction);

      void setBrowserBounds(tab.browserId, {
        x: rect.x,
        y: clampedY,
        width: rect.width,
        height: clampedHeight,
      }).catch(() => undefined);
      updateVisibility(true);
    };

    updateBounds();

    const resizeObserver = new ResizeObserver(updateBounds);
    resizeObserver.observe(el);
    window.addEventListener("resize", updateBounds);

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener("resize", updateBounds);
      // Native child webviews outlive React DOM nodes; cleanup also covers Fast Refresh remounts.
      updateVisibility(false);
    };
  }, [liveTab.loadError, tab.browserId, visible]);

  const runFind = async (query: string, backwards = false) => {
    setFindQuery(query);
    setFindError(null);
    if (!query.trim()) {
      setFindResult({ matchCount: 0, found: false });
      await clearBrowserFind(tab.browserId).catch(() => undefined);
      return;
    }
    try {
      setFindResult(await findBrowser(tab.browserId, query, backwards));
    } catch (error) {
      setFindError(error instanceof Error ? error.message : "Find failed");
    }
  };

  const closeFind = () => {
    setFindOpen(false);
    setFindQuery("");
    setFindResult({ matchCount: 0, found: false });
    setFindError(null);
    void clearBrowserFind(tab.browserId).catch(() => undefined);
  };

  const saveDownload = async () => {
    if (!downloadUrl) return;
    const filePath = await save({ defaultPath: suggestedDownloadName(downloadUrl) });
    if (!filePath) return;
    setDownloadStatus("Saving…");
    try {
      await downloadBrowserUrl(downloadUrl, filePath);
      setDownloadStatus("Saved");
    } catch (error) {
      setDownloadStatus(error instanceof Error ? error.message : "Download failed");
    }
  };

  return (
    <div className="flex flex-col w-full h-full bg-[#4b4b4b] overflow-hidden">
      <div ref={toolbarRef}>
        <BrowserToolbar tab={liveTab} onNavigate={onNavigate} onReload={onReload} />
        {findOpen ? (
          <div className="flex items-center gap-1.5 border-b border-border bg-popover px-3 py-1.5" data-testid="browser-find-bar">
            <input
              ref={findInputRef}
              value={findQuery}
              onChange={(event) => void runFind(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  closeFind();
                } else if (event.key === "Enter") {
                  event.preventDefault();
                  void runFind(findQuery, event.shiftKey);
                }
              }}
              aria-label="Find in page"
              placeholder="Find in page"
              className="h-7 min-w-0 flex-1 rounded border border-border bg-background px-2 text-xs outline-none focus:border-ring"
            />
            <span className="min-w-14 text-center text-[10px] text-muted-foreground">
              {findQuery ? `${findResult.matchCount} matches` : "0 matches"}
            </span>
            <button type="button" aria-label="Previous match" onClick={() => void runFind(findQuery, true)} className="rounded p-1 hover:bg-accent"><ChevronUp className="size-3.5" /></button>
            <button type="button" aria-label="Next match" onClick={() => void runFind(findQuery, false)} className="rounded p-1 hover:bg-accent"><ChevronDown className="size-3.5" /></button>
            <button type="button" aria-label="Close find" onClick={closeFind} className="rounded p-1 hover:bg-accent"><X className="size-3.5" /></button>
            {findError ? <span className="text-[10px] text-destructive">{findError}</span> : null}
          </div>
        ) : null}
        {downloadUrl ? (
          <div className="flex items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]" data-testid="browser-download-prompt">
            <Download className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate">Download {suggestedDownloadName(downloadUrl)}</span>
            {downloadStatus ? <span className="text-muted-foreground">{downloadStatus}</span> : null}
            <button type="button" onClick={() => void openExternalUrl(downloadUrl)} className="flex items-center gap-1 rounded border border-border px-2 py-1 hover:bg-accent"><ExternalLink className="size-3" /> Open in system browser</button>
            <button type="button" onClick={() => void saveDownload()} className="rounded border border-border px-2 py-1 hover:bg-accent">Save as…</button>
            <button type="button" aria-label="Dismiss download" onClick={() => { setDownloadUrl(null); setDownloadStatus(null); }} className="rounded p-1 hover:bg-accent"><X className="size-3.5" /></button>
          </div>
        ) : null}
      </div>
      <div
        ref={containerRef}
        data-testid="browser-viewport"
        className="flex-1 w-full bg-card relative"
        onDragOver={(event) => {
          if (event.dataTransfer.types.includes("text/uri-list") || event.dataTransfer.types.includes("text/plain")) {
            event.preventDefault();
          }
        }}
        onDrop={(event) => {
          const url = extractDroppedHttpUrl(event.dataTransfer);
          if (!url) return;
          event.preventDefault();
          onNavigate(url);
        }}
      >
        {liveTab.loadError ? (
          <div className="absolute inset-0 z-20 flex items-center justify-center bg-background p-8" data-testid="browser-load-error">
            <div className="max-w-lg rounded-lg border border-border bg-card p-5 shadow-lg">
              <div className="text-sm font-semibold">This page could not be loaded</div>
              <div className="mt-2 break-all font-mono text-[11px] text-muted-foreground">{liveTab.url}</div>
              <div className="mt-2 text-xs text-destructive">{liveTab.loadError}</div>
              <button type="button" onClick={onReload} className="mt-4 flex items-center gap-1.5 rounded border border-border px-3 py-1.5 text-xs hover:bg-accent">
                <RefreshCw className="size-3.5" /> Retry
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
