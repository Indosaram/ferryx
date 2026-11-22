import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";

import {
  createPairedDaemonProjectAdapter,
  PairedOperationError,
  type PairedDaemonProjectAdapter,
} from "../lib/pairedDaemonProject";
import { layoutGet, layoutPut, pairedTerminalReattach } from "../lib/tauri";
import type { RegisteredProject, TerminalSession } from "../lib/types";
import {
  applySharedLayout,
  createLayoutSyncLoop,
  toRevision,
  toSharedLayout,
  type LayoutSyncTransport,
  type SharedLayout,
  type SharedTab,
} from "./layoutSync";
import { remoteHostStore } from "./remoteHostStore";
import type { WorkspaceAction, WorkspaceState } from "./workspaceStore";

/**
 * Frontend session minted for a remote leaf this window has never shown. It carries only the
 * host backend identity, so the ordinary attach/retry admission adopts it - the pane is never
 * created by the sync path and no host PTY is ever spawned from it.
 */
export function layoutSyncSessionId(backendSessionId: string): string {
  return `layout-sync:${backendSessionId}`;
}

function findFrontendSessionId(sessions: WorkspaceState["sessions"], backendSessionId: string): string | null {
  for (const session of Object.values(sessions)) {
    if (session.backendSessionId === backendSessionId || session.id === layoutSyncSessionId(backendSessionId)) return session.id;
  }
  return null;
}

/**
 * A pane for a leaf this window has never shown. Only the host identity is known here; the
 * host's session list fills in the rest (epoch, cwd, worktree) so the ordinary attach path can
 * admit it, and nothing is spawned: an unattachable leaf resolves to a pane that says so.
 */
function detachedSession(workspaceId: string, backendSessionId: string): TerminalSession {
  return {
    id: layoutSyncSessionId(backendSessionId),
    cwd: "",
    workspaceId,
    worktree: null,
    backendSessionId,
    lifecycle: "starting",
    processState: "standby",
  };
}

export function layoutSyncHostTransport(workspaceId: string): LayoutSyncTransport {
  return {
    get: async (afterRevision) => {
      const doc = await layoutGet(workspaceId, afterRevision);
      return doc.workspaceId === workspaceId ? doc : null;
    },
    put: (baseRevision, tabs) => layoutPut(workspaceId, baseRevision, tabs),
  };
}

function pairedTransport(adapter: PairedDaemonProjectAdapter, remoteWorkspaceId: string): LayoutSyncTransport {
  const withNumberRevision = (layout: SharedLayout): SharedLayout => {
    const revision = toRevision(layout.revision);
    if (revision === null) throw new PairedOperationError("INVALID_RESPONSE");
    return { ...layout, revision };
  };
  return {
    get: async (afterRevision) => {
      const { layout } = await adapter.layoutGet(
        remoteWorkspaceId,
        afterRevision === null ? undefined : String(afterRevision),
      );
      return withNumberRevision(layout);
    },
    put: async (baseRevision, tabs) => {
      const result = await adapter.layoutPut(remoteWorkspaceId, String(baseRevision), tabs);
      return { layout: withNumberRevision(result.layout), conflict: result.conflict };
    },
  };
}

/**
 * The paired adapter, pinned to the pairing generation this render saw. A new adapter means a
 * new generation, and the loop must restart because every operation is gated on it.
 */
export function usePairedLayoutAdapter(project: RegisteredProject): PairedDaemonProjectAdapter | null {
  const hosts = useSyncExternalStore(remoteHostStore.subscribe, () => remoteHostStore.getState().hosts);
  const target = project.target?.kind === "pairedDaemon" ? project.target : null;
  const hostId = target?.hostId;
  const generation = hostId ? hosts[hostId]?.generation : undefined;
  return useMemo(
    () => (hostId && generation ? createPairedDaemonProjectAdapter({ hostId, generation }) : null),
    [hostId, generation],
  );
}

export type UseLayoutSyncOptions = {
  project: RegisteredProject;
  state: WorkspaceState;
  dispatch: (action: WorkspaceAction) => void;
  /** Registration and restore must have completed: sync starts from the first real layout. */
  enabled: boolean;
  adapter?: PairedDaemonProjectAdapter | null;
  onError?: (error: unknown) => void;
  debounceMs?: number;
};

/**
 * Keeps one workspace's layout document in sync: the host publishes its layout after a local
 * change and long-polls for clients; a paired client long-polls and publishes the same way.
 * A document only reshapes the local view: a pane that disappears is detached here, and its
 * session record survives so the host PTY is never closed by a remote change.
 */
