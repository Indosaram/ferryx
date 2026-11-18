import { useEffect } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SidebarView } from "../components/sidebar/SidebarView";
import { buildSidebarModel, sidebarLiveState } from "../components/sidebar/sidebarModel";
import { setLaneLifecycleTransport, type LaneLifecycleModel, type LaneLifecycleTransport, type LaneLifecycleView } from "../lib/sidebarPersistence";
import type { PanelEvent } from "../lib/supervisorPanel/contract";
import * as tauri from "../lib/tauri";
import { clearHmrWorkspaceState, useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

const workspaceId = "lane-publication";
const root = { path: "/code/publication", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null };
// The branch owner, not the rendering project's workspace id, supplies the lifecycle key.
const managed = { ...root, path: "/code/publication/review", branch: "ferryx/owner/review" };
const row: LaneLifecycleView = {
  worktreeKey: "w:owner/review", workspaceId: "owner", worktreePath: managed.path,
  state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
  position: 0, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
  pullRequest: {
    laneKey: "review", repo: "owner/repo", prNumber: 42, state: "open",
    url: "https://github.com/owner/repo/pull/42", title: "Review", headBranch: "lane/review", observedAt: "2026-11-15T00:00:00Z",
  },
};
const model = (value: LaneLifecycleView): LaneLifecycleModel => ({ sections: [{ workspaceId: "owner", rows: [value] }], revision: value.revision });
const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined), spawnTerminal: vi.fn(async () => "unused"),
  getTerminalCwd: vi.fn(async () => null), closeTerminal: vi.fn(async () => undefined), waitForTerminalExit: vi.fn(async () => undefined),
};

function Host({ onState, query = "" }: { onState: (state: WorkspaceState) => void; query?: string }) {
  const { state } = useWorkspaceStore({ workspaceId, initialWorktrees: [root, managed], services });
  useEffect(() => onState(state), [onState, state]);
  const sidebar = buildSidebarModel({
    source: "desktop", generatedAt: "", projects: [{ workspaceId, repoRoot: root.path }], activeProjectId: workspaceId,
    worktrees: state.worktrees, inactiveProjectWorktrees: {}, hosts: [],
    remoteState: { hosts: {}, activeHostId: null, discovering: false }, reachability: () => ({ supported: false, entry: null }),
    worktreeOrder: {}, laneOrder: state.laneOrder,
    laneLifecycles: Object.fromEntries(Object.entries(state.laneLifecycle ?? {}).map(([key, value]) => [key, {
      state: value, pinnedTabs: state.lanePinnedTabs?.[key], pullRequest: state.lanePullRequests?.[key],
    }])),
    collapsedGroupIds: new Set(), advancedFeatures: false, listingFailures: {},
  });
  return <SidebarView model={sidebar} live={sidebarLiveState(sidebar, {}, query)} filter={{ query, onChange: () => undefined }} actions={{ selectWorktree: () => undefined, toggleGroup: () => undefined }} />;
}

async function mount(initial: LaneLifecycleView = row, early?: unknown) {
  clearHmrWorkspaceState(workspaceId);
  const handlers = new Set<(event: PanelEvent) => void>();
  const subscribe = vi.spyOn(tauri, "onSupervisorPanelEvent").mockImplementation(async (handler) => {
    handlers.add(handler);
    return () => { handlers.delete(handler); };
  });
  let answer!: (value: LaneLifecycleModel) => void;
  const read = new Promise<LaneLifecycleModel>((resolve) => { answer = resolve; });
  const api: LaneLifecycleTransport = {
    read: vi.fn(() => read), write: vi.fn(async () => initial), writePinTabs: vi.fn(async () => initial),
    importOrder: vi.fn(async () => false), reorder: vi.fn(async () => undefined),
  };
  setLaneLifecycleTransport(api);
  let latest: WorkspaceState | undefined;
  let finishRead!: () => void;
  const readsApplied = new Promise<void>((resolve) => { finishRead = resolve; });
  const onState = (state: WorkspaceState) => { latest = state; if (state.laneLifecycle?.[row.worktreeKey]) finishRead(); };
  const view = render(<Host onState={onState} />);
  const emit = (event: unknown) => act(() => { for (const handler of handlers) handler(event as PanelEvent); });
  await act(async () => {
    if (early) for (const handler of handlers) handler(early as PanelEvent);
    answer(model(initial));
    await read;
  });
  await readsApplied;
  return { ...view, api, subscribe, emit, onState, latest: () => latest };
}

