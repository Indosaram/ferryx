import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useReducer, useRef } from "react";

import type { SidebarModel } from "../components/sidebar/sidebarModel";
import { emptyProjectGroupIds, initialCollapsedProjects } from "../components/sidebar/sidebarRows";
import { sidebarSessionSignals } from "../components/sidebar/sidebarSessionSignals";
import { agentStatesByPath, useSidebarModel } from "../components/sidebar/useSidebarModel";
import { groupProjects } from "./projectGrouping";
import { loadCollapsedProjects, loadWorktreeOrder, subscribeSidebarPersistence } from "./sidebarPersistence";
import { syncDesktopSidebar, type RegisteredProject } from "./tauri";
import type { ActiveAgent, Worktree } from "./types";
import type { ActivitySummary } from "./activity";
import type { PublishedAttentionRow } from "../components/sidebar/sidebarSessionSignals";

/** The daemon owns the revision and the clock only stamps the snapshot, so neither decides "changed". */
function modelContentKey(model: SidebarModel) {
  return JSON.stringify({ ...model, revision: 0, generatedAt: "" });
}

/**
 * Sends each distinct sidebar model once with at most one call in flight; models queued behind a
 * running call collapse to the latest. `resend` forces the next queued publish, even if an older
 * in-flight call succeeds with unchanged content after the resend request.
 */
export function createDesktopSidebarSync() {
  let lastSentKey: string | null = null;
  let pending: SidebarModel | null = null;
  let forceNext = false;
  let running: Promise<void> | null = null;
  return (model: SidebarModel, resend = false): Promise<void> => {
    if (resend) forceNext = true;
    pending = model;
    if (running) return running;
    running = (async () => {
      try {
        while (pending !== null) {
          const snapshot = pending;
          pending = null;
          const force = forceNext;
          forceNext = false;
          const key = modelContentKey(snapshot);
          if (!force && key === lastSentKey) continue;
          try {
            await syncDesktopSidebar({ ...snapshot, generatedAt: new Date().toISOString() });
            lastSentKey = key;
          } catch (error) {
            if (pending === null) throw error;
          }
        }
      } finally {
        running = null;
      }
    })();
    return running;
  };
}

const sendDesktopSidebar = createDesktopSidebarSync();

function publish(model: SidebarModel, resend = false) {
  void sendDesktopSidebar(model, resend).catch((error) => {
    console.error("Desktop sidebar publish failed:", error);
  });
}

export type DesktopSidebarInput = {
  readonly enabled: boolean;
  readonly projects: RegisteredProject[];
  readonly activeProjectId: string;
  readonly worktrees: readonly Worktree[];
  readonly inactiveProjectWorktrees: Readonly<Record<string, Worktree[]>>;
  readonly emptyWorkspaceIds: readonly string[];
  /** The live-only signals the Sidebar receives; published so the remote shows the same rows. */
  readonly activityByWorktreePath: Readonly<Record<string, ActivitySummary | undefined>>;
  readonly unreadWorktreePaths: Readonly<Record<string, boolean>>;
  readonly agents: readonly ActiveAgent[];
  /** The desktop Inbox's rows, published so the remote's Inbox lists and counts the same rows. */
  readonly attentionRows: readonly PublishedAttentionRow[];
};

/** Publishes the model the desktop sidebar renders, from the same builder hook the Sidebar calls. */
export function useDesktopSidebarPublisher(input: DesktopSidebarInput) {
  const { enabled, projects, activeProjectId, worktrees, inactiveProjectWorktrees } = input;
  const [persistenceVersion, bumpPersistence] = useReducer((version: number) => version + 1, 0);
  useEffect(() => {
    const unsubscribe = subscribeSidebarPersistence(bumpPersistence);
    window.addEventListener("storage", bumpPersistence);
    return () => {
      unsubscribe();
      window.removeEventListener("storage", bumpPersistence);
    };
  }, []);
  // The caller derives this list on every render; key on its content so identical lists do not rebuild.
  const emptyKey = input.emptyWorkspaceIds.join("\n");
  // The Sidebar holds these in React state and persists every change; read them back from storage.
  const persisted = useMemo(() => {
    const emptyGroupIds = emptyProjectGroupIds(groupProjects(projects), emptyKey === "" ? [] : emptyKey.split("\n"));
    return {
      worktreeOrder: loadWorktreeOrder(),
      collapsedGroupIds: initialCollapsedProjects({ stored: loadCollapsedProjects(), projects, activeProjectId, emptyGroupIds }),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- persistenceVersion invalidates the storage reads inside
  }, [projects, activeProjectId, emptyKey, persistenceVersion]);
  const built = useSidebarModel({ projects, activeProjectId, worktrees, inactiveProjectWorktrees }, persisted).model;
  const { activityByWorktreePath, unreadWorktreePaths, agents, attentionRows } = input;
  const model = useMemo(() => enabled
    ? {
        ...built,
        signals: sidebarSessionSignals(built, {
          activityByWorktreePath,
          unreadWorktreePaths,
          agentStateByPath: agentStatesByPath(agents.map((agent) => [agent.worktreePath, agent] as const)),
          attentionRows,
        }),
      }
    : null, [activityByWorktreePath, agents, attentionRows, built, enabled, unreadWorktreePaths]);

  const modelRef = useRef(model);
  modelRef.current = model;
  useEffect(() => {
    if (model) publish(model);
  }, [model]);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    // The bridge emits this whenever the daemon event stream (re)connects; the daemon may have lost the snapshot.
    void listen("desktop_local_projects_resync", () => {
      if (modelRef.current) publish(modelRef.current, true);
    }).then((stop) => {
      if (disposed) stop(); else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enabled]);
}
