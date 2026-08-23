import type { ReactNode } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import type { TabDropEdge } from "./tab-dnd/tabDragTypes";
import { dropPriority, resolveWorkspaceDropCommand } from "./tab-dnd/tabDragTypes";

type DroppableRegistration = {
  id: string;
  data?: {
    type?: string;
    tabId?: string;
    leafId?: string;
    edge?: TabDropEdge;
    groupId?: string;
  };
};

type DndHarnessProps = {
  onDragStart?: (event: { active: { data: { current: unknown } } }) => void;
  onDragEnd?: (event: { over: { id?: string; data: { current: unknown } } | null }) => void;
};

const dndHarness = vi.hoisted(() => ({
  props: null as null | DndHarnessProps,
  droppables: [] as DroppableRegistration[],
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: {
    children: ReactNode;
    onDragStart?: (event: { active: { data: { current: unknown } } }) => void;
    onDragEnd?: (event: { over: { id?: string; data: { current: unknown } } | null }) => void;
  }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  pointerWithin: vi.fn(() => []),
  useDraggable: () => ({ setNodeRef: vi.fn(), attributes: {}, listeners: {}, isDragging: false }),
  useDroppable: (args: DroppableRegistration) => {
    dndHarness.droppables.push(args);
    return { setNodeRef: vi.fn(), isOver: false };
  },
  useSensor: vi.fn(() => ({})),
  useSensors: vi.fn(() => []),
}));

