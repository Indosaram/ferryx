import type { ReactNode } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Active, ClientRect, CollisionDetection, DroppableContainer } from "@dnd-kit/core";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { layoutReducer } from "../state/layout";
import { resolveWorkspaceDropCommand } from "./tab-dnd/tabDragTypes";
import type { PaneDragData, TabDropEdge, WorkspaceDropData } from "./tab-dnd/tabDragTypes";

type DroppableData = {
  type?: string;
  tabId?: string;
  leafId?: string;
  edge?: TabDropEdge;
  groupId?: string;
  index?: number;
  tabCount?: number;
};

type DroppableRegistration = { id: string; data?: DroppableData };

type DragEvent = { active: { data: { current: unknown } } };
type OverEvent = { over: { id?: string; data: { current: unknown } } | null };

type DndHarnessProps = {
  onDragStart?: (event: DragEvent) => void;
  onDragOver?: (event: OverEvent) => void;
  onDragEnd?: (event: OverEvent) => void;
};

const dndHarness = vi.hoisted(() => ({
  props: null as null | DndHarnessProps,
  droppables: [] as DroppableRegistration[],
  pointerWithin: vi.fn((_args?: unknown) => [] as Array<{ id: string }>),
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: DndHarnessProps & { children: ReactNode }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  pointerWithin: (args: unknown) => dndHarness.pointerWithin(args),
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
  useSortable: (args: DroppableRegistration) => {
    dndHarness.droppables.push(args);
    return {
      setNodeRef: vi.fn(),
      attributes: {},
      listeners: {},
      transform: null,
      transition: undefined,
      isDragging: false,
    };
  },
}));

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView, createWorkspaceCollisionDetection, snapOverlayToPointer } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
  dndHarness.droppables.length = 0;
});

beforeEach(() => {
  dndHarness.droppables.length = 0;
  dndHarness.pointerWithin.mockReturnValue([]);
});

function requireHandlers(): Required<DndHarnessProps> {
  const props = dndHarness.props;
  if (!props?.onDragStart || !props.onDragOver || !props.onDragEnd) {
    throw new Error("DndContext handlers were not captured");
  }
  return { onDragStart: props.onDragStart, onDragOver: props.onDragOver, onDragEnd: props.onDragEnd };
}

function requireDroppable(predicate: (registration: DroppableRegistration) => boolean): {
  id: string;
  data: DroppableData;
} {
  const found = dndHarness.droppables.find(predicate);
  if (!found || !found.data) throw new Error("expected droppable registration was not found");
  return { id: found.id, data: found.data };
}

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

/** Source tab is a split (2 leaves) so detaching a pane is valid. */
function layout(): LayoutState {
  const source: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-a" };
  const target: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-target" };
  return {
    tabs: [source, target],
    activeTabId: source.id,
    tabGroups: {
      "group-default": {
        id: "group-default",
        tabIds: [source.id, target.id],
        activeTabId: source.id,
      },
    },
    tabGroupLayout: { type: "group", groupId: "group-default" },
    focusedGroupId: "group-default",
    layoutsByTabId: {
      [source.id]: {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-a" },
          second: { type: "leaf", leafId: "leaf-b" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-a",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-a": "session-a", "leaf-b": "session-b" },
      },
      [target.id]: {
        root: { type: "leaf", leafId: "leaf-target" },
        activeLeafId: "leaf-target",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-target": "session-target" },
      },
    },
  };
}

const sessions = {
  "session-a": session("session-a"),
  "session-b": session("session-b"),
  "session-target": session("session-target"),
};

const paneDragStart: DragEvent = {
  active: {
    data: {
      current: { type: "pane", tabId: "tab-source", sourcePaneId: "leaf-a", sourceSessionId: "session-a" },
    },
  },
};

function rect(overrides: Partial<ClientRect>): ClientRect {
  return { top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, ...overrides };
}

function dropContainer(id: string, data: WorkspaceDropData): DroppableContainer {
  return {
    id,
    key: id,
    data: { current: data },
    disabled: false,
    node: { current: null },
    rect: { current: null },
  };
}

function activePaneDrag(): Active {
  const data: PaneDragData = { type: "pane", tabId: "tab-source", sourcePaneId: "leaf-a" };
  return {
    id: "pane:tab-source:leaf-a",
    data: { current: data },
    rect: { current: { initial: null, translated: null } },
  };
}

