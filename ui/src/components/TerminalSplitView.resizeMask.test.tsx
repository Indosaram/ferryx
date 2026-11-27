import type { ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab, WorkspaceTab } from "../lib/types";
import type { TerminalActivity } from "../lib/activity";
import { __resetTabDisplayCacheForTests } from "../state/tabDisplay";

vi.mock("@dnd-kit/core", () => ({
  DndContext: ({ children }: { children: ReactNode }) => children,
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

// The native webview is the pane's own concern; the layout owner only decides the `visible` prop.
vi.mock("./BrowserPane", () => ({
  BrowserPane: ({ tab, visible }: { tab: { browserId: string }; visible?: boolean }) => (
    <div data-testid="browser-pane" data-browser-id={tab.browserId} data-visible={String(visible)} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

function clearPointerCaptures() {
  (globalThis as unknown as { __clearPointerCaptures?: () => void }).__clearPointerCaptures?.();
}

afterEach(() => {
  cleanup();
  clearPointerCaptures();
  document.body.style.cursor = "";
});

beforeEach(() => {
  clearPointerCaptures();
  __resetTabDisplayCacheForTests();
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

function terminalTab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function groupsFor(tabs: WorkspaceTab[], activeTabId: string): LayoutState["tabGroups"] {
  return {
    "group-default": { id: "group-default", tabIds: tabs.map((tab) => tab.id), activeTabId },
  };
}

/**
 * A terminal tab split into a native browser pane and a terminal pane. The browser leaf is what
 * the resize must hide: a live child webview paints above the DOM and would cover the drag.
 */
function splitBrowserLayout(browserId: string, direction: "horizontal" | "vertical" = "horizontal"): LayoutState {
  const tab = terminalTab("tab-1", "session-terminal");
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: groupsFor([tab], tab.id),
    tabGroupLayout: { type: "group", groupId: "group-default" },
    focusedGroupId: "group-default",
    layoutsByTabId: {
      [tab.id]: {
        root: {
          type: "split",
          direction,
          first: { type: "leaf", leafId: "leaf-browser" },
          second: { type: "leaf", leafId: "leaf-terminal" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-terminal",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-browser": "", "leaf-terminal": "session-terminal" },
        contentsByLeafId: {
          "leaf-browser": {
            kind: "browser",
            browser: { browserId, url: "https://example.com", title: "Example" },
          },
          "leaf-terminal": { kind: "terminal", sessionId: "session-terminal" },
        },
      },
    },
  };
}

/**
 * One tab, two terminal leaves, with the focused leaf as the only variable. `tabs` is passed in so
 * a rerender can keep the exact same array reference while the pane layout changes.
 */
function twoLeafLayout(tabs: WorkspaceTab[], activeLeafId: string): LayoutState {
  const [tab] = tabs;
  return {
    tabs,
    activeTabId: tab.id,
    tabGroups: groupsFor(tabs, tab.id),
    tabGroupLayout: { type: "group", groupId: "group-default" },
    focusedGroupId: "group-default",
    layoutsByTabId: {
      [tab.id]: {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-left" },
          second: { type: "leaf", leafId: "leaf-right" },
          ratio: 0.5,
        },
        activeLeafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-left": "session-left", "leaf-right": "session-right" },
        contentsByLeafId: {
          "leaf-left": { kind: "terminal", sessionId: "session-left" },
          "leaf-right": { kind: "terminal", sessionId: "session-right" },
        },
      },
    },
  };
}

const resizeDivider = () => screen.getByRole("separator", { name: "Resize terminal panes" });
const browserPane = () => screen.getByTestId("browser-pane");
const resizeMask = () => screen.queryByTestId("pane-resize-mask");

describe("TerminalSplitView pane resize mask", () => {
  it("captures the pointer, mounts the yielding mask, hides native browser panes, and restores both on pointerup", () => {
    const browserId = "browser-instance-resize";
    render(
      <TerminalSplitView
        layout={splitBrowserLayout(browserId)}
        sessions={{ "session-terminal": session("session-terminal") }}
      />,
    );

    expect(resizeMask()).toBeNull();
    expect(browserPane()).toHaveAttribute("data-browser-id", browserId);
    expect(browserPane()).toHaveAttribute("data-visible", "true");

    const divider = resizeDivider();
    const capture = vi.spyOn(Element.prototype, "setPointerCapture");

    fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });

    expect(capture).toHaveBeenCalledTimes(1);
    const mask = resizeMask();
    expect(mask).not.toBeNull();
    expect(mask).toHaveAttribute("data-native-terminal-force-yield", "true");
    expect(mask).toHaveAttribute("aria-hidden", "true");
    expect(mask).toHaveAttribute("role", "dialog");
    expect(mask).toHaveClass("fixed", "inset-0", "z-50", "cursor-col-resize");
    expect(browserPane()).toHaveAttribute("data-visible", "false");
    expect(document.body.style.cursor).toBe("col-resize");

    fireEvent.pointerUp(window, { clientX: 260, clientY: 100 });

    expect(resizeMask()).toBeNull();
    expect(browserPane()).toHaveAttribute("data-visible", "true");
    expect(document.body.style.cursor).toBe("");
  });

  it("ends an interrupted drag on lost pointer capture, dropping the mask, the listeners and the resize cursor", () => {
    const onSetRatio = vi.fn();
    render(
      <TerminalSplitView
        layout={splitBrowserLayout("browser-instance-interrupted", "vertical")}
        sessions={{ "session-terminal": session("session-terminal") }}
        onSetRatio={onSetRatio}
      />,
    );

    const divider = resizeDivider();
    const parent = divider.parentElement as HTMLElement;
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
      left: 0,
      right: 500,
      width: 500,
      top: 0,
      bottom: 500,
      height: 500,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    fireEvent.pointerDown(divider, { clientX: 200, clientY: 250 });

    expect(resizeMask()).toHaveClass("cursor-row-resize");
    expect(browserPane()).toHaveAttribute("data-visible", "false");
    expect(document.body.style.cursor).toBe("row-resize");

    fireEvent.pointerMove(window, { clientX: 200, clientY: 400 });

    // The window never delivers pointerup (a native surface took it); capture loss must finish it
    // and still commit the ratio the drag reached.
    fireEvent.lostPointerCapture(divider, { pointerId: 1 });

    expect(resizeMask()).toBeNull();
    expect(browserPane()).toHaveAttribute("data-visible", "true");
    expect(document.body.style.cursor).toBe("");
    expect(onSetRatio).toHaveBeenCalledTimes(1);
    expect(onSetRatio).toHaveBeenCalledWith("tab-1", "", 0.8, expect.objectContaining({ isolated: false }));

    // The document listeners are gone: a stray move after the interrupted drag writes nothing.
    fireEvent.pointerMove(window, { clientX: 200, clientY: 100 });
    fireEvent.pointerUp(window, { clientX: 200, clientY: 100 });
    expect(onSetRatio).toHaveBeenCalledTimes(1);
    expect(resizeMask()).toBeNull();
  });
});

describe("TerminalSplitView tab labels", () => {
  it("recomputes tab labels when only the pane layout changes", () => {
    const tabs: WorkspaceTab[] = [terminalTab("tab-1", "session-left")];
    const sessions = {
      "session-left": session("session-left"),
      "session-right": session("session-right"),
    };
    const activityBySessionId: Record<string, TerminalActivity> = {
      "session-right": { state: "working", title: "π fixing the resize mask", isAgent: true },
    };

    const tabRow = () => document.querySelector('[data-tab-dnd-id="tab-1"]') as HTMLElement;

    const { rerender } = render(
      <TerminalSplitView
        layout={twoLeafLayout(tabs, "leaf-left")}
        sessions={sessions}
        activityBySessionId={activityBySessionId}
      />,
    );

    // The focused leaf is the left pane, which has no agent title: the tab keeps its base label.
    expect(tabRow()).toHaveTextContent("tab-1");
    expect(tabRow()).not.toHaveTextContent("fixing the resize mask");

    // Same `tabs` array identity, different focused leaf: only layoutsByTabId changed.
    rerender(
      <TerminalSplitView
        layout={twoLeafLayout(tabs, "leaf-right")}
        sessions={sessions}
        activityBySessionId={activityBySessionId}
      />,
    );

    expect(tabRow()).toHaveTextContent("fixing the resize mask");
  });
});
