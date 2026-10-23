import { FolderGit2, LoaderCircle, X } from "lucide-react";
import { useRef, useState } from "react";

import { rememberPromptedExternalWorktrees } from "../lib/externalWorktrees";
import { registerProject, type ExternalWorktree, type RegisteredProject } from "../lib/tauri";
import { deriveWorkspaceId } from "./ProjectDialogs";
import { useModalDialog } from "./useModalDialog";

export type ExternalWorktreeImportDialogProps = {
  worktrees: ExternalWorktree[];
  projects: RegisteredProject[];
  /** Called once with every worktree that registered successfully. */
  onImported: (projects: RegisteredProject[]) => void;
  /** Closes without remembering the answer; the same worktrees may be offered again next launch. */
  onLater: () => void;
  onClose: () => void;
};

function errorMessage(cause: unknown): string {
  if (cause instanceof Error && cause.message) return cause.message;
  if (typeof cause === "object" && cause !== null) {
    const message = (cause as { message?: unknown }).message;
    if (typeof message === "string" && message) return message;
  }
  return "Could not import this worktree.";
}

function branchLabel(worktree: ExternalWorktree): string {
  if (worktree.branch) return worktree.branch.replace(/^refs\/heads\//, "");
  return worktree.head ? `detached ${worktree.head.slice(0, 7)}` : "detached";
}

/**
 * Asks whether to add worktrees that Git knows about but that live outside the project folder,
 * such as ones Orca created. Nothing is imported without an explicit choice.
 */
export function ExternalWorktreeImportDialog({ worktrees, projects, onImported, onLater, onClose }: ExternalWorktreeImportDialogProps) {
  const [selected, setSelected] = useState(() => new Set(worktrees.map((worktree) => worktree.path)));
  const [busy, setBusy] = useState(false);
  const [failures, setFailures] = useState<Record<string, string>>({});
  const busyRef = useRef(false);
  const { dialogRef } = useModalDialog({ onEscape: onLater, dismissible: !busy });
  const fromOrca = worktrees.some((worktree) => worktree.createdByOrca);
  const title = fromOrca ? "Orca worktrees found" : "Worktrees found outside this project";

  const toggle = (path: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  };

  const handleDecline = () => {
    rememberPromptedExternalWorktrees(worktrees.map((worktree) => worktree.path));
    onClose();
  };

  const handleImport = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    const imported: RegisteredProject[] = [];
    const failed: Record<string, string> = {};
    const known = [...projects];
    for (const worktree of worktrees) {
      if (!selected.has(worktree.path)) continue;
      try {
        const project = await registerProject({
          workspaceId: deriveWorkspaceId(worktree.path, known),
          repoPath: worktree.path,
        });
        imported.push(project);
        known.push(project);
      } catch (cause) {
        failed[worktree.path] = errorMessage(cause);
      }
    }
    // Failed imports stay unanswered so they are offered again; everything else is settled.
    rememberPromptedExternalWorktrees(worktrees.map((worktree) => worktree.path).filter((path) => !(path in failed)));
    if (imported.length > 0) onImported(imported);
    busyRef.current = false;
    setBusy(false);
    if (Object.keys(failed).length > 0) {
      setFailures(failed);
      setSelected(new Set(Object.keys(failed)));
      return;
    }
    onClose();
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-[480px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
      >
        <div className="flex h-9 items-center border-b border-border px-3">
          <FolderGit2 className="mr-2 size-3.5 text-muted-foreground" />
          <h2 className="text-[13px] font-medium">{title}</h2>
          <button
            type="button"
            aria-label="Close"
            disabled={busy}
            className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
            onClick={onLater}
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="selectable space-y-2 p-3 text-xs">
          <p className="text-foreground">
            Git lists {worktrees.length === 1 ? "a worktree" : `${worktrees.length} worktrees`} of this repository outside
            the project folder, so {worktrees.length === 1 ? "it is" : "they are"} not in your worktree list. Import the
            selected ones as projects?
          </p>
          <ul className="max-h-60 space-y-1 overflow-y-auto">
            {worktrees.map((worktree) => (
              <li key={worktree.path} className="rounded-md border border-border px-2 py-1.5">
                <label className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    className="mt-0.5"
                    checked={selected.has(worktree.path)}
                    disabled={busy}
                    onChange={() => toggle(worktree.path)}
                  />
                  <span className="min-w-0">
                    <span className="block font-medium text-foreground">
                      {branchLabel(worktree)}
                      {worktree.createdByOrca ? <span className="ml-1.5 text-[10px] text-muted-foreground">Orca</span> : null}
                    </span>
                    <span className="block break-all font-mono text-[11px] text-muted-foreground">{worktree.path}</span>
                    {failures[worktree.path] ? (
                      <span role="alert" className="block text-[11px] text-destructive">{failures[worktree.path]}</span>
                    ) : null}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button
            type="button"
            disabled={busy}
            className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-50"
            onClick={handleDecline}
          >
            Don't import
          </button>
          <button
            type="button"
            disabled={busy || selected.size === 0}
            className="inline-flex h-7 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
            onClick={() => void handleImport()}
          >
            {busy ? <LoaderCircle className="size-3 animate-spin" /> : null}
            Import selected
          </button>
        </div>
      </div>
    </div>
  );
}
