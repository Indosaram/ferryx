import type { ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const dndHarness = vi.hoisted(() => ({
  props: null as null | {
    onDragStart?: (event: unknown) => void;
    onDragOver?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
  },
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: {
    children: ReactNode;
    onDragStart?: (event: unknown) => void;
    onDragOver?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
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

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
});

function createSession(id: string): TerminalSession {
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

describe("TerminalSplitView onDragOver active tab preview suppression", () => {
  it("does not redundantly call onActivateTab when dragging over the already active tab's pane edge, but activates inactive targets", () => {
    const tabActive: TerminalTab = {
      kind: "terminal",
      id: "tab-active",
      label: "Active Tab",
      sessionId: "session-active",
    };
    const tabSource: TerminalTab = {
      kind: "terminal",
      id: "tab-source",
      label: "Source Tab",
      sessionId: "session-source",
    };
    const tabInactive: TerminalTab = {
      kind: "terminal",
      id: "tab-inactive",
      label: "Inactive Target Tab",
      sessionId: "session-inactive",
    };

    const layout: LayoutState = {
      tabs: [tabActive, tabSource, tabInactive],
      activeTabId: "tab-active",
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: ["tab-active", "tab-source", "tab-inactive"],
          activeTabId: "tab-active",
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        "tab-active": {
          root: { type: "leaf", leafId: "leaf-active" },
          activeLeafId: "leaf-active",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-active": "session-active" },
        },
        "tab-source": {
          root: { type: "leaf", leafId: "leaf-source" },
          activeLeafId: "leaf-source",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-source": "session-source" },
        },
        "tab-inactive": {
          root: { type: "leaf", leafId: "leaf-inactive" },
          activeLeafId: "leaf-inactive",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-inactive": "session-inactive" },
        },
      },
    };

    const sessions: Record<string, TerminalSession> = {
      "session-active": createSession("session-active"),
      "session-source": createSession("session-source"),
      "session-inactive": createSession("session-inactive"),
    };

    const onActivateTab = vi.fn();

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onActivateTab={onActivateTab}
      />,
    );

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragOver).toBeTypeOf("function");

    // Start dragging inactive source tab
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

    // Hover over pane-edge of already-active tab
    dndHarness.props!.onDragOver!({
      over: {
        data: {
          current: {
            type: "pane-edge",
            tabId: "tab-active",
            leafId: "leaf-active",
            edge: "right",
          },
        },
      },
    });

    // onActivateTab must NOT be called because tab-active is already activeTabId
    expect(onActivateTab).not.toHaveBeenCalled();

    // Hover over pane-edge of inactive distinct target tab
    dndHarness.props!.onDragOver!({
      over: {
        data: {
          current: {
            type: "pane-edge",
            tabId: "tab-inactive",
            leafId: "leaf-inactive",
            edge: "left",
          },
        },
      },
    });

    // onActivateTab MUST be called to preview inactive target tab
    expect(onActivateTab).toHaveBeenCalledTimes(1);
    expect(onActivateTab).toHaveBeenCalledWith("tab-inactive");
  });
});
