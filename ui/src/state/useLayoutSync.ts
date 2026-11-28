import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";

import {
  createPairedDaemonProjectAdapter,
  PairedOperationError,
  type PairedDaemonProjectAdapter,
} from "../lib/pairedDaemonProject";
import { layoutGet, layoutPut } from "../lib/tauri";
import type { RegisteredProject, TerminalSession } from "../lib/types";
import { isSameWorktreePath } from "../components/sidebar/sidebarSupervisor";
import {
  applySharedLayout,
  createLayoutSyncLoop,
  toRevision,
  toSharedLayout,
  wireWorktreeKey,
  worktreeLayoutKey,
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
    if (session.backendSessionId === backendSessionId) return session.id;
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

export function layoutSyncHostTransport(workspaceId: string, worktreeKey?: string): LayoutSyncTransport {
  return {
    get: async (afterRevision) => {
      const doc = await layoutGet(workspaceId, afterRevision, worktreeKey);
      if (doc.workspaceId !== workspaceId) return null;
      // A legacy host answers the workspace-level document; anything else must be our bucket.
      if (worktreeKey !== undefined && doc.worktreeKey !== undefined && doc.worktreeKey !== worktreeKey) return null;
      return doc;
    },
    put: (baseRevision, tabs) => layoutPut(workspaceId, baseRevision, tabs, worktreeKey),
  };
}

function pairedTransport(
  adapter: PairedDaemonProjectAdapter,
  remoteWorkspaceId: string,
  worktreeKey?: string,
): LayoutSyncTransport {
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
        worktreeKey,
      );
      return withNumberRevision(layout);
    },
    put: async (baseRevision, tabs) => {
      const result = await adapter.layoutPut(remoteWorkspaceId, String(baseRevision), tabs, worktreeKey);
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

function backendIdsMatch(
  sessions: WorkspaceState["sessions"],
  prev: ReadonlyMap<string, string | null>,
): boolean {
  let count = 0;
  for (const id in sessions) {
    if (!Object.prototype.hasOwnProperty.call(sessions, id)) continue;
    count += 1;
    if (!prev.has(id) || prev.get(id) !== (sessions[id]?.backendSessionId ?? null)) {
      return false;
    }
  }
  return count === prev.size;
}

function recordBackendIds(
  sessions: WorkspaceState["sessions"],
  target: Map<string, string | null>,
): void {
  target.clear();
  for (const id in sessions) {
    if (!Object.prototype.hasOwnProperty.call(sessions, id)) continue;
    target.set(id, sessions[id]?.backendSessionId ?? null);
  }
}

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
  const prevLayoutRef = useRef<WorkspaceState["layout"] | null>(null);
  const prevBackendIdsRef = useRef<Map<string, string | null>>(new Map());
  const loopRef = useRef<ReturnType<typeof createLayoutSyncLoop> | null>(null);

  const paired = project.target?.kind === "pairedDaemon" ? project : null;
  const isPaired = paired !== null;
  const remoteWorkspaceId = paired?.remoteWorkspaceId;
  const workspaceId = project.workspaceId;
  // The document is scoped to one worktree: a managed worktree travels as its identity slug,
  // the main checkout as `main`, an unmanaged external one as a digest. A raw path never does.
  const activeWorktree = state.worktrees.find((worktree) => isSameWorktreePath(worktree.path, state.activeWorktreePath ?? ""));
  const wireKey = wireWorktreeKey(activeWorktree, project.repoRoot);
  const localWorktreePath = state.activeWorktreePath ?? activeWorktree?.path ?? null;
  const wireKeyRef = useRef<string | undefined>(wireKey);
  wireKeyRef.current = wireKey;

  useEffect(() => {
    if (!enabled) return;
    if (isPaired && (!adapter || !remoteWorkspaceId)) return;
    const transport = adapter && remoteWorkspaceId && isPaired
      ? pairedTransport(adapter, remoteWorkspaceId, wireKey)
      : layoutSyncHostTransport(workspaceId, wireKey);
    const readShared = (layout: WorkspaceState["layout"], sessions: WorkspaceState["sessions"]): SharedTab[] =>
      toSharedLayout(layout, sessions, undefined, project.repoRoot);
    // Every async continuation below runs after this effect may have been torn down by a
    // workspace switch, a worktree switch or unmount; none of them may dispatch then.
    let cancelled = false;
    const stillCurrent = (): boolean => {
      if (cancelled) return false;
      if (stateRef.current.workspaceId !== undefined && stateRef.current.workspaceId !== workspaceId) return false;
      return wireKeyRef.current === wireKey;
    };

    /**
     * Fills in the host-side identity of panes that appeared from a document. The layout only
     * carries the backend session id, while attaching needs the daemon epoch and the working
     * directory, and both live on the host's session list.
     *
     * `synced` is the state the minting restore dispatched. React may not have committed it yet
     * when the host answers, so `stateRef.current` can still be one state behind and know nothing
     * about these sessions. Enrichment then builds on that dispatched state, so it patches the
     * records the layout actually references and never rewrites the pre-sync layout.
     */
    const enrichDetached = (minted: Map<string, TerminalSession>, synced: WorkspaceState) => {
      if (!adapter || !remoteWorkspaceId || minted.size === 0) return;
      void adapter.sessions(remoteWorkspaceId).then((listed) => {
        if (cancelled) return;
        const rows = new Map(listed.sessions.map((row) => [row.target.sessionId, row]));
        const live = stateRef.current;
        const committed = [...minted.keys()].every((id) => live.sessions[id] !== undefined);
        const base: WorkspaceState = committed
          ? live
          : { ...live, layout: synced.layout, sessions: { ...synced.sessions, ...live.sessions }, worktreeLayouts: synced.worktreeLayouts };
        const patched: Record<string, TerminalSession> = {};
        for (const [id, mintedSession] of minted) {
          const session = base.sessions[id] ?? mintedSession;
          const row = session.backendSessionId ? rows.get(session.backendSessionId) : undefined;
          if (!row) continue;
          const epoch = /^(0|[1-9][0-9]*)$/.test(row.target.daemonEpoch) ? row.target.daemonEpoch : null;
          patched[id] = {
            ...session,
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
        if (Object.keys(patched).length === 0) return;
        if (!stillCurrent()) return;
        dispatchRef.current({
          type: "RESTORE_WORKSPACE",
          state: { ...base, sessions: { ...base.sessions, ...patched } },
        });
      }).catch((error) => {
        if (cancelled) return;
        errorRef.current?.(error);
      });
    };

    const applyRemote = (doc: SharedLayout) => {
      if (cancelled) return;
      const current = stateRef.current;
      if (current.workspaceId !== undefined && current.workspaceId !== workspaceId) return;
      if (wireKeyRef.current !== wireKey) return;
      const minted = new Map<string, TerminalSession>();
      const resolveSession = (backendSessionId: string) => {
        const known = findFrontendSessionId(current.sessions, backendSessionId);
        if (known) return known;
        const session = detachedSession(workspaceId, backendSessionId);
        minted.set(session.id, session);
        return session.id;
      };
      const { layout } = applySharedLayout(current.layout, doc, resolveSession, { workspaceRoot: project.repoRoot });
      const sessions = { ...current.sessions, ...Object.fromEntries(minted) };
      // The document is bucketed per worktree, so the parked copy of this worktree keeps the
      // synced layout too: switching away and back must not fall back to a stale tab set.
      const worktreeLayouts = localWorktreePath
        ? { ...(current.worktreeLayouts ?? {}), [worktreeLayoutKey(current.worktreeLayouts, localWorktreePath)]: layout }
        : current.worktreeLayouts;
      // The baseline is what this layout publishes, so applying a document can never replay as
      // a local change (a server-side ratio clamp included) and echo straight back out.
      const applied = JSON.stringify(readShared(layout, sessions));
      const unchanged = applied === snapshotRef.current;
      snapshotRef.current = applied;
      // A document that already matches the local layout is not a change: dispatching it would
      // replace every tab object on each poll.
      if (unchanged && minted.size === 0) return;
      const next: WorkspaceState = { ...current, layout, sessions, worktreeLayouts };
      prevLayoutRef.current = layout;
      recordBackendIds(sessions, prevBackendIdsRef.current);
      dispatchRef.current({ type: "RESTORE_WORKSPACE", state: next });
      if (minted.size > 0) enrichDetached(minted, next);
    };

    const loop = createLayoutSyncLoop({
      transport,
      isPaired,
      readLocal: () => readShared(stateRef.current.layout, stateRef.current.sessions),
      applyRemote,
      onError: (error) => {
        if (!cancelled) errorRef.current?.(error);
      },
      debounceMs,
    });
    loopRef.current = loop;
    snapshotRef.current = JSON.stringify(readShared(stateRef.current.layout, stateRef.current.sessions));
    prevLayoutRef.current = stateRef.current.layout;
    recordBackendIds(stateRef.current.sessions, prevBackendIdsRef.current);
    void loop.start();
    return () => {
      cancelled = true;
      loopRef.current = null;
      prevLayoutRef.current = null;
      prevBackendIdsRef.current.clear();
      loop.stop();
    };
  }, [adapter, debounceMs, enabled, isPaired, project.repoRoot, remoteWorkspaceId, wireKey, workspaceId, localWorktreePath]);

  useEffect(() => {
    const loop = loopRef.current;
    if (!loop) return;
    if (
      state.layout === prevLayoutRef.current &&
      backendIdsMatch(state.sessions, prevBackendIdsRef.current)
    ) {
      return;
    }
    prevLayoutRef.current = state.layout;
    recordBackendIds(state.sessions, prevBackendIdsRef.current);
    const shared: SharedTab[] = toSharedLayout(state.layout, state.sessions, undefined, project.repoRoot);
    const key = JSON.stringify(shared);
    if (key === snapshotRef.current) return;
    snapshotRef.current = key;
    loop.localChanged();
  }, [state.layout, state.sessions, project.repoRoot]);
}