describe("TerminalSplitView pane -> tab row drop", () => {
  it("registers a droppable for the blank tab strip that detaches a dragged pane into a new appended tab", () => {
    const onDetachPaneToTab = vi.fn(() => "tab-detached");
    render(<TerminalSplitView layout={layout()} sessions={sessions} onDetachPaneToTab={onDetachPaneToTab} />);
    const handlers = requireHandlers();

    const strip = requireDroppable((d) => d.data?.type === "tab-strip" && d.data?.groupId === "group-default");

    act(() => handlers.onDragStart(paneDragStart));
    act(() => handlers.onDragEnd({ over: { id: strip.id, data: { current: strip.data } } }));

    expect(onDetachPaneToTab).toHaveBeenCalledTimes(1);
    // Blank strip appends at the end: explicit index = current tab count (not undefined, which the
    // reducer treats as source-index+1 for a same-group detach and would land mid-strip).
    expect(strip.data.tabCount).toBe(2);
    expect(onDetachPaneToTab).toHaveBeenCalledWith("tab-source", "leaf-a", "group-default", 2);
  });

  it("appends the detached tab to the END of a same-group strip via the real layout reducer even when the source is first", () => {
    // Source is first, target second. undefined index -> source+1 = mid-strip; explicit count -> end.
    const command = resolveWorkspaceDropCommand(
      { type: "pane", tabId: "tab-source", sourcePaneId: "leaf-a" },
      { type: "tab-strip", groupId: "group-default", tabCount: 2 },
    );
    expect(command).toEqual({
      type: "detach-pane-to-tab",
      sourceTabId: "tab-source",
      leafId: "leaf-a",
      targetGroupId: "group-default",
      targetIndex: 2,
    });

    const next = layoutReducer(layout(), {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: "tab-source",
      leafId: "leaf-a",
      newTab: { id: "tab-detached", label: "source", sessionId: "session-a" },
      targetGroupId: "group-default",
      targetIndex: 2,
    });
    const appended = next.tabGroups?.["group-default"];
    if (!appended) throw new Error("target group missing after detach");
    expect(appended.tabIds).toEqual(["tab-source", "tab-target", "tab-detached"]);

    // Prove the undefined default would NOT append (regression guard for the old claim).
    const midStrip = layoutReducer(layout(), {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: "tab-source",
      leafId: "leaf-a",
      newTab: { id: "tab-detached", label: "source", sessionId: "session-a" },
      targetGroupId: "group-default",
    });
    const mid = midStrip.tabGroups?.["group-default"];
    if (!mid) throw new Error("target group missing after default detach");
    expect(mid.tabIds).toEqual(["tab-source", "tab-detached", "tab-target"]);
  });

  it("ranks an indexed tab target ahead of the whole-strip target when both rects collide (real collision detection)", () => {
    const detect = createWorkspaceCollisionDetection();
    dndHarness.pointerWithin.mockReturnValue([
      { id: "tab-strip:group-default" },
      { id: "tab:tab-target" },
    ]);
    const args: Parameters<CollisionDetection>[0] = {
      active: activePaneDrag(),
      collisionRect: rect({ left: 60, top: 8, width: 1, height: 1, right: 61, bottom: 9 }),
      droppableRects: new Map<string, ClientRect>(),
      droppableContainers: [
        dropContainer("tab-strip:group-default", { type: "tab-strip", groupId: "group-default", tabCount: 2 }),
        dropContainer("tab:tab-target", { type: "tab", tabId: "tab-target", groupId: "group-default", index: 1 }),
      ],
      pointerCoordinates: { x: 60, y: 8 },
    };
    const result = detect(args);
    // Indexed tab must win the append target regardless of pointerWithin order.
    expect(result.map((collision) => collision.id)).toEqual(["tab:tab-target", "tab-strip:group-default"]);
  });

  it("centers the bounded tab-mode overlay on the pointer using the source origin and measured wrapper size", () => {
    const activatorEvent = new MouseEvent("pointerdown", { clientX: 250, clientY: 36 });
    // Overlay was grabbed from a wide pane (source origin), but its wrapper measured a bounded tab.
    const activeNodeRect = rect({ left: 100, top: 20, width: 500, height: 400, right: 600, bottom: 420 });
    const overlayRect = rect({ left: 100, top: 20, width: 224, height: 32, right: 324, bottom: 52 });
    const out = snapOverlayToPointer({
      activatorEvent,
      active: null,
      activeNodeRect,
      draggingNodeRect: overlayRect,
      containerNodeRect: null,
      over: null,
      overlayNodeRect: overlayRect,
      scrollableAncestors: [],
      scrollableAncestorRects: [],
      transform: { x: 250, y: -20, scaleX: 1, scaleY: 1 },
      windowRect: null,
    });
    // The current pointer includes the drag delta, not just its position at activation.
    expect(activeNodeRect.left + out.x + overlayRect.width / 2).toBeCloseTo(500);
    expect(activeNodeRect.top + out.y + overlayRect.height / 2).toBeCloseTo(16);
  });

  it("clamps the tab-mode overlay fully inside the viewport when the pointer is near the left edge", () => {
    const activatorEvent = new MouseEvent("pointerdown", { clientX: 60, clientY: 8 });
    const activeNodeRect = rect({ left: 100, top: 20, width: 500, height: 400, right: 600, bottom: 420 });
    const overlayRect = rect({ left: 100, top: 20, width: 224, height: 32, right: 324, bottom: 52 });
    const out = snapOverlayToPointer({
      activatorEvent,
      active: null,
      activeNodeRect,
      draggingNodeRect: overlayRect,
      containerNodeRect: null,
      over: null,
      overlayNodeRect: overlayRect,
      scrollableAncestors: [],
      scrollableAncestorRects: [],
      transform: { x: 0, y: 0, scaleX: 1, scaleY: 1 },
      windowRect: rect({ width: 1000, height: 800, right: 1000, bottom: 800 }),
    });
    const predictedLeft = activeNodeRect.left + out.x;
    // Whole preview inside the viewport, and the pointer (60) still lands within [left, left+width].
    expect(predictedLeft).toBeGreaterThanOrEqual(0);
    expect(predictedLeft + overlayRect.width).toBeLessThanOrEqual(1000);
    expect(predictedLeft).toBeLessThanOrEqual(60);
    expect(predictedLeft + overlayRect.width).toBeGreaterThanOrEqual(60);
  });

  it("keeps an indexed drop when the pane is released on an existing tab (existing tab wins over whole-strip append)", () => {
    const onDetachPaneToTab = vi.fn(() => "tab-detached");
    render(<TerminalSplitView layout={layout()} sessions={sessions} onDetachPaneToTab={onDetachPaneToTab} />);
    const handlers = requireHandlers();

    const targetTab = requireDroppable((d) => d.data?.type === "tab" && d.data?.tabId === "tab-target");

    act(() => handlers.onDragStart(paneDragStart));
    act(() => handlers.onDragEnd({ over: { id: targetTab.id, data: { current: targetTab.data } } }));

    expect(onDetachPaneToTab).toHaveBeenCalledWith("tab-source", "leaf-a", "group-default", (targetTab.data.index ?? 0) + 1);
  });

  it("switches the drag overlay preview to a tab while a pane hovers the tab row, and back to a pane elsewhere", () => {
    render(<TerminalSplitView layout={layout()} sessions={sessions} onDetachPaneToTab={vi.fn(() => "tab-detached")} />);
    const handlers = requireHandlers();

    const strip = requireDroppable((d) => d.data?.type === "tab-strip" && d.data?.groupId === "group-default");
    const targetTab = requireDroppable((d) => d.data?.type === "tab" && d.data?.tabId === "tab-target");

    act(() => handlers.onDragStart(paneDragStart));

    // Over the blank strip: preview becomes a tab.
    act(() => handlers.onDragOver({ over: { id: strip.id, data: { current: strip.data } } }));
    expect(screen.getByTestId("workspace-drag-overlay").getAttribute("data-preview-kind")).toBe("tab");

    // Over an existing tab: still a tab preview.
    act(() => handlers.onDragOver({ over: { id: targetTab.id, data: { current: targetTab.data } } }));
    expect(screen.getByTestId("workspace-drag-overlay").getAttribute("data-preview-kind")).toBe("tab");

    // Over a pane-edge (a split, not the tab row): reverts to the pane preview.
    act(() =>
      handlers.onDragOver({
        over: {
          id: "pane-edge:tab-source:leaf-b:left",
          data: { current: { type: "pane-edge", tabId: "tab-source", leafId: "leaf-b", edge: "left" } },
        },
      }),
    );
    expect(screen.getByTestId("workspace-drag-overlay").getAttribute("data-preview-kind")).toBe("pane");
  });
});
