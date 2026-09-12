import {
  AlertCircle,
  AlertTriangle,
  Clock,
  HardDrive,
  Loader2,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { cn } from "../lib/cn";
import { WORKTREE_DISK_UNUSED_DAYS_KEY } from "../lib/storageKeys";
import {
  cancelWorktreeDiskScan,
  deleteWorktree,
  deleteWorktreeDestructive,
  getWorktreeDiskScanResult,
  onWorktreeDiskScanProgress,
  previewWorktreeDelete,
  startWorktreeDiskScan,
  toIpcError,
} from "../lib/tauri";
import {
  worktreeIdentity,
  type BranchDeletionPreview,
  type DiskScanSnapshot,
  type DiskScanStatus,
  type StructuredIpcError,
  type Worktree,
  type WorktreeDiskRow,
} from "../lib/types";
import { Badge } from "./ui/badge";
import { Progress } from "./ui/progress";
import { WorktreeDeleteDialog } from "./WorktreeDeleteDialog";

export type { DiskScanSnapshot, DiskScanStatus, WorktreeDiskRow };

export type WorktreeDiskServices = {
  startScan: (workspaceId: string, refresh: boolean) => Promise<DiskScanSnapshot>;
  cancelScan: (workspaceId: string, scanId: string) => Promise<boolean>;
  getScanResult: (workspaceId: string) => Promise<DiskScanSnapshot | null>;
  onScanProgress: (handler: (snapshot: DiskScanSnapshot) => void) => Promise<() => void>;
  previewDelete: (workspaceId: string, worktree: Worktree) => Promise<BranchDeletionPreview>;
  deleteSafe: (workspaceId: string, worktree: Worktree) => Promise<void>;
  deleteDestructive: (workspaceId: string, worktree: Worktree) => Promise<void>;
};

export type WorktreeDiskDialogProps = {
  workspaceId: string;
  projectName?: string;
  onClose: () => void;
  services?: WorktreeDiskServices;
};

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "—";
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const unitIndex = Math.min(i, units.length - 1);
  const value = bytes / Math.pow(1024, unitIndex);
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export function formatRelativeTime(unixSeconds: number | null | undefined): string {
  if (!unixSeconds) return "—";
  const nowSec = Math.floor(Date.now() / 1000);
  const diffSec = nowSec - unixSeconds;
  if (diffSec < 60) return "just now";
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
  const days = Math.floor(diffSec / 86400);
  if (days === 1) return "yesterday";
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

export function worktreeSlug(worktree: Worktree): string {
  const identity = worktreeIdentity(worktree);
  if (identity?.slug) return identity.slug;
  const parts = worktree.path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || worktree.path;
}

export function isPrimaryWorktree(worktree: Worktree): boolean {
  return worktreeIdentity(worktree) === null;
}

export function isCleanupCandidate(row: WorktreeDiskRow, unusedDaysThreshold: number): boolean {
  if (isPrimaryWorktree(row.worktree)) return false;
  if (row.worktree.prunable !== null) return true;
  if (row.lastCommitAt !== null) {
    const ageSeconds = Date.now() / 1000 - row.lastCommitAt;
    return ageSeconds > unusedDaysThreshold * 86400;
  }
  return false;
}

export function candidateReason(row: WorktreeDiskRow, unusedDaysThreshold: number): string | null {
  if (isPrimaryWorktree(row.worktree)) return null;
  if (row.worktree.prunable !== null) return "Prunable";
  if (row.lastCommitAt !== null) {
    const ageSeconds = Date.now() / 1000 - row.lastCommitAt;
    if (ageSeconds > unusedDaysThreshold * 86400) {
      const days = Math.floor(ageSeconds / 86400);
      return `Inactive (${days}d)`;
    }
  }
  return null;
}

type SortField = "size-desc" | "size-asc" | "commit-desc" | "commit-asc" | "name-asc";

const DEFAULT_UNUSED_DAYS = 14;

function createDefaultServices(): WorktreeDiskServices {
  return {
    startScan: (workspaceId, refresh) => startWorktreeDiskScan({ workspaceId, refresh }),
    cancelScan: (workspaceId, scanId) => cancelWorktreeDiskScan({ workspaceId, scanId }),
    getScanResult: (workspaceId) => getWorktreeDiskScanResult(workspaceId),
    onScanProgress: (handler) => onWorktreeDiskScanProgress(handler),
    previewDelete: async (workspaceId, worktree) => {
      const identity = worktreeIdentity(worktree);
      if (!identity) {
        throw {
          code: "INVALID_NAMESPACE",
          message: "Only Ferryx-managed worktrees can be deleted from this UI.",
          details: { path: worktree.path },
        };
      }
      return previewWorktreeDelete({ workspaceId, worktree: identity });
    },
    deleteSafe: async (workspaceId, worktree) => {
      const identity = worktreeIdentity(worktree);
      if (!identity) {
        throw {
          code: "INVALID_NAMESPACE",
          message: "Only Ferryx-managed worktrees can be deleted from this UI.",
          details: { path: worktree.path },
        };
      }
      await deleteWorktree({ workspaceId, worktree: identity, deleteBranch: true });
    },
    deleteDestructive: async (workspaceId, worktree) => {
      const identity = worktreeIdentity(worktree);
      if (!identity) {
        throw {
          code: "INVALID_NAMESPACE",
          message: "Only Ferryx-managed worktrees can be deleted from this UI.",
          details: { path: worktree.path },
        };
      }
      await deleteWorktreeDestructive({ workspaceId, worktree: identity, deleteBranch: true });
    },
  };
}

export function WorktreeDiskDialog({
  workspaceId,
  projectName,
  onClose,
  services,
}: WorktreeDiskDialogProps) {
  const resolvedServices = useMemo(() => services ?? createDefaultServices(), [services]);

  const [snapshot, setSnapshot] = useState<DiskScanSnapshot | null>(null);
  const [error, setError] = useState<StructuredIpcError | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [deletingRow, setDeletingRow] = useState<WorktreeDiskRow | null>(null);
  const [sortField, setSortField] = useState<SortField>("size-desc");

  const [unusedDays, setUnusedDays] = useState<number>(() => {
    try {
      if (typeof window !== "undefined" && window.localStorage) {
        const stored = window.localStorage.getItem(WORKTREE_DISK_UNUSED_DAYS_KEY);
        if (stored) {
          const parsed = parseInt(stored, 10);
          if (!isNaN(parsed) && parsed > 0) return parsed;
        }
      }
    } catch {
      // ignore
    }
    return DEFAULT_UNUSED_DAYS;
  });

  const handleUnusedDaysChange = (days: number) => {
    setUnusedDays(days);
    try {
      if (typeof window !== "undefined" && window.localStorage) {
        window.localStorage.setItem(WORKTREE_DISK_UNUSED_DAYS_KEY, String(days));
      }
    } catch {
      // ignore
    }
  };

  const activeScanIdRef = useRef<string | null>(null);

  const startScan = async (refresh: boolean) => {
    setError(null);
    setCancelling(false);
    try {
      const initial = await resolvedServices.startScan(workspaceId, refresh);
      activeScanIdRef.current = initial.scanId;
      setSnapshot(initial);
      if (initial.error) {
        setError(initial.error);
      }
    } catch (cause) {
      setError(toIpcError(cause));
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let mounted = true;

    const setup = async () => {
      try {
        unlisten = await resolvedServices.onScanProgress((latest) => {
          if (!mounted) return;
          if (latest.workspaceId !== workspaceId) return;
          setSnapshot(latest);
          activeScanIdRef.current = latest.scanId;
          if (latest.error) {
            setError(latest.error);
          }
        });
      } catch (cause) {
        if (mounted) setError(toIpcError(cause));
      }

      if (mounted) {
        await startScan(false);
      }
    };

    void setup();

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [workspaceId, resolvedServices]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (deletingRow) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, deletingRow]);

  const handleCancelScan = async () => {
    const scanId = snapshot?.scanId ?? activeScanIdRef.current;
    if (!scanId) return;
    setCancelling(true);
    try {
      await resolvedServices.cancelScan(workspaceId, scanId);
    } catch (cause) {
      setError(toIpcError(cause));
    } finally {
      setCancelling(false);
    }
  };

  const sortedRows = useMemo(() => {
    const rows = [...(snapshot?.rows ?? [])];
    rows.sort((a, b) => {
      switch (sortField) {
        case "size-desc":
          return (b.sizeBytes ?? -1) - (a.sizeBytes ?? -1);
        case "size-asc":
          return (a.sizeBytes ?? -1) - (b.sizeBytes ?? -1);
        case "commit-desc":
          return (b.lastCommitAt ?? -1) - (a.lastCommitAt ?? -1);
        case "commit-asc":
          return (a.lastCommitAt ?? -1) - (b.lastCommitAt ?? -1);
        case "name-asc":
          return worktreeSlug(a.worktree).localeCompare(worktreeSlug(b.worktree));
        default:
          return 0;
      }
    });
    return rows;
  }, [snapshot?.rows, sortField]);

  const totalBytes = useMemo(() => {
    return (snapshot?.rows ?? []).reduce((acc, row) => acc + (row.sizeBytes ?? 0), 0);
  }, [snapshot?.rows]);

  const candidateCount = useMemo(() => {
    return (snapshot?.rows ?? []).filter((row) => isCleanupCandidate(row, unusedDays)).length;
  }, [snapshot?.rows, unusedDays]);

  const isScanning = snapshot?.status === "running";
  const isFailed = snapshot?.status === "failed" || error !== null;
  const isCancelled = snapshot?.status === "cancelled";
  const progressPercent =
    snapshot?.progress && snapshot.progress.totalWorktrees > 0
      ? Math.round((snapshot.progress.completedWorktrees / snapshot.progress.totalWorktrees) * 100)
      : 0;

  const handleRowDeleted = (deletedPath: string) => {
    if (snapshot) {
      const updatedRows = snapshot.rows.filter((r) => r.worktree.path !== deletedPath);
      setSnapshot({
        ...snapshot,
        rows: updatedRows,
      });
    }
    setDeletingRow(null);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-label="Worktree Disk Management"
        aria-modal="true"
        className="flex max-h-[85vh] w-full max-w-4xl flex-col animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        {/* Header */}
        <header className="flex items-center justify-between border-b border-border px-5 py-3.5">
          <div className="flex items-center gap-2.5 text-sm font-semibold text-foreground">
            <HardDrive className="size-4 text-primary" />
            <span>Worktree Disk Management</span>
            {projectName ? (
              <span className="rounded-md bg-muted px-2 py-0.5 text-xs font-normal text-muted-foreground">
                {projectName}
              </span>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={isScanning}
              onClick={() => void startScan(true)}
              aria-label="Refresh scan"
              title="Re-scan worktree disk usage"
              className="flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
            >
              <RefreshCw className={cn("size-3.5", isScanning && "animate-spin")} />
              <span>Refresh</span>
            </button>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close dialog"
              className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <X className="size-4" />
            </button>
          </div>
        </header>

        {/* Estimation disclaimer */}
        <div className="border-b border-border/60 bg-muted/20 px-5 py-2 text-[11px] text-muted-foreground">
          Apparent size is estimated from file sizes. Reclaimed disk space may vary due to filesystem deduplication and hard links.
        </div>

        {/* Content area */}
        <div className="selectable flex min-h-0 flex-1 flex-col overflow-y-auto p-5 text-xs">
          {/* Scan Error Banner */}
          {isFailed ? (
            <div className="mb-4 rounded-lg border border-destructive/35 bg-destructive/10 p-3.5 text-destructive">
              <div className="flex items-start gap-2.5">
                <AlertCircle className="mt-0.5 size-4 shrink-0" />
                <div className="flex-1 space-y-1">
                  <div className="flex items-center gap-2 font-semibold">
                    <span>Scan failed</span>
                    {error?.code ? (
                      <Badge variant="destructive" className="font-mono text-[10px]">
                        {error.code}
                      </Badge>
                    ) : null}
                  </div>
                  <div className="text-[11px] leading-relaxed text-destructive/90">
                    {error?.message || snapshot?.error?.message || "An error occurred during disk scan."}
                  </div>
                  {error?.code === "UNSUPPORTED" ? (
                    <div className="mt-1 text-[11px] text-destructive/80">
                      Disk measurement requires direct local filesystem access and is not supported for remote SSH workspaces.
                    </div>
                  ) : null}
                </div>
                <button
                  type="button"
                  onClick={() => void startScan(true)}
                  className="rounded-md bg-destructive/20 px-2.5 py-1 text-xs font-medium hover:bg-destructive/30"
                >
                  Retry
                </button>
              </div>
            </div>
          ) : null}

          {/* Cancelled Banner */}
          {isCancelled ? (
            <div className="mb-4 flex items-center justify-between rounded-lg border border-status-warning/30 bg-status-warning/10 p-3 text-status-warning">
              <div className="flex items-center gap-2 text-xs">
                <Clock className="size-4 shrink-0" />
                <span>Scan was cancelled before completion.</span>
              </div>
              <button
                type="button"
                onClick={() => void startScan(true)}
                className="rounded-md bg-status-warning/20 px-2.5 py-1 text-xs font-medium hover:bg-status-warning/30"
              >
                Restart scan
              </button>
            </div>
          ) : null}

          {/* Running Progress */}
          {isScanning ? (
            <div className="mb-4 space-y-2 rounded-lg border border-border bg-background/50 p-4">
              <div className="flex items-center justify-between text-xs">
                <div className="flex items-center gap-2 text-foreground font-medium">
                  <Loader2 className="size-3.5 animate-spin text-status-working" />
                  <span>Scanning worktrees…</span>
                </div>
                <button
                  type="button"
                  disabled={cancelling}
                  onClick={() => void handleCancelScan()}
                  aria-label="Cancel scan"
                  className="rounded-md border border-border px-2.5 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
                >
                  {cancelling ? "Cancelling…" : "Cancel scan"}
                </button>
              </div>

              <Progress value={progressPercent} className="h-1.5" />

              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                <div>
                  {snapshot?.progress
                    ? `${snapshot.progress.completedWorktrees} of ${snapshot.progress.totalWorktrees} worktrees (${formatBytes(snapshot.progress.scannedBytes)} scanned)`
                    : "Starting scan…"}
                </div>
                {snapshot?.progress?.currentPath ? (
                  <div className="max-w-xs truncate font-mono text-[10px] text-muted-foreground/75">
                    {snapshot.progress.currentPath}
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}

          {/* Filter & Summary Bar */}
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-background/40 px-3.5 py-2.5">
            <div className="flex flex-wrap items-center gap-3 text-xs">
              <div>
                <span className="text-muted-foreground">Total apparent size: </span>
                <span className="font-semibold text-foreground">{formatBytes(totalBytes)}</span>
              </div>
              <span className="text-border">|</span>
              <div>
                <span className="text-muted-foreground">Worktrees: </span>
                <span className="font-medium text-foreground">{sortedRows.length}</span>
              </div>
              {candidateCount > 0 ? (
                <>
                  <span className="text-border">|</span>
                  <Badge variant="secondary" className="border border-status-warning/40 bg-status-warning/15 text-status-warning text-[10px]">
                    {candidateCount} cleanup candidate{candidateCount === 1 ? "" : "s"}
                  </Badge>
                </>
              ) : null}
            </div>

            <div className="flex flex-wrap items-center gap-3 text-xs">
              {/* Threshold selector */}
              <div className="flex items-center gap-1.5">
                <label htmlFor="unused-days-select" className="text-[11px] text-muted-foreground">
                  Inactive after:
                </label>
                <select
                  id="unused-days-select"
                  value={unusedDays}
                  onChange={(e) => handleUnusedDaysChange(Number(e.target.value))}
                  className="h-7 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none focus:border-ring"
                >
                  <option value={7}>7 days</option>
                  <option value={14}>14 days (default)</option>
                  <option value={30}>30 days</option>
                  <option value={60}>60 days</option>
                </select>
              </div>

              {/* Sort selector */}
              <div className="flex items-center gap-1.5">
                <label htmlFor="sort-select" className="text-[11px] text-muted-foreground">
                  Sort:
                </label>
                <select
                  id="sort-select"
                  value={sortField}
                  onChange={(e) => setSortField(e.target.value as SortField)}
                  className="h-7 rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none focus:border-ring"
                >
                  <option value="size-desc">Size (Large to Small)</option>
                  <option value="size-asc">Size (Small to Large)</option>
                  <option value="commit-desc">Last Commit (Newest)</option>
                  <option value="commit-asc">Last Commit (Oldest)</option>
                  <option value="name-asc">Name (A–Z)</option>
                </select>
              </div>
            </div>
          </div>

          {/* Rows List */}
          {sortedRows.length === 0 && !isScanning ? (
            <div className="flex flex-1 flex-col items-center justify-center rounded-lg border border-dashed border-border py-12 text-center text-muted-foreground">
              <HardDrive className="mb-2 size-8 text-muted-foreground/40" />
              <div className="text-sm font-medium text-foreground">No worktrees found</div>
              <p className="mt-1 text-xs">This project has no worktrees or the scan returned empty.</p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-lg border border-border bg-background/30">
              <table className="w-full border-collapse text-left text-xs">
                <thead>
                  <tr className="border-b border-border bg-muted/40 text-[11px] font-medium text-muted-foreground">
                    <th className="px-3.5 py-2.5">Worktree</th>
                    <th className="px-3.5 py-2.5 text-right">Apparent Size</th>
                    <th className="px-3.5 py-2.5 text-right">Last Commit</th>
                    <th className="px-3.5 py-2.5">Status</th>
                    <th className="px-3.5 py-2.5 text-right">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/60">
                  {sortedRows.map((row) => {
                    const slug = worktreeSlug(row.worktree);
                    const isRoot = isPrimaryWorktree(row.worktree);
                    const isCandidate = isCleanupCandidate(row, unusedDays);
                    const reason = candidateReason(row, unusedDays);
                    const isDirty = row.isDirty === true;

                    return (
                      <tr
                        key={row.worktree.path}
                        className="transition-colors hover:bg-accent/40"
                      >
                        {/* Worktree name & details */}
                        <td className="px-3.5 py-3">
                          <div className="flex flex-col gap-0.5">
                            <div className="flex items-center gap-2">
                              <span
                                data-testid={`worktree-disk-row-name-${slug}`}
                                className="font-semibold text-foreground"
                              >
                                {slug}
                              </span>
                              {isRoot ? (
                                <Badge variant="secondary" className="px-1.5 py-0 text-[10px] font-normal">
                                  Primary Root
                                </Badge>
                              ) : null}
                              {isCandidate ? (
                                <Badge
                                  variant="secondary"
                                  data-testid={`candidate-badge-${slug}`}
                                  className="border border-status-warning/40 bg-status-warning/15 text-[10px] font-medium text-status-warning"
                                >
                                  Candidate: {reason}
                                </Badge>
                              ) : null}
                            </div>
                            <div className="max-w-md truncate font-mono text-[10px] text-muted-foreground">
                              {row.worktree.branch || row.worktree.path}
                            </div>
                            {row.error ? (
                              <div className="mt-1 flex items-center gap-1 text-[11px] text-destructive">
                                <AlertTriangle className="size-3 shrink-0" />
                                <span>{row.error.message}</span>
                              </div>
                            ) : null}
                          </div>
                        </td>

                        {/* Size */}
                        <td className="px-3.5 py-3 text-right font-mono font-medium text-foreground">
                          {formatBytes(row.sizeBytes)}
                        </td>

                        {/* Last Commit */}
                        <td className="px-3.5 py-3 text-right text-muted-foreground">
                          {formatRelativeTime(row.lastCommitAt)}
                        </td>

                        {/* Dirty / Clean Status */}
                        <td className="px-3.5 py-3">
                          {row.isDirty === null ? (
                            <span className="text-muted-foreground">—</span>
                          ) : isDirty ? (
                            <span className="inline-flex items-center gap-1 text-destructive font-medium">
                              <span className="size-1.5 rounded-full bg-destructive" />
                              {row.dirtyFiles.length > 0
                                ? `${row.dirtyFiles.length} dirty file${row.dirtyFiles.length === 1 ? "" : "s"}`
                                : "Dirty"}
                            </span>
                          ) : (
                            <span className="inline-flex items-center gap-1 text-status-success">
                              <span className="size-1.5 rounded-full bg-status-success" />
                              Clean
                            </span>
                          )}
                        </td>

                        {/* Actions */}
                        <td className="px-3.5 py-3 text-right">
                          <button
                            type="button"
                            disabled={isRoot}
                            onClick={() => setDeletingRow(row)}
                            data-testid={`delete-btn-${slug}`}
                            title={isRoot ? "Primary root worktree cannot be deleted" : "Clean up worktree"}
                            className={cn(
                              "inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
                              isRoot
                                ? "cursor-not-allowed opacity-40 text-muted-foreground"
                                : "bg-destructive/15 text-destructive hover:bg-destructive hover:text-destructive-foreground",
                            )}
                          >
                            <Trash2 className="size-3" />
                            <span>Clean up</span>
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Footer */}
        <footer className="flex items-center justify-end border-t border-border px-5 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-border bg-background px-4 py-2 text-xs font-medium text-foreground hover:bg-accent"
          >
            Close
          </button>
        </footer>
      </div>

      {/* Delete Confirmation Dialog */}
      {deletingRow ? (
        <WorktreeDeleteDialog
          workspaceId={workspaceId}
          worktree={deletingRow.worktree}
          initialDirty={deletingRow.isDirty === true}
          dirtyFiles={deletingRow.dirtyFiles}
          onClose={() => setDeletingRow(null)}
          onDeleted={() => handleRowDeleted(deletingRow.worktree.path)}
          services={
            services
              ? {
                  previewDelete: (wt) => services.previewDelete(workspaceId, wt),
                  deleteSafe: (wt) => services.deleteSafe(workspaceId, wt),
                  deleteDestructive: (wt) => services.deleteDestructive(workspaceId, wt),
                }
              : undefined
          }
        />
      ) : null}
    </div>
  );
}
