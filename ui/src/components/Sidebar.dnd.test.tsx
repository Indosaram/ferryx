import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { LANE_LIFECYCLE_IMPORT_STORAGE_KEY, setLaneLifecycleTransport, type LaneLifecycleTransport, type LaneLifecycleView } from "../lib/sidebarPersistence";
import { useWorkspaceStore, type WorkspaceServices } from "../state/workspaceStore";
import { SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, SIDEBAR_WORKTREE_ORDER_STORAGE_KEY } from "./Sidebar";

type DragData =
  | { type: "sidebar-project"; workspaceId: string }
  | { type: "sidebar-worktree"; workspaceId: string; worktreePath: string };

type DragStart = (event: { active: { data: { current: DragData } } }) => void;
type DragEnd = (event: {
  active: { data: { current: DragData } };
  over: { data: { current: DragData } } | null;
}) => void;

const dndHarness = vi.hoisted(() => ({
  props: null as null | { onDragStart?: DragStart; onDragEnd?: DragEnd; sensors?: unknown },
  sensors: [] as Array<{ sensor: unknown; options?: unknown }>,
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: { children: ReactNode; sensors: Array<{ sensor: unknown; options?: unknown }>; onDragStart?: DragStart; onDragEnd?: DragEnd }) => {
    dndHarness.props = props;
    dndHarness.sensors = props.sensors;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  useSensor: (sensor: unknown, options?: unknown) => ({ sensor, options }),
  useSensors: (...sensors: unknown[]) => sensors,
}));

vi.mock("@dnd-kit/sortable", async (importOriginal) => {
  const original = await importOriginal<typeof import("@dnd-kit/sortable")>();
  return {
    ...original,
    SortableContext: ({ children }: { children: ReactNode }) => children,
    sortableKeyboardCoordinates: vi.fn(),
    useSortable: (args: { id: string; data?: DragData }) => ({
      setNodeRef: vi.fn(),
      setActivatorNodeRef: vi.fn(),
      attributes: { "data-sortable-id": args.id },
      listeners: {},
      transform: null,
      transition: undefined,
      isDragging: false,
    }),
  };
});

import { Sidebar } from "./Sidebar";

