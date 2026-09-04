import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
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
  props: null as null | { onDragStart?: DragStart; onDragEnd?: DragEnd },
  sensors: [] as Array<{ sensor: unknown; options?: unknown }>,
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: { children: ReactNode; onDragStart?: DragStart; onDragEnd?: DragEnd }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  useSensor: (sensor: unknown, options?: unknown) => {
    const descriptor = { sensor, options };
    dndHarness.sensors.push(descriptor);
    return descriptor;
  },
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
    expect(screen.queryByRole("list", { name: "alpha worktrees" })).not.toBeInTheDocument();

    const toggle = screen.getByRole("button", { name: "Expand alpha" });
    fireEvent.pointerDown(toggle, { pointerId: 2, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerUp(toggle, { pointerId: 2, button: 0, clientX: 10, clientY: 10 });
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
      .toEqual({ alpha: [second.path, first.path] });
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);

    unmount();
    renderSidebar();
    expect(visibleWorktreeNames()).toEqual(["second", "first"]);
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
      .toEqual({ alpha: [first.path, second.path] });
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