vi.mock("@dnd-kit/sortable", () => ({
  SortableContext: ({ children }: { children: ReactNode }) => children,
  horizontalListSortingStrategy: {},
  sortableKeyboardCoordinates: vi.fn(),
  useSortable: () => ({
    setNodeRef: vi.fn(),
    attributes: {},
    listeners: {},
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
  dndHarness.droppables.length = 0;
});

beforeEach(() => {
  dndHarness.droppables.length = 0;
});

function session(id: string): TerminalSession {
  return {
    id,
    cwd: `/repo/${id}`,
    worktreePath: `/repo/${id}`,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: id },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function layoutWithTargetSplit(): LayoutState {
  const target: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-left" };
  const source: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-source" };
  return {
    tabs: [target, source],
    activeTabId: target.id,
    tabGroups: {
      "group-default": {
        id: "group-default",
        tabIds: [target.id, source.id],
        activeTabId: target.id,
      },
    },
    tabGroupLayout: { type: "group", groupId: "group-default" },
    focusedGroupId: "group-default",
    layoutsByTabId: {
      [target.id]: {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-left" },
          second: { type: "leaf", leafId: "leaf-right" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-left",
        expandedLeafId: null,
        sessionIdsByLeafId: {
          "leaf-left": "session-left",
          "leaf-right": "session-right",
        },
      },
      [source.id]: {
        root: { type: "leaf", leafId: "leaf-source" },
        activeLeafId: "leaf-source",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-source": "session-source" },
      },
    },
  };
}

describe("TerminalSplitView pane-targeted tab drop", () => {
  it("registers four edge drop zones inside every rendered terminal pane instead of only around the group/window", () => {
    render(
      <TerminalSplitView
        layout={layoutWithTargetSplit()}
        sessions={{
          "session-left": session("session-left"),
          "session-right": session("session-right"),
          "session-source": session("session-source"),
        }}
      />,
    );

    const panes = screen.getAllByTestId("pane-leaf");
    expect(panes).toHaveLength(2);
    for (const pane of panes) {
      const leafId = pane.getAttribute("data-leaf-id");
      const paneZones = pane.querySelectorAll('[data-dnd-type="pane-edge"]');
      expect(paneZones).toHaveLength(4);
      expect(new Set(Array.from(paneZones, (zone) => zone.getAttribute("data-drop-edge")))).toEqual(
        new Set(["left", "right", "top", "bottom"]),
      );
      for (const zone of paneZones) expect(zone.getAttribute("data-leaf-id")).toBe(leafId);
    }

    expect(screen.getAllByTestId("pane-edge-drop-zone")).toHaveLength(8);
  });

  it("always resolves the hovered pane edge ahead of the overlapping group edge, including the active tab top edge", () => {
    const active = { type: "tab" as const, tabId: "tab-source", groupId: "group-default", index: 1 };
    const paneEdge = {
      type: "pane-edge" as const,
      tabId: "tab-target",
      leafId: "leaf-right",
      edge: "left" as const,
    };
    const selfTopPaneEdge = {
      type: "pane-edge" as const,
      tabId: "tab-source",
      leafId: "leaf-source",
      edge: "top" as const,
    };
    const groupTopEdge = { type: "group-edge" as const, groupId: "group-default", edge: "top" as const };

    expect(dropPriority(active, paneEdge)).toBeLessThan(dropPriority(active, groupTopEdge));
    expect(dropPriority(active, selfTopPaneEdge)).toBeLessThan(dropPriority(active, groupTopEdge));
    expect(resolveWorkspaceDropCommand(active, selfTopPaneEdge)).toEqual({
      type: "move-tab-to-pane-split",
      sourceTabId: "tab-source",
      targetTabId: "tab-source",
      targetLeafId: "leaf-source",
      direction: "vertical",
      position: "first",
    });
    expect(resolveWorkspaceDropCommand(active, paneEdge)).toEqual({
      type: "move-tab-to-pane-split",
      sourceTabId: "tab-source",
      targetTabId: "tab-target",
      targetLeafId: "leaf-right",
      direction: "horizontal",
      position: "first",
    });
  });

  it("obtains registered pane-edge droppable data from useDroppable and executes production onMoveTabToSplit payload on drop", () => {
    const onMoveTabToSplit = vi.fn();

    render(
      <TerminalSplitView
        layout={layoutWithTargetSplit()}
        sessions={{
          "session-left": session("session-left"),
          "session-right": session("session-right"),
          "session-source": session("session-source"),
        }}
        onMoveTabToSplit={onMoveTabToSplit}
      />,
    );

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragEnd).toBeTypeOf("function");

    // 1. Obtain the registered pane-edge droppable data directly from useDroppable
    const registeredTargetDroppable = dndHarness.droppables.find(
      (droppable) =>
        droppable.data?.type === "pane-edge" &&
        droppable.data?.tabId === "tab-target" &&
        droppable.data?.leafId === "leaf-right" &&
        droppable.data?.edge === "left",
    );

    expect(registeredTargetDroppable).toBeDefined();
    expect(registeredTargetDroppable?.id).toBe("pane-edge:tab-target:leaf-right:left");
    expect(registeredTargetDroppable?.data).toEqual({
      type: "pane-edge",
      tabId: "tab-target",
      leafId: "leaf-right",
      edge: "left",
    });

    // 2. Feed active tab drag through DndContext dragStart
    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "tab",
            tabId: "tab-source",
            groupId: "group-default",
            index: 1,
          },
        },
      },
    });

    // 3. Feed the registered pane-edge droppable data through DndContext dragEnd
    dndHarness.props!.onDragEnd!({
      over: {
        id: registeredTargetDroppable!.id,
        data: {
          current: registeredTargetDroppable!.data,
        },
      },
    });

    // 4. Assert the production onMoveTabToSplit payload
    expect(onMoveTabToSplit).toHaveBeenCalledTimes(1);
    expect(onMoveTabToSplit).toHaveBeenCalledWith(
      "tab-source",
      "group-default",
      "horizontal",
      "first",
      { tabId: "tab-target", leafId: "leaf-right" },
    );
  });

  it("calls onSplitPane and avoids onMoveTabToSplit when dragging an active terminal tab onto an edge of its own pane", () => {
    const onSplitPane = vi.fn();
    const onMoveTabToSplit = vi.fn();

    render(
      <TerminalSplitView
        layout={layoutWithTargetSplit()}
        sessions={{
          "session-left": session("session-left"),
          "session-right": session("session-right"),
          "session-source": session("session-source"),
        }}
        onSplitPane={onSplitPane}
        onMoveTabToSplit={onMoveTabToSplit}
      />,
    );

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragEnd).toBeTypeOf("function");

    const onDragStart = dndHarness.props?.onDragStart;
    const onDragEnd = dndHarness.props?.onDragEnd;
    if (!onDragStart || !onDragEnd) {
      throw new Error("DndContext handlers were not captured");
    }

    const registeredTargetDroppable = dndHarness.droppables.find(
      (droppable) =>
        droppable.data?.type === "pane-edge" &&
        droppable.data?.tabId === "tab-target" &&
        droppable.data?.leafId === "leaf-left" &&
        droppable.data?.edge === "right",
    );

    expect(registeredTargetDroppable).toBeDefined();
    if (!registeredTargetDroppable || !registeredTargetDroppable.data) {
      throw new Error("Registered target pane-edge droppable was not found");
    }

    expect(registeredTargetDroppable.id).toBe("pane-edge:tab-target:leaf-left:right");
    expect(registeredTargetDroppable.data).toEqual({
      type: "pane-edge",
      tabId: "tab-target",
      leafId: "leaf-left",
      edge: "right",
    });

    onDragStart({
      active: {
        data: {
          current: {
            type: "tab",
            tabId: "tab-target",
            groupId: "group-default",
            index: 0,
          },
        },
      },
    });

    onDragEnd({
      over: {
        id: registeredTargetDroppable.id,
        data: {
          current: registeredTargetDroppable.data,
        },
      },
    });

    expect(onSplitPane).toHaveBeenCalledTimes(1);
    expect(onSplitPane).toHaveBeenCalledWith("tab-target", "leaf-left", "horizontal", {
      position: "second",
    });
    expect(onMoveTabToSplit).not.toHaveBeenCalled();
  });
});
