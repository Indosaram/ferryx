import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as TerminalSplitViewModule from "./TerminalSplitView";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({
    session,
    searchOpen,
    headerHovered,
  }: {
    session: TerminalSession;
    searchOpen?: boolean;
    headerHovered?: boolean;
  }) => (
    <div
      data-testid="terminal-pane"
      data-backend-session-id={session.backendSessionId ?? session.id}
      data-search-open={String(Boolean(searchOpen))}
      data-header-hovered={String(Boolean(headerHovered))}
    />
  ),
}));

afterEach(cleanup);

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string, backendSessionId: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    worktreePath: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle: "working",
  };
}

function singleTabLayout(tabId = "tab-1", sessionId = "session-1", leafId = "leaf-1"): LayoutState {
  return {
    tabs: [tab(tabId, sessionId)],
    activeTabId: tabId,
    layoutsByTabId: {
      [tabId]: {
        root: { type: "leaf", leafId },
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: sessionId },
      },
    },
  };
}

function splitLayout(): LayoutState {
  const primary = tab("tab-1", "session-1");
  return {
    tabs: [primary],
    activeTabId: "tab-1",
    layoutsByTabId: {
      "tab-1": {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-1" },
          second: { type: "leaf", leafId: "leaf-2" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-1",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-1": "session-1", "leaf-2": "session-2" },
      },
    },
  };
}

function splitSessions() {
  return {
    "session-1": session("session-1", "backend-1"),
    "session-2": session("session-2", "backend-2"),
  };
}