afterEach(() => { cleanup(); setLaneLifecycleTransport(null); clearHmrWorkspaceState(workspaceId); vi.restoreAllMocks(); });

describe("lane lifecycle publication to rendered desktop rows", () => {
  // Deleting this test loses the complete event -> workspace store -> real rendered PR link
  // path, including canonical owner identity, unchanged DOM identity and zero extra reads.
  it("applies an open-to-merged frame to the existing pinned row without reload or remount", async () => {
    const host = await mount({ ...row, state: "pinned", effectiveState: "pinned" });
    const before = screen.getByRole("link", { name: "#42 open" });
    const reads = vi.mocked(host.api.read).mock.calls.length;
    const subscriptions = host.subscribe.mock.calls.length;
    host.emit({ type: "panel.lanes", model: model({ ...row, state: "pinned", effectiveState: "pinned", pullRequest: { ...row.pullRequest!, state: "merged" } }) });
    expect(screen.getByRole("link", { name: "#42 merged" })).toBe(before);
    expect(before).toHaveAttribute("data-pr-state", "merged");
    expect(host.latest()?.lanePullRequests?.[row.worktreeKey]?.state).toBe("merged");
    expect(host.api.read).toHaveBeenCalledTimes(reads);
    expect(host.subscribe).toHaveBeenCalledTimes(subscriptions);
  });

  // Deleting this test leaves settlement filtering untested: state could change in the store
  // while the active list still shows the row, or the merge link could disappear from search.
  it("removes a merged-and-settled lane from the active list and keeps it searchable", async () => {
    const host = await mount();
    const reads = vi.mocked(host.api.read).mock.calls.length;
    host.emit({ type: "panel.lanes", model: model({ ...row, state: "settled", effectiveState: "settled", revision: 2, pullRequest: { ...row.pullRequest!, state: "merged" } }) });
    expect(host.latest()?.laneLifecycle?.[row.worktreeKey]).toBe("settled");
    expect(screen.queryByRole("link", { name: /#42/ })).not.toBeInTheDocument();
    host.rerender(<Host onState={host.onState} query="review" />);
    expect(screen.getByRole("link", { name: "#42 merged" })).toHaveAttribute("data-pr-state", "merged");
    expect(host.api.read).toHaveBeenCalledTimes(reads);
  });

  // Deleting this test allows a stale read already in flight to overwrite a newer row frame.
  it("keeps a frame received during the initial lifecycle read", async () => {
    const merged = { ...row, pullRequest: { ...row.pullRequest!, state: "merged" as const } };
    const host = await mount(row, { type: "panel.lanes", model: model(merged) });
    expect(screen.getByRole("link", { name: "#42 merged" })).toBeInTheDocument();
    expect(host.latest()?.lanePullRequests?.[row.worktreeKey]?.state).toBe("merged");
  });

  // Deleting this test permits an unsupported/old frame shape to clear rows or crash the store.
  it("ignores unknown siblings and older lifecycle frame shapes", async () => {
    const host = await mount();
    const before = screen.getByRole("link", { name: "#42 open" });
    const reads = vi.mocked(host.api.read).mock.calls.length;
    host.emit({ type: "panel.future", rows: [] });
    host.emit({ type: "panel.lanes", rows: [] });
    host.emit({ type: "panel.lanes", model: { sections: [{ workspaceId: "owner", rows: [{}] }], revision: 2 } });
    expect(screen.getByRole("link", { name: "#42 open" })).toBe(before);
    expect(host.api.read).toHaveBeenCalledTimes(reads);
  });
});
