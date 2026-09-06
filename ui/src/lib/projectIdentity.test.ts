import { describe, expect, it } from "vitest";
import { createLayoutState } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import { hasValidProjectTarget, projectRootWorktree } from "./projectIdentity";
import { serializeWorkspaceState, deserializeWorkspaceState } from "./sessionPersistence";
import { resolveWorktreeOwnerId } from "./worktreeOwnership";
import type { RegisteredProject } from "./types";

const remote: RegisteredProject = { workspaceId: "ssh:opaque", repoRoot: "/repo", gitRoot: "/repo", target: { kind: "ssh", hostId: "build" } };
const local: RegisteredProject = { workspaceId: "local", repoRoot: "/repo", gitRoot: null };
function state(project: RegisteredProject): WorkspaceState {
  return { workspaceId: project.workspaceId, worktrees: [projectRootWorktree(project)], activeWorktreePath: project.repoRoot, layout: createLayoutState(), sessions: {}, unreadTabIds: {}, unreadWorktreePaths: {} };
}

describe("project target identity", () => {
  it("keeps absent targets local but rejects missing or malformed targets for reserved remote identities", () => {
    expect(hasValidProjectTarget(local)).toBe(true);
    expect(hasValidProjectTarget(remote)).toBe(true);
    expect(hasValidProjectTarget({ workspaceId: "ssh:opaque" })).toBe(false);
    expect(hasValidProjectTarget({ workspaceId: "alias", target: { kind: "ssh" } })).toBe(false);
    expect(hasValidProjectTarget({ workspaceId: "alias", target: null })).toBe(false);
    expect(hasValidProjectTarget({ workspaceId: "ssh:opaque", target: { kind: "local" } })).toBe(false);
  });

  it("never deduces SSH ownership from a path shared with local or another host", () => {
    const otherHost: RegisteredProject = { ...remote, workspaceId: "ssh:other", target: { kind: "ssh", hostId: "other" } };
    const projects = [local, remote, otherHost];
    expect(resolveWorktreeOwnerId(projectRootWorktree(remote), projects)).toBe(remote.workspaceId);
    expect(resolveWorktreeOwnerId(projectRootWorktree(otherHost), projects)).toBe(otherHost.workspaceId);
    expect(resolveWorktreeOwnerId(projectRootWorktree(local), projects, remote.workspaceId)).toBe(local.workspaceId);
  });

  it("preserves remote metadata through session serialization, local merges, and restored root ownership", () => {
    const saved = serializeWorkspaceState(remote.workspaceId, remote.repoRoot, state(remote), null, remote);
    const localSaved = serializeWorkspaceState(local.workspaceId, local.repoRoot, state(local), saved, local);
    const resaved = serializeWorkspaceState(remote.workspaceId, remote.repoRoot, state(remote), localSaved);
    expect(resaved.workspaces[remote.workspaceId].target).toEqual(remote.target);
    expect(resaved.workspaces[remote.workspaceId].gitRoot).toBe(remote.gitRoot);
    expect(resaved.workspaces[local.workspaceId].target).toBeUndefined();
    const restored = deserializeWorkspaceState(remote.workspaceId, resaved);
    expect(restored?.worktrees[0]).toMatchObject({ workspaceId: remote.workspaceId, path: remote.repoRoot });
    const hmrSession = serializeWorkspaceState(remote.workspaceId, remote.repoRoot, state(remote));
    expect(deserializeWorkspaceState(remote.workspaceId, hmrSession)?.worktrees[0].workspaceId).toBe(remote.workspaceId);
  });
});
