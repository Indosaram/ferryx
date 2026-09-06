import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  FolderGit2,
  FolderPlus,
  GitBranch,
  LoaderCircle,
  Radio,
  Settings,
  Trash2,
  X,
} from "lucide-react";
import { FormEvent, useEffect, useRef, useState } from "react";

import { registerRemoteProject, toRegisteredProject } from "../lib/remoteProject";
import { formatSshTarget, useSshHosts } from "../lib/sshHosts";
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

export type AddProjectDialogProps = {
  projects?: RegisteredProject[];
  onClose: () => void;
  onRegistered: (project: RegisteredProject) => void;
  onOpenSettings?: (section: "ssh") => void;
};

type AddProjectStep =
  | "choose-location"
  | "local-pending"
  | "local-confirm"
  | "local-manual"
  | "remote-form";

export function AddProjectDialog({
  projects = [],
  onClose,
  onRegistered,
  onOpenSettings,
}: AddProjectDialogProps) {
  const [step, setStep] = useState<AddProjectStep>("choose-location");
  const [workspaceId, setWorkspaceId] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  // Remote flow state
  const { hosts, loading: hostsLoading, error: hostsLoadError, refresh: refreshHosts } = useSshHosts();
  const enabledHosts = hosts.filter((h) => !h.disabled);
  const [selectedHostId, setSelectedHostId] = useState<string>("");
  const [remoteRepoPath, setRemoteRepoPath] = useState("");
  const [remoteWorkspaceId, setRemoteWorkspaceId] = useState("");
  const [remoteIdEdited, setRemoteIdEdited] = useState(false);
  const [remoteError, setRemoteError] = useState<string | null>(null);

  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const onRegisteredRef = useRef(onRegistered);
  onRegisteredRef.current = onRegistered;
  const projectsRef = useRef(projects);
  projectsRef.current = projects;
  const onOpenSettingsRef = useRef(onOpenSettings);
  onOpenSettingsRef.current = onOpenSettings;

  const pickerOpenedRef = useRef(false);
  const dismissedRef = useRef(false);
  const isMountedRef = useRef(false);
  const hasHadSelectionRef = useRef(false);
  const isTauri = checkIsTauri();

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // Sync selectedHostId with available enabled hosts
  useEffect(() => {
    if (enabledHosts.length === 0) {
      setSelectedHostId("");
      return;
    }
    setSelectedHostId((prev) => {
      if (prev) {
        if (enabledHosts.some((h) => h.id === prev)) {
          return prev;
        }
        // Previously selected host was deleted or disabled: clear selection and require explicit choice.
        return "";
      }
      // Initial auto-selection
      if (!hasHadSelectionRef.current) {
        hasHadSelectionRef.current = true;
        return enabledHosts[0].id;
      }
      return "";
    });
  }, [enabledHosts]);

  const handleDismiss = () => {
    dismissedRef.current = true;
    onCloseRef.current();
  };

  const handleChooseLocal = () => {
    if (isTauri) {
      setStep("local-pending");
      if (pickerOpenedRef.current) return;
      pickerOpenedRef.current = true;
      void open({
        directory: true,
        multiple: false,
        title: "Add Project",
      })
        .then((selected) => {
          if (dismissedRef.current || !isMountedRef.current) return;
          if (typeof selected === "string" && selected.length > 0) {
            setSelectedPath(selected);
            setStep("local-confirm");
          } else {
            handleDismiss();
          }
        })
        .catch((cause) => {
          if (dismissedRef.current || !isMountedRef.current) return;
          console.error(cause);
          const message = cause instanceof Error ? cause.message : String(cause);
          setLocalError(message || "Could not open folder picker.");
          setStep("local-manual");
        })
        .finally(() => {
          pickerOpenedRef.current = false;
        });
    } else {
      setStep("local-manual");
    }
  };

  const handleChooseRemote = () => {
    setRemoteError(null);
    setStep("remote-form");
  };

  const handleLocalConfirm = async () => {
    if (submitting || !selectedPath) return;
    setSubmitting(true);
    setLocalError(null);
    const derivedId = deriveWorkspaceId(selectedPath, projectsRef.current);
    try {
      const project = await registerProject({ workspaceId: derivedId, repoPath: selectedPath });
      if (dismissedRef.current || !isMountedRef.current) return;
      onRegisteredRef.current(project);
      handleDismiss();
    } catch (cause) {
      if (dismissedRef.current || !isMountedRef.current) return;
      setLocalError(extractErrorMessage(cause, "Could not register this project."));
    } finally {
      if (!dismissedRef.current && isMountedRef.current) {
        setSubmitting(false);
      }
    }
  };

  const handleManualSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedWorkspaceId = workspaceId.trim();
    const trimmedPath = repoPath.trim();
    if (!trimmedWorkspaceId || !trimmedPath || submitting) return;
    setSubmitting(true);
    setLocalError(null);
    try {
      const project = await registerProject({ workspaceId: trimmedWorkspaceId, repoPath: trimmedPath });
      if (dismissedRef.current || !isMountedRef.current) return;
      onRegisteredRef.current(project);
      handleDismiss();
    } catch (cause) {
      if (dismissedRef.current || !isMountedRef.current) return;
      setLocalError(extractErrorMessage(cause, "Could not register this project."));
    } finally {
      if (!dismissedRef.current && isMountedRef.current) {
        setSubmitting(false);
      }
    }
  };

  const handleRemoteSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedWorkspaceId = remoteWorkspaceId.trim();
    const trimmedPath = remoteRepoPath.trim();
    const hostId = selectedHostId;
    if (!trimmedWorkspaceId || !trimmedPath || !hostId || submitting) return;

    setSubmitting(true);
    setRemoteError(null);

    // Refresh authoritative list to prevent submitting stale/disabled/removed host
    let authoritativeHosts: typeof hosts;
    try {
      authoritativeHosts = await refreshHosts();
    } catch (cause) {
      if (dismissedRef.current || !isMountedRef.current) return;
      setRemoteError(extractErrorMessage(cause, "Failed to refresh SSH machines."));
      setSubmitting(false);
      return;
    }

    if (dismissedRef.current || !isMountedRef.current) return;

    const host = authoritativeHosts.find((h) => h.id === hostId);
    if (!host || host.disabled) {
      setRemoteError("The selected SSH machine is no longer available or has been disabled.");
      setSubmitting(false);
      return;
    }

    try {
      const response = await registerRemoteProject({
        workspaceId: trimmedWorkspaceId,
        hostId,
        repoPath: trimmedPath,
      });

      if (dismissedRef.current || !isMountedRef.current) return;

      const project = toRegisteredProject(response);

      onRegisteredRef.current(project);
      handleDismiss();
    } catch (cause) {
      if (dismissedRef.current || !isMountedRef.current) return;
      setRemoteError(extractErrorMessage(cause, "Could not register this remote project."));
    } finally {
      if (!dismissedRef.current && isMountedRef.current) {
        setSubmitting(false);
      }
    }
  };

  // 1. Initial Location Chooser
  if (step === "choose-location") {
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
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              onClick={handleDismiss}
            >
              <X className="size-3.5" />
            </button>
          </div>
          <div className="space-y-3 p-3">
            <p className="text-xs text-muted-foreground">
              Select where the project repository is located.
            </p>
            <div className="space-y-2">
              <button
                type="button"
                data-testid="project-type-local"
                onClick={handleChooseLocal}
                className="group flex w-full items-start gap-3 rounded-md border border-border/80 bg-background p-3 text-left transition-colors hover:bg-accent/40 focus-visible:border-ring focus:outline-none focus:ring-1 focus:ring-ring"
              >
                <FolderGit2 className="mt-0.5 size-4 text-primary shrink-0 transition-transform group-hover:scale-105" />
                <div>
                  <div className="text-xs font-medium text-foreground">Local Project</div>
                  <div className="mt-0.5 text-[11px] leading-normal text-muted-foreground">
                    Choose a folder on this machine using the folder picker.
                  </div>
                </div>
              </button>

              <button
                type="button"
                data-testid="project-type-remote"
                onClick={handleChooseRemote}
                className="group flex w-full items-start gap-3 rounded-md border border-border/80 bg-background p-3 text-left transition-colors hover:bg-accent/40 focus-visible:border-ring focus:outline-none focus:ring-1 focus:ring-ring"
              >
                <Radio className="mt-0.5 size-4 text-primary shrink-0 transition-transform group-hover:scale-105" />
                <div>
                  <div className="text-xs font-medium text-foreground">Remote (SSH)</div>
                  <div className="mt-0.5 text-[11px] leading-normal text-muted-foreground">
                    Connect to a repository hosted on an SSH machine.
                  </div>
                </div>
              </button>
            </div>
          </div>
          <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              data-testid="add-project-cancel"
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent"
              onClick={handleDismiss}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    );
  }

  // 2. Pending native folder picker
  if (step === "local-pending") {
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
              onClick={handleDismiss}
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
              onClick={handleDismiss}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    );
  }

  // 3. Confirm picked local folder
  if (step === "local-confirm" && selectedPath) {
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
              onClick={handleDismiss}
            >
              <X className="size-3.5" />
            </button>
          </div>
          <div className="selectable space-y-3 p-3">
            <p className="text-xs text-muted-foreground">
              Add this folder as a separate Ferryx project.
            </p>
            <div className="rounded-md border border-border/70 bg-muted/35 px-3 py-2 text-xs">
              <div className="break-all font-mono text-muted-foreground">{selectedPath}</div>
            </div>
            {localError ? <p className="text-xs text-destructive">{localError}</p> : null}
          </div>
          <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              disabled={submitting}
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
              onClick={handleDismiss}
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={submitting || !selectedPath}
              onClick={handleLocalConfirm}
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

  // 4. Remote SSH project form
  if (step === "remote-form") {
    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
        <form
          role="dialog"
          aria-label="Add Project"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
          onSubmit={handleRemoteSubmit}
        >
          <div className="flex h-9 items-center border-b border-border px-3">
            <button
              type="button"
              aria-label="Back"
              data-testid="add-project-back"
              disabled={submitting}
              className="mr-1.5 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
              onClick={() => {
                setRemoteError(null);
                setStep("choose-location");
              }}
            >
              <ArrowLeft className="size-3.5" />
            </button>
            <Radio className="mr-2 size-3.5 text-muted-foreground" />
            <h2 className="text-[13px] font-medium">Add Remote Project</h2>
            <button
              type="button"
              aria-label="Close Add Project"
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
              onClick={handleDismiss}
            >
              <X className="size-3.5" />
            </button>
          </div>

          <div className="space-y-3 p-3">
            {hostsLoading ? (
              <div className="flex items-center gap-2 py-4 text-xs text-muted-foreground">
                <LoaderCircle className="size-3.5 animate-spin" />
                <span>Loading SSH machines...</span>
              </div>
            ) : enabledHosts.length === 0 ? (
              <div className="space-y-3 py-1">
                <p className="text-xs text-muted-foreground">
                  No active SSH machines found. Configure SSH machines in Settings before adding a remote project.
                </p>
                {hostsLoadError ? (
                  <p className="text-[11px] text-destructive">{hostsLoadError}</p>
                ) : null}
                <div>
                  <button
                    type="button"
                    data-testid="configure-ssh-settings"
                    className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-background px-2.5 text-xs text-foreground hover:bg-accent"
                    onClick={() => {
                      onOpenSettingsRef.current?.("ssh");
                      handleDismiss();
                    }}
                  >
                    <Settings className="size-3.5 text-muted-foreground" />
                    Configure SSH Machines in Settings
                  </button>
                </div>
              </div>
            ) : (
              <>
                <label className="block space-y-1 text-[11px] text-muted-foreground" htmlFor="remote-host-select">
                  <span>SSH Machine</span>
                  <select
                    id="remote-host-select"
                    aria-label="SSH Machine"
                    data-testid="remote-host-select"
                    className={fieldClass}
                    value={selectedHostId}
                    disabled={submitting}
                    onChange={(event) => setSelectedHostId(event.target.value)}
                  >
                    {!selectedHostId ? (
                      <option value="" disabled>
                        Select an SSH machine...
                      </option>
                    ) : null}
                    {enabledHosts.map((h) => (
                      <option key={h.id} value={h.id}>
                        {h.label} ({formatSshTarget(h)})
                      </option>
                    ))}
                  </select>
                </label>

                <label className="block space-y-1 text-[11px] text-muted-foreground" htmlFor="remote-repo-path">
                  <span>Remote repository path</span>
                  <input
                    id="remote-repo-path"
                    aria-label="Remote repository path"
                    data-testid="remote-repo-path-input"
                    className={fieldClass}
                    value={remoteRepoPath}
                    disabled={submitting}
                    onChange={(event) => {
                      const val = event.target.value;
                      setRemoteRepoPath(val);
                      if (!remoteIdEdited) {
                        setRemoteWorkspaceId(deriveWorkspaceId(val, projectsRef.current));
                      }
                    }}
                    placeholder="/home/ubuntu/my-app"
                    autoFocus
                  />
                </label>

                <label className="block space-y-1 text-[11px] text-muted-foreground" htmlFor="remote-workspace-id">
                  <span>Workspace id</span>
                  <input
                    id="remote-workspace-id"
                    aria-label="Workspace id"
                    data-testid="remote-workspace-id-input"
                    className={fieldClass}
                    value={remoteWorkspaceId}
                    disabled={submitting}
                    onChange={(event) => {
                      setRemoteIdEdited(true);
                      setRemoteWorkspaceId(event.target.value);
                    }}
                    placeholder="my-app"
                  />
                </label>

                {remoteError ? <p className="text-[11px] text-destructive">{remoteError}</p> : null}
              </>
            )}
          </div>

          <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              disabled={submitting}
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
              onClick={() => {
                setRemoteError(null);
                setStep("choose-location");
              }}
            >
              Back
            </button>
            <button
              type="button"
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent"
              onClick={handleDismiss}
            >
              Cancel
            </button>
            <button
              type="submit"
              data-testid="add-project-confirm-remote"
              disabled={
                submitting ||
                enabledHosts.length === 0 ||
                !selectedHostId ||
                !remoteRepoPath.trim() ||
                !remoteWorkspaceId.trim()
              }
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
        </form>
      </div>
    );
  }

  // 5. Manual fallback entry (non-Tauri or picker capability error)
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <form
        role="dialog"
        aria-label="Add Project"
        className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        onSubmit={handleManualSubmit}
      >
        <div className="flex h-9 items-center border-b border-border px-3">
          <button
            type="button"
            aria-label="Back"
            data-testid="add-project-back"
            disabled={submitting}
            className="mr-1.5 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
            onClick={() => {
              setLocalError(null);
              setStep("choose-location");
            }}
          >
            <ArrowLeft className="size-3.5" />
          </button>
          <FolderGit2 className="mr-2 size-3.5 text-muted-foreground" />
          <h2 className="text-[13px] font-medium">Add Project</h2>
          <button
            type="button"
            aria-label="Close Add Project"
            disabled={submitting}
            className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
            onClick={handleDismiss}
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
              disabled={submitting}
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
              disabled={submitting}
              onChange={(event) => setRepoPath(event.target.value)}
              placeholder="/path/to/repository"
            />
          </label>
          {localError ? <p className="text-[11px] text-destructive">{localError}</p> : null}
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button
            type="button"
            disabled={submitting}
            className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
            onClick={() => {
              setLocalError(null);
              setStep("choose-location");
            }}
          >
            Back
          </button>
          <button
            type="button"
            disabled={submitting}
            className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
            onClick={handleDismiss}
          >
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

