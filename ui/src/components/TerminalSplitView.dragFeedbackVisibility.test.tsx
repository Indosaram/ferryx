import type { ReactNode } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const dndHarness = vi.hoisted(() => ({
  props: null as null | {
    onDragStart?: (event: unknown) => void;
    onDragOver?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
    onDragCancel?: (event: unknown) => void;
  },
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: {
    children: ReactNode;
    onDragStart?: (event: unknown) => void;
    onDragOver?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
    onDragCancel?: (event: unknown) => void;
  }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  pointerWithin: vi.fn(() => []),
  useDraggable: () => ({ setNodeRef: vi.fn(), attributes: {}, listeners: {}, isDragging: false }),
  useDroppable: () => ({ setNodeRef: vi.fn(), isOver: false }),
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

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

// isTauri() is mocked true, so event subscriptions pass their runtime guard and
// would reach the real bridge, which has no __TAURI_INTERNALS__ under jsdom.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

class TestResizeObserver implements ResizeObserver {
  readonly observe = vi.fn();
  readonly unobserve = vi.fn();
  readonly disconnect = vi.fn();
  constructor(readonly callback: ResizeObserverCallback) {}
}

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
  vi.unstubAllGlobals();
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

function splitLayout(): LayoutState {
  const tab: TerminalTab = { id: "tab-main", label: "main", sessionId: "session-a" };
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: {
      "group-main": { id: "group-main", tabIds: [tab.id], activeTabId: tab.id },
    },
    tabGroupLayout: { type: "group", groupId: "group-main" },
    focusedGroupId: "group-main",
    layoutsByTabId: {
      [tab.id]: {
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
    },
  };
}

function paneDragStart() {
  return {
    active: {
      id: "pane:tab-main:leaf-a",
      data: { current: { type: "pane", tabId: "tab-main", sourcePaneId: "leaf-a", leafId: "leaf-a" } },
    },
  };
}

function overPaneEdge(leafId: string) {
  return {
    ...paneDragStart(),
    over: {
      id: `pane-edge:tab-main:${leafId}:right`,
      data: { current: { type: "pane-edge", tabId: "tab-main", leafId, edge: "right" } },
    },
  };
}

function visibilityByLeafId(): Record<string, string> {
  const entries = screen.getAllByTestId("pane-leaf").map((leaf) => {
    const pane = leaf.querySelector('[data-testid="native-terminal-pane"]');
    return [
      leaf.getAttribute("data-leaf-id") ?? "",
      pane?.getAttribute("data-native-terminal-visible") ?? "",
    ] as const;
  });
  return Object.fromEntries(entries);
}

function renderSplit() {
  vi.stubGlobal("ResizeObserver", TestResizeObserver);
  render(
    <TerminalSplitView
      layout={splitLayout()}
      sessions={{ "session-a": session("session-a"), "session-b": session("session-b") }}
    />,
  );
}

describe("drop feedback visibility over native terminal surfaces", () => {
  it("suppresses only the targeted pane so other terminals stay visible during a drag", () => {
    // macOS native compositor child views paint above WKWebView, so DOM drop feedback cannot cover a live terminal surface.
    renderSplit();

    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "true" });

    act(() => {
      dndHarness.props?.onDragStart?.(paneDragStart());
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "true" });

    act(() => {
      dndHarness.props?.onDragOver?.(overPaneEdge("leaf-b"));
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "false" });

    act(() => {
      dndHarness.props?.onDragOver?.(overPaneEdge("leaf-a"));
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "false", "leaf-b": "true" });

    act(() => {
      dndHarness.props?.onDragEnd?.({ ...paneDragStart(), over: null });
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "true" });
  });

  it("restores the targeted pane surface when the pointer leaves every pane target", () => {
    renderSplit();

    act(() => {
      dndHarness.props?.onDragStart?.(paneDragStart());
      dndHarness.props?.onDragOver?.(overPaneEdge("leaf-b"));
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "false" });

    act(() => {
      dndHarness.props?.onDragOver?.({ ...paneDragStart(), over: null });
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "true" });
  });

  it("restores native terminal surfaces when a drag is cancelled", () => {
    renderSplit();

    act(() => {
      dndHarness.props?.onDragStart?.(paneDragStart());
      dndHarness.props?.onDragOver?.(overPaneEdge("leaf-b"));
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "false" });

    act(() => {
      dndHarness.props?.onDragCancel?.(paneDragStart());
    });
    expect(visibilityByLeafId()).toEqual({ "leaf-a": "true", "leaf-b": "true" });
  });
});
