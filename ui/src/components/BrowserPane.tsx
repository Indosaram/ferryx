import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronUp, CircleAlert, Download, ExternalLink, RefreshCw, X } from "lucide-react";

import {
  BROWSER_SHORTCUT_EVENT,
  clearBrowserFind,
  downloadBrowserUrlWithSession,
  extractBrowserErrorCode,
  findBrowser,
  listBrowsers,
  onBrowserDownloadRequested,
  onBrowserShortcutRequested,
  openExternalUrl,
  setBrowserBounds,
  setBrowserVisible,
  type BrowserDisplayError,
  type BrowserFindResult,
  type BrowserShortcutAction,
  type BrowserShortcutDomEvent,
  type BrowserReloadOptions,
  type DesignFeedbackTarget,
} from "../lib/browserTauri";
import {
  BROWSER_LIBRARY_OPEN_EVENT,
  useBookmarks,
  type BrowserLibraryOpenDetail,
} from "../lib/browserLibrary";
import { PRIVATE_BROWSER_PROFILE, useBrowserSettings } from "../lib/browserSettings";
import { useNativeTerminalVisibility } from "../lib/nativeTerminalVisibility";
import { BrowserToolbar } from "./BrowserToolbar";
import { BrowserBookmarkBar } from "./BrowserBookmarkBar";
import { BrowserLibraryPanel, type BrowserLibraryView } from "./BrowserLibraryPanel";
import { BrowserSignInHandoffBar } from "./BrowserSignInHandoffBar";
import { BrowserDownloadsShelf } from "./BrowserDownloadsShelf";
import { BrowserEngineFallbackNotice } from "./BrowserEngineFallbackNotice";
import { BrowserMediaCaptureIndicator, BrowserPermissionPrompts } from "./BrowserPermissionPrompt";
import { onBrowserFindResult } from "../lib/browserSystem";
import { browserErrorText } from "../lib/browserErrorText";
import { BrowserVaultBar } from "./BrowserVaultBar";
import type { BrowserTab } from "../lib/types";

interface BrowserPaneProps {
  tab: BrowserTab;
  visible?: boolean;
  onNavigate: (url: string) => void;
  onReload: (options?: BrowserReloadOptions) => void;
  designFeedbackTargets?: DesignFeedbackTarget[];
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
  engine?: BrowserTab["engine"];
};

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

function parseErrorCode(err: unknown): string {
  return extractBrowserErrorCode(err);
}

