import { isPairedWorkspaceId, isRemoteWorkspaceId } from "./remoteProject";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import type { TerminalSession } from "./types";
import type { WorkspaceAction, WorkspaceState } from "../state/workspaceStore";

export type ParkedRemoteProject = {
  workspaceId: string;
  target?: { kind: string } | null;
};

export type ParkedRemoteRecoveryOptions = {
  projects: readonly ParkedRemoteProject[];
  isMounted: (workspaceId: string) => boolean;
  getState: (workspaceId: string) => WorkspaceState | null;
  dispatch: (workspaceId: string, action: WorkspaceAction) => void;
  /** Fires whenever any project's snapshot changes, including a project becoming mounted. */
  subscribe: (listener: () => void) => () => void;
  startSsh: (workspaceId: string, sessions: TerminalSession[]) => () => void;
  refreshPaired: (
    workspaceId: string,
    sessionId: string,
    onReady: (session: TerminalSession) => void,
  ) => () => void;
};

function isPairedProject(project: ParkedRemoteProject): boolean {
  return project.target?.kind === "pairedDaemon" || isPairedWorkspaceId(project.workspaceId);
}

/**
 * Reconnects the remote projects that are not on screen.
 *
 * SSH reconciliation and the paired retry admission are normally driven by the mounted project or
 * by an on-screen pane, so after a restart every other remote project stayed "reconnecting" until
 * the user opened it. This starts both for every parked project at launch. SSH reconciliation is
 * handed back the moment its project is mounted, because the mounted store then runs its own.
 */
export function startParkedRemoteRecovery(options: ParkedRemoteRecoveryOptions): () => void {
  const sshStops = new Map<string, () => void>();
  const pairedDisposers = new Map<string, Array<() => void>>();
  const disposedWorkspaces = new Set<string>();
  let stopped = false;

  for (const project of options.projects) {
    const { workspaceId } = project;
    if (options.isMounted(workspaceId)) continue;
    const sessions = Object.values(options.getState(workspaceId)?.sessions ?? {});

    if (isRemoteWorkspaceId(workspaceId)) {
      const live = sessions.filter(
        (session) => isRemoteWorkspaceId(session.workspaceId) && session.lifecycle !== "exited",
      );
      if (live.length > 0) sshStops.set(workspaceId, options.startSsh(workspaceId, live));
      continue;
    }

    if (!isPairedProject(project)) continue;
    for (const session of sessions) {
      if (!session.backendSessionId || isStandbyBackendSessionId(session.backendSessionId)) continue;
      if (session.lifecycle === "exited") continue;
      if (session.remoteConnectionState !== "reconnecting" && session.remoteConnectionState !== "disconnected") continue;
      const disposers = pairedDisposers.get(workspaceId) ?? [];
      pairedDisposers.set(workspaceId, disposers);
      disposers.push(options.refreshPaired(workspaceId, session.id, (ready) => {
        if (stopped || disposedWorkspaces.has(workspaceId) || options.isMounted(workspaceId)) return;
        const current = options.getState(workspaceId);
        if (!current?.sessions[session.id]) return;
        options.dispatch(workspaceId, {
          type: "RESTORE_WORKSPACE",
          state: { ...current, sessions: { ...current.sessions, [session.id]: ready } },
        });
      }));
    }
  }

  const unsubscribe = options.subscribe(() => {
    for (const [workspaceId, disposers] of pairedDisposers) {
      if (!options.isMounted(workspaceId)) continue;
      disposedWorkspaces.add(workspaceId);
      pairedDisposers.delete(workspaceId);
      for (const dispose of disposers) dispose();
    }
    for (const [workspaceId, stop] of sshStops) {
      if (!options.isMounted(workspaceId)) continue;
      stop();
      sshStops.delete(workspaceId);
    }
  });

  return () => {
    if (stopped) return;
    stopped = true;
    unsubscribe();
    for (const disposers of pairedDisposers.values()) {
      for (const dispose of disposers) dispose();
    }
    pairedDisposers.clear();
    for (const stop of sshStops.values()) stop();
    sshStops.clear();
  };
}
