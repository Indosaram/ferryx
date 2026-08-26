import { useEffect, useState } from "react";

import { listWorktrees as defaultListWorktrees, registerProject as defaultRegisterProject } from "../lib/tauri";
import type { RegisteredProject, Worktree } from "../lib/types";
import { switchDebug } from "../lib/switchDebug";

export type InactiveProjectWorktreeServices = {
  registerProject: (request: { workspaceId: string; repoPath: string }) => Promise<RegisteredProject>;
  listWorktrees: (workspaceId: string) => Promise<Worktree[]>;
};

const defaultServices: InactiveProjectWorktreeServices = {
  registerProject: defaultRegisterProject,
  listWorktrees: defaultListWorktrees,
};

function plainRootWorktree(project: RegisteredProject): Worktree {
  return {
    path: project.repoRoot,
    head: "",
    branch: null,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

/**
 * The workspace store only holds the active project's worktrees, so sidebar rows
 * for every other registered project would render an empty list. This lists them
 * separately, registering each workspace first because listing an unregistered
 * workspace fails with WORKSPACE_NOT_FOUND.
 */
export function useInactiveProjectWorktrees(
  projects: RegisteredProject[],
  activeProjectId: string,
  activeWorktrees: Worktree[] = [],
  services: InactiveProjectWorktreeServices = defaultServices,
): Record<string, Worktree[]> {
  const [worktreesByProject, setWorktreesByProject] = useState<Record<string, Worktree[]>>(() =>
    activeProjectId && activeWorktrees.length > 0
      ? { [activeProjectId]: activeWorktrees }
      : {},
  );

  useEffect(() => {
    if (activeProjectId && activeWorktrees.length > 0) {
      setWorktreesByProject((current) => {
        if (current[activeProjectId] === activeWorktrees) return current;
        return { ...current, [activeProjectId]: activeWorktrees };
      });
    }
  }, [activeProjectId, activeWorktrees]);

  const inactiveKey = projects
    .filter((project) => project.workspaceId !== activeProjectId)
    .map((project) => `${project.workspaceId}\u0000${project.repoRoot}\u0000${project.gitRoot ?? ""}`)
    .join("\u0001");

  useEffect(() => {
    if (!inactiveKey) {
      switchDebug("inactive-worktrees.cleared", {
        activeProjectId,
      });
      return;
    }

    let cancelled = false;
    const targets = inactiveKey.split("\u0001").map((entry) => {
      const [workspaceId, repoRoot, gitRoot] = entry.split("\u0000");
      return { workspaceId, repoRoot, gitRoot: gitRoot ? gitRoot : null } satisfies RegisteredProject;
    });
    switchDebug("inactive-worktrees.load.start", {
      activeProjectId,
      targetWorkspaceIds: targets.map((project) => project.workspaceId),
    });

    void (async () => {
      const resolved = await Promise.all(
        targets.map(async (project) => {
          try {
            // A rejection means this ID is bound to a different root, so listing
            // would report another repository's worktrees under this project.
            const registered = await services.registerProject({
              workspaceId: project.workspaceId,
              repoPath: project.repoRoot,
            });
            switchDebug("inactive-worktrees.registered", {
              requestedWorkspaceId: project.workspaceId,
              registeredWorkspaceId: registered.workspaceId,
            });
            const listed = await services.listWorktrees(project.workspaceId);
            const worktrees = listed.length > 0 || project.gitRoot !== null ? listed : [plainRootWorktree(project)];
            switchDebug("inactive-worktrees.listed", {
              workspaceId: project.workspaceId,
              listedCount: listed.length,
              resolvedCount: worktrees.length,
              paths: worktrees.map((worktree) => worktree.path),
            });
            return [project.workspaceId, worktrees] as const;
          } catch (error) {
            switchDebug("inactive-worktrees.error", {
              workspaceId: project.workspaceId,
              error: String(error),
            });
            return [project.workspaceId, [] as Worktree[]] as const;
          }
        }),
      );
      if (cancelled) {
        switchDebug("inactive-worktrees.load.ignored", {
          activeProjectId,
        });
        return;
      }
      switchDebug("inactive-worktrees.load.complete", {
        activeProjectId,
        workspaceIds: resolved.map(([workspaceId]) => workspaceId),
      });
      setWorktreesByProject((current) => ({
        ...current,
        ...Object.fromEntries(resolved),
      }));
    })();

    return () => {
      cancelled = true;
      switchDebug("inactive-worktrees.load.cancel", {
        activeProjectId,
      });
    };
  }, [activeProjectId, inactiveKey, services]);

  return worktreesByProject;
}