export function BrowserPane({
  tab,
  visible = true,
  onNavigate,
  onReload,
  designFeedbackTargets = [],
}: BrowserPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const findInputRef = useRef<HTMLInputElement>(null);
  const [liveTab, setLiveTab] = useState<BrowserTab>(tab);
  // Parents rebuild `tab` from store state that never sees `browser_state_changed`, so a new
  // object with unchanged values must not roll the live url/title/navigation state back.
  const previousTabRef = useRef<BrowserTab>(tab);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findResult, setFindResult] = useState<BrowserFindResult>({ matchCount: 0, found: false });
  const [findError, setFindError] = useState<string | null>(null);
  // Registration and find-clear failures are not find-bar content (the bar can be closed), so they
  // surface in a pane-level alert row instead of being discarded. Each action clears only its own
  // failure, so a later success never erases an unrelated one.
  const [paneAlert, setPaneAlert] = useState<{ action: string; code: string } | null>(null);
  const [downloadUrl, setDownloadUrl] = useState<string | null>(null);
  const [downloadStatus, setDownloadStatus] = useState<string | null>(null);
  // A request the host could not attribute to the user (no trusted bridge): nothing is fetched
  // until the user confirms with Save.
  const [downloadNeedsConfirmation, setDownloadNeedsConfirmation] = useState(false);
  const [displayError, setDisplayError] = useState<BrowserDisplayError | null>(null);
  const [downloadsShelfOpen, setDownloadsShelfOpen] = useState(true);
  // The download registry emits no lifecycle event of its own, so every shelf refresh is driven
  // by an explicit signal: the page's `browser_download_requested` event, or a finished save.
  const [downloadsRefreshToken, setDownloadsRefreshToken] = useState(0);
  // The Library (bookmarks manager / history page) replaces the page view while open; the
  // native page view is hidden then, because DOM cannot be drawn over it.
  const [libraryView, setLibraryView] = useState<BrowserLibraryView | null>(null);
  const [openTabs, setOpenTabs] = useState<Array<{ browserId: string; url: string; title: string | null }>>([]);
  const { settings: browserSettings } = useBrowserSettings();
  const { tree: bookmarkTree } = useBookmarks();
  const updateBoundsRef = useRef<() => void>(() => undefined);
  // Latest find request wins: async find/clear responses can resolve out of order, so a stale
  // response for a superseded or cleared query must not overwrite the current result.
  const findRequestSeqRef = useRef(0);
  // Native child webviews live above the WKWebView on macOS, so DOM z-index cannot cover them.
  // The browser surface must yield (hide) whenever a global modal/search surface is mounted, then
  // be restored only if this owner is itself visible. Mirrors NativeTerminalPane's occlusion fix.
  const surfaceVisible = useNativeTerminalVisibility();
  const maskAwareVisible = visible && surfaceVisible && libraryView === null;

  useEffect(() => {
    const previous = previousTabRef.current;
    previousTabRef.current = tab;
    if (previous === tab) return;
    if (previous.browserId !== tab.browserId) {
      setLiveTab(tab);
      return;
    }
    const changed: Partial<BrowserTab> = {};
    for (const key of Object.keys({ ...previous, ...tab }) as (keyof BrowserTab)[]) {
      if (!Object.is(previous[key], tab[key])) {
        (changed as Record<string, unknown>)[key] = tab[key];
      }
    }
    if (Object.keys(changed).length === 0) return;
    setLiveTab((current) => ({ ...current, ...changed }));
  }, [tab]);

  useEffect(() => {
    if (findOpen) findInputRef.current?.focus();
  }, [findOpen]);

  useEffect(() => {
    // Invalidate any in-flight find when the target browser changes or the pane unmounts, so a
    // late response cannot apply to a different browser reusing this component instance.
    setDisplayError(null);
    return () => {
      findRequestSeqRef.current += 1;
    };
  }, [tab.browserId]);

  useEffect(() => {
    // A Chromium tab finds natively and reports the count afterwards, as events.
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void onBrowserFindResult((payload) => {
      if (payload.browserId !== tab.browserId) return;
      setFindResult({ matchCount: payload.matchCount, found: payload.matchCount > 0 });
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else cleanup = unlisten;
      })
      .catch((error: unknown) => {
        if (!disposed) setPaneAlert({ action: "Find listener", code: parseErrorCode(error) });
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [tab.browserId]);

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
        engine: payload.engine ?? current.engine,
        loadError: payload.loadError ?? null,
      }));
      // History is recorded host-side from the same state change (both engines), and never for
      // private-profile tabs; see src-tauri/src/browser/library/recorder.rs.
    };

    void listen<BrowserStateChangedPayload>("browser_state_changed", (event) => {
      applyState(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch((error: unknown) => {
      // Without this subscription the pane stops tracking the native tab (url, title, loading,
      // navigation state) and silently freezes at whatever it last saw.
      if (!disposed) setPaneAlert({ action: "Live state updates", code: parseErrorCode(error) });
    });

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
      if (detail?.browserId === tab.browserId) openFind(detail.action);
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
    }).catch((error: unknown) => {
      if (!disposed) setPaneAlert({ action: "Shortcut listener", code: parseErrorCode(error) });
    });
    void onBrowserDownloadRequested((payload) => {
      if (payload.browserId !== tab.browserId) return;
      setDownloadUrl(payload.targetUrl);
      setDownloadStatus(null);
      setDownloadNeedsConfirmation(payload.requiresConfirmation === true);
      // A new download re-reveals the shelf the user dismissed earlier, and refreshes it.
      setDownloadsShelfOpen(true);
      setDownloadsRefreshToken((current) => current + 1);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else downloadCleanup = cleanup;
    }).catch((error: unknown) => {
      if (!disposed) setPaneAlert({ action: "Download listener", code: parseErrorCode(error) });
    });

    return () => {
      disposed = true;
      shortcutCleanup?.();
      downloadCleanup?.();
      window.removeEventListener(BROWSER_SHORTCUT_EVENT, handleDomShortcut);
    };
  }, [tab.browserId]);

  useEffect(() => {
    const openLibrary = (event: Event) => {
      const detail = (event as CustomEvent<BrowserLibraryOpenDetail>).detail;
      if (detail?.browserId === tab.browserId) setLibraryView(detail.view);
    };
    window.addEventListener(BROWSER_LIBRARY_OPEN_EVENT, openLibrary);
    return () => window.removeEventListener(BROWSER_LIBRARY_OPEN_EVENT, openLibrary);
  }, [tab.browserId]);

  // Open tabs for the omnibox "switch to tab" rows; private tabs are never offered elsewhere.
  const isPrivatePane = tab.profileId === PRIVATE_BROWSER_PROFILE.id;
  useEffect(() => {
    if (!visible) return;
    let disposed = false;
    void Promise.resolve()
      .then(() => listBrowsers())
      .then((sessions) => {
        if (disposed || !Array.isArray(sessions)) return;
        setOpenTabs(sessions
          .filter((session) => isPrivatePane || session.profileId !== PRIVATE_BROWSER_PROFILE.id)
          .map((session) => ({ browserId: session.browserId, url: session.url, title: session.title ?? null })));
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
    };
  }, [isPrivatePane, liveTab.url, visible]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    // A show that races its own bounds update is what makes the child webview cover the
    // wrong region: `boundsSeq` marks the newest geometry request, so only the reveal that
    // belongs to the latest acknowledged frame is allowed to run.
    let disposed = false;
    let boundsSeq = 0;

    const updateVisibility = (nextVisible: boolean) => {
      void setBrowserVisible(tab.browserId, nextVisible)
        .then(() => {
          if (!disposed && nextVisible) {
            setDisplayError(null);
          }
        })
        .catch((error: unknown) => {
          if (disposed) return;
          setDisplayError({
            operation: "visibility",
            code: parseErrorCode(error),
          });
        });
    };

    const updateBounds = () => {
      if (!maskAwareVisible || liveTab.loadError) {
        boundsSeq += 1;
        updateVisibility(false);
        return;
      }

      const rect = el.getBoundingClientRect();
      const toolbarRect = toolbarRef.current?.getBoundingClientRect();
      const minTop = toolbarRect ? toolbarRect.bottom : rect.y;
      const clampedY = Math.max(rect.y, minTop);
      const heightReduction = clampedY - rect.y;
      const clampedHeight = Math.max(0, rect.height - heightReduction);

      // On macOS the child webview is a sibling NSView stacked above the app WKWebView, and
      // the native terminal surface sits below that webview. An opaque child shown before its
      // frame matches this pane therefore paints over app chrome and blanks whatever terminal
      // occupies the stale rect, which no DOM z-index can undo. Reveal only after the backend
      // has acknowledged the geometry.
      const seq = ++boundsSeq;
      void setBrowserBounds(tab.browserId, {
        x: rect.x,
        y: clampedY,
        width: rect.width,
        height: clampedHeight,
      })
        .then(() => {
          if (disposed || seq !== boundsSeq) return;
          updateVisibility(true);
        })
        .catch((error: unknown) => {
          if (disposed || seq !== boundsSeq) return;
          setDisplayError({
            operation: "bounds",
            code: parseErrorCode(error),
          });
        });
    };

    updateBoundsRef.current = updateBounds;
    updateBounds();

    const resizeObserver = new ResizeObserver(updateBounds);
    resizeObserver.observe(el);
    window.addEventListener("resize", updateBounds);

    return () => {
      disposed = true;
      boundsSeq += 1;
      updateBoundsRef.current = () => undefined;
      resizeObserver.disconnect();
      window.removeEventListener("resize", updateBounds);
      // Native child webviews outlive React DOM nodes; cleanup also covers Fast Refresh remounts.
      updateVisibility(false);
    };
  }, [liveTab.loadError, tab.browserId, maskAwareVisible]);

  const retryBoundsAndVisibility = () => {
    setDisplayError(null);
    updateBoundsRef.current();
  };

  const runFind = async (query: string, backwards = false) => {
    const seq = ++findRequestSeqRef.current;
    setFindQuery(query);
    setFindError(null);
    if (!query.trim()) {
      setFindResult({ matchCount: 0, found: false });
      try {
        await clearBrowserFind(tab.browserId);
      } catch (error) {
        if (seq === findRequestSeqRef.current) setFindError(parseErrorCode(error));
      }
      return;
    }
    try {
      const result = await findBrowser(tab.browserId, query, backwards);
      if (seq === findRequestSeqRef.current) setFindResult(result);
    } catch (error) {
      if (seq === findRequestSeqRef.current) {
        // The find bar is the context, so it shows the backend's structured code instead of a
        // generic fallback that hides which failure occurred.
        setFindError(parseErrorCode(error));
      }
    }
  };

  const closeFind = () => {
    // Supersede any in-flight find so its late response cannot repopulate a closed bar.
    findRequestSeqRef.current += 1;
    setFindOpen(false);
    setFindQuery("");
    setFindResult({ matchCount: 0, found: false });
    setFindError(null);
    // Closing find is a no-op when there is nothing to clear; a stale highlight is not worth an
    // alert after the user dismissed the bar.
    void clearBrowserFind(tab.browserId).catch(() => undefined);
  };

  const saveDownload = async () => {
    if (!downloadUrl) return;
    let saveAttempted = false;
    try {
      const filePath = await save({ defaultPath: suggestedDownloadName(downloadUrl) });
      if (!filePath) return;
      saveAttempted = true;
      setDownloadStatus("Saving…");
      await downloadBrowserUrlWithSession(tab.browserId, downloadUrl, filePath);
      setDownloadStatus("Saved");
      setDownloadNeedsConfirmation(false);
    } catch (error) {
      setDownloadStatus(browserErrorText(extractBrowserErrorCode(error)));
    } finally {
      // The registry only gains a record (completed or failed) once this call settles.
      if (saveAttempted) setDownloadsRefreshToken((current) => current + 1);
    }
  };

  const openDownloadExternally = (url: string) => {
    void Promise.resolve(openExternalUrl(url))
      .then(() => setPaneAlert((current) => (current?.action === "Open in system browser" ? null : current)))
      .catch((error: unknown) => setPaneAlert({ action: "Open in system browser", code: parseErrorCode(error) }));
  };

  return (
    <div className="flex flex-col w-full h-full bg-background overflow-hidden">
      <div ref={toolbarRef} className="relative">
        {liveTab.loading && liveTab.engine !== "chromium" ? (
          <div
            role="progressbar"
            aria-label="Loading page"
            aria-busy="true"
            data-testid="browser-loading-bar"
            className="pointer-events-none absolute inset-x-0 top-0 z-20 h-0.5 overflow-hidden bg-primary/15"
          >
            <div className="h-full w-1/3 animate-pulse rounded-full bg-primary" />
          </div>
        ) : null}
        {/* A Chromium tab draws its own toolbar (address bar, extensions, menu) inside the pane. */}
        {liveTab.engine === "chromium" ? null : (
          <BrowserToolbar tab={liveTab} onNavigate={onNavigate} onReload={onReload} designFeedbackTargets={designFeedbackTargets} openTabs={openTabs} />
        )}
        {liveTab.engine === "chromium" || !browserSettings.showBookmarkBar ? null : (
          <BrowserBookmarkBar tree={bookmarkTree} onOpen={onNavigate} />
        )}
        {/* Shown for both engines: the Chromium tab hides the Ferryx toolbar. */}
        <div className="flex justify-end px-3 empty:hidden">
          <BrowserMediaCaptureIndicator browserId={tab.browserId} />
        </div>
        <BrowserPermissionPrompts browserId={tab.browserId} />
        <BrowserSignInHandoffBar tab={liveTab} />
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
            <span aria-live="polite" className="min-w-14 text-center text-[10px] text-muted-foreground">
              {findQuery ? `${findResult.matchCount} matches` : "0 matches"}
            </span>
            <button type="button" aria-label="Previous match" onClick={() => void runFind(findQuery, true)} className="rounded p-1 hover:bg-accent"><ChevronUp className="size-3.5" /></button>
            <button type="button" aria-label="Next match" onClick={() => void runFind(findQuery, false)} className="rounded p-1 hover:bg-accent"><ChevronDown className="size-3.5" /></button>
            <button type="button" aria-label="Close find" onClick={closeFind} className="rounded p-1 hover:bg-accent"><X className="size-3.5" /></button>
            {findError ? <span className="text-[10px] text-destructive" data-error-code={findError}>{browserErrorText(findError)}</span> : null}
          </div>
        ) : null}
        {downloadUrl ? (
          <div
            className="flex items-center gap-2 border-b border-border bg-popover px-3 py-1.5 text-[11px]"
            data-testid="browser-download-prompt"
            data-requires-confirmation={downloadNeedsConfirmation ? "true" : undefined}
          >
            <Download className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate">Download {suggestedDownloadName(downloadUrl)}</span>
            {downloadNeedsConfirmation ? (
              <span className="text-amber-500" data-testid="browser-download-needs-confirmation">
                Needs confirmation: the page started this without a click
              </span>
            ) : null}
            {downloadStatus ? <span className="text-muted-foreground">{downloadStatus}</span> : null}
            <button type="button" onClick={() => openDownloadExternally(downloadUrl)} className="flex items-center gap-1 rounded border border-border px-2 py-1 hover:bg-accent"><ExternalLink className="size-3" /> Open in system browser</button>
            <button type="button" onClick={() => void saveDownload()} className="rounded border border-border px-2 py-1 hover:bg-accent">{downloadNeedsConfirmation ? "Save" : "Save as…"}</button>
            <button type="button" aria-label="Dismiss download" onClick={() => { setDownloadUrl(null); setDownloadStatus(null); setDownloadNeedsConfirmation(false); }} className="rounded p-1 hover:bg-accent"><X className="size-3.5" /></button>
          </div>
        ) : null}
        <BrowserEngineFallbackNotice browserId={tab.browserId} />
        <BrowserVaultBar browserId={liveTab.browserId} url={liveTab.url} />
        {paneAlert ? (
          <div
            role="alert"
            data-testid="browser-pane-alert"
            className="flex items-center gap-2 border-b border-destructive/20 bg-destructive/10 px-3 py-1.5 text-xs text-destructive"
          >
            <CircleAlert className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate">
              {paneAlert.action} failed: {browserErrorText(paneAlert.code)}
            </span>
          </div>
        ) : null}
        {displayError ? (
          <div
            className="flex items-center gap-2 border-b border-destructive/20 bg-destructive/10 px-3 py-1.5 text-xs text-destructive"
            data-testid="browser-pane-error"
          >
            <CircleAlert className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate">
              Couldn't show the page: {displayError.code} ({displayError.operation})
            </span>
            <button
              type="button"
              onClick={retryBoundsAndVisibility}
              className="flex items-center gap-1 rounded border border-destructive/30 px-2 py-0.5 text-xs hover:bg-destructive/20"
              data-testid="browser-retry-bounds"
            >
              <RefreshCw className="size-3" /> Retry
            </button>
          </div>
        ) : null}
        {downloadsShelfOpen ? (
          <BrowserDownloadsShelf
            onClose={() => setDownloadsShelfOpen(false)}
            refreshToken={downloadsRefreshToken}
          />
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
        {libraryView ? (
          <BrowserLibraryPanel
            view={libraryView}
            onViewChange={setLibraryView}
            onOpen={(url) => {
              setLibraryView(null);
              onNavigate(url);
            }}
            onClose={() => setLibraryView(null)}
          />
        ) : null}
        {liveTab.loadError ? (
          <div className="absolute inset-0 z-20 flex items-center justify-center bg-background p-8" data-testid="browser-load-error">
            <div className="w-full max-w-lg rounded-lg border border-border bg-card p-5 text-card-foreground shadow-lg">
              <div className="flex items-center gap-2 text-sm font-semibold">
                <CircleAlert className="size-4 shrink-0 text-destructive" />
                This page could not be loaded
              </div>
              <div className="mt-2 truncate font-mono text-[11px] text-muted-foreground" title={liveTab.url}>{liveTab.url}</div>
              <div className="mt-2 break-words text-xs text-destructive">{liveTab.loadError}</div>
              <div className="mt-4 flex flex-wrap items-center gap-2">
                <button type="button" onClick={() => onReload()} className="flex items-center gap-1.5 rounded bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  <RefreshCw className="size-3.5" /> Retry
                </button>
                <button
                  type="button"
                  onClick={() => openDownloadExternally(liveTab.url)}
                  data-testid="browser-load-error-open-external"
                  className="flex items-center gap-1.5 rounded border border-border px-3 py-1.5 text-xs text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <ExternalLink className="size-3.5" /> Open in system browser
                </button>
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
