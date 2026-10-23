import { useEffect, useRef, useState } from "react";

import { loadPromptedExternalWorktrees, pendingExternalWorktrees } from "../lib/externalWorktrees";
import { listExternalWorktrees, type ExternalWorktree, type RegisteredProject } from "../lib/tauri";

export type ExternalWorktreePrompt = { workspaceId: string; worktrees: ExternalWorktree[] };

type Services = { listExternalWorktrees: typeof listExternalWorktrees };

/**
 * Once the active local project is registered, asks Git for its worktrees outside the project
 * folder and surfaces the unanswered ones as a prompt. Each project is checked once per session;
 * "Later" hides the current set until the next launch.
 */
export function useExternalWorktreePrompt(
  activeProject: RegisteredProject,
  readyWorkspaceId: string | null,
  projects: RegisteredProject[],
  services: Services = { listExternalWorktrees },
) {
  const [prompt, setPrompt] = useState<ExternalWorktreePrompt | null>(null);
  const checkedRef = useRef(new Set<string>());
  const hiddenRef = useRef(new Set<string>());
  const projectsRef = useRef(projects);
  projectsRef.current = projects;
  const servicesRef = useRef(services);
  servicesRef.current = services;

  const workspaceId = activeProject.workspaceId;
  const isLocal = !activeProject.target || activeProject.target.kind === "local";
  const isGit = activeProject.gitRoot !== null;

  useEffect(() => {
    if (!isLocal || !isGit || readyWorkspaceId !== workspaceId || checkedRef.current.has(workspaceId)) return;
    let cancelled = false;
    void servicesRef.current
      .listExternalWorktrees(workspaceId)
      .then((discovered) => {
        if (cancelled) return;
        checkedRef.current.add(workspaceId);
        const pending = pendingExternalWorktrees(
          discovered,
          projectsRef.current,
          loadPromptedExternalWorktrees(),
          hiddenRef.current,
        );
        if (pending.length > 0) setPrompt((current) => current ?? { workspaceId, worktrees: pending });
      })
      .catch((error: unknown) => {
        // Discovery is advisory: a failure only means no prompt, and the next launch retries.
        console.warn("External worktree discovery failed:", workspaceId, error);
      });
    return () => {
      cancelled = true;
    };
  }, [isGit, isLocal, readyWorkspaceId, workspaceId]);

  const later = () => {
    for (const worktree of prompt?.worktrees ?? []) hiddenRef.current.add(worktree.path);
    setPrompt(null);
  };
  const close = () => setPrompt(null);

  return { prompt, later, close };
}
