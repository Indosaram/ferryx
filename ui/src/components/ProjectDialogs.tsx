import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft,
  Cloud,
  FolderGit2,
  FolderPlus,
  GitBranch,
  Laptop,
  LoaderCircle,
  Radio,
  Settings,
  Trash2,
  X,
} from "lucide-react";
import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { copyTextToClipboard } from "../lib/clipboard";
import { listCloudProviders, type CloudProviderStatus } from "../lib/cloudHost";
import { createPairedWorktreeActions, pairedActionMessage } from "../lib/pairedWorktreeActions";
import { createRemoteWorktree, registerRemoteProject, toRegisteredProject } from "../lib/remoteProject";
import { formatSshTarget, useSshHosts } from "../lib/sshHosts";
import {
  createWorktree,
  isTauriRuntime,
  listProjectRefs,
  previewGitHubIssue,
  registerProject,
  toIpcError,
  type GitHubIssuePreview,
  type ProjectRefs,
  type RegisteredProject,
} from "../lib/tauri";
import type { Worktree } from "../lib/types";
import { RemoteDirectoryPicker } from "./RemoteDirectoryPicker";
import { PairedDaemonProjectForm } from "./PairedDaemonProjectForm";
import { useModalDialog } from "./useModalDialog";

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

/// The text a user copies out of an issue preview and pastes into the new pane
/// themselves. Ferryx never sends it anywhere on its own.
function issueContextText(issue: GitHubIssuePreview): string {
  const lines = [`Issue #${issue.number}: ${issue.title}`, issue.url];
  const body = issue.body.trim();
  if (body) lines.push("", body);
  return `${lines.join("\n")}\n`;
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

import type { MachineProjectTarget } from "../lib/machineNavigation";

export type AddProjectDialogProps = {
  projects?: RegisteredProject[];
  onClose: () => void;
  onRegistered: (project: RegisteredProject) => void;
  onOpenSettings?: (section: "ssh") => void;
  initialHostId?: string;
  initialTarget?: MachineProjectTarget;
};

type AddProjectStep =
  | "choose-location"
  | "local-pending"
  | "local-confirm"
  | "local-manual"
  | "remote-form"
  | "paired-form";

export function AddProjectDialog({
  projects = [],
  onClose,
  onRegistered,
  onOpenSettings,
  initialHostId: legacyHostId,
  initialTarget,
}: AddProjectDialogProps) {
  const initialHostId = initialTarget?.kind === "ssh" ? initialTarget.hostId : legacyHostId;
  const [step, setStep] = useState<AddProjectStep>(initialTarget?.kind === "pairedDaemon" ? "paired-form" : initialHostId ? "remote-form" : "choose-location");
  const [workspaceId, setWorkspaceId] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  // Remote flow state
  const { hosts, loading: hostsLoading, error: hostsLoadError, refresh: refreshHosts } = useSshHosts();
  const enabledHosts = hosts.filter((h) => !h.disabled);
  const [selectedHostId, setSelectedHostId] = useState(initialHostId ?? "");
  const selectedHost = enabledHosts.find((host) => host.id === selectedHostId);
  const hostKey = selectedHost ? JSON.stringify(selectedHost) : "";
  const [remoteSelection, setRemoteSelection] = useState<{ hostKey: string; path: string } | null>(null);
  const remoteRepoPath = remoteSelection?.hostKey === hostKey ? remoteSelection.path : "";
  const handleRemoteSelect = useCallback((path: string | null) => {
    setRemoteSelection(path === null ? null : { hostKey, path });
  }, [hostKey]);
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
  const hasHadSelectionRef = useRef(Boolean(initialHostId));
  const isTauri = checkIsTauri();

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // Sync selectedHostId with available enabled hosts
  useEffect(() => {
    if (hostsLoading) return;
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
  }, [enabledHosts, hostsLoading]);

  const handleDismiss = () => {
    dismissedRef.current = true;
    onCloseRef.current();
  };

  // Escape follows the Close/Cancel buttons: no dismissal while a registration is in
  // flight, otherwise onRegistered would be skipped for a project that did register.
  const { dialogRef } = useModalDialog({
    onEscape: handleDismiss,
    dismissible: !submitting,
    active: step !== "paired-form",
    focusKey: step,
  });

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
        .then(async (selected) => {
          if (dismissedRef.current || !isMountedRef.current) return;
          if (typeof selected === "string" && selected.length > 0) {
            // Restore native focus before showing confirmation so its first click
            // submits instead of only activating the window after the picker.
            await getCurrentWindow().setFocus();
            if (dismissedRef.current || !isMountedRef.current) return;
            setSelectedPath(selected);
            setStep("local-confirm");
          } else {
            handleDismiss();
          }
        })
        .catch((cause) => {
          if (dismissedRef.current || !isMountedRef.current) return;
          console.error(cause);
          const message = typeof cause === "string" ? cause : toIpcError(cause).message;
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
    const trimmedWorkspaceId = deriveWorkspaceId(remoteRepoPath, projectsRef.current);
    const trimmedPath = remoteRepoPath;
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
    if (JSON.stringify(host) !== hostKey) {
      setRemoteSelection(null);
      setRemoteError("The SSH machine changed. Open the folder again before adding it.");
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

  if (step === "paired-form") {
    return <PairedDaemonProjectForm initialTarget={initialTarget?.kind === "pairedDaemon" ? initialTarget : undefined}
      onBack={initialTarget ? handleDismiss : () => setStep("choose-location")}
      onClose={handleDismiss} onRegistered={onRegistered} />;
  }

  // 1. Initial Location Chooser
  if (step === "choose-location") {
    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
        <div
          ref={dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label="Add Project"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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
              <button type="button" data-testid="project-type-paired-daemon" onClick={() => setStep("paired-form")}
                className="flex w-full items-start gap-3 rounded-md border border-border/80 bg-background p-3 text-left hover:bg-accent/40 focus-visible:border-ring">
                <Radio className="mt-0.5 size-4 shrink-0 text-primary" />
                <div>
                  <div className="text-xs font-medium text-foreground">Paired Daemon</div>
                  <div className="mt-0.5 text-[11px] text-muted-foreground">Choose a folder on a paired machine without SSH.</div>
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
          ref={dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label="Add Project"
          aria-busy="true"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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
          ref={dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label="Add Project"
          className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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
          ref={dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label="Add Project"
          className="flex max-h-[calc(100dvh-3rem)] w-full max-w-xl flex-col overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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
              disabled={submitting}
              className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
              onClick={handleDismiss}
            >
              <X className="size-3.5" />
            </button>
          </div>

          <div className="min-h-0 space-y-3 overflow-y-auto p-3">
            {hostsLoading && hosts.length === 0 ? (
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
                    onChange={(event) => {
                      setRemoteSelection(null);
                      setRemoteError(null);
                      setSelectedHostId(event.target.value);
                    }}
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

                {onOpenSettings ? (
                  <button type="button" disabled={submitting}
                    className="text-xs text-muted-foreground underline underline-offset-4 hover:text-foreground"
                    onClick={() => {
                      onOpenSettingsRef.current?.("ssh");
                      handleDismiss();
                    }}>Add or manage SSH machines</button>
                ) : null}
                {selectedHost ? (
                  <RemoteDirectoryPicker key={hostKey} hostId={selectedHost.id}
                    disabled={submitting} onSelect={handleRemoteSelect} />
                ) : null}
                {remoteRepoPath ? (
                  <p className="break-all font-mono text-[11px] text-muted-foreground" aria-label="Selected remote folder">
                    {remoteRepoPath}
                  </p>
                ) : null}

                {remoteError ? <p className="text-[11px] text-destructive">{remoteError}</p> : null}
              </>
            )}
          </div>

          <div className="flex shrink-0 justify-end gap-2 border-t border-border px-3 py-2">
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
              disabled={submitting}
              className="h-7 rounded-md border border-border px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45"
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
                !remoteRepoPath
              }
              className="inline-flex h-7 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
            >
              {submitting ? (
                <LoaderCircle className="size-3.5 animate-spin" />
              ) : (
                <FolderPlus className="size-3.5" />
              )}
              Add this folder
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
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Add Project"
        className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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

/** Agent the new worktree should start in; omitted when the user picks "Shell only". */
export type WorktreeAgentLaunch = { agentId: string };

export type CloudRunLaunchOptions = {
  prompt: string;
  provider?: string;
  keep?: boolean;
};

type AddWorktreeDialogProps = {
  project: RegisteredProject;
  onClose: () => void;
  onCreated: (worktree: Worktree, launch?: WorktreeAgentLaunch) => void | Promise<void>;
  /** Launchable agents (enabled and available), the same list the New Tab menu offers. */
  agents?: ReadonlyArray<{ name: string }>;
  defaultAgentId?: string | null;
  onLaunchCloudRun?: (options: CloudRunLaunchOptions) => void | Promise<void>;
  onOpenRemoteSettings?: () => void;
};

const EMPTY_REFS: ProjectRefs = { branches: [], tags: [] };
const NO_AGENTS: ReadonlyArray<{ name: string }> = [];

function agentLabel(name: string) {
  return name.charAt(0).toUpperCase() + name.slice(1);
}

export function AddWorktreeDialog({
  project,
  onClose,
  onCreated,
  agents = NO_AGENTS,
  defaultAgentId = null,
  onLaunchCloudRun,
  onOpenRemoteSettings,
}: AddWorktreeDialogProps) {
  const remoteTarget = project.target?.kind === "ssh" ? project.target : null;
  const isPaired = project.target?.kind === "pairedDaemon";
  const pairedActions = useMemo(() => isPaired ? createPairedWorktreeActions(project) : null, [project, isPaired]);
  const isRemote = remoteTarget !== null || isPaired;
  const isGitBacked = project.gitRoot !== null;
  const offersCloudToggle = !isRemote && Boolean(onLaunchCloudRun);
  const { hosts } = useSshHosts();
  const [executionMode, setExecutionMode] = useState<"local" | "cloud">("local");
  const [cloudProviders, setCloudProviders] = useState<CloudProviderStatus[] | null>(null);
  const [cloudPrompt, setCloudPrompt] = useState("");
  const [cloudProvider, setCloudProvider] = useState("auto");
  const [cloudKeep, setCloudKeep] = useState(false);
  const [refs, setRefs] = useState<ProjectRefs>(EMPTY_REFS);
  const [baseRef, setBaseRef] = useState("");
  const [agentId, setAgentId] = useState(() =>
    defaultAgentId && agents.some((agent) => agent.name === defaultAgentId) ? defaultAgentId : "",
  );
  const [slug, setSlug] = useState("");
  const [loadingBranches, setLoadingBranches] = useState(!isRemote && isGitBacked);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [issueRef, setIssueRef] = useState("");
  const [issue, setIssue] = useState<GitHubIssuePreview | null>(null);
  const [loadingIssue, setLoadingIssue] = useState(false);
  const [issueError, setIssueError] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const submitInFlight = useRef(false);
  const issueRequestId = useRef(0);
  const supportsIssueIntake = isGitBacked && !isRemote;
  // Remote and paired creation stay slug-only and never start an agent.
  const offersAgents = isGitBacked && !isRemote && agents.length > 0;
  const refListId = `worktree-base-refs-${project.workspaceId}`;
  // No dismissal while the create request is in flight: onClose would otherwise run
  // once for the dismissal and again when the create succeeds.
  const { dialogRef } = useModalDialog({ onEscape: onClose, dismissible: !submitting });

  const remoteHostId = remoteTarget?.hostId ?? null;
  const hostLabel = remoteHostId
    ? hosts.find((h) => h.id === remoteHostId)?.label ?? remoteHostId
    : null;
  const dialogTitle = isRemote && hostLabel
    ? `Add Worktree · ${hostLabel}`
    : `Add Worktree · ${project.workspaceId}`;

  useEffect(() => {
    if (isRemote || !isGitBacked) {
      setRefs(EMPTY_REFS);
      setBaseRef("");
      setLoadingBranches(false);
      return;
    }
    let alive = true;
    setLoadingBranches(true);
    setError(null);
    void listProjectRefs(project.workspaceId)
      .then((items) => {
        if (!alive) return;
        setRefs(items);
        const preferred = items.branches.find((branch) => branch.isCurrent) ?? items.branches[0];
        setBaseRef(preferred?.name ?? "");
      })
      .catch((cause) => {
        if (!alive) return;
        setError(extractErrorMessage(cause, "Could not load local branches and tags."));
      })
      .finally(() => {
        if (alive) setLoadingBranches(false);
      });
    return () => {
      alive = false;
    };
  }, [project.workspaceId, isGitBacked, isRemote]);

  const loadIssue = async () => {
    const trimmed = issueRef.trim();
    if (!trimmed || loadingIssue) return;
    // Only the newest request may publish: an earlier one resolving late would
    // otherwise preview - and pre-fill the slug from - a different issue than
    // the reference now in the field.
    const requestId = issueRequestId.current + 1;
    issueRequestId.current = requestId;
    setLoadingIssue(true);
    setIssueError(null);
    setCopyState("idle");
    try {
      const preview = await previewGitHubIssue({ workspaceId: project.workspaceId, issueRef: trimmed });
      if (issueRequestId.current !== requestId) return;
      setIssue(preview);
      setSlug(preview.suggestedSlug);
    } catch (cause) {
      if (issueRequestId.current !== requestId) return;
      setIssue(null);
      setIssueError(extractErrorMessage(cause, "Could not read that GitHub issue."));
    } finally {
      if (issueRequestId.current === requestId) setLoadingIssue(false);
    }
  };

  const copyIssueContext = async () => {
    if (!issue) return;
    setCopyState((await copyTextToClipboard(issueContextText(issue))) ? "copied" : "failed");
  };

  useEffect(() => {
    if (!offersCloudToggle || executionMode !== "cloud") return;
    let alive = true;
    void listCloudProviders().then((rows) => {
      if (alive) setCloudProviders(rows);
    });
    return () => {
      alive = false;
    };
  }, [offersCloudToggle, executionMode]);

  const readyCloudProviders = useMemo(
    () => (cloudProviders ?? []).filter((p) => p.configured),
    [cloudProviders],
  );

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (executionMode === "cloud" && onLaunchCloudRun) {
      const trimmedPrompt = cloudPrompt.trim();
      if (!trimmedPrompt || submitting || submitInFlight.current) return;
      submitInFlight.current = true;
      setSubmitting(true);
      setError(null);
      try {
        await onLaunchCloudRun({
          prompt: trimmedPrompt,
          provider: cloudProvider === "auto" ? undefined : cloudProvider,
          keep: cloudKeep,
        });
        onClose();
      } catch (cause) {
        setError(extractErrorMessage(cause, "Could not start the cloud run."));
      } finally {
        submitInFlight.current = false;
        setSubmitting(false);
      }
      return;
    }
    const trimmedSlug = slug.trim();
    // A ref guards the double click that lands before React re-renders; the
    // `submitting` state alone cannot, since both clicks see the same render.
    if (!trimmedSlug || submitting || submitInFlight.current) return;
    submitInFlight.current = true;
    setSubmitting(true);
    setError(null);
    try {
      if (pairedActions) {
        await onCreated(await pairedActions.create(trimmedSlug));
      } else if (isRemote) {
        const created = await createRemoteWorktree({
          workspaceId: project.workspaceId,
          slug: trimmedSlug,
        });
        const worktree: Worktree = {
          workspaceId: project.workspaceId,
          path: created.path,
          head: created.head ?? "",
          branch: created.branch ? "refs/heads/" + created.branch : null,
          bare: created.bare,
          detached: created.detached,
          locked: null,
          prunable: null,
        };
        await onCreated(worktree);
      } else {
        // An empty ref bases the worktree on HEAD; the backend validates any typed ref.
        const worktree = await createWorktree({
          workspaceId: project.workspaceId,
          worktree: { wsId: project.workspaceId, slug: trimmedSlug },
          baseRef: baseRef.trim() || null,
        });
        if (offersAgents && agentId) await onCreated(worktree, { agentId });
        else await onCreated(worktree);
      }
      onClose();
    } catch (cause) {
      setError(isPaired ? pairedActionMessage(cause) : extractErrorMessage(cause, "Could not create the worktree."));
    } finally {
      submitInFlight.current = false;
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6" role="presentation">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Add Worktree"
        className="w-full max-w-[420px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
      >
      <form aria-label="Add Worktree" onSubmit={submit}>
        <div className="flex h-9 items-center border-b border-border px-3">
          <GitBranch className="mr-2 size-3.5 text-muted-foreground" />
          <h2 className="min-w-0 flex-1 truncate text-[13px] font-medium">{dialogTitle}</h2>
          <button
            type="button"
            aria-label="Close Add Worktree"
            disabled={submitting}
            className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-45"
            onClick={onClose}
          >
            <X className="size-3.5" />
          </button>
        </div>
        <div className="space-y-3 p-3">
          {offersCloudToggle ? (
            <div role="radiogroup" aria-label="Execution target" className="grid grid-cols-2 gap-1 rounded-md border border-border bg-muted/40 p-0.5">
              <button
                type="button"
                role="radio"
                aria-checked={executionMode === "local"}
                disabled={submitting}
                className={`flex h-7 items-center justify-center gap-1.5 rounded text-[11px] font-medium transition-colors ${
                  executionMode === "local"
                    ? "bg-background text-foreground shadow-xs"
                    : "text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => {
                  setExecutionMode("local");
                  setError(null);
                }}
              >
                <Laptop className="size-3.5" />
                Local
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={executionMode === "cloud"}
                disabled={submitting}
                className={`flex h-7 items-center justify-center gap-1.5 rounded text-[11px] font-medium transition-colors ${
                  executionMode === "cloud"
                    ? "bg-background text-foreground shadow-xs"
                    : "text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => {
                  setExecutionMode("cloud");
                  setError(null);
                }}
              >
                <Cloud className="size-3.5" />
                Cloud
              </button>
            </div>
          ) : null}
          {executionMode === "cloud" && offersCloudToggle ? (
            <div className="space-y-3">
              {cloudProviders !== null && readyCloudProviders.length === 0 ? (
                <div className="space-y-2 rounded-md border border-border bg-muted/30 p-2.5">
                  <p className="text-[11px] text-muted-foreground">
                    No cloud provider key is configured yet. Add a Daytona or E2B key once in Remote Settings.
                  </p>
                  {onOpenRemoteSettings ? (
                    <button
                      type="button"
                      className="inline-flex h-7 items-center gap-1.5 rounded-md border border-border bg-background px-2.5 text-[11px] font-medium text-foreground hover:bg-accent"
                      onClick={() => {
                        onClose();
                        onOpenRemoteSettings();
                      }}
                    >
                      <Settings className="size-3" />
                      Open Remote Settings
                    </button>
                  ) : null}
                </div>
              ) : null}
              <label className="block space-y-1 text-[11px] text-muted-foreground">
                <span>Task prompt</span>
                <textarea
                  aria-label="Cloud task prompt"
                  rows={4}
                  className="w-full resize-none rounded-md border border-input bg-background px-2.5 py-2 text-xs text-foreground outline-none focus:border-ring"
                  value={cloudPrompt}
                  disabled={submitting}
                  placeholder="Describe the task to run in a disposable cloud box..."
                  autoFocus
                  onChange={(event) => setCloudPrompt(event.target.value)}
                />
              </label>
              <label className="block space-y-1 text-[11px] text-muted-foreground">
                <span>Provider</span>
                <select
                  aria-label="Cloud provider"
                  className={fieldClass}
                  value={cloudProvider}
                  disabled={submitting}
                  onChange={(event) => setCloudProvider(event.target.value)}
                >
                  <option value="auto">
                    {readyCloudProviders.length > 0
                      ? `Auto (${readyCloudProviders.map((p) => p.label).join(" → ")})`
                      : "Auto (recommended)"}
                  </option>
                  {readyCloudProviders.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label} ({p.freeQuota})
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
                <input
                  type="checkbox"
                  checked={cloudKeep}
                  disabled={submitting}
                  onChange={(event) => setCloudKeep(event.target.checked)}
                />
                <span>Keep cloud box running after task completes</span>
              </label>
            </div>
          ) : isGitBacked ? (
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
              {!isRemote ? (
                <label className="block space-y-1 text-[11px] text-muted-foreground">
                  <span>Base ref</span>
                  <input
                    type="text"
                    role="combobox"
                    aria-label="Base ref"
                    list={refListId}
                    className={fieldClass}
                    value={baseRef}
                    disabled={loadingBranches || submitting}
                    placeholder="Branch, tag or commit (empty = HEAD)"
                    autoComplete="off"
                    spellCheck={false}
                    onChange={(event) => setBaseRef(event.target.value)}
                  />
                  <datalist id={refListId}>
                    {refs.branches.map((branch) => (
                      <option key={`branch:${branch.name}`} value={branch.name}>
                        {branch.isCurrent ? "current branch" : "branch"}
                      </option>
                    ))}
                    {refs.tags.map((tag) => (
                      <option key={`tag:${tag}`} value={tag}>
                        tag
                      </option>
                    ))}
                  </datalist>
                </label>
              ) : null}
              {offersAgents ? (
                <label className="block space-y-1 text-[11px] text-muted-foreground">
                  <span>Agent</span>
                  <select
                    aria-label="Agent"
                    className={fieldClass}
                    value={agentId}
                    disabled={submitting}
                    onChange={(event) => setAgentId(event.target.value)}
                  >
                    <option value="">Shell only</option>
                    {agents.map((agent) => (
                      <option key={agent.name} value={agent.name}>
                        {agentLabel(agent.name)}
                      </option>
                    ))}
                  </select>
                </label>
              ) : null}
              {supportsIssueIntake ? (
                <div className="space-y-2 rounded-md border border-border/60 p-2" aria-label="GitHub issue intake">
                  <label className="block space-y-1 text-[11px] text-muted-foreground">
                    <span>GitHub issue (optional)</span>
                    <input
                      aria-label="GitHub issue"
                      className={fieldClass}
                      value={issueRef}
                      disabled={loadingIssue || submitting}
                      placeholder="12 or https://github.com/owner/repo/issues/12"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          void loadIssue();
                        }
                      }}
                      onChange={(event) => {
                        setIssueRef(event.target.value);
                        // Editing the reference retires any preview still in flight.
                        issueRequestId.current += 1;
                        setLoadingIssue(false);
                        setIssue(null);
                        setIssueError(null);
                        setCopyState("idle");
                      }}
                    />
                  </label>
                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      className="h-7 rounded-md border border-border px-2.5 text-[11px] text-muted-foreground hover:bg-accent disabled:opacity-45"
                      disabled={loadingIssue || !issueRef.trim()}
                      onClick={() => void loadIssue()}
                    >
                      {loadingIssue ? "Reading issue..." : "Load issue"}
                    </button>
                    {issue ? (
                      <button
                        type="button"
                        aria-label="Copy issue context"
                        className="h-7 rounded-md border border-border px-2.5 text-[11px] text-muted-foreground hover:bg-accent"
                        onClick={() => void copyIssueContext()}
                      >
                        Copy issue context
                      </button>
                    ) : null}
                  </div>
                  {issueError ? (
                    <p role="alert" className="text-[11px] text-destructive">
                      {issueError}
                    </p>
                  ) : null}
                  {copyState === "copied" ? (
                    <p className="text-[10px] text-muted-foreground">
                      Copied. Paste it into the new pane when you are ready.
                    </p>
                  ) : null}
                  {copyState === "failed" ? (
                    <p role="alert" className="text-[11px] text-destructive">
                      Could not copy the issue context. Select the preview text and copy it manually.
                    </p>
                  ) : null}
                  {issue ? (
                    <div className="space-y-1" aria-label="Issue preview">
                      <p className="selectable text-[11px] font-medium text-foreground">{issue.title}</p>
                      <p className="selectable break-all font-mono text-[10px] text-muted-foreground">{issue.url}</p>
                      <pre className="selectable max-h-24 overflow-auto whitespace-pre-wrap break-words rounded bg-muted/40 p-1.5 font-mono text-[10px] text-muted-foreground">
                        {issue.body || "No description."}
                      </pre>
                      {issue.bodyTruncated ? (
                        <p className="text-[10px] text-muted-foreground/70">Description truncated for this preview.</p>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              ) : null}
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
          <button type="button" disabled={submitting} className="h-7 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent disabled:opacity-45" onClick={onClose}>
            {executionMode === "cloud" || isGitBacked ? "Cancel" : "Close"}
          </button>
          {executionMode === "cloud" && offersCloudToggle ? (
            <button
              type="submit"
              disabled={submitting || !cloudPrompt.trim() || (cloudProviders !== null && readyCloudProviders.length === 0)}
              className="h-7 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
            >
              {submitting ? "Starting..." : "Run in Cloud"}
            </button>
          ) : isGitBacked ? (
            <button
              type="submit"
              disabled={submitting || (!isRemote && loadingBranches) || !slug.trim()}
              className="h-7 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground disabled:opacity-45"
            >
              Create Worktree{issue ? ` from Issue #${issue.number}` : ""}
            </button>
          ) : null}
        </div>
      </form>
      </div>
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
  const { dialogRef } = useModalDialog({ onEscape: onClose });

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/45 p-6"
      role="presentation"
      onMouseDown={onClose}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Remove Project"
        className="w-full max-w-[400px] overflow-hidden rounded-lg border border-border bg-popover shadow-elevation-4"
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
            This removes the project from your sidebar and terminates all running terminal sessions and processes in this project. Your repository files at{" "}
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
