import type { ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const dndHarness = vi.hoisted(() => ({
  props: null as null | {
    onDragStart?: (event: unknown) => void;
    onDragEnd?: (event: unknown) => void;
  },
  draggableArgs: [] as Array<{
    id: string;
    disabled?: boolean;
    data?: unknown;
  }>,
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: { children: ReactNode; onDragStart?: (event: unknown) => void; onDragEnd?: (event: unknown) => void }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  pointerWithin: vi.fn(() => []),
  useDraggable: (args: { id: string; disabled?: boolean; data?: unknown }) => {
    dndHarness.draggableArgs.push(args);
    return { setNodeRef: vi.fn(), attributes: {}, listeners: {}, isDragging: false };
  },
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
  TerminalPane: ({ session }: { session: TerminalSession }) => <div data-testid="terminal-pane" data-session-id={session.id} />,
}));

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
  dndHarness.draggableArgs.length = 0;
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
          second: {
            type: "split",
            direction: "vertical",
            first: { type: "leaf", leafId: "leaf-b" },
            second: { type: "leaf", leafId: "leaf-c" },
            ratio: 0.5,
          },
          ratio: 0.4,
        },
        activeLeafId: "leaf-a",
        expandedLeafId: null,
        sessionIdsByLeafId: {
          "leaf-a": "session-a",
          "leaf-b": "session-b",
          "leaf-c": "session-c",
        },
      },
    },
  };
}

function singlePaneLayout(): LayoutState {
  const sourceTab: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-source" };
  const targetTab: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-target" };
  return {
    tabs: [sourceTab, targetTab],
    activeTabId: sourceTab.id,
    tabGroups: {
      "group-main": {
        id: "group-main",
        tabIds: [sourceTab.id, targetTab.id],
        activeTabId: sourceTab.id,
      },
    },
    tabGroupLayout: { type: "group", groupId: "group-main" },
    focusedGroupId: "group-main",
    layoutsByTabId: {
      [sourceTab.id]: {
        root: { type: "leaf", leafId: "leaf-source" },
        activeLeafId: "leaf-source",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-source": "session-source" },
      },
      [targetTab.id]: {
        root: { type: "leaf", leafId: "leaf-target" },
        activeLeafId: "leaf-target",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-target": "session-target" },
      },
    },
  };
}

describe("TerminalSplitView pane-handle edge execution", () => {
  it("registers every pane toolbar as a draggable with pane/session source payload", () => {
    render(
      <TerminalSplitView
        layout={splitLayout()}
        sessions={{
          "session-a": session("session-a"),
          "session-b": session("session-b"),
          "session-c": session("session-c"),
        }}
      />,
    );

    expect(dndHarness.draggableArgs).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "pane:tab-main:leaf-a",
          data: {
            type: "pane",
            tabId: "tab-main",
            sourcePaneId: "leaf-a",
            sourceSessionId: "session-a",
          },
        }),
      ]),
    );
    expect(dndHarness.draggableArgs.some((args) => args.disabled === true)).toBe(false);
  });

  it("composes detach + pane-targeted tab merge so the dragged PTY is moved instead of respawned", () => {
    const onDetachPaneToTab = vi.fn(() => "tab-detached");
    const onMoveTabToSplit = vi.fn();

    render(
      <TerminalSplitView
        layout={splitLayout()}
        sessions={{
          "session-a": session("session-a"),
          "session-b": session("session-b"),
          "session-c": session("session-c"),
        }}
        onDetachPaneToTab={onDetachPaneToTab}
        onMoveTabToSplit={onMoveTabToSplit}
      />,
    );

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragEnd).toBeTypeOf("function");

    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "pane",
            tabId: "tab-main",
            sourcePaneId: "leaf-a",
            sourceSessionId: "session-a",
          },
        },
      },
    });
    dndHarness.props!.onDragEnd!({
      over: { data: { current: { type: "pane-edge", tabId: "tab-main", leafId: "leaf-c", edge: "top" } } },
    });

    expect(onDetachPaneToTab).toHaveBeenCalledWith("tab-main", "leaf-a", "group-main");
    expect(onMoveTabToSplit).toHaveBeenCalledWith(
      "tab-detached",
      "group-main",
      "vertical",
      "first",
      { tabId: "tab-main", leafId: "leaf-c" },
    );
  });

  it("moves a one-pane source tab directly into the hovered pane edge instead of disabling the handle", () => {
    const onDetachPaneToTab = vi.fn(() => "unexpected-detached-tab");
    const onMoveTabToSplit = vi.fn();

    render(
      <TerminalSplitView
        layout={singlePaneLayout()}
        sessions={{
          "session-source": session("session-source"),
          "session-target": session("session-target"),
        }}
        onDetachPaneToTab={onDetachPaneToTab}
        onMoveTabToSplit={onMoveTabToSplit}
      />,
    );

    expect(dndHarness.draggableArgs).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "pane:tab-source:leaf-source",
          data: {
            type: "pane",
            tabId: "tab-source",
            sourcePaneId: "leaf-source",
            sourceSessionId: "session-source",
          },
        }),
      ]),
    );

    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "pane",
            tabId: "tab-source",
            sourcePaneId: "leaf-source",
            sourceSessionId: "session-source",
          },
        },
      },
    });
    dndHarness.props!.onDragEnd!({
      over: { data: { current: { type: "pane-edge", tabId: "tab-target", leafId: "leaf-target", edge: "right" } } },
    });

    expect(onDetachPaneToTab).not.toHaveBeenCalled();
    expect(onMoveTabToSplit).toHaveBeenCalledWith(
      "tab-source",
      "group-main",
      "horizontal",
      "second",
      { tabId: "tab-target", leafId: "leaf-target" },
    );
  });
});
