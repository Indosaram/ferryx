import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SidebarView } from "../components/sidebar/SidebarView";
import { sidebarLiveState, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { LaneLifecycleModel, LaneLifecycleView } from "../lib/sidebarPersistence";
import { useRemoteSidebarModel, type RemoteSidebarMachine } from "./useRemoteSidebarModel";
import type { SupervisorHttpResponse } from "./remoteSupervisor";
import { applyLaneLifecycleModel } from "./supervisorPanel/panelModel";

const row: LaneLifecycleView = {
  worktreeKey: "p:owner/C:/code/review", workspaceId: "owner", worktreePath: "C:\\code\\review\\",
  state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
  position: 0, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
  pullRequest: {
    laneKey: "review", repo: "owner/repo", prNumber: 42, state: "open",
    url: "https://github.com/owner/repo/pull/42", title: "Review", headBranch: "lane/review", observedAt: "2026-11-15T00:00:00Z",
  },
};
const lifecycle = (value: LaneLifecycleView): LaneLifecycleModel => ({ sections: [{ workspaceId: "owner", rows: [value] }], revision: value.revision });
const sidebar: SidebarModel = {
  schemaVersion: 1, revision: 1, generatedAt: "2026-11-15T00:00:00Z", source: "desktop",
  collapsedGroupIds: [], advancedFeatures: false, emptyState: null,
  projects: [{
    groupId: "owner", label: "Repository", hostLabel: null, primaryWorkspaceId: "owner", status: null,
    members: [{ workspaceId: "owner", kind: "local", machineId: null }], filterLabels: ["Repository"], worktreesEmptyHint: null,
    worktrees: [{
      key: "sidebar-worktree:owner:C:/code/review", workspaceId: "owner", path: row.worktreePath, worktreeSlug: null,
      lifecycleKey: row.worktreeKey, lifecycle: "active", pinnedTabs: [], pullRequest: row.pullRequest,
      label: "review", hostLabel: null, stale: false, listingFailure: null,
    }],
  }],
};
const response = (body: unknown): SupervisorHttpResponse => ({ status: 200, body: new TextEncoder().encode(JSON.stringify(body)) });

function Host({ machine }: { machine: RemoteSidebarMachine }) {
  const [section] = useRemoteSidebarModel({ machines: [machine] });
  if (!section?.model) return null;
  return <SidebarView model={section.model} live={sidebarLiveState(section.model, {}, "")} density="touch" filter={{ query: "", onChange: () => undefined }} actions={{ selectWorktree: () => undefined, toggleGroup: () => undefined }} />;
}

function machine() {
  const handlers = new Set<(frame: unknown) => void>();
  let reply!: (body: SupervisorHttpResponse) => void;
  const read = new Promise<SupervisorHttpResponse>((resolve) => { reply = resolve; });
  const fetchLike = vi.fn(() => read);
  const subscribe = vi.fn((handler: (frame: unknown) => void) => { handlers.add(handler); return () => { handlers.delete(handler); }; });
  const value: RemoteSidebarMachine = { machineId: "owner-machine", label: "Owner", inventory: null, selection: null, fetchLike, subscribe };
  const emit = (frame: unknown) => act(() => { for (const handler of handlers) handler(frame); });
  return { value, fetchLike, subscribe, emit, answer: async (body: unknown) => { await act(async () => { reply(response(body)); await read; }); } };
}

afterEach(() => cleanup());

describe("remote lane publication", () => {
  // Deleting this lets an out-of-order frame regress phone lifecycle, pins and PR together.
  it("rejects lower row revisions but applies equal-revision PR observations", () => {
    const merged = { ...row, revision: 3, effectiveState: "pinned" as const, pinnedTabs: ["tab"], pullRequest: { ...row.pullRequest!, state: "merged" as const } };
    const current = applyLaneLifecycleModel(sidebar, lifecycle(merged));
    const stale = applyLaneLifecycleModel(current, lifecycle(row));
    expect(stale.projects[0].worktrees[0]).toBe(current.projects[0].worktrees[0]);
    const equal = applyLaneLifecycleModel(current, lifecycle({ ...row, revision: 3 }));
    expect(equal.projects[0].worktrees[0].pullRequest?.state).toBe("open");
    expect(equal.projects[0].worktrees[0].lifecycleRevision).toBe(3);
  });

  // Deleting this test loses the remote socket -> parsed frame -> real phone row delta path,
  // including normalized plain-folder identity, DOM continuity and the no-refetch guarantee.
  it("updates an open PR to merged in place and removes it on settlement without fetching", async () => {
    const m = machine();
    render(<Host machine={m.value} />);
    await m.answer({ model: sidebar });
    const before = screen.getByRole("link", { name: "#42 open" });
    const merged = { ...row, pullRequest: { ...row.pullRequest!, state: "merged" as const } };
    m.emit(JSON.stringify({ type: "panel.lanes", model: lifecycle(merged) }));
    expect(screen.getByRole("link", { name: "#42 merged" })).toBe(before);
    m.emit({ type: "panel.lanes", model: lifecycle({ ...merged, state: "settled", effectiveState: "settled", revision: 2 }) });
    expect(screen.queryByRole("link", { name: /#42/ })).not.toBeInTheDocument();
    expect(m.fetchLike).toHaveBeenCalledTimes(1);
    expect(m.subscribe).toHaveBeenCalledTimes(1);
  });

  // Deleting this test allows a stale first fetch to overwrite a frame received after subscribe.
  it("replays a lifecycle frame that races the initial sidebar read", async () => {
    const m = machine();
    render(<Host machine={m.value} />);
    m.emit({ type: "panel.lanes", model: lifecycle({ ...row, pullRequest: { ...row.pullRequest!, state: "merged" } }) });
    await m.answer({ model: sidebar });
    expect(screen.getByRole("link", { name: "#42 merged" })).toBeInTheDocument();
    expect(m.fetchLike).toHaveBeenCalledTimes(1);
  });

  // Deleting this test permits reconnect correctness to depend on a desktop republishing a
  // stale snapshot or on the phone having heard the event while it was disconnected.
  it("converges from the durable lifecycle read even when the cached desktop snapshot is stale", async () => {
    const m = machine();
    render(<Host machine={m.value} />);
    await m.answer({ model: sidebar, lifecycle: lifecycle({ ...row, pullRequest: { ...row.pullRequest!, state: "merged" } }) });
    expect(screen.getByRole("link", { name: "#42 merged" })).toBeInTheDocument();
    expect(m.fetchLike).toHaveBeenCalledTimes(1);
  });
});
