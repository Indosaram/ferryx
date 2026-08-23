import type { ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const dndHarness = vi.hoisted(() => ({
  props: null as null | {
    onDragStart?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
  },
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: {
    children: ReactNode;
    onDragStart?: (event: unknown) => void;
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

describe("TerminalSplitView browser tab to pane edge drop", () => {
  it("routes browser tab drop onto a concrete terminal pane edge to pane-targeted split with targetPane", () => {
    // Given: one terminal target tab with leaf-target and one browser source tab (kind: 'browser') in the same group
    const terminalTargetTab: TerminalTab = {
      kind: "terminal",
      id: "terminal-target",
      label: "Terminal Target",
      sessionId: "session-target",
    };

    const browserSourceTab: BrowserTab = {
      kind: "browser",
      id: "browser-source",
      label: "Browser Source",
      browserId: "browser-1",
      url: "https://example.com",
    };

    const layout: LayoutState = {
      tabs: [terminalTargetTab, browserSourceTab],
      activeTabId: "terminal-target",
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: ["terminal-target", "browser-source"],
          activeTabId: "terminal-target",
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        "terminal-target": {
          root: { type: "leaf", leafId: "leaf-target" },
          activeLeafId: "leaf-target",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-target": "session-target" },
        },
      },
    };

    const sessions: Record<string, TerminalSession> = {
      "session-target": createSession("session-target"),
    };

    const onMoveTabToSplit = vi.fn();

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onMoveTabToSplit={onMoveTabToSplit}
      />,
    );

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragEnd).toBeTypeOf("function");

    // When: dragging the inactive browser source tab and dropping it onto the right edge of leaf-target
    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "tab",
            tabId: "browser-source",
            groupId: "group-default",
            index: 1,
          },
        },
      },
    });

    dndHarness.props!.onDragEnd!({
      over: {
        data: {
          current: {
            type: "pane-edge",
            tabId: "terminal-target",
            leafId: "leaf-target",
            edge: "right",
          },
        },
      },
    });

    // Then: onMoveTabToSplit is called with browser-source, group-default, horizontal, second, and targetPane
    expect(onMoveTabToSplit).toHaveBeenCalledWith(
      "browser-source",
      "group-default",
      "horizontal",
      "second",
      { tabId: "terminal-target", leafId: "leaf-target" },
    );
  });
});
