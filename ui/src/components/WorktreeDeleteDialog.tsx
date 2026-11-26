import { AlertTriangle, GitBranch, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  DEFAULT_WORKSPACE_ID,
  deleteWorktree,
  deleteWorktreeDestructive,
  previewWorktreeDelete,
  toIpcError,
} from "../lib/tauri";
import { createRemoteWorktreeDeleteServices, isPairedWorkspaceId } from "../lib/remoteProject";
import { createPairedWorktreeActions, pairedActionMessage } from "../lib/pairedWorktreeActions";
import { PairedOperationError } from "../lib/pairedDaemonProject";
import {
  worktreeIdentity,
  type BranchDeletionPreview,
  type DirtyFile,
  type RegisteredProject,
  type StructuredIpcError,
  type Worktree,
} from "../lib/types";
import { unmanagedWorktreeDeleteError } from "../lib/unmanagedWorktree";
import { useModalDialog } from "./useModalDialog";

// allow: SIZE_OK — owns one indivisible flow: the single-worktree delete lifecycle
// (preview, refusal, confirmation, mutation errors). It already exceeded the soft ceiling at
// HEAD; splitting it would add files outside this task's allowed scope.

// Destructive deletion is irreversible, so the confirmation has to name what is being
// discarded rather than describing it in the abstract. Long lists are truncated with a
// remainder count so the dialog cannot be pushed off-screen by a large dirty worktree.
const DIRTY_FILES_SHOWN = 8;
type DeletePreview = BranchDeletionPreview | Awaited<ReturnType<ReturnType<typeof createPairedWorktreeActions>["previewDelete"]>>;

/** The safe-delete refusal seeded by a known-dirty row or by the backend itself. */
function dirtyRefusal(): StructuredIpcError {
  return {
    code: "DIRTY_WORKTREE",
    message:
      "Safe deletion refused because the worktree has uncommitted or untracked changes. Destructive deletion will discard all changes permanently.",
    details: {},
  };
}

/** Files the backend named in a refusal, so the loss stays visible when a refresh fails. */
function refusalDirtyFiles(error: StructuredIpcError | null): DirtyFile[] {
  const detail: unknown = error?.details?.files;
  if (!Array.isArray(detail)) return [];
  return detail.flatMap((entry: unknown) =>
    typeof entry === "string" ? [{ statusCode: "", path: entry }] : [],
  );
}

export type WorktreeDeleteServices = {
  previewDelete: (worktree: Worktree) => Promise<BranchDeletionPreview>;
  deleteSafe: (worktree: Worktree) => Promise<void>;
  deleteDestructive: (worktree: Worktree) => Promise<void>;
};

type WorktreeDeleteDialogProps = {
  workspaceId?: string;
  project?: RegisteredProject;
  worktree: Worktree;
  onClose: () => void;
  onDeleted: () => void;
  services?: WorktreeDeleteServices;
  initialDirty?: boolean;
  dirtyFiles?: DirtyFile[];
};

