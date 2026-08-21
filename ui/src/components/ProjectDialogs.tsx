import { FolderGit2, GitBranch, X } from "lucide-react";
import { FormEvent, useEffect, useState } from "react";

import {
  createWorktree,
  listProjectBranches,
  registerProject,
  type LocalBranch,
  type RegisteredProject,
} from "../lib/tauri";
import type { Worktree } from "../lib/types";

const fieldClass =
  "h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors placeholder:text-muted-foreground/45 focus:border-ring";

type AddProjectDialogProps = {
  onClose: () => void;
  onRegistered: (project: RegisteredProject) => void;
};

export function AddProjectDialog({ onClose, onRegistered }: AddProjectDialogProps) {
  const [workspaceId, setWorkspaceId] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedWorkspaceId = workspaceId.trim();
    const trimmedPath = repoPath.trim();
    if (!trimmedWorkspaceId || !trimmedPath || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const project = await registerProject({ workspaceId: trimmedWorkspaceId, repoPath: trimmedPath });
      onRegistered(project);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not register this project.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <form
        aria-label="Add Project"
        className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        onSubmit={submit}
      >
        <div className="flex h-9 items-center border-b border-border px-3">
          <FolderGit2 className="mr-2 size-3.5 text-muted-foreground" />
          <h2 className="text-[13px] font-medium">Add Project</h2>
          <button
            type="button"
            aria-label="Close Add Project"
            className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            onClick={onClose}
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="space-y-3 p-3">
          <label className="block space-y-1 text-[11px] text-muted-foreground">
            <span>Workspace id</span>
            <input
              aria-label="Workspace id"
              className={fieldClass}
              value={workspaceId}
              onChange={(event) => setWorkspaceId(event.target.value)}
              placeholder="orca-lite"
              autoFocus
            />
          </label>
          <label className="block space-y-1 text-[11px] text-muted-foreground">
            <span>Repository path</span>
            <input
              aria-label="Repository path"
              className={fieldClass}
              value={repoPath}
              onChange={(event) => setRepoPath(event.target.value)}
              placeholder="/path/to/repository"
            />
          </label>
          {error ? <p className="text-[11px] text-destructive">{error}</p> : null}
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button type="button" className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            disabled={submitting || !workspaceId.trim() || !repoPath.trim()}
            className="h-7 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
          >
            Add Project
          </button>
        </div>
      </form>
    </div>
  );
}

type AddWorktreeDialogProps = {
  project: RegisteredProject;
  onClose: () => void;
  onCreated: (worktree: Worktree) => void | Promise<void>;
};

export function AddWorktreeDialog({ project, onClose, onCreated }: AddWorktreeDialogProps) {
  const [branches, setBranches] = useState<LocalBranch[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [slug, setSlug] = useState("");
  const [loadingBranches, setLoadingBranches] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setLoadingBranches(true);
    setError(null);
    void listProjectBranches(project.workspaceId)
      .then((items) => {
        if (!alive) return;
        setBranches(items);
        const preferred = items.find((branch) => branch.isCurrent) ?? items[0];
        setBaseRef(preferred?.name ?? "");
      })
      .catch((cause) => {
        if (!alive) return;
        setError(cause instanceof Error ? cause.message : "Could not load local branches.");
      })
      .finally(() => {
        if (alive) setLoadingBranches(false);
      });
    return () => {
      alive = false;
    };
  }, [project.workspaceId]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedSlug = slug.trim();
    if (!trimmedSlug || !baseRef || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const worktree = await createWorktree({
        workspaceId: project.workspaceId,
        worktree: { wsId: project.workspaceId, slug: trimmedSlug },
        baseRef,
      });
      await onCreated(worktree);
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not create the worktree.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <form
        aria-label="Add Worktree"
        className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        onSubmit={submit}
      >
        <div className="flex h-9 items-center border-b border-border px-3">
          <GitBranch className="mr-2 size-3.5 text-muted-foreground" />
          <h2 className="min-w-0 flex-1 truncate text-[13px] font-medium">Add Worktree · {project.workspaceId}</h2>
          <button
            type="button"
            aria-label="Close Add Worktree"
            className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            onClick={onClose}
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="space-y-3 p-3">
          <label className="block space-y-1 text-[11px] text-muted-foreground">
            <span>Worktree slug</span>
            <input
              aria-label="Worktree slug"
              className={fieldClass}
              value={slug}
              onChange={(event) => setSlug(event.target.value)}
              placeholder="feature-ui"
              autoFocus
            />
          </label>
          <label className="block space-y-1 text-[11px] text-muted-foreground">
            <span>Base branch</span>
            <select
              aria-label="Base branch"
              className={fieldClass}
              value={baseRef}
              disabled={loadingBranches || branches.length === 0}
              onChange={(event) => setBaseRef(event.target.value)}
            >
              {branches.map((branch) => (
                <option key={branch.name} value={branch.name}>
                  {branch.name}{branch.isCurrent ? " (current)" : ""}
                </option>
              ))}
            </select>
          </label>
          <p className="truncate text-[10px] text-muted-foreground/70">{project.repoRoot}</p>
          {error ? <p className="text-[11px] text-destructive">{error}</p> : null}
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button type="button" className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            disabled={submitting || loadingBranches || !slug.trim() || !baseRef}
            className="h-7 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
          >
            Create Worktree
          </button>
        </div>
      </form>
    </div>
  );
}
