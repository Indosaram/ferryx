import { open } from "@tauri-apps/plugin-dialog";
import { FolderGit2, FolderPlus, GitBranch, LoaderCircle, X } from "lucide-react";
import { FormEvent, useEffect, useRef, useState } from "react";

import {
  createWorktree,
  isTauriRuntime,
  listProjectBranches,
  registerProject,
  type LocalBranch,
  type RegisteredProject,
} from "../lib/tauri";
import type { Worktree } from "../lib/types";

const fieldClass =
  "h-8 w-full rounded-md border border-input bg-background px-2 text-xs text-foreground outline-none transition-colors placeholder:text-muted-foreground/45 focus:border-ring";

function checkIsTauri(): boolean {
  try {
    return Boolean(isTauriRuntime());
  } catch {
    return false;
  }
}

/// IPC failures arrive as structured `{ code, message }` objects (not `Error`
/// instances), so extract the reason from both shapes before falling back.
function extractErrorMessage(cause: unknown, fallback: string): string {
  if (cause instanceof Error && cause.message) return cause.message;
  if (typeof cause === "string" && cause) return cause;
  if (typeof cause === "object" && cause !== null) {
    const message = (cause as { message?: unknown }).message;
    if (typeof message === "string" && message) return message;
  }
  return fallback;
}

export function deriveWorkspaceId(folderPath: string, existingProjects: RegisteredProject[] = []): string {
  const normalized = folderPath.replace(/[/\\]+$/, "");
  const basename = normalized.split(/[/\\]/).pop() || "";
  let baseId = basename.replace(/[\s/\\\x00-\x1f\x7f]+/g, "-").replace(/^-+/, "");
  if (!baseId) {
    baseId = "project";
  }
  const existingIds = new Set(existingProjects.map((p) => p.workspaceId));
  if (!existingIds.has(baseId)) {
    return baseId;
  }
  let index = 2;
  while (existingIds.has(`${baseId}-${index}`)) {
    index++;
  }
  return `${baseId}-${index}`;
}

type AddProjectDialogProps = {
  projects?: RegisteredProject[];
  onClose: () => void;
  onRegistered: (project: RegisteredProject) => void;
};

