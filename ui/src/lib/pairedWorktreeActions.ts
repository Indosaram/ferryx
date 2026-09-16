import { createPairedDaemonProjectAdapter, PairedOperationError, type PairedDaemonProjectAdapter } from "./pairedDaemonProject";
import { remoteHostStore } from "../state/remoteHostStore";
import { safeRandomUUID } from "./uuid";
import { worktreeIdentity, type RegisteredProject, type Worktree } from "./types";

export function pairedActionMessage(cause: unknown): string {
  if (!(cause instanceof PairedOperationError)) return cause instanceof Error ? cause.message : "The paired operation failed.";
  const messages: Record<string, string> = {
    PAIRED_HOST_UNAVAILABLE: "The owning machine is offline. No Local or SSH fallback was attempted.",
    STALE_HOST_GENERATION: "The machine pairing changed. Reopen this dialog to use the current pairing.",
    REVISION_CONFLICT: "The remote state changed. Review a fresh deletion preview before deleting.",
    WORKSPACE_BUSY: "The project still has live sessions. Close them explicitly on the owning machine first.",
    DIRTY_WORKTREE: "The remote worktree has uncommitted changes. Safe deletion was refused.",
    UNMERGED_BRANCH: "The remote branch is unmerged. Safe deletion was refused.",
    UNSUPPORTED_CAPABILITY: "This action is not supported by the owning machine.",
  };
  const message = messages[cause.code] ?? `The owning machine reported ${cause.code}.`;
  return cause.ambiguous ? `${message} Outcome unresolved; reconcile request ${cause.requestId ?? "unknown"}. Do not resend the mutation.` : message;
}

/** Captures ownership at dialog open; switching the selected machine cannot reroute a mutation. */
export function createPairedWorktreeActions(project: RegisteredProject, supplied?: PairedDaemonProjectAdapter) {
  if (project.target?.kind !== "pairedDaemon" || !project.remoteWorkspaceId) throw new PairedOperationError("PAIRED_OWNER_REQUIRED");
  const host = remoteHostStore.getState().hosts[project.target.hostId];
  const adapter = supplied ?? (host?.generation ? createPairedDaemonProjectAdapter({ hostId: project.target.hostId, generation: host.generation }) : null);
  const workspaceId = project.remoteWorkspaceId;
  let pending: { requestId: string; kind: "create" | "delete" } | null = null;
  let revision: string | null = null;
  async function ready() {
    if (!adapter) throw new PairedOperationError("PAIRED_HOST_UNAVAILABLE");
    await adapter.capabilities();
    return adapter;
  }
  function owned(row: { workspaceId: string } & Omit<Worktree, "workspaceId">): Worktree {
    if (row.workspaceId !== workspaceId) throw new PairedOperationError("CROSS_HOST_RESULT");
    return { ...row, workspaceId: project.workspaceId };
  }
  async function reconcile(a: PairedDaemonProjectAdapter, kind: "create" | "delete"): Promise<Worktree | null> {
    if (!pending || pending.kind !== kind) throw new PairedOperationError("MUTATION_PENDING");
    const result = await a.operation(pending.requestId);
    if (result.state !== "completed") throw new PairedOperationError("MUTATION_PENDING", false, pending.requestId, {}, true);
    if (result.outcome.kind === "error" && result.outcome.error) {
      pending = null;
      throw new PairedOperationError(result.outcome.error.code);
    }
    if (kind === "create" && result.outcome.kind === "worktree" && result.outcome.worktree) {
      const row = owned(result.outcome.worktree);
      pending = null;
      return row;
    }
    if (kind === "delete" && result.outcome.kind === "noContent") { pending = null; return null; }
    throw new PairedOperationError("INVALID_RESPONSE");
  }
  return {
    async create(slug: string): Promise<Worktree> {
      const a = await ready();
      if (pending) return (await reconcile(a, "create"))!;
      const requestId = safeRandomUUID();
      try {
        return owned(await a.createWorktree({ requestId, workspaceId, worktree: { wsId: workspaceId, slug } }));
      } catch (cause) {
        if (cause instanceof PairedOperationError && cause.ambiguous) pending = { requestId, kind: "create" };
        throw cause;
      }
    },
    async previewDelete(worktree: Worktree) {
      const identity = worktreeIdentity(worktree);
      const branchParts = worktree.branch?.replace(/^refs\/heads\//, "").split("/");
      const branchWsId = branchParts && branchParts[0] === "orca" ? branchParts[1] : null;
      if (!identity || identity.wsId !== workspaceId || (branchWsId && branchWsId !== workspaceId)) throw new PairedOperationError("INVALID_NAMESPACE");
      const a = await ready();
      const status = await a.worktreeStatus(workspaceId, identity);
      if (status.workspaceId !== workspaceId || status.worktree.wsId !== identity.wsId || status.worktree.slug !== identity.slug) throw new PairedOperationError("CROSS_HOST_RESULT");
      if (!status.branchDeletion) throw new PairedOperationError("INVALID_NAMESPACE");
      revision = status.revision;
      return status.branchDeletion;
    },
    async deleteSafe(worktree: Worktree) {
      const a = await ready();
      if (pending) { await reconcile(a, "delete"); return; }
      const identity = worktreeIdentity(worktree);
      const branchParts = worktree.branch?.replace(/^refs\/heads\//, "").split("/");
      const branchWsId = branchParts && branchParts[0] === "orca" ? branchParts[1] : null;
      if (!identity || identity.wsId !== workspaceId || (branchWsId && branchWsId !== workspaceId) || revision === null) throw new PairedOperationError("INVALID_NAMESPACE");
      const requestId = safeRandomUUID();
      try {
        await a.deleteWorktree({ requestId, workspaceId, worktree: identity, expectedRevision: revision, deleteBranch: true });
      } catch (cause) {
        if (cause instanceof PairedOperationError && cause.ambiguous) pending = { requestId, kind: "delete" };
        throw cause;
      }
    },
    async deleteDestructive(): Promise<void> { throw new PairedOperationError("UNSUPPORTED_CAPABILITY"); },
  };
}
