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
}