export function useLayoutSync({
  project,
  state,
  dispatch,
  enabled,
  adapter,
  onError,
  debounceMs = 300,
}: UseLayoutSyncOptions): void {
  const stateRef = useRef(state);
  stateRef.current = state;
  const dispatchRef = useRef(dispatch);
  dispatchRef.current = dispatch;
  const errorRef = useRef(onError);
  errorRef.current = onError;
  const snapshotRef = useRef<string>("");
  const loopRef = useRef<ReturnType<typeof createLayoutSyncLoop> | null>(null);

  const paired = project.target?.kind === "pairedDaemon" ? project : null;
  const isPaired = paired !== null;
  const remoteWorkspaceId = paired?.remoteWorkspaceId;
  const workspaceId = project.workspaceId;
  // The document always carries host ids, not the local proxy ids adopted below.
  const sharedTabs = (current: WorkspaceState) => toSharedLayout(current.layout, current.sessions,
    isPaired ? (id) => id.startsWith("layout-sync:") ? id.slice("layout-sync:".length) : null : undefined);

  useEffect(() => {
    if (!enabled) return;
    if (isPaired && (!adapter || !remoteWorkspaceId)) return;
    const transport = adapter && remoteWorkspaceId && isPaired
      ? pairedTransport(adapter, remoteWorkspaceId)
      : layoutSyncHostTransport(workspaceId);

    /**
     * Fills in the host-side identity of panes that appeared from a document. The layout only
     * carries the backend session id, while attaching needs the daemon epoch and the working
     * directory, and both live on the host's session list.
     */
    let live = true;
    const enrichDetached = (mintedIds: string[]) => {
      if (!adapter || !remoteWorkspaceId || mintedIds.length === 0) return;
      void adapter.sessions(remoteWorkspaceId).then(async (listed) => {
        const rows = new Map(listed.sessions.map((row) => [row.target.sessionId, row]));
        const patched: Record<string, TerminalSession> = {};
        for (const id of mintedIds) {
          const session = stateRef.current.sessions[id];
          const row = session?.backendSessionId ? rows.get(session.backendSessionId) : undefined;
          if (!session || !row) continue;
          const backendSessionId = await pairedTerminalReattach({ ...adapter.context, target: row.target });
          if (!live) return;
          const retained = stateRef.current.sessions[id];
          if (!retained || retained.backendSessionId !== session.backendSessionId) continue;
          const epoch = /^(0|[1-9][0-9]*)$/.test(row.target.daemonEpoch) ? row.target.daemonEpoch : null;
          patched[id] = {
            ...retained,
            backendSessionId,
            cwd: row.cwd || session.cwd,
            worktree: row.worktree ?? session.worktree,
            ...(epoch ? { daemonEpoch: epoch } : {}),
            ...(row.providerSession ? { providerSession: row.providerSession } : {}),
            ...(row.agentType ? { agentType: row.agentType } : {}),
            lifecycle: row.running ? "working" : "exited",
            processState: row.running ? "running" : "standby",
            remoteConnectionState: "disconnected",
          };
        }
        if (!live || Object.keys(patched).length === 0) return;
        const current = stateRef.current;
        dispatchRef.current({
          type: "RESTORE_WORKSPACE",
          state: { ...current, sessions: { ...current.sessions, ...patched } },
        });
      }).catch((error) => errorRef.current?.(error));
    };

    const applyRemote = (doc: SharedLayout) => {
      const current = stateRef.current;
      const minted = new Map<string, TerminalSession>();
      const resolveSession = (backendSessionId: string) => {
        const known = findFrontendSessionId(current.sessions, backendSessionId);
        if (known) return known;
        const session = detachedSession(workspaceId, backendSessionId);
        minted.set(session.id, session);
        return session.id;
      };
      const { layout } = applySharedLayout(current.layout, doc, resolveSession);
      const sessions = { ...current.sessions, ...Object.fromEntries(minted) };
      // The baseline is what this layout publishes, so applying a document can never replay as
      // a local change (a server-side ratio clamp included) and echo straight back out.
      const applied = JSON.stringify(sharedTabs({ ...current, layout, sessions }));
      const unchanged = applied === snapshotRef.current;
      snapshotRef.current = applied;
      // A document that already matches the local layout is not a change: dispatching it would
      // replace every tab object on each poll.
      if (unchanged && minted.size === 0) return;
      dispatchRef.current({ type: "RESTORE_WORKSPACE", state: { ...current, layout, sessions } });
      if (minted.size > 0) enrichDetached([...minted.keys()]);
    };

    const loop = createLayoutSyncLoop({
      transport,
      isPaired,
      readLocal: () => sharedTabs(stateRef.current),
      applyRemote,
      onError: (error) => errorRef.current?.(error),
      debounceMs,
    });
    loopRef.current = loop;
    snapshotRef.current = JSON.stringify(sharedTabs(stateRef.current));
    void loop.start();
    return () => {
      live = false;
      loopRef.current = null;
      loop.stop();
    };
  }, [adapter, debounceMs, enabled, isPaired, remoteWorkspaceId, workspaceId]);

  useEffect(() => {
    const loop = loopRef.current;
    if (!loop) return;
    const shared: SharedTab[] = sharedTabs(state);
    const key = JSON.stringify(shared);
    if (key === snapshotRef.current) return;
    snapshotRef.current = key;
    loop.localChanged();
  }, [state.layout, state.sessions]);
}
