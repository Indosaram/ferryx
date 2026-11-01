import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  extractBrowserErrorCode,
  onBrowserDownloadUpdated,
  type DownloadRecord,
  type DownloadStatus,
} from "../lib/browserTauri";
import { downloadFailureLabel, pauseBrowserDownload } from "../lib/browserSystem";
import {
  AlertCircle,
  CheckCircle2,
  Download,
  ExternalLink,
  Folder,
  Pause,
  Play,
  RotateCw,
  X,
  XCircle,
} from "lucide-react";

export const MAX_RENDERED_DOWNLOADS = 10;

export type { DownloadRecord, DownloadStatus };

export interface BrowserDownloadsShelfProps {
  onClose?: () => void;
  className?: string;
  /**
   * Change signal from the owning pane. The download registry exposes no lifecycle event of its
   * own, so the pane forwards the app's existing `browser_download_requested` event and its own
   * post-save refresh here; the shelf never polls the registry on a fixed timer.
   */
  refreshToken?: number;
}

export function sanitizeDownloadFilename(raw: string): string {
  if (!raw || typeof raw !== "string") return "download";
  const base = raw.split(/[?#]/)[0] ?? "";
  const parts = base.split(/[/\\]/).filter(Boolean);
  const candidate = parts.pop() || "download";
  const sanitized = candidate
    .replace(/[\x00-\x1f\x7f]/g, "")
    .replace(/\.\.+/g, "")
    .replace(/[<>:"/\\|?*]/g, "_")
    .trim();
  const noLeadingDot = sanitized.replace(/^\.+/, "");
  return noLeadingDot.length > 0 ? noLeadingDot : "download";
}

/** Merges a registry list into the rendered records by id. A record already rendered from a
 * newer `browser_download_updated` event wins over an older copy in the list, and records only
 * known from events are kept. */
export function mergeDownloadRecords(current: DownloadRecord[], incoming: DownloadRecord[]): DownloadRecord[] {
  const currentById = new Map(current.map((record) => [record.id, record]));
  const incomingIds = new Set(incoming.map((record) => record.id));
  const merged = incoming.map((record) => {
    const existing = currentById.get(record.id);
    return existing && existing.updatedAtMs > record.updatedAtMs ? existing : record;
  });
  return [...current.filter((record) => !incomingIds.has(record.id)), ...merged];
}

export function extractDownloadName(filePath: string, url: string): string {
  if (filePath && filePath.trim()) {
    const fromPath = sanitizeDownloadFilename(filePath);
    if (fromPath !== "download") return fromPath;
  }
  return sanitizeDownloadFilename(url);
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * A missing download target is reported by `cmd_open_file_path` as code `INVALID_PATH` carrying the
 * structured `details.reason = "missing"` marker that `prepare_launch` attaches
 * (src-tauri/src/ipc/file_link.rs). `INVALID_PATH` alone also covers malformed path tokens, so the
 * reason field is what separates "gone from disk" from every other rejection. Nothing here reads
 * the error message prose: the backend rejects with `{ code, message, details }`.
 */
export function isMissingFilePathError(error: unknown): boolean {
  if (extractBrowserErrorCode(error) !== "INVALID_PATH") return false;
  if (!error || typeof error !== "object") return false;
  const details = (error as { details?: unknown }).details;
  if (!details || typeof details !== "object") return false;
  return (details as { reason?: unknown }).reason === "missing";
}

export function BrowserDownloadsShelf({ onClose, className = "", refreshToken = 0 }: BrowserDownloadsShelfProps) {
  const [downloads, setDownloads] = useState<DownloadRecord[]>([]);
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(() => new Set());
  const [missingFileIds, setMissingFileIds] = useState<Set<string>>(() => new Set());
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({});
  const [loadErrorCode, setLoadErrorCode] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  // Overlapping list reads can resolve out of order; only the newest request may apply.
  const fetchSeqRef = useRef(0);

  // A failed read must not look like an empty registry: the failure is kept and rendered as an
  // inline row, because "no downloads" and "the list could not be read" are different states.
  const fetchDownloads = useCallback(async () => {
    const seq = ++fetchSeqRef.current;
    try {
      const records = await invoke<DownloadRecord[]>("cmd_browser_download_list");
      if (seq !== fetchSeqRef.current) return;
      setDownloads((current) => mergeDownloadRecords(current, records));
      setLoadErrorCode(null);
    } catch (error) {
      if (seq === fetchSeqRef.current) setLoadErrorCode(extractBrowserErrorCode(error));
    }
  }, []);

  // The download registry exposes no lifecycle event of its own, so the shelf reads it once when
  // it opens and again only when its owner signals a change (the app's existing
  // `browser_download_requested` event, or a finished save). `refreshToken` is that signal: the
  // ref records which signal the currently rendered data belongs to, so only the first read
  // toggles the loading placeholder and no fixed timer polls the registry.
  const loadedRefreshTokenRef = useRef<number | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    void onBrowserDownloadUpdated((record) => {
      if (disposed) return;
      setDownloads((prev) => {
        const index = prev.findIndex((item) => item.id === record.id);
        if (index >= 0) {
          const next = [...prev];
          next[index] = record;
          return next;
        }
        return [record, ...prev];
      });
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch((error: unknown) => {
        if (!disposed) setLoadErrorCode(extractBrowserErrorCode(error));
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const isInitialLoad = loadedRefreshTokenRef.current === null;
    loadedRefreshTokenRef.current = refreshToken;
    if (isInitialLoad) setLoading(true);
    void fetchDownloads().finally(() => {
      if (isInitialLoad) setLoading(false);
    });
  }, [fetchDownloads, refreshToken]);

  const visibleDownloads = useMemo(() => {
    return downloads
      .filter((record) => !dismissedIds.has(record.id))
      .slice(0, MAX_RENDERED_DOWNLOADS);
  }, [downloads, dismissedIds]);

  const handleCancel = async (id: string) => {
    try {
      await invoke<boolean>("cmd_browser_download_cancel", { downloadId: id });
    } catch (error) {
      setActionErrors((prev) => ({
        ...prev,
        [id]: extractBrowserErrorCode(error),
      }));
    }
  };

  const handlePause = async (id: string, pause: boolean) => {
    try {
      await pauseBrowserDownload(id, pause);
    } catch (error) {
      setActionErrors((prev) => ({
        ...prev,
        [id]: extractBrowserErrorCode(error),
      }));
    }
  };

  const handleOpen = async (record: DownloadRecord) => {
    if (missingFileIds.has(record.id)) return;
    try {
      await invoke<boolean>("cmd_open_file_path", {
        path: record.filePath,
        editor: "system",
      });
      setActionErrors((prev) => {
        const next = { ...prev };
        delete next[record.id];
        return next;
      });
    } catch (error) {
      if (isMissingFilePathError(error)) {
        setMissingFileIds((prev) => new Set(prev).add(record.id));
        setActionErrors((prev) => ({
          ...prev,
          [record.id]: "File missing on disk",
        }));
      } else {
        setActionErrors((prev) => ({
          ...prev,
          [record.id]: extractBrowserErrorCode(error),
        }));
      }
    }
  };

  const handleReveal = async (record: DownloadRecord) => {
    const parentDir = record.filePath.replace(/[/\\][^/\\]+$/, "");
    try {
      await invoke<boolean>("cmd_open_file_path", {
        path: parentDir || record.filePath,
        editor: "system",
      });
    } catch (error) {
      setActionErrors((prev) => ({
        ...prev,
        [record.id]: extractBrowserErrorCode(error),
      }));
    }
  };

  const handleDismiss = (id: string) => {
    setDismissedIds((prev) => new Set(prev).add(id));
  };

  if (visibleDownloads.length === 0 && !loading && !loadErrorCode) {
    return null;
  }

  return (
    <div
      className={`border-b border-border bg-popover text-popover-foreground px-3 py-2 text-xs shadow-sm ${className}`}
      data-testid="browser-downloads-shelf"
    >
      <div className="flex items-center justify-between pb-1.5 border-b border-border/50 mb-1.5">
        <div className="flex items-center gap-1.5 font-medium text-[11px] text-muted-foreground uppercase tracking-wider">
          <Download className="size-3.5" />
          <span>Downloads ({visibleDownloads.length})</span>
        </div>
        {onClose ? (
          <button
            type="button"
            aria-label="Close downloads shelf"
            onClick={onClose}
            className="rounded p-0.5 hover:bg-accent text-muted-foreground hover:text-foreground"
          >
            <X className="size-3.5" />
          </button>
        ) : null}
      </div>

      <div className="flex flex-col gap-1.5 max-h-48 overflow-y-auto">
        {loadErrorCode ? (
          <div
            className="flex items-center gap-2 rounded border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-[11px] text-destructive"
            data-testid="downloads-load-error"
          >
            <AlertCircle className="size-3.5 shrink-0" />
            <span className="min-w-0 flex-1 truncate">Could not read the download list</span>
            <span className="shrink-0 rounded bg-destructive/10 px-1 py-0.2 font-mono text-[10px]">
              {loadErrorCode}
            </span>
          </div>
        ) : null}

        {visibleDownloads.map((item) => {
          const isMissing = missingFileIds.has(item.id);
          const name = extractDownloadName(item.filePath, item.url);
          const percent =
            item.totalBytes && item.totalBytes > 0
              ? Math.min(100, Math.round((item.receivedBytes / item.totalBytes) * 100))
              : null;
          const isPaused = item.status === "paused";
          const isInProgress = item.status === "inProgress" || item.status === "pending" || isPaused;
          const canCancel = item.canCancel !== false;
          const isCompleted = item.status === "completed";
          const isFailed = item.status === "failed";
          const isCancelled = item.status === "cancelled";

          return (
            <div
              key={item.id}
              className="flex items-center gap-2 rounded border border-border/60 bg-background/80 px-2.5 py-1.5 text-[11px]"
              data-testid={`download-row-${item.id}`}
            >
              <div className="shrink-0">
                {isCompleted && !isMissing ? (
                  <CheckCircle2 className="size-3.5 text-emerald-500" />
                ) : isMissing ? (
                  <AlertCircle className="size-3.5 text-amber-500" />
                ) : isFailed ? (
                  <XCircle className="size-3.5 text-destructive" />
                ) : isCancelled ? (
                  <XCircle className="size-3.5 text-muted-foreground" />
                ) : isPaused ? (
                  <Pause className="size-3.5 text-muted-foreground" />
                ) : (
                  <RotateCw className="size-3.5 text-primary animate-spin" />
                )}
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="font-medium truncate max-w-[220px]" title={item.filePath || item.url}>
                    {name}
                  </span>
                  {isMissing ? (
                    <span className="rounded bg-amber-500/10 px-1 py-0.2 text-[10px] text-amber-600 dark:text-amber-400">
                      Deleted on disk
                    </span>
                  ) : isFailed ? (
                    <span
                      className="rounded bg-destructive/10 px-1 py-0.2 text-[10px] text-destructive"
                      data-testid={`download-failure-${item.id}`}
                      data-failure={item.failure ?? undefined}
                    >
                      {downloadFailureLabel(item.failure, item.error)}
                    </span>
                  ) : isCancelled ? (
                    <span className="rounded bg-muted px-1 py-0.2 text-[10px] text-muted-foreground">
                      Cancelled
                    </span>
                  ) : isPaused ? (
                    <span className="rounded bg-muted px-1 py-0.2 text-[10px] text-muted-foreground">
                      Paused
                    </span>
                  ) : null}
                </div>

                {isInProgress ? (
                  <div className="mt-1 flex items-center gap-2">
                    <div className="h-1 flex-1 rounded-full bg-secondary overflow-hidden">
                      <div
                        className="h-full bg-primary transition-all duration-200"
                        style={{ width: `${percent ?? 100}%` }}
                      />
                    </div>
                    <span className="shrink-0 text-[10px] text-muted-foreground">
                      {percent !== null ? `${percent}%` : formatBytes(item.receivedBytes)}
                    </span>
                  </div>
                ) : null}

                {actionErrors[item.id] ? (
                  <div className="text-[10px] text-destructive mt-0.5">
                    {actionErrors[item.id]}
                  </div>
                ) : null}
              </div>

              <div className="flex items-center gap-1 shrink-0">
                {isInProgress && item.canPause ? (
                  <button
                    type="button"
                    aria-label={isPaused ? "Resume download" : "Pause download"}
                    onClick={() => void handlePause(item.id, !isPaused)}
                    className="rounded border border-border p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
                  >
                    {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
                  </button>
                ) : null}

                {isInProgress && canCancel ? (
                  <button
                    type="button"
                    onClick={() => void handleCancel(item.id)}
                    className="rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-destructive/10 hover:text-destructive"
                  >
                    Cancel
                  </button>
                ) : null}

                {isCompleted && !isMissing ? (
                  <button
                    type="button"
                    onClick={() => void handleOpen(item)}
                    className="flex items-center gap-1 rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-accent"
                  >
                    <ExternalLink className="size-3" />
                    Open
                  </button>
                ) : null}

                {isCompleted && !isMissing ? (
                  <button
                    type="button"
                    onClick={() => void handleReveal(item)}
                    aria-label="Reveal in folder"
                    className="rounded border border-border p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
                  >
                    <Folder className="size-3" />
                  </button>
                ) : null}

                <button
                  type="button"
                  aria-label="Dismiss download"
                  onClick={() => handleDismiss(item.id)}
                  className="rounded p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
                >
                  <X className="size-3" />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