type RemoveProjectDialogProps = {
  project: RegisteredProject;
  onClose: () => void;
  onConfirm: () => void;
};

export function RemoveProjectDialog({ project, onClose, onConfirm }: RemoveProjectDialogProps) {
  const { hosts } = useSshHosts();
  const target = project.target;
  const isRemote = target?.kind === "ssh";
  const remoteHostId = target?.kind === "ssh" ? target.hostId : null;
  const hostLabel = remoteHostId
    ? hosts.find((h) => h.id === remoteHostId)?.label ?? remoteHostId
    : null;
  const remoteFolder = isRemote
    ? project.repoRoot.split(/[/\\]/).filter(Boolean).at(-1) ?? project.repoRoot
    : null;
  const displayName = isRemote
    ? `${remoteFolder} (${hostLabel})`
    : project.workspaceId;

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6"
      role="presentation"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-label="Remove Project"
        className="w-full max-w-[400px] overflow-hidden rounded-lg border border-border bg-card shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="flex h-9 items-center border-b border-border px-3">
          <Trash2 className="mr-2 size-3.5 text-destructive" />
          <h2 className="text-[13px] font-medium">Remove Project</h2>
          <button
            type="button"
            aria-label="Close Remove Project"
            className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            onClick={onClose}
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="selectable space-y-2 p-3 text-xs">
          <p className="text-foreground">
            Are you sure you want to remove <span className="font-semibold">{displayName}</span> from Ferryx?
          </p>
          <p className="text-[11px] text-muted-foreground">
            This only removes the project from your sidebar. Your repository files at{" "}
            <span className="break-all font-mono text-muted-foreground">{project.repoRoot}</span> will not be deleted.
          </p>
        </div>
        <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
          <button
            type="button"
            className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent"
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="h-7 rounded-md bg-destructive px-3 text-xs font-medium text-destructive-foreground hover:bg-destructive/90"
            onClick={() => {
              onConfirm();
              onClose();
            }}
          >
            Remove Project
          </button>
        </div>
      </div>
    </div>
  );
}
