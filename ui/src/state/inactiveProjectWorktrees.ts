import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import {
  listWorktrees as defaultListWorktrees,
  onWorktreeChanged as defaultOnWorktreeChanged,
  registerProject as defaultRegisterProject,
} from "../lib/tauri";
import type { RegisteredProject, Worktree, WorktreeChangedPayload } from "../lib/types";
import { switchDebug } from "../lib/switchDebug";
import { projectRootWorktree as plainRootWorktree } from "../lib/projectIdentity";
import { getWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { listPairedProjectWorktrees } from "./pairedProjectWorktrees";
import { remoteHostStore } from "./remoteHostStore";

export type WorktreeFreshness = {
  stale: boolean;
  offline: boolean;
  generation?: string | null;
  cachedAt?: number;
};

export const DEFAULT_PAIRED_WORKTREE_TTL_MS = 60_000;

export type InactiveProjectWorktreeServices = {
  registerProject: (request: { workspaceId: string; repoPath: string }) => Promise<RegisteredProject>;
  listWorktrees: (workspaceId: string) => Promise<Worktree[]>;
  onWorktreeChanged?: (handler: (payload: WorktreeChangedPayload) => void) => Promise<() => void>;
  pairedWorktreeTtlMs?: number;
};

const defaultServices: InactiveProjectWorktreeServices = {
  registerProject: defaultRegisterProject,
  listWorktrees: defaultListWorktrees,
  onWorktreeChanged: defaultOnWorktreeChanged,
};

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
  onRegistered?: (project: RegisteredProject) => void,
): Record<string, Worktree[]> {
  const [worktreesByProject, setWorktreesByProject] = useState<Record<string, Worktree[]>>(() => {
    const initial: Record<string, Worktree[]> = {};
    for (const project of projects) {
      if (project.workspaceId === activeProjectId && activeWorktrees.length > 0) {
        initial[project.workspaceId] = activeWorktrees;
        continue;
      }
      const snapshot = getWorkspaceSnapshot(project.workspaceId);
      if (snapshot && snapshot.worktrees && snapshot.worktrees.length > 0) {
        initial[project.workspaceId] = snapshot.worktrees;
      }
    }
    return initial;
  });

  useEffect(() => {
    if (activeProjectId && activeWorktrees.length > 0) {
      setWorktreesByProject((current) => {
        if (current[activeProjectId] === activeWorktrees) return current;
        return { ...current, [activeProjectId]: activeWorktrees };
      });
    }
  }, [activeProjectId, activeWorktrees]);

  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const pairedRefreshKey = JSON.stringify(projects.filter(project => project.target?.kind === "pairedDaemon").map(project => {
    const host = project.target?.kind === "pairedDaemon" ? remoteState.hosts[project.target.hostId] : undefined;
    return [project.workspaceId, host?.generation, host?.online, host?.authStatus, remoteState.nativeStatus, remoteState.machineFeaturesEnabled];
  }));
  const inactiveTargets = projects.filter((project) => project.workspaceId !== activeProjectId || project.target?.kind === "pairedDaemon");
  const inactiveKey = JSON.stringify(inactiveTargets.map(({ workspaceId, repoRoot, gitRoot, target }) => ({
    workspaceId, repoRoot, gitRoot, target,
  })));
  const onRegisteredRef = useRef(onRegistered);
  onRegisteredRef.current = onRegistered;

  const lastAuthoritativeGenRef = useRef<Record<string, string | null>>({});
  const ttlTimersRef = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  useEffect(() => {
    return () => {
      for (const timer of Object.values(ttlTimersRef.current)) {
        clearTimeout(timer);
      }
      ttlTimersRef.current = {};
    };
  }, []);

  // Deletions of an inactive project's worktree (sidebar trash icon, another
  // desktop, remote client) arrive as backend `worktree_changed` events. The
  // active workspace refreshes itself, so this hook only needs to re-list the
  // affected inactive project — otherwise its deleted row lingers in the
  // sidebar as stale and still actionable.
  const inactiveTargetsRef = useRef<RegisteredProject[]>([]);
  inactiveTargetsRef.current = inactiveTargets;
  const servicesRef = useRef(services);
  servicesRef.current = services;

  useEffect(() => {
    const subscribe = servicesRef.current.onWorktreeChanged;
    if (!subscribe) return;
    let cancelled = false;
    const unlistenPromise = subscribe((payload: WorktreeChangedPayload) => {
      const workspaceId = payload.workspaceId;
      const target = inactiveTargetsRef.current.find((project) => project.workspaceId === workspaceId);
      if (!target) return;

      if (target.target?.kind === "pairedDaemon") {
        const pairedTarget = target.target;
        const handleRelistStaleTransition = () => {
          setWorktreesByProject((current) => {
            const existing = current[workspaceId];
            if (!existing || existing.length === 0) return current;
            const host = remoteHostStore.getState().hosts[pairedTarget.hostId];
            const isOffline = host ? !host.online : true;
            const cachedGen = lastAuthoritativeGenRef.current[workspaceId] ?? host?.generation ?? (existing[0] as any)?.freshness?.generation ?? null;
            const cachedAt = (existing[0] as any)?.freshness?.cachedAt ?? Date.now();
            const staleRows: Worktree[] = existing.map((row) => ({
              ...row,
              stale: true,
              offline: isOffline,
              disabled: true,
              hostSummary: isOffline ? "Offline (stale)" : "Stale",
              freshness: {
                stale: true,
                offline: isOffline,
                generation: cachedGen,
                cachedAt,
              },
            }));

            const ttlMs = servicesRef.current.pairedWorktreeTtlMs ?? DEFAULT_PAIRED_WORKTREE_TTL_MS;
            if (ttlMs > 0 && !ttlTimersRef.current[workspaceId]) {
              ttlTimersRef.current[workspaceId] = setTimeout(() => {
                setWorktreesByProject((curr) => {
                  const r = curr[workspaceId];
                  if (r && r.some((item) => (item as any).stale)) {
                    return { ...curr, [workspaceId]: [] };
                  }
                  return curr;
                });
                delete ttlTimersRef.current[workspaceId];
              }, ttlMs);
            }

            return { ...current, [workspaceId]: staleRows };
          });
        };

        void listPairedProjectWorktrees(target).then((listed) => {
          if (cancelled) return;
          if (listed !== null) {
            const host = remoteHostStore.getState().hosts[pairedTarget.hostId];
            lastAuthoritativeGenRef.current[workspaceId] = host?.generation ?? null;
            if (ttlTimersRef.current[workspaceId]) {
              clearTimeout(ttlTimersRef.current[workspaceId]);
              delete ttlTimersRef.current[workspaceId];
            }
            setWorktreesByProject((current) => ({ ...current, [workspaceId]: listed }));
          } else {
            handleRelistStaleTransition();
          }
        }).catch((error) => {
          switchDebug("inactive-worktrees.relist.error", { workspaceId, error: String(error) });
          if (!cancelled) {
            handleRelistStaleTransition();
          }
        });
        return;
      }
      const isSsh = target.target?.kind === "ssh";
      if (!isSsh) {
        // Rescan-emitted `created`/`updated` events carry worktrees the sidebar
        // does not know about yet, so they must trigger a re-list too; only
        // dirty-state noise is ignored.
        if (
          payload.kind !== "created" &&
          payload.kind !== "updated" &&
          payload.kind !== "deleted" &&
          payload.kind !== "destructivelyDeleted" &&
          payload.kind !== "pruned"
        ) {
          return;
        }
      }
      switchDebug("inactive-worktrees.relist", { workspaceId, kind: payload.kind });
      void servicesRef.current
        .listWorktrees(workspaceId)
        .then((listed) => {
          if (cancelled) return;
          const worktrees = !target || listed.length > 0 || target.gitRoot !== null ? listed : [plainRootWorktree(target)];
          setWorktreesByProject((current) => ({ ...current, [workspaceId]: worktrees }));
        })
        .catch((error: unknown) => {
          switchDebug("inactive-worktrees.relist.error", {
            workspaceId,
            error: String(error),
          });
        });
    });
    return () => {
      cancelled = true;
      void Promise.resolve(unlistenPromise)
        .then((unlisten) => unlisten?.())
        .catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (inactiveTargetsRef.current.length === 0) {
      switchDebug("inactive-worktrees.cleared", {
        activeProjectId,
      });
      return;
    }

    let cancelled = false;
    const targets = inactiveTargetsRef.current;
    switchDebug("inactive-worktrees.load.start", {
      activeProjectId,
      targetWorkspaceIds: targets.map((project) => project.workspaceId),
    });

    void (async () => {
      const resolved = await Promise.all(
        targets.map(async (project) => {
          if (project.target?.kind === "pairedDaemon") {
            try {
              const listed = await listPairedProjectWorktrees(project);
              return [project.workspaceId, listed] as const;
            } catch (error) {
              switchDebug("inactive-worktrees.error", { workspaceId: project.workspaceId, error: String(error) });
              return [project.workspaceId, null] as const;
            }
          }
          // Inactive SSH targets skip local project registration, but git-backed
          // SSH projects still list their remote worktrees on initial load.
          if (project.target?.kind === "ssh") {
            if (project.gitRoot === null) {
              return [project.workspaceId, [plainRootWorktree(project)]] as const;
            }
            try {
              const listed = await services.listWorktrees(project.workspaceId);
              const worktrees = listed.length > 0 ? listed : [plainRootWorktree(project)];
              return [project.workspaceId, worktrees] as const;
            } catch (error) {
              switchDebug("inactive-worktrees.error", { workspaceId: project.workspaceId, error: String(error) });
              return [project.workspaceId, null] as const;
            }
          }
          try {
            // A rejection means this ID is bound to a different root, so listing
            // would report another repository's worktrees under this project.
            const registered = await services.registerProject({
              workspaceId: project.workspaceId,
              repoPath: project.repoRoot,
            });
            if (!cancelled) onRegisteredRef.current?.(registered);
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
            return [project.workspaceId, null] as const;
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
        ...Object.fromEntries(resolved.flatMap(([id, rows]) => {
          const project = targets.find(target => target.workspaceId === id);
          if (rows !== null) {
            if (project?.target?.kind === "pairedDaemon") {
              const host = remoteHostStore.getState().hosts[project.target.hostId];
              lastAuthoritativeGenRef.current[id] = host?.generation ?? null;
              if (ttlTimersRef.current[id]) {
                clearTimeout(ttlTimersRef.current[id]);
                delete ttlTimersRef.current[id];
              }
            }
            return [[id, [...rows]]];
          }
          if (project?.target?.kind === "pairedDaemon") {
            const existing = current[id];
            if (existing && existing.length > 0) {
              const host = remoteHostStore.getState().hosts[project.target.hostId];
              const isOffline = host ? !host.online : true;
              const cachedGen = lastAuthoritativeGenRef.current[id] ?? host?.generation ?? (existing[0] as any)?.freshness?.generation ?? null;
              const cachedAt = (existing[0] as any)?.freshness?.cachedAt ?? Date.now();
              const staleRows: Worktree[] = existing.map(row => ({
                ...row,
                stale: true,
                offline: isOffline,
                disabled: true,
                hostSummary: isOffline ? "Offline (stale)" : "Stale",
                freshness: {
                  stale: true,
                  offline: isOffline,
                  generation: cachedGen,
                  cachedAt,
                },
              }));

              const ttlMs = servicesRef.current.pairedWorktreeTtlMs ?? DEFAULT_PAIRED_WORKTREE_TTL_MS;
              if (ttlMs > 0 && !ttlTimersRef.current[id]) {
                ttlTimersRef.current[id] = setTimeout(() => {
                  setWorktreesByProject(curr => {
                    const r = curr[id];
                    if (r && r.some(item => (item as any).stale)) {
                      return { ...curr, [id]: [] };
                    }
                    return curr;
                  });
                  delete ttlTimersRef.current[id];
                }, ttlMs);
              }

              return [[id, staleRows]];
            }
          }
          // Preserve existing data on error. Keep the legacy local empty-cache
          // shape only when this workspace has never produced any rows.
          return !current[id] && (!project?.target || project.target.kind === "local") ? [[id, []]] : [];
        })),
      }));
    })();

    return () => {
      cancelled = true;
      switchDebug("inactive-worktrees.load.cancel", {
        activeProjectId,
      });
    };
  }, [activeProjectId, inactiveKey, pairedRefreshKey, services]);

  return worktreesByProject;
}
