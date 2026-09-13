import { expect, it, vi } from "vitest";
import { createPairedWorktreeActions } from "./pairedWorktreeActions";
import { PairedOperationError, type PairedDaemonProjectAdapter } from "./pairedDaemonProject";
import type { RegisteredProject } from "./types";
import fixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import lifecycle from "../../../docs/evidence/paired-daemon/fixtures/lifecycle.json";
import { decodeMachineJson } from "./pairedDaemonContracts";
const project: RegisteredProject = { workspaceId: `daemon:${"a".repeat(64)}`, remoteWorkspaceId: "project-a", target: { kind: "pairedDaemon", hostId: "host-a" }, repoRoot: "/srv/repo", gitRoot: "/srv/repo" };
function fixture(kind: string) {
  return decodeMachineJson(kind, JSON.stringify([...fixtures, ...lifecycle].find(row => row.kind === kind)!.value));
}
function setup() {
  const row = fixture("worktree");
  if (!("managed" in row) || row.managed === undefined) throw Error("worktree fixture");
  const status = fixture("worktreeStatus");
  if (!("dirtyCount" in status) || status.dirtyCount === undefined) throw Error("status fixture");
  const adapter = {
    capabilities: vi.fn(async () => fixture("capabilities")),
    createWorktree: vi.fn(async (_request: Parameters<PairedDaemonProjectAdapter["createWorktree"]>[0]) => row),
    worktreeStatus: vi.fn(async () => status),
    deleteWorktree: vi.fn(async (_request: Parameters<PairedDaemonProjectAdapter["deleteWorktree"]>[0]) => null),
    operation: vi.fn(),
  };
  const actions = createPairedWorktreeActions(project, adapter as unknown as PairedDaemonProjectAdapter);
  return { actions, adapter, row, status };
}
it("creates through the owning namespace without supplying a client filesystem path", async () => {
  const s = setup();
  const result = await s.actions.create("feature");
  expect(result.workspaceId).toBe(project.workspaceId);
  expect(s.adapter.createWorktree).toHaveBeenCalledWith({ requestId: expect.any(String), workspaceId: "project-a", worktree: { wsId: "project-a", slug: "feature" } });
});
it("ambiguous create uses only journal lookup and never sends another mutation", async () => {
  const s = setup();
  s.adapter.createWorktree.mockRejectedValueOnce(new PairedOperationError("PAIRED_HOST_UNAVAILABLE", false, undefined, {}, true));
  await expect(s.actions.create("feature")).rejects.toMatchObject({ ambiguous: true });
  const requestId = s.adapter.createWorktree.mock.calls[0][0].requestId;
  s.adapter.operation.mockResolvedValueOnce({ state: "outcomeUnknown", requestId });
  await expect(s.actions.create("different-slug")).rejects.toMatchObject({ ambiguous: true, requestId });
  s.adapter.operation.mockResolvedValueOnce({ state: "completed", requestId, outcome: { kind: "worktree", worktree: s.row } });
  expect((await s.actions.create("different-slug")).workspaceId).toBe(project.workspaceId);
  expect(s.adapter.createWorktree).toHaveBeenCalledTimes(1);
  expect(s.adapter.operation).toHaveBeenNthCalledWith(1, requestId);
  expect(s.adapter.operation).toHaveBeenNthCalledWith(2, requestId);
});
it("safe delete uses authoritative preview revision and reconciles ambiguous no-content success", async () => {
  const s = setup();
  const worktree = { ...s.row, workspaceId: project.workspaceId, branch: `refs/heads/orca/project-a/${s.status.worktree.slug}` };
  s.adapter.worktreeStatus.mockResolvedValue({ ...s.status, workspaceId: "project-a", worktree: { wsId: "project-a", slug: s.status.worktree.slug } });
  await s.actions.previewDelete(worktree);
  s.adapter.deleteWorktree.mockRejectedValueOnce(new PairedOperationError("PAIRED_HOST_UNAVAILABLE", false, undefined, {}, true));
  await expect(s.actions.deleteSafe(worktree)).rejects.toMatchObject({ ambiguous: true });
  const request = s.adapter.deleteWorktree.mock.calls[0][0];
  expect(request.expectedRevision).toBe(s.status.revision);
  expect(request.workspaceId).toBe("project-a");
  s.adapter.operation.mockResolvedValue({ state: "completed", requestId: request.requestId, outcome: { kind: "noContent" } });
  await s.actions.deleteSafe(worktree);
  expect(s.adapter.deleteWorktree).toHaveBeenCalledTimes(1);
  await expect(s.actions.deleteDestructive()).rejects.toMatchObject({ code: "UNSUPPORTED_CAPABILITY" });
});
it("rejects cross-host workspace results and cross-namespace deletion before mutation", async () => {
  const s = setup();
  s.adapter.createWorktree.mockResolvedValue({ ...s.row, workspaceId: "other-project" });
  await expect(s.actions.create("feature")).rejects.toMatchObject({ code: "CROSS_HOST_RESULT" });
  await expect(s.actions.previewDelete({ ...s.row, branch: "orca/other-project/feature" })).rejects.toMatchObject({ code: "INVALID_NAMESPACE" });
  expect(s.adapter.deleteWorktree).not.toHaveBeenCalled();
});