export function AddProjectDialog({ projects = [], onClose, onRegistered }: AddProjectDialogProps) {
  const [workspaceId, setWorkspaceId] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const onRegisteredRef = useRef(onRegistered);
  onRegisteredRef.current = onRegistered;
  const projectsRef = useRef(projects);
  projectsRef.current = projects;

  const pickerOpenedRef = useRef(false);
  const isTauri = checkIsTauri();

  useEffect(() => {
    // `pickerOpenedRef` alone guards against duplicate invocations (parent
    // re-renders and StrictMode's double-mounted effects). The selection must
    // NOT be discarded when StrictMode runs the effect cleanup, or a picked
    // folder would silently vanish — hence no "alive" cancellation flag.
    if (!isTauri || pickerOpenedRef.current) return;
    pickerOpenedRef.current = true;
    void open({
      directory: true,
      multiple: false,
      title: "Add Project",
    })
      .then((selected) => {
        if (typeof selected === "string" && selected.length > 0) {
          setSelectedPath(selected);
        } else {
          onCloseRef.current();
        }
      })
      .catch((cause) => {
        console.error(cause);
        const message = cause instanceof Error ? cause.message : String(cause);
        setError(message || "Could not open folder picker.");
      });
  }, [isTauri]);

  if (isTauri && selectedPath) {
    const handleConfirm = async () => {
      if (submitting || !selectedPath) return;
      setSubmitting(true);
      setError(null);
      const derivedId = deriveWorkspaceId(selectedPath, projectsRef.current);
      try {
        const project = await registerProject({ workspaceId: derivedId, repoPath: selectedPath });
        onRegisteredRef.current(project);
        onCloseRef.current();
      } catch (cause) {
        setError(extractErrorMessage(cause, "Could not register this project."));
      } finally {
        setSubmitting(false);
      }
    };

    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
        <div
          role="dialog"
          aria-label="Add Project"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        >
          <div className="flex h-9 items-center border-b border-border px-3">
            <FolderGit2 className="mr-2 size-3.5 text-muted-foreground" />
            <h2 className="text-[13px] font-medium">Add Project</h2>
            <button
              type="button"
              aria-label="Close Add Project"
              disabled={submitting}
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
              onClick={() => onCloseRef.current()}
            >
              <X className="size-3.5" />
            </button>
          </div>
          <div className="space-y-3 p-3">
            <p className="text-xs text-muted-foreground">
              Add this folder as a separate Ferryx project.
            </p>
            <div className="rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs">
              <div className="break-all font-mono text-muted-foreground">{selectedPath}</div>
            </div>
            {error ? <p className="text-xs text-destructive">{error}</p> : null}
          </div>
          <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              disabled={submitting}
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
              onClick={() => onCloseRef.current()}
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={submitting || !selectedPath}
              onClick={handleConfirm}
              className="inline-flex h-7 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
            >
              {submitting ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : (
                <FolderPlus className="size-3.5" />
              )}
              Add Project
            </button>
          </div>
        </div>
      </div>
    );
  }

  // The native folder picker is a sheet on the main window, and it resolves
  // asynchronously. Rendering nothing while it is pending broke two things:
  // the `[role="dialog"]` surface that makes the native terminal compositor
  // yield (`lib/nativeTerminalVisibility.tsx`) never existed, and a picker that
  // never resolves left `isAddProjectOpen` latched true with no visible dialog,
  // so every later "Add project" click was a silent no-op. Mount a real dialog
  // surface for the pending phase instead.
  if (isTauri && !error) {
    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
        <div
          role="dialog"
          aria-label="Add Project"
          aria-busy="true"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        >
          <div className="flex h-9 items-center border-b border-border px-3">
            <FolderGit2 className="mr-2 size-3.5 text-muted-foreground" />
            <h2 className="text-[13px] font-medium">Add Project</h2>
            <button
              type="button"
              aria-label="Close Add Project"
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              onClick={() => onCloseRef.current()}
            >
              <X className="size-3.5" />
            </button>
          </div>
          <div className="flex items-center gap-2 p-3 text-xs text-muted-foreground">
            <LoaderCircle className="size-3.5 animate-spin" />
            <span>Waiting for the folder picker.</span>
          </div>
          <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent"
              onClick={() => onCloseRef.current()}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    );
  }

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedWorkspaceId = workspaceId.trim();
    const trimmedPath = repoPath.trim();
    if (!trimmedWorkspaceId || !trimmedPath || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const project = await registerProject({ workspaceId: trimmedWorkspaceId, repoPath: trimmedPath });
      onRegisteredRef.current(project);
      onCloseRef.current();
    } catch (cause) {
      setError(extractErrorMessage(cause, "Could not register this project."));
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
            onClick={() => onCloseRef.current()}
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
              placeholder="my-project"
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
          <button type="button" className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent" onClick={() => onCloseRef.current()}>
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
  const isGitBacked = project.gitRoot !== null;
  const [branches, setBranches] = useState<LocalBranch[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [slug, setSlug] = useState("");
  const [loadingBranches, setLoadingBranches] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isGitBacked) {
      setBranches([]);
      setBaseRef("");
      setLoadingBranches(false);
      return;
    }
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
        setError(extractErrorMessage(cause, "Could not load local branches."));
      })
      .finally(() => {
        if (alive) setLoadingBranches(false);
      });
    return () => {
      alive = false;
    };
  }, [project.workspaceId, isGitBacked]);

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
      setError(extractErrorMessage(cause, "Could not create the worktree."));
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
          {isGitBacked ? (
            <>
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
            </>
          ) : (
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              This project is not a Git repository, so it has no branches to base a worktree on. Open a terminal on the
              folder instead, or add a Git repository as a separate project.
            </p>
          )}
          <p className="truncate text-[10px] text-muted-foreground/70">{project.repoRoot}</p>
          {error ? <p className="text-[11px] text-destructive">{error}</p> : null}
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button type="button" className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent" onClick={onClose}>
            {isGitBacked ? "Cancel" : "Close"}
          </button>
          {isGitBacked ? (
            <button
              type="submit"
              disabled={submitting || loadingBranches || !slug.trim() || !baseRef}
              className="h-7 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
            >
              Create Worktree
            </button>
          ) : null}
        </div>
      </form>
    </div>
  );
}
