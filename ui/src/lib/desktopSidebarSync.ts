import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useReducer, useRef, useSyncExternalStore } from "react";

import { buildSidebarModel, type SidebarModel } from "../components/sidebar/sidebarModel";
import { emptyProjectGroupIds, initialCollapsedProjects } from "../components/sidebar/sidebarRows";
import { remoteHostStore } from "../state/remoteHostStore";
import { worktreeListingFailureStore } from "../state/worktreeListingFailures";
import { useAdvancedFeatures } from "./advancedFeatures";
import { getPairedHostReachability, subscribePairedHostReachability } from "./pairedHostReachability";
import { groupProjects } from "./projectGrouping";
import { loadCollapsedProjects, loadWorktreeOrder, subscribeSidebarPersistence } from "./sidebarPersistence";
import { useSshHosts } from "./sshHosts";
import { syncDesktopSidebar, type RegisteredProject } from "./tauri";
import type { Worktree } from "./types";

/** The daemon owns the revision and the clock only stamps the snapshot, so neither decides "changed". */
function modelContentKey(model: SidebarModel) {
  return JSON.stringify({ ...model, revision: 0, generatedAt: "" });
}

/**
 * Sends each distinct sidebar model once with at most one call in flight; models queued behind a
 * running call collapse to the latest. `resend` republishes an unchanged model after a reconnect.
 */
export function createDesktopSidebarSync() {
  let lastSentKey: string | null = null;
  let pending: SidebarModel | null = null;
  let running: Promise<void> | null = null;
  return (model: SidebarModel, resend = false): Promise<void> => {
    if (resend) lastSentKey = null;
    pending = model;
    if (running) return running;
    running = (async () => {
      try {
        while (pending !== null) {
          const snapshot = pending;
          pending = null;
          const key = modelContentKey(snapshot);
          if (key === lastSentKey) continue;
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
};

/** Publishes the model the desktop sidebar renders, built from the same inputs the Sidebar receives. */
export function useDesktopSidebarPublisher(input: DesktopSidebarInput) {
  const { enabled, projects, activeProjectId, worktrees, inactiveProjectWorktrees } = input;
  const { hosts } = useSshHosts();
  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const listingFailures = useSyncExternalStore(worktreeListingFailureStore.subscribe, worktreeListingFailureStore.getState);
  const advancedFeatures = useAdvancedFeatures();
  const [reachabilityVersion, bumpReachability] = useReducer((version: number) => version + 1, 0);
  const [persistenceVersion, bumpPersistence] = useReducer((version: number) => version + 1, 0);
  useEffect(() => subscribePairedHostReachability(bumpReachability), []);
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

  const model = useMemo(() => {
    if (!enabled) return null;
    const emptyGroupIds = emptyProjectGroupIds(groupProjects(projects), emptyKey === "" ? [] : emptyKey.split("\n"));
    return buildSidebarModel({
      source: "desktop",
      generatedAt: "",
      projects,
      activeProjectId,
      worktrees,
      inactiveProjectWorktrees,
      hosts,
      remoteState,
      reachability: getPairedHostReachability,
      worktreeOrder: loadWorktreeOrder(),
      collapsedGroupIds: initialCollapsedProjects({ stored: loadCollapsedProjects(), projects, activeProjectId, emptyGroupIds }),
      advancedFeatures,
      listingFailures,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the versions invalidate non-reactive store reads inside
  }, [enabled, projects, activeProjectId, worktrees, inactiveProjectWorktrees, emptyKey, hosts, remoteState,
    listingFailures, advancedFeatures, reachabilityVersion, persistenceVersion]);

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
