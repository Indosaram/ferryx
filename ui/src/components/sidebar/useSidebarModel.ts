// The one place the desktop calls buildSidebarModel. The Sidebar renders this hook's model and
// desktopSidebarSync publishes this hook's model, so a builder input added for one reaches the
// other. Only the persisted state (order, collapsed set) differs in where it is read from: the
// Sidebar holds it in React state, the publisher reads storage because the Sidebar may be closed.
import { useEffect, useMemo, useReducer, useSyncExternalStore } from "react";

import { useAdvancedFeatures } from "../../lib/advancedFeatures";
import { getPairedHostReachability, subscribePairedHostReachability } from "../../lib/pairedHostReachability";
import type { SidebarLaneLifecycle, WorktreeOrder } from "../../lib/sidebarPersistence";
import { useSshHosts } from "../../lib/sshHosts";
import type { RegisteredProject } from "../../lib/tauri";
import type { ActiveAgent, AgentState, Worktree } from "../../lib/types";
import { remoteHostStore } from "../../state/remoteHostStore";
import { worktreeListingFailureStore } from "../../state/worktreeListingFailures";
import { buildSidebarModel } from "./sidebarModel";

export function agentStatesByPath(agents: Iterable<readonly [string, Pick<ActiveAgent, "state">]>): Record<string, AgentState> {
  return Object.fromEntries([...agents].map(([path, agent]) => [path, agent.state]));
}

export type DesktopSidebarSource = {
  readonly projects: RegisteredProject[];
  readonly activeProjectId: string | undefined;
  readonly worktrees: readonly Worktree[];
  readonly inactiveProjectWorktrees: Readonly<Record<string, Worktree[]>> | undefined;
};

export type DesktopSidebarPersisted = {
  /** The pre-import `localStorage` order; ignored for any workspace the server orders. */
  readonly worktreeOrder: WorktreeOrder;
  readonly collapsedGroupIds: ReadonlySet<string>;
  /**
   * The server-owned section order (`workspaceId -> lifecycle keys`). The sidebar reads its
   * order from here, so a drag outlives this window. Absent before the first server read (or
   * without a hub), which keeps the local order as the fallback.
   */
  readonly laneOrder?: Readonly<Record<string, readonly string[]>>;
  /** Per-row server state and folded pin set, keyed by lifecycle key. */
  readonly laneLifecycles?: Readonly<Record<string, SidebarLaneLifecycle>>;
};

/** Rows input (for drag-and-drop grouping) and the model, from the live desktop stores. */
export function useSidebarModel(source: DesktopSidebarSource, persisted: DesktopSidebarPersisted) {
  const { projects, activeProjectId, worktrees, inactiveProjectWorktrees } = source;
  const { hosts } = useSshHosts();
  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const listingFailures = useSyncExternalStore(worktreeListingFailureStore.subscribe, worktreeListingFailureStore.getState);
  const advancedFeatures = useAdvancedFeatures();
  // Paired rows read reachability non-reactively, so rebuild whenever it changes.
  const [reachabilityVersion, bumpReachability] = useReducer((version: number) => version + 1, 0);
  useEffect(() => {
    const unsubscribe = subscribePairedHostReachability(bumpReachability);
    return () => { unsubscribe(); };
  }, []);
  const rowsInput = useMemo(
    () => ({ worktrees, projects, activeProjectId, inactiveProjectWorktrees, hosts, remoteState, reachability: getPairedHostReachability }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reachabilityVersion invalidates the store read inside
    [activeProjectId, hosts, inactiveProjectWorktrees, projects, worktrees, remoteState, reachabilityVersion],
  );
  const { worktreeOrder, collapsedGroupIds, laneOrder, laneLifecycles } = persisted;
  const model = useMemo(() => buildSidebarModel({
    ...rowsInput,
    source: "desktop",
    generatedAt: "",
    worktreeOrder,
    ...(laneOrder ? { laneOrder } : {}),
    ...(laneLifecycles ? { laneLifecycles } : {}),
    collapsedGroupIds,
    advancedFeatures,
    listingFailures,
  }), [advancedFeatures, collapsedGroupIds, laneLifecycles, laneOrder, listingFailures, rowsInput, worktreeOrder]);
  return { model, rowsInput };
}
