import { AlertCircle, CheckCircle2, ExternalLink, Folder, Pause, Play, RotateCw, X, XCircle } from "lucide-react";
import type { DownloadRecord } from "../lib/browserTauri";
import { downloadFailureLabel } from "../lib/browserSystem";

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

export interface BrowserDownloadRowProps {
  record: DownloadRecord;
  /** The finished file is gone from disk. */
  missing?: boolean;
  actionError?: string;
  onCancel: (id: string) => void;
  /** Pause (true) or resume (false). */
  onPause: (id: string, pause: boolean) => void;
  onOpen: (record: DownloadRecord) => void;
  onReveal: (record: DownloadRecord) => void;
  /** Without it the row has no dismiss button. */
  onDismiss?: (id: string) => void;
}

/** One download with its status, progress and the controls its engine supports: Pause/Resume
 * only with `canPause`, Cancel unless `canCancel` is false. */
export function BrowserDownloadRow({
  record: item,
  missing: isMissing = false,
  actionError,
  onCancel,
  onPause,
  onOpen,
  onReveal,
  onDismiss,
}: BrowserDownloadRowProps) {
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
            <span className="rounded bg-muted px-1 py-0.2 text-[10px] text-muted-foreground">Cancelled</span>
          ) : isPaused ? (
            <span className="rounded bg-muted px-1 py-0.2 text-[10px] text-muted-foreground">Paused</span>
          ) : null}
        </div>

        {isInProgress ? (
          <div className="mt-1 flex items-center gap-2">
            <div className="h-1 flex-1 rounded-full bg-secondary overflow-hidden">
              <div className="h-full bg-primary transition-all duration-200" style={{ width: `${percent ?? 100}%` }} />
            </div>
            <span className="shrink-0 text-[10px] text-muted-foreground">
              {percent !== null ? `${percent}%` : formatBytes(item.receivedBytes)}
            </span>
          </div>
        ) : null}

        {actionError ? <div className="text-[10px] text-destructive mt-0.5">{actionError}</div> : null}
      </div>

      <div className="flex items-center gap-1 shrink-0">
        {isInProgress && item.canPause ? (
          <button
            type="button"
            aria-label={isPaused ? "Resume download" : "Pause download"}
            onClick={() => onPause(item.id, !isPaused)}
            className="rounded border border-border p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
          >
            {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
          </button>
        ) : null}

        {isInProgress && canCancel ? (
          <button
            type="button"
            onClick={() => onCancel(item.id)}
            className="rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-destructive/10 hover:text-destructive"
          >
            Cancel
          </button>
        ) : null}

        {isCompleted && !isMissing ? (
          <button
            type="button"
            onClick={() => onOpen(item)}
            className="flex items-center gap-1 rounded border border-border px-1.5 py-0.5 text-[10px] hover:bg-accent"
          >
            <ExternalLink className="size-3" />
            Open
          </button>
        ) : null}

        {isCompleted && !isMissing ? (
          <button
            type="button"
            onClick={() => onReveal(item)}
            aria-label="Reveal in folder"
            className="rounded border border-border p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
          >
            <Folder className="size-3" />
          </button>
        ) : null}

        {onDismiss ? (
          <button
            type="button"
            aria-label="Dismiss download"
            onClick={() => onDismiss(item.id)}
            className="rounded p-1 hover:bg-accent text-muted-foreground hover:text-foreground"
          >
            <X className="size-3" />
          </button>
        ) : null}
      </div>
    </div>
  );
}