function createDefaultServices(workspaceId: string, paired: boolean): WorktreeDeleteServices {
  // Either ownership signal must prevent Local/SSH fallback without a paired project.
  if (paired) {
    const unsupported = async (): Promise<never> => { throw new PairedOperationError("PAIRED_OWNER_REQUIRED"); };
    return { previewDelete: unsupported, deleteSafe: unsupported, deleteDestructive: unsupported };
  }
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
  project,
  worktree,
  onClose,
  onDeleted,
  services,
  initialDirty = false,
  dirtyFiles: knownDirtyFiles,
}: WorktreeDeleteDialogProps) {
  const paired = project?.target?.kind === "pairedDaemon" || isPairedWorkspaceId(workspaceId) || isPairedWorkspaceId(worktree.workspaceId);
  const isRemoteWorkspace = paired || Boolean(
    workspaceId?.startsWith("ssh:") || worktree.workspaceId?.startsWith("ssh:"),
  );
  const resolvedServices = useMemo(
    () => services ?? (project?.target?.kind === "pairedDaemon"
      ? createPairedWorktreeActions(project)
      : paired
        ? createDefaultServices(worktree.workspaceId ?? workspaceId, true)
        : isRemoteWorkspace
          ? createRemoteWorktreeDeleteServices(worktree.workspaceId ?? workspaceId)
          : createDefaultServices(worktree.workspaceId ?? workspaceId, false)),
    [services, project, paired, isRemoteWorkspace, worktree.workspaceId, workspaceId],
  );
  const actionError = (cause: unknown): StructuredIpcError => cause instanceof PairedOperationError
    ? { code: cause.code, message: pairedActionMessage(cause) } : toIpcError(cause);
  const [preview, setPreview] = useState<DeletePreview | null>(null);
  // Preview failures and mutation failures answer different questions: "what is the state?"
  // versus "why was this refused?". Keeping them apart means a failed refresh can never
  // erase the refusal that told the user why safe deletion was blocked.
  const [previewError, setPreviewError] = useState<StructuredIpcError | null>(null);
  const [deleteError, setDeleteError] = useState<StructuredIpcError | null>(
    initialDirty ? dirtyRefusal() : null,
  );
  const [destructiveRequired, setDestructiveRequired] = useState(initialDirty && !paired);
  const [busy, setBusy] = useState(false);
  // Every preview request takes the next epoch; a response only lands if it still owns it.
  // Without this, a slow preview for the previous worktree could describe the new one.
  const previewEpoch = useRef(0);
  // Paired previews carry branch metadata only; force deletion remains unavailable there.
  const dirtyState = preview && "dirtyState" in preview ? preview.dirtyState : undefined;
  // Falls back to the row's known dirty files and to the refusal's own file list, so an
  // unreachable preview still names exactly what the destructive action would discard.
  const dirtyFiles = dirtyState?.files
    ?? (knownDirtyFiles?.length ? knownDirtyFiles : refusalDirtyFiles(deleteError));
  const hasDirtyLoss = dirtyState?.isDirty || deleteError?.code === "DIRTY_WORKTREE";
  // Escape closes only this dialog (it is rendered inside WorktreeDiskDialog's backdrop)
  // and, like a backdrop click, is ignored while a preview or delete is in flight.
  const { dialogRef } = useModalDialog({ onEscape: onClose, dismissible: !busy });

  useEffect(() => {
    const epoch = ++previewEpoch.current;
    setPreview(null);
    setPreviewError(null);
    if (initialDirty) {
      setDeleteError(dirtyRefusal());
      setDestructiveRequired(!paired);
    } else {
      setDeleteError(null);
      setDestructiveRequired(false);
    }
    setBusy(true);
    void resolvedServices
      .previewDelete(worktree)
      .then((result: DeletePreview) => {
        if (previewEpoch.current !== epoch) return;
        setPreview(result);
        setDestructiveRequired(!paired && "dirtyState" in result && result.dirtyState.isDirty);
        // A fresh preview is authoritative about the worktree's current state, so it may
        // retire a refusal; a *failed* preview never does (see the catch below).
        setDeleteError(null);
        setPreviewError(null);
      })
      .catch((cause) => {
        if (previewEpoch.current !== epoch) return;
        // Unknown state is reported as unknown: no merge row is drawn, and nothing implies
        // the worktree is clean or the branch is merged.
        setPreviewError(actionError(cause));
      })
      .finally(() => {
        if (previewEpoch.current === epoch) setBusy(false);
      });
    return () => {
      previewEpoch.current += 1;
    };
  }, [resolvedServices, worktree, initialDirty, paired]);

  const finishDelete = () => {
    onDeleted();
    onClose();
  };

  const handleSafeDelete = async () => {
    // The mutation owns the preview slot from here: a preview still in flight for the
    // previous state must not overwrite what this refusal reports.
    const epoch = ++previewEpoch.current;
    setBusy(true);
    setDeleteError(null);
    try {
      await resolvedServices.deleteSafe(worktree);
      finishDelete();
    } catch (cause) {
      // Always recorded: this refusal explains the click the user just made, so a stale
      // response must never be able to silence it.
      const ipcError = actionError(cause);
      setDeleteError(ipcError);
      const requiresDestructive = !paired && (ipcError.code === "UNMERGED_BRANCH" || ipcError.code === "DIRTY_WORKTREE");
      setDestructiveRequired(requiresDestructive);
      if (requiresDestructive) {
        // Refresh what the destructive action would discard, keeping the refusal on screen:
        // a failed refresh is a preview problem, never a reason to erase the refusal.
        setPreview(null);
        setPreviewError(null);
        try {
          const refreshed = await resolvedServices.previewDelete(worktree);
          if (previewEpoch.current === epoch) setPreview(refreshed);
        } catch (previewCause) {
          if (previewEpoch.current === epoch) setPreviewError(actionError(previewCause));
        }
      }
    } finally {
      // A preview that started after this attempt owns the busy state now.
      if (previewEpoch.current === epoch) setBusy(false);
    }
  };

  const handleDestructiveDelete = async () => {
    if (busy || !preview) return;
    const epoch = ++previewEpoch.current;
    setBusy(true);
    setDeleteError(null);
    try {
      await resolvedServices.deleteDestructive(worktree);
      finishDelete();
    } catch (cause) {
      setDeleteError(actionError(cause));
    } finally {
      if (previewEpoch.current === epoch) setBusy(false);
    }
  };

  // SSH deletion removes the remote worktree directory only; the branch stays on the host.
  // Every destructive label and note below must say exactly that.
  const sshWorktree = isRemoteWorkspace && !paired;
  // The banner already spells out a refusal; repeating it in the error box is noise.
  const refusalShownInBanner = destructiveRequired
    && (deleteError?.code === "DIRTY_WORKTREE" || deleteError?.code === "UNMERGED_BRANCH");
  const destructiveLabel = hasDirtyLoss
    ? "Delete worktree and discard changes permanently"
    : (sshWorktree ? "Delete remote worktree permanently" : "Delete unmerged branch permanently");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onMouseDown={(event) => {
        // React bubbles through portals and nesting: without this the parent
        // WorktreeDiskDialog backdrop would also receive the outside click.
        event.stopPropagation();
        if (!busy) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-label="Delete worktree"
        aria-modal="true"
        className="w-full max-w-lg animate-enter overflow-hidden rounded-xl border border-border bg-popover shadow-elevation-4"
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

        <div className="selectable space-y-4 p-4 text-xs">
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
              <PreviewRow
                label="Merge state"
                value={preview.merged ? "Merged" : "Not merged"}
                testId="worktree-delete-merge"
                dataMerged={preview.merged}
              />
              <PreviewRow
                label="Divergence"
                value={preview.upstream === null ? "No upstream" : `${preview.ahead ?? "—"} ahead · ${preview.behind ?? "—"} behind`}
                dataState={preview.upstream === null ? "no-upstream" : "upstream"}
              />
            </div>
          ) : null}

          {previewError && !preview ? (
            <div
              data-testid="worktree-delete-preview-error"
              data-error-kind={isRemoteWorkspace ? "remote-git-status" : "git-status"}
              className="rounded-md border border-status-warning-border bg-status-warning-container px-3 py-2 text-status-warning-on"
            >
              <div className="flex items-start gap-2">
                <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
                <div className="min-w-0">
                  <div className="font-semibold">
                    {isRemoteWorkspace ? "Could not inspect remote Git status" : "Could not inspect Git status"}
                  </div>
                  <div className="mt-0.5 text-[11px] leading-relaxed">
                    {isRemoteWorkspace
                      ? "The remote host did not report the worktree's uncommitted changes, upstream, or merge state. Nothing about its cleanliness is known from here."
                      : "The worktree's uncommitted changes, upstream, and merge state are unknown."}
                  </div>
                  <div className="mt-1 font-mono text-[10px] opacity-80">
                    <span className="font-semibold">{previewError.code}</span>
                    <span> {previewError.message}</span>
                  </div>
                </div>
              </div>
            </div>
          ) : null}

          {destructiveRequired ? (
            <div
              data-testid="worktree-delete-refusal"
              data-dirty={hasDirtyLoss ? "true" : "false"}
              className="space-y-2 rounded-lg border border-destructive/35 bg-destructive/10 p-3"
            >
              <div className="flex items-start gap-2 text-destructive">
                <AlertTriangle className="mt-0.5 size-4 shrink-0" />
                <div>
                  <div className="font-semibold">
                    {hasDirtyLoss ? "Uncommitted changes" : "Unmerged branch"}
                  </div>
                  <p className="mt-1 text-[11px] leading-relaxed text-destructive/85">
                    {hasDirtyLoss
                      ? "Safe deletion refused because the worktree has uncommitted or untracked changes. Destructive deletion will discard all changes permanently."
                      : sshWorktree
                        ? "Safe deletion refused to discard unmerged commits. This removes the remote worktree directory only; the Git branch and its commits stay on the host."
                        : "Safe deletion refused to discard unmerged commits. Destructive deletion is a separate explicit action."}
                  </p>
                  {dirtyFiles.length > 0 ? (
                    <div className="mt-2" data-testid="dirty-file-preview">
                      <div className="text-[11px] font-semibold text-destructive">
                        {dirtyFiles.length} file{dirtyFiles.length === 1 ? "" : "s"} will be discarded
                      </div>
                      <ul className="mt-1 space-y-0.5 font-mono text-[10px] leading-relaxed text-destructive/80">
                        {dirtyFiles.slice(0, DIRTY_FILES_SHOWN).map((file) => (
                          <li key={file.path} className="flex gap-1.5">
                            <span className="shrink-0 opacity-70">{file.statusCode.trim() || "?"}</span>
                            <span className="truncate">{file.path}</span>
                          </li>
                        ))}
                      </ul>
                      {dirtyFiles.length > DIRTY_FILES_SHOWN ? (
                        <div className="mt-1 text-[10px] text-destructive/70">
                          and {dirtyFiles.length - DIRTY_FILES_SHOWN} more
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              </div>
              <button
                type="button"
                disabled={busy || !preview}
                onClick={() => void handleDestructiveDelete()}
                className="w-full rounded-md bg-destructive px-3 py-2 font-semibold text-destructive-foreground disabled:opacity-50"
              >
                {destructiveLabel}
              </button>
              {sshWorktree ? (
                <p className="text-[10px] leading-relaxed text-destructive/70">
                  This removes the worktree directory on the SSH host. The Git branch is kept.
                </p>
              ) : null}
            </div>
          ) : null}

          {deleteError && !refusalShownInBanner ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-destructive">
              <div className="font-semibold">{deleteError.code}</div>
              <div className="mt-0.5 text-[11px]">{deleteError.message}</div>
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
                {isRemoteWorkspace ? "Delete remote worktree" : "Delete worktree and branch"}
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function PreviewRow({ label, value, mono = false, icon, dataState, testId, dataMerged }: {
  label: string;
  value: string;
  mono?: boolean;
  icon?: React.ReactNode;
  dataState?: string;
  testId?: string;
  dataMerged?: boolean;
}) {
  return (
    <div
      data-testid={testId ?? (dataState ? "worktree-delete-divergence" : undefined)}
      data-state={dataState}
      data-merged={dataMerged === undefined ? undefined : dataMerged ? "true" : "false"}
      className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-b-0"
    >
      <span className="flex w-24 shrink-0 items-center gap-1 text-muted-foreground">{icon}{label}</span>
      <span className={`min-w-0 flex-1 truncate text-right text-foreground ${mono ? "font-mono" : ""}`}>{value}</span>
    </div>
  );
}

function requireIdentity(worktree: Worktree) {
  const identity = worktreeIdentity(worktree);
  if (identity) return identity;
  throw unmanagedWorktreeDeleteError(worktree.path);
}