const projects = [
  { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
  { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
  { workspaceId: "gamma", repoRoot: "/repos/gamma", gitRoot: "/repos/gamma" },
];

function worktree(path: string, slug: string): Worktree {
  return {
    path,
    head: slug,
    branch: `refs/heads/orca/alpha/${slug}`,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

const first = worktree("/repos/alpha/first", "first");
const second = worktree("/repos/alpha/second", "second");
const third = worktree("/repos/alpha/third", "third");

function renderSidebar(overrides: Partial<React.ComponentProps<typeof Sidebar>> = {}) {
  return render(
    <Sidebar
      projects={projects}
      activeProjectId="alpha"
      worktrees={[first, second]}
      agents={[]}
      activePath=""
      onSelectWorktree={vi.fn()}
      onCreateWorktree={vi.fn()}
      {...overrides}
    />,
  );
}

function drag(active: DragData, over: DragData) {
  const onDragStart = dndHarness.props?.onDragStart;
  const onDragEnd = dndHarness.props?.onDragEnd;
  if (!onDragStart || !onDragEnd) throw new Error("Sidebar DndContext handlers were not captured");
  act(() => {
    onDragStart({ active: { data: { current: active } } });
    onDragEnd({ active: { data: { current: active } }, over: { data: { current: over } } });
  });
}

function visibleWorktreeNames() {
  return within(screen.getByRole("list", { name: "alpha worktrees" }))
    .getAllByRole("button", { name: /first|second|third/ })
    .map((button) => button.textContent?.trim());
}

beforeEach(() => {
  localStorage.clear();
  dndHarness.sensors.length = 0;
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
});

afterEach(() => {
  cleanup();
  setLaneLifecycleTransport(null);
  dndHarness.props = null;
});

describe("Sidebar drag reorder", () => {
  it("uses the five-pixel pointer threshold and keyboard sensor", () => {
    renderSidebar();

    expect(dndHarness.sensors).toHaveLength(2);
    expect(dndHarness.sensors[0]?.options).toEqual({ activationConstraint: { distance: 5 } });
    expect(dndHarness.sensors[1]?.options).toEqual({ coordinateGetter: expect.any(Function) });
  });

  it("calls onReorderProjects with project ids moved to the drop position", () => {
    const onReorderProjects = vi.fn();
    renderSidebar({ onReorderProjects });

    drag(
      { type: "sidebar-project", workspaceId: "alpha" },
      { type: "sidebar-project", workspaceId: "gamma" },
    );

    expect(onReorderProjects).toHaveBeenCalledExactlyOnceWith(["beta", "gamma", "alpha"]);
  });

  it("keeps project select and accordion toggle clicks usable after a non-dragging pointer press", () => {
    const onSelectProject = vi.fn();
    renderSidebar({ onSelectProject });

    const projectButton = screen.getByRole("button", { name: "alpha" });
    fireEvent.pointerDown(projectButton, { pointerId: 1, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerUp(projectButton, { pointerId: 1, button: 0, clientX: 10, clientY: 10 });
    fireEvent.click(projectButton);
    expect(onSelectProject).toHaveBeenCalledExactlyOnceWith(projects[0]);
    // IS-14: a name click selects but never collapses; collapsing is the chevron's job.
    expect(screen.getByRole("list", { name: "alpha worktrees" })).toBeInTheDocument();

    const collapse = screen.getByRole("button", { name: "Collapse alpha" });
    fireEvent.pointerDown(collapse, { pointerId: 2, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerUp(collapse, { pointerId: 2, button: 0, clientX: 10, clientY: 10 });
    fireEvent.click(collapse);
    expect(screen.queryByRole("list", { name: "alpha worktrees" })).not.toBeInTheDocument();

    const toggle = screen.getByRole("button", { name: "Expand alpha" });
    fireEvent.pointerDown(toggle, { pointerId: 3, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerUp(toggle, { pointerId: 3, button: 0, clientX: 10, clientY: 10 });
    fireEvent.click(toggle);
    expect(screen.getByRole("list", { name: "alpha worktrees" })).toBeInTheDocument();
    expect(onSelectProject).toHaveBeenCalledTimes(1);
  });

  it("persists a worktree drop and reapplies its path order after remount", () => {
    const { unmount } = renderSidebar();

    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );

    expect(JSON.parse(localStorage.getItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY) ?? "{}"))
      .toEqual({ alpha: ["sidebar-worktree:alpha:/repos/alpha/second", "sidebar-worktree:alpha:/repos/alpha/first"] });
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    unmount();
    renderSidebar();
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
  });

  it("refreshes the authoritative store order after a saved drag and retains it on rerender", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const keys = ["w:alpha/first", "w:alpha/second"];
    let serverOrder = [...keys];
    let finishSave!: () => void;
    const save = new Promise<void>((resolve) => { finishSave = resolve; });
    const rows = new Map(keys.map((key, index) => [key, {
      worktreeKey: key, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    } satisfies LaneLifecycleView]));
    const read = vi.fn(async () => ({
      sections: [{ workspaceId: "alpha", rows: serverOrder.map((key) => rows.get(key)!) }], revision: 1,
    }));
    const reorder = vi.fn(async (_workspaceId: string, identifiers: readonly string[]) => {
      await save;
      serverOrder = [...identifiers];
    });
    const api: LaneLifecycleTransport = {
      read, reorder, importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    };
    setLaneLifecycleTransport(api);
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined), spawnTerminal: vi.fn(async () => "unused"),
      getTerminalCwd: vi.fn(async () => null), closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };
    function Harness({ activePath = "" }: { activePath?: string }) {
      const store = useWorkspaceStore({ workspaceId: "alpha", initialWorktrees: [first, second], services });
      return <Sidebar projects={projects} activeProjectId="alpha" worktrees={store.state.worktrees}
        agents={[]} activePath={activePath} laneOrder={store.state.laneOrder}
        onRefreshLaneOrder={store.refreshLaneLifecycles} onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />;
    }
    let view!: ReturnType<typeof render>;
    await act(async () => { view = render(<Harness />); });
    expect(visibleWorktreeNames()).toEqual(["first", "second"]);
    const readsBeforeSave = read.mock.calls.length;
    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );
    expect(reorder).toHaveBeenCalledExactlyOnceWith("alpha", [keys[1], keys[0]]);
    expect(read).toHaveBeenCalledTimes(readsBeforeSave);
    await act(async () => { finishSave(); await save; });
    expect(read).toHaveBeenCalledTimes(readsBeforeSave + 1);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
    view.rerender(<Harness activePath={second.path} />);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
  });

  it("keeps an optimistic server-owned drag visible and reports a rejected save", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const reorder = vi.fn(async () => { throw new Error("offline"); });
    setLaneLifecycleTransport({
      read: vi.fn(async () => ({ sections: [], revision: 1 })), reorder,
      importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    });
    const keys = ["w:alpha/first", "w:alpha/second"];
    const lifecycleRows: Record<string, LaneLifecycleView> = Object.fromEntries(keys.map((worktreeKey, index) => [worktreeKey, {
      worktreeKey, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    }]));
    renderSidebar({ laneOrder: { alpha: keys }, laneLifecycles: lifecycleRows });

    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );

    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save worktree order");
    expect(reorder).toHaveBeenCalledExactlyOnceWith("alpha", ["w:alpha/second", "w:alpha/first"]);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
  });

  it("serializes same-workspace saves and does not let an older completion clear a newer drag", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const keys = ["w:alpha/first", "w:alpha/second"];
    const resolvers: Array<() => void> = [];
    const reorder = vi.fn((_workspaceId: string, _identifiers: readonly string[]) => new Promise<void>((resolve) => {
      resolvers.push(resolve);
    }));
    const rows: Record<string, LaneLifecycleView> = Object.fromEntries(keys.map((worktreeKey, index) => [worktreeKey, {
      worktreeKey, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    }]));
    setLaneLifecycleTransport({
      read: vi.fn(async () => ({ sections: [{ workspaceId: "alpha", rows: [rows[keys[0]!]!, rows[keys[1]!]!] }], revision: 1 })),
      reorder, importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    });
    const onRefreshLaneOrder = vi.fn(async () => undefined);
    const view = renderSidebar({ laneOrder: { alpha: keys }, laneLifecycles: rows, onRefreshLaneOrder });

    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );
    // The first save leaves inside the drop handler, so the drop itself already issued it once.
    expect(reorder).toHaveBeenCalledExactlyOnceWith("alpha", [keys[1], keys[0]]);
    await act(async () => { await Promise.resolve(); });
    expect(reorder).toHaveBeenCalledTimes(1);

    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
    );
    await act(async () => { await Promise.resolve(); });
    expect(reorder).toHaveBeenCalledTimes(1);
    expect(visibleWorktreeNames()).toEqual(["first", "second"]);

    view.rerender(<Sidebar projects={projects} activeProjectId="alpha" worktrees={[first, second]} agents={[]}
      activePath="" laneOrder={{ alpha: [keys[1]!, keys[0]!] }} laneLifecycles={rows}
      onRefreshLaneOrder={onRefreshLaneOrder} onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
    expect(visibleWorktreeNames()).toEqual(["first", "second"]);

    await act(async () => { resolvers[0]!(); await Promise.resolve(); });
    expect(reorder).toHaveBeenCalledTimes(2);
    expect(onRefreshLaneOrder).not.toHaveBeenCalled();
    await act(async () => { resolvers[1]!(); await Promise.resolve(); });
    expect(onRefreshLaneOrder).toHaveBeenCalledTimes(1);
    expect(visibleWorktreeNames()).toEqual(["first", "second"]);
    view.unmount();
  });

  it("retains a successful optimistic order when no refresh callback can publish server state", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const keys = ["w:alpha/first", "w:alpha/second"];
    let finishWrite!: () => void;
    const save = new Promise<void>((resolve) => { finishWrite = resolve; });
    const reorder = vi.fn(async () => save);
    const rows: Record<string, LaneLifecycleView> = Object.fromEntries(keys.map((worktreeKey, index) => [worktreeKey, {
      worktreeKey, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    }]));
    setLaneLifecycleTransport({
      read: vi.fn(async () => ({ sections: [], revision: 1 })), reorder,
      importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    });
    const view = renderSidebar({ laneOrder: { alpha: keys }, laneLifecycles: rows });
    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );
    await act(async () => { await Promise.resolve(); });
    expect(reorder).toHaveBeenCalledExactlyOnceWith("alpha", [keys[1], keys[0]]);
    await act(async () => { finishWrite(); await save; });
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
    view.rerender(<Sidebar projects={projects} activeProjectId="alpha" worktrees={[first, second]} agents={[]}
      activePath="" laneOrder={{ alpha: keys }} laneLifecycles={rows}
      onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
  });

  it("keeps optimistic order until the refreshed authoritative props commit", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const keys = ["w:alpha/first", "w:alpha/second"];
    const reorder = vi.fn(async () => undefined);
    const rows: Record<string, LaneLifecycleView> = Object.fromEntries(keys.map((worktreeKey, index) => [worktreeKey, {
      worktreeKey, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    }]));
    setLaneLifecycleTransport({
      read: vi.fn(async () => ({ sections: [], revision: 1 })), reorder,
      importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    });
    const onRefreshLaneOrder = vi.fn(async () => undefined);
    const view = renderSidebar({ laneOrder: { alpha: keys }, laneLifecycles: rows, onRefreshLaneOrder });
    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );
    await act(async () => { await Promise.resolve(); });
    expect(reorder).toHaveBeenCalledExactlyOnceWith("alpha", [keys[1], keys[0]]);
    expect(onRefreshLaneOrder).toHaveBeenCalledExactlyOnceWith();
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    view.rerender(<Sidebar projects={projects} activeProjectId="alpha" worktrees={[first, second]} agents={[]}
      activePath="" laneOrder={{ alpha: keys }} laneLifecycles={rows}
      onRefreshLaneOrder={onRefreshLaneOrder} onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    view.rerender(<Sidebar projects={projects} activeProjectId="alpha" worktrees={[first, second]} agents={[]}
      activePath="" laneOrder={{ alpha: [keys[1]!, keys[0]!] }} laneLifecycles={rows}
      onRefreshLaneOrder={onRefreshLaneOrder} onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
  });

  it("drops a failed optimistic order when a newer external server order arrives", async () => {
    localStorage.setItem(LANE_LIFECYCLE_IMPORT_STORAGE_KEY, "imported");
    const keys = ["w:alpha/first", "w:alpha/second"];
    const reorder = vi.fn(async () => { throw new Error("offline"); });
    const rows: Record<string, LaneLifecycleView> = Object.fromEntries(keys.map((worktreeKey, index) => [worktreeKey, {
      worktreeKey, workspaceId: "alpha", worktreePath: [first, second][index]!.path,
      state: "active", effectiveState: "active", pinnedTabs: [], snoozedUntil: null,
      position: index, revision: 1, updatedAt: "2026-11-15T00:00:00Z",
    }]));
    setLaneLifecycleTransport({
      read: vi.fn(async () => ({ sections: [], revision: 1 })), reorder,
      importOrder: vi.fn(async () => false),
      write: vi.fn(async () => { throw new Error("Unexpected lifecycle write"); }),
      writePinTabs: vi.fn(async () => { throw new Error("Unexpected pin write"); }),
    });
    const view = renderSidebar({ laneOrder: { alpha: keys }, laneLifecycles: rows });
    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    view.rerender(<Sidebar projects={projects} activeProjectId="alpha" worktrees={[first, second]} agents={[]}
      activePath="" laneOrder={{ alpha: [keys[0]!, keys[1]!] }} laneLifecycles={rows}
      onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
    expect(visibleWorktreeNames()).toEqual(["first", "second"]);
  });

  it("appends a newly appearing worktree after the stored paths in incoming order", () => {
    localStorage.setItem(
      SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
      JSON.stringify({ alpha: [second.path, first.path] }),
    );

    renderSidebar({ worktrees: [first, second, third] });

    expect(visibleWorktreeNames()).toEqual(["second", "first", "third"]);
  });

  it("ignores a deleted stored path and prunes it on the next completed drop", () => {
    const deletedPath = "/repos/alpha/deleted";
    localStorage.setItem(
      SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
      JSON.stringify({ alpha: [deletedPath, second.path, first.path] }),
    );
    renderSidebar();

    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    drag(
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: second.path },
      { type: "sidebar-worktree", workspaceId: "alpha", worktreePath: first.path },
    );

    expect(JSON.parse(localStorage.getItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY) ?? "{}"))
      .toEqual({ alpha: ["sidebar-worktree:alpha:/repos/alpha/first", "sidebar-worktree:alpha:/repos/alpha/second"] });
  });

  it("does not render separate reorder grip handles and makes rows directly draggable", () => {
    renderSidebar();

    expect(screen.queryByRole("button", { name: /reorder/i })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/reorder project/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/reorder worktree/i)).not.toBeInTheDocument();

    const projectButton = screen.getByRole("button", { name: "alpha" });
    expect(projectButton).toBeInTheDocument();
  });
});