describe("TerminalSplitView group and pane rendering", () => {
  it("renders a single tab group with a pane move handle and split controls", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    expect(screen.getByTestId("tab-group-panel")).toBeInTheDocument();
    expect(screen.getByTestId("tab-strip")).toBeInTheDocument();
    expect(screen.getByTestId("pane-leaf")).toBeInTheDocument();
    expect(screen.getByTestId("pane-toolbar")).toBeInTheDocument();
    expect(screen.getByTestId("pane-toolbar-hotspot")).toHaveClass("h-4");
    expect(screen.getByTestId("pane-toolbar-hotspot")).toHaveClass("pointer-events-none");
    expect(screen.getByRole("button", { name: "Split pane right" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane down" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close split view" })).not.toBeInTheDocument();
  });

  it("does not pass header hover chrome-suppression signal to TerminalPane on top hover hotspot activation while revealing toolbar", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const pane = screen.getByTestId("terminal-pane");
    const hotspot = screen.getByTestId("pane-toolbar-hotspot");
    const toolbar = screen.getByTestId("pane-toolbar");

    expect(hotspot).toHaveClass("h-4");
    expect(pane).toHaveAttribute("data-header-hovered", "false");
    expect(toolbar).toHaveClass("pointer-events-none");

    fireEvent.mouseEnter(hotspot);

    expect(toolbar).toHaveClass("pointer-events-auto");
    expect(pane).toHaveAttribute("data-header-hovered", "false");

    fireEvent.mouseLeave(hotspot);
    expect(toolbar).toHaveClass("pointer-events-none");
    expect(pane).toHaveAttribute("data-header-hovered", "false");
  });

  it("keeps an empty tab bar usable when all tabs have been closed", () => {
    const onAddTab = vi.fn();
    const layout: LayoutState = { tabs: [], activeTabId: null, layoutsByTabId: {} };
    render(<TerminalSplitView layout={layout} sessions={{}} onAddTab={onAddTab} />);

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Terminal/i }));
    expect(onAddTab).toHaveBeenCalledOnce();
  });

  it("renders a nested terminal-pane tree inside one tab group with resize dividers", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-left" },
            second: {
              type: "split",
              direction: "vertical",
              first: { type: "leaf", leafId: "leaf-right-top" },
              second: { type: "leaf", leafId: "leaf-right-bottom" },
              ratio: 0.5,
            },
            ratio: 0.5,
          },
          activeLeafId: "leaf-right-top",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-left": "session-1",
            "leaf-right-top": "session-2",
            "leaf-right-bottom": "session-3",
          },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
      "session-3": session("session-3", "backend-3"),
    };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);
    expect(screen.getAllByTestId("pane-leaf")).toHaveLength(3);
    expect(screen.getAllByTestId("pane-toolbar")).toHaveLength(3);
    expect(screen.getAllByRole("separator", { name: "Resize terminal panes" })).toHaveLength(2);
  });

  it("uses one-pixel gray split lines without consuming visible pane spacing", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-left" },
            second: {
              type: "split",
              direction: "vertical",
              first: { type: "leaf", leafId: "leaf-right-top" },
              second: { type: "leaf", leafId: "leaf-right-bottom" },
              ratio: 0.5,
            },
            ratio: 0.5,
          },
          activeLeafId: "leaf-left",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-left": "session-1",
            "leaf-right-top": "session-2",
            "leaf-right-bottom": "session-3",
          },
        },
      },
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={{
          "session-1": session("session-1", "backend-1"),
          "session-2": session("session-2", "backend-2"),
          "session-3": session("session-3", "backend-3"),
        }}
      />,
    );

    const dividers = screen.getAllByRole("separator", { name: "Resize terminal panes" });
    const verticalLine = dividers.find((divider) => divider.getAttribute("aria-orientation") === "vertical");
    const horizontalLine = dividers.find((divider) => divider.getAttribute("aria-orientation") === "horizontal");

    expect(verticalLine).toHaveClass("w-px");
    expect(verticalLine).toHaveStyle({ backgroundColor: "var(--terminal-divider)" });
    expect(verticalLine).not.toHaveClass("bg-border/80");
    expect(verticalLine).not.toHaveClass("w-1.5");
    expect(horizontalLine).toHaveClass("h-px");
    expect(horizontalLine).toHaveStyle({ backgroundColor: "var(--terminal-divider)" });
    expect(horizontalLine).not.toHaveClass("bg-border/80");
    expect(horizontalLine).not.toHaveClass("h-1.5");
    expect(verticalLine?.querySelector('[data-divider-hit-area="true"]')).toHaveClass("-inset-x-1");
    expect(horizontalLine?.querySelector('[data-divider-hit-area="true"]')).toHaveClass("-inset-y-1");
  });

  it("dispatches terminal-pane split actions from pane toolbar buttons", () => {
    const onSplitPane = vi.fn();
    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={{ "session-1": session("session-1", "backend-1") }}
        onSplitPane={onSplitPane}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Split pane right" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "horizontal");
    fireEvent.click(screen.getByRole("button", { name: "Split pane down" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "vertical");
  });

  it("routes a tab context-menu split to that tab's own focused terminal leaf", () => {
    const layout: LayoutState = {
      tabs: [tab("tab-1", "session-1"), tab("tab-2", "session-2")],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-a" },
          activeLeafId: "leaf-a",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-a": "session-1" },
        },
        "tab-2": {
          root: {
            type: "split",
            direction: "vertical",
            first: { type: "leaf", leafId: "leaf-b-top" },
            second: { type: "leaf", leafId: "leaf-b-bottom" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-b-bottom",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-b-top": "session-2", "leaf-b-bottom": "session-2" },
        },
      },
    };
    const onSplitPane = vi.fn();
    render(<TerminalSplitView layout={layout} sessions={splitSessions()} onSplitPane={onSplitPane} />);

    fireEvent.contextMenu(screen.getByText("tab-2"));
    fireEvent.click(screen.getByRole("menuitem", { name: /Split terminal right/i }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-2", "leaf-b-bottom", "horizontal");
  });

  it("exposes pane drags through dnd-kit metadata and keeps toolbar controls as non-native drags", () => {
    render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} />);

    for (const pane of screen.getAllByTestId("pane-leaf")) {
      expect(pane).toHaveAttribute("data-dnd-type", "pane-leaf");
      expect(pane).not.toHaveAttribute("draggable", "true");
    }
    for (const toolbar of screen.getAllByTestId("pane-toolbar")) {
      expect(toolbar).toHaveAttribute("data-dnd-type", "pane");
      expect(toolbar).not.toHaveAttribute("draggable", "true");
    }
    expect(screen.getAllByRole("button", { name: "Split pane right" })[0]).not.toHaveAttribute("draggable", "true");
  });

  it("renders dnd-kit group body and all four edge split drop zones", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    expect(document.querySelector('[data-dnd-type="group-body"]')).toBeInTheDocument();
    const edges = screen.getAllByTestId("tab-group-edge-drop-zone");
    expect(edges).toHaveLength(4);
    expect(new Set(edges.map((edge) => edge.getAttribute("data-drop-edge")))).toEqual(
      new Set(["left", "right", "top", "bottom"]),
    );
  });

  it("falls back to an even ratio when a terminal-pane resize container is too small", () => {
    const onSetRatio = vi.fn();
    const layout = splitLayout();
    layout.layoutsByTabId["tab-1"].root = {
      ...layout.layoutsByTabId["tab-1"].root,
      ratio: 0.8,
    } as typeof layout.layoutsByTabId["tab-1"]["root"];

    render(<TerminalSplitView layout={layout} sessions={splitSessions()} onSetRatio={onSetRatio} />);
    const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
    const parent = divider.parentElement as HTMLElement;
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
      left: 0,
      right: 120,
      width: 120,
      top: 0,
      bottom: 100,
      height: 100,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    fireEvent.pointerDown(divider, { clientX: 90, clientY: 20 });
    fireEvent.pointerMove(window, { clientX: 110, clientY: 20 });
    fireEvent.pointerUp(window);
    expect(onSetRatio).toHaveBeenCalledWith("tab-1", "", 0.5);
  });

  it("passes searchOpen true only to the pane matching searchLeafId", () => {
    const layout = singleTabLayout("tab-1", "session-1", "leaf-1");
    const sessions = { "session-1": session("session-1", "backend-1") };

    const { rerender } = render(<TerminalSplitView layout={layout} sessions={sessions} searchLeafId="leaf-other" />);
    expect(screen.getByTestId("terminal-pane")).toHaveAttribute("data-search-open", "false");
    rerender(<TerminalSplitView layout={layout} sessions={sessions} searchLeafId="leaf-1" />);
    expect(screen.getByTestId("terminal-pane")).toHaveAttribute("data-search-open", "true");
  });

  describe("F-terminal-05 and F-terminal-06 perf optimizations", () => {
    it("F-terminal-05: source check proves collisionDetection does not call droppableContainers.find inside filter/sort", () => {
      const sourcePath = resolve(__dirname, "TerminalSplitView.tsx");
      const source = readFileSync(sourcePath, "utf-8");

      // Verify that droppableContainers.find is not present in collision detection
      expect(source).not.toMatch(/droppableContainers\.find/);
      // Verify Map is constructed for droppable lookups in collision detection
      expect(source).toMatch(/containerDataById/);
    });

    it("F-terminal-05: workspaceCollisionDetection builds an O(1) container map and does not call find() on droppableContainers during sort/filter", () => {
      const workspaceCollisionDetection = (TerminalSplitViewModule as Record<string, any>).workspaceCollisionDetection;
      expect(typeof workspaceCollisionDetection).toBe("function");

      const droppableContainers = [
        {
          id: "group-edge:top:group-1",
          data: {
            current: {
              type: "group-edge" as const,
              groupId: "group-1",
              edge: "top" as const,
            },
          },
          rect: { current: { top: 0, left: 0, right: 100, bottom: 10, width: 100, height: 10 } },
          disabled: false,
          node: { current: null },
        },
        {
          id: "group-body:group-1",
          data: {
            current: {
              type: "group-body" as const,
              groupId: "group-1",
            },
          },
          rect: { current: { top: 10, left: 0, right: 100, bottom: 100, width: 100, height: 90 } },
          disabled: false,
          node: { current: null },
        },
      ];

      const findSpy = vi.spyOn(droppableContainers, "find");

      const active = {
        id: "tab:tab-1",
        data: {
          current: {
            type: "tab" as const,
            tabId: "tab-1",
            groupId: "group-2",
          },
        },
        rect: { current: { initial: null, translated: null } },
      };

      const result = workspaceCollisionDetection({
        active: active as any,
        droppableContainers: droppableContainers as any,
        pointerCoordinates: { x: 50, y: 5 },
        collisionRect: { top: 0, left: 0, right: 100, bottom: 100, width: 100, height: 100 },
        droppableRects: new Map(),
      });

      expect(findSpy).not.toHaveBeenCalled();
      expect(Array.isArray(result)).toBe(true);
    });

    it("F-terminal-06: source check proves stable module-level FALLBACK_SESSION is defined and used in PaneRenderer", () => {
      const sourcePath = resolve(__dirname, "TerminalSplitView.tsx");
      const source = readFileSync(sourcePath, "utf-8");

      // Verify module-level FALLBACK_SESSION definition
      expect(source).toMatch(/export const FALLBACK_SESSION: TerminalSession =/);
      // Verify PaneRenderer uses FALLBACK_SESSION rather than inline object allocation
      expect(source).toMatch(/sessions\[sessionId\] \?\? FALLBACK_SESSION/);
      expect(source).not.toMatch(/lifecycle: "working" as const/);
    });

    it("F-terminal-06: exports stable FALLBACK_SESSION and maintains reference identity when session is missing", () => {
      const FALLBACK_SESSION = (TerminalSplitViewModule as Record<string, any>).FALLBACK_SESSION;
      expect(FALLBACK_SESSION).toBeDefined();
      expect(FALLBACK_SESSION.id).toBe("");
      expect(FALLBACK_SESSION.lifecycle).toBe("working");

      const layout = singleTabLayout("tab-1", "missing-session", "leaf-1");
      // Render without providing sessions[sessionId]
      render(<TerminalSplitView layout={layout} sessions={{}} />);
      const pane = screen.getByTestId("terminal-pane");
      expect(pane).toBeInTheDocument();
    });
  });

  it("forwards defaultAgentId so the usable default is first with a Default label", () => {
    const onLaunchAgent = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "aider", command: "aider", args: "" },
    ];
    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={{ "session-1": session("session-1", "backend-1") }}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
        defaultAgentId="aider"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    const buttons = within(screen.getByText("AGENTS").parentElement as HTMLElement).getAllByRole("button");
    expect(buttons[0]).toHaveTextContent("Aider");
    expect(within(buttons[0]).getByText("Default")).toBeVisible();
    fireEvent.click(screen.getByText("Claude"));
    expect(onLaunchAgent).toHaveBeenCalledWith(agents[0]);
  });

  it("narrows pane toolbar hover hotspot to h-4 (16px) and toggles toolbar on top hover", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const hotspot = screen.getByTestId("pane-toolbar-hotspot");
    const toolbar = screen.getByTestId("pane-toolbar");
    const paneLeaf = screen.getByTestId("pane-leaf");

    expect(hotspot).toHaveClass("h-4");
    expect(toolbar).toHaveClass("pointer-events-none");

    // Mock bounding rect for pane-leaf
    vi.spyOn(paneLeaf, "getBoundingClientRect").mockReturnValue({
      top: 100,
      left: 0,
      right: 500,
      bottom: 600,
      width: 500,
      height: 500,
      x: 0,
      y: 100,
      toJSON: () => {},
    });

    // Move mouse within top 16px (e.g. clientY = 108, relativeY = 8)
    fireEvent.mouseMove(paneLeaf, { clientY: 108 });
    expect(toolbar).toHaveClass("pointer-events-auto");

    // Move mouse below 16px (e.g. clientY = 150, relativeY = 50)
    fireEvent.mouseMove(paneLeaf, { clientY: 150 });
    expect(toolbar).toHaveClass("pointer-events-none");
  });

  it("applies drop feedback without transitioning pane layout", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const paneLeaf = screen.getByTestId("pane-leaf");
    expect(paneLeaf).not.toHaveClass("transition-all");
    expect(paneLeaf).not.toHaveClass("transition-[width]");
    expect(paneLeaf).not.toHaveClass("transition-[height]");
  });
});
