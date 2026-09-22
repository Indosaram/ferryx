import { createPairedDaemonProjectAdapter, PairedOperationError } from "../lib/pairedDaemonProject";
import type { RegisteredProject, Worktree } from "../lib/types";
import { remoteHostStore } from "./remoteHostStore";

export type PairedWorktreeFreshness = {
  stale: boolean;
  offline: boolean;
  generation?: string | null;
  cachedAt?: number;
};

/** A missing/partial project inventory is not permission to erase cached rows. */
export async function listPairedProjectWorktrees(project: RegisteredProject): Promise<Worktree[] | null> {
  if (project.target?.kind !== "pairedDaemon" || !project.remoteWorkspaceId) throw new Error("PAIRED_OWNER_REQUIRED");
  const state = remoteHostStore.getState();
  const host = state.hosts[project.target.hostId];
  if (state.machineFeaturesEnabled !== true || state.nativeStatus !== "ready" || !host?.online ||
      host.authStatus !== "paired" || host.grantScope !== "machine" || !host.generation) return null;
  const adapter = createPairedDaemonProjectAdapter({ hostId: host.hostId, generation: host.generation });
  // The relay tunnel loses ~1 in 3 body-bearing responses right after a daemon
  // handover (measured 5 OK / 8 FAIL in 13 probes). Backend spawn carries a
  // bounded resubmit for this exact class; the listing path must too, or one
  // lost response disables every sidebar row for the TTL.
  const retryable = (error: unknown): boolean =>
    error instanceof PairedOperationError &&
    ["PAIRED_HOST_INVALID_RESPONSE", "TIMEOUT", "PAIRED_HOST_UNAVAILABLE"].includes(error.code);
  let attempts = 0;
  for (;;) {
    try {
      await adapter.capabilities();
      const inventory = await adapter.projects();
      const metadata = inventory.projects.find(row => row.workspaceId === project.workspaceId);
      if (inventory.unavailableWorkspaceIds.includes(project.workspaceId) || !metadata || metadata.availability !== "ready") return null;
      const result = await adapter.worktrees(project.remoteWorkspaceId);
      if (remoteHostStore.getState().hosts[host.hostId]?.generation !== host.generation) return null;
      return result.worktrees.map(row => {
        if (row.workspaceId !== project.remoteWorkspaceId) throw new PairedOperationError("CROSS_HOST_RESULT");
        return { ...row, workspaceId: project.workspaceId };
      });
    } catch (error) {
      attempts += 1;
      if (attempts > 2 || !retryable(error)) throw error;
      await new Promise(resolve => setTimeout(resolve, 300 * attempts));
    }
  }
}
