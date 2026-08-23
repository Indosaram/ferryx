import { AlertTriangle, GitBranch, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  DEFAULT_WORKSPACE_ID,
  deleteWorktree,
  deleteWorktreeDestructive,
  previewWorktreeDelete,
  toIpcError,
} from "../lib/tauri";
import { worktreeIdentity, type BranchDeletionPreview, type StructuredIpcError, type Worktree } from "../lib/types";

export type WorktreeDeleteServices = {
  previewDelete: (worktree: Worktree) => Promise<BranchDeletionPreview>;
  deleteSafe: (worktree: Worktree) => Promise<void>;
  deleteDestructive: (worktree: Worktree) => Promise<void>;
};

type WorktreeDeleteDialogProps = {
  workspaceId?: string;
  worktree: Worktree;
  onClose: () => void;
  onDeleted: () => void;
  services?: WorktreeDeleteServices;
};

function createDefaultServices(workspaceId: string): WorktreeDeleteServices {
  return {
    previewDelete: async (worktree) => {
      const identity = requireIdentity(worktree);
      return previewWorktreeDelete({ workspaceId, worktree: identity });
    },
    deleteSafe: async (worktree) => {
      const identity = requireIdentity(worktree);
      await deleteWorktree({ workspaceId, worktree: identity, deleteBranch: true });
    },
    deleteDestructive: async (worktree) => {
      const identity = requireIdentity(worktree);
      await deleteWorktreeDestructive({ workspaceId, worktree: identity, deleteBranch: true });
    },
  };
}

export function WorktreeDeleteDialog({
  workspaceId = DEFAULT_WORKSPACE_ID,
  worktree,
  onClose,
  onDeleted,
  services,
}: WorktreeDeleteDialogProps) {
  const resolvedServices = useMemo(() => services ?? createDefaultServices(workspaceId), [services, workspaceId]);
  const [preview, setPreview] = useState<BranchDeletionPreview | null>(null);
  const [error, setError] = useState<StructuredIpcError | null>(null);
  const [destructiveRequired, setDestructiveRequired] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setPreview(null);
    setError(null);
    setDestructiveRequired(false);
    setBusy(true);
    void resolvedServices
      .previewDelete(worktree)
      .then((result) => {
        if (!cancelled) setPreview(result);
      })
      .catch((cause) => {
        if (!cancelled) setError(toIpcError(cause));
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [resolvedServices, worktree]);

  const finishDelete = () => {
    onDeleted();
    onClose();
  };

  const handleSafeDelete = async () => {
    setBusy(true);
    setError(null);
    try {
      await resolvedServices.deleteSafe(worktree);
      finishDelete();
    } catch (cause) {
      const ipcError = toIpcError(cause);
      setError(ipcError);
      setDestructiveRequired(ipcError.code === "UNMERGED_BRANCH");
    } finally {
      setBusy(false);
    }
  };

  const handleDestructiveDelete = async () => {
    setBusy(true);
    setError(null);
    try {
      await resolvedServices.deleteDestructive(worktree);
      finishDelete();
    } catch (cause) {
      setError(toIpcError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm" onMouseDown={onClose}>
      <div
        role="dialog"
        aria-label="Delete worktree"
        aria-modal="true"
        className="w-full max-w-lg animate-enter overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
            <Trash2 className="size-4 text-destructive" />
            Delete worktree
          </div>
          <button type="button" onClick={onClose} aria-label="Close delete dialog" className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="size-4" />
          </button>
        </header>

        <div className="space-y-4 p-4 text-xs">
          <p className="text-muted-foreground">Review the branch state before deleting this worktree.</p>
          <div data-testid="worktree-delete-path" className="break-all rounded-md border border-border bg-muted/40 px-3 py-2 font-mono text-foreground">
            {worktree.path}
          </div>

          {busy && !preview ? <div className="rounded-md border border-border bg-background/50 px-3 py-4 text-muted-foreground">Loading deletion preview…</div> : null}

          {preview ? (
            <div className="overflow-hidden rounded-lg border border-border bg-background/45">
              <PreviewRow label="Branch" value={preview.branch} icon={<GitBranch className="size-3" />} />
              <PreviewRow label="HEAD" value={preview.head} mono />
              <PreviewRow label="Upstream" value={preview.upstream ?? "None"} />
              <PreviewRow label="Merge state" value={preview.merged ? "Merged" : "Not merged"} />
              <PreviewRow
                label="Divergence"
                value={preview.upstream === null ? "No upstream" : `${preview.ahead ?? "—"} ahead · ${preview.behind ?? "—"} behind`}
                dataState={preview.upstream === null ? "no-upstream" : "upstream"}
              />
            </div>
          ) : null}

          {destructiveRequired ? (
            <div className="space-y-2 rounded-lg border border-destructive/35 bg-destructive/10 p-3">
              <div className="flex items-start gap-2 text-destructive">
                <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                <div>
                  <div className="font-semibold">Unmerged branch</div>
                  <p className="mt-1 text-[11px] leading-relaxed text-destructive/85">
                    Safe deletion refused to discard unmerged commits. Destructive deletion is a separate explicit action.
                  </p>
                </div>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() => void handleDestructiveDelete()}
                className="w-full rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground disabled:opacity-50"
              >
                Delete unmerged branch permanently
              </button>
            </div>
          ) : null}

          {error && !destructiveRequired ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-destructive">
              <div className="font-semibold">{error.code}</div>
              <div className="mt-0.5 text-[11px]">{error.message}</div>
            </div>
          ) : null}

          <div className="flex justify-end gap-2">
            <button type="button" onClick={onClose} className="rounded-md px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
              Cancel
            </button>
            {!destructiveRequired ? (
              <button
                type="button"
                disabled={busy || !preview}
                onClick={() => void handleSafeDelete()}
                className="rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground disabled:opacity-50"
              >
                Delete worktree and branch
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function PreviewRow({ label, value, mono = false, icon, dataState }: { label: string; value: string; mono?: boolean; icon?: React.ReactNode; dataState?: string }) {
  return (
    <div data-testid={dataState ? "worktree-delete-divergence" : undefined} data-state={dataState} className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-b-0">
      <span className="flex w-24 shrink-0 items-center gap-1 text-muted-foreground">{icon}{label}</span>
      <span className={`min-w-0 flex-1 truncate text-right text-foreground ${mono ? "font-mono" : ""}`}>{value}</span>
    </div>
  );
}

function requireIdentity(worktree: Worktree) {
  const identity = worktreeIdentity(worktree);
  if (identity) return identity;
  throw {
    code: "INVALID_NAMESPACE",
    message: "Only Ferryx-managed worktrees can be deleted from this UI.",
    details: { path: worktree.path },
  } satisfies StructuredIpcError;
}
