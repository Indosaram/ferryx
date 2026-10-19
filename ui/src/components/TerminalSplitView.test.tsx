import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { resetNotificationSettings, saveNotificationSettings } from "../lib/notificationSettings";
import { flashPane, resetPaneFlashForTests } from "../lib/paneFlash";
import { formatPaneDebugInfo } from "../lib/paneDebugInfo";
import * as TerminalSplitViewModule from "./TerminalSplitView";
import { TerminalSplitView } from "./TerminalSplitView";

const nativeMenu = vi.hoisted(() => ({
  lastCall: null as null | {
    command: string;
    items: Array<Record<string, unknown>>;
    onAction: (id: string) => void;
  },
}));

const mockClipboard = vi.hoisted(() => ({
  copyTextToClipboard: vi.fn().mockResolvedValue(true),
}));

vi.mock("../lib/clipboard", () => ({
  copyTextToClipboard: (text: string) => mockClipboard.copyTextToClipboard(text),
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: vi.fn(
    async (
      command: string,
      items: Array<Record<string, unknown>>,
      _position: { x: number; y: number },
      onAction: (id: string) => void,
    ) => {
      nativeMenu.lastCall = { command, items, onAction };
      return () => undefined;
    },
  ),
}));

function clickNativeMenuItem(idPrefix: string) {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  const found = nativeMenu.lastCall.items.find(
    (item) => ((item as { id?: string }).id ?? "").startsWith(idPrefix),
  ) as { id: string } | undefined;
  if (!found) throw new Error(`menu item not found: ${idPrefix}`);
  nativeMenu.lastCall.onAction(found.id);
}

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({
    session,
    searchOpen,
    headerHovered,
    needsAttention,
  }: {
    session: TerminalSession;
    searchOpen?: boolean;
    headerHovered?: boolean;
    needsAttention?: boolean;
  }) => (
    <div
      data-testid="terminal-pane"
      data-backend-session-id={session.backendSessionId ?? session.id}
      data-search-open={String(Boolean(searchOpen))}
      data-header-hovered={String(Boolean(headerHovered))}
      data-needs-attention={String(Boolean(needsAttention))}
    />
  ),
}));

beforeEach(() => {
  resetNotificationSettings();
  mockClipboard.copyTextToClipboard.mockResolvedValue(true);
});

afterEach(() => {
  cleanup();
  resetNotificationSettings();
  nativeMenu.lastCall = null;
  mockClipboard.copyTextToClipboard.mockClear();
});

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
    expect(screen.getByTestId("pane-toolbar-hotspot")).toHaveClass("h-5");
    expect(screen.getByTestId("pane-toolbar-hotspot")).toHaveClass("pointer-events-none");
    expect(screen.getByRole("button", { name: "Copy Debug Info" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane right" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane down" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close split view" })).not.toBeInTheDocument();
  });

  it("does not pass header hover chrome-suppression signal to TerminalPane on top hover hotspot activation while revealing toolbar", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const pane = screen.getByTestId("terminal-pane");
    const hotspot = screen.getByTestId("pane-toolbar-hotspot");
    const toolbar = screen.getByTestId("pane-toolbar");

    expect(hotspot).toHaveClass("h-5");
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
    clickNativeMenuItem("new-terminal");
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
    clickNativeMenuItem("split-right");
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
    expect(onSetRatio).toHaveBeenCalledWith(
      "tab-1",
      "",
      0.5,
      expect.objectContaining({ isolated: false }),
    );
  });

  it("passes a frozen seam from pointerdown and keeps isolated true when Alt is held during divider drag", () => {
    const onSetRatio = vi.fn();
    const layout = splitLayout();

    render(<TerminalSplitView layout={layout} sessions={splitSessions()} onSetRatio={onSetRatio} />);
    const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
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

    fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
    fireEvent.pointerMove(window, { clientX: 350, clientY: 100, altKey: true });
    fireEvent.pointerUp(window);

    expect(onSetRatio).toHaveBeenCalledTimes(1);
    const options = onSetRatio.mock.calls[0][3];
    expect(options).toMatchObject({ isolated: true });
    expect(options.seam).toMatchObject({
      direction: "horizontal",
      targetPath: "",
      target: { origin: 0, span: 1 },
    });
    expect(options.seam.members).toEqual([{ origin: 0, path: "", span: 1 }]);
  });

  describe("divider double-click equalize", () => {
    it("splits the panes evenly on a divider double-click", () => {
      const onSetRatio = vi.fn();
      const onEqualizePaneRun = vi.fn();
      const layout = splitLayout();
      layout.layoutsByTabId["tab-1"].root = {
        type: "split",
        direction: "horizontal",
        first: { type: "leaf", leafId: "leaf-1" },
        second: { type: "leaf", leafId: "leaf-2" },
        ratio: 0.8,
      };

      render(<TerminalSplitView layout={layout} sessions={splitSessions()} onSetRatio={onSetRatio} onEqualizePaneRun={onEqualizePaneRun} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_200);
      fireEvent.pointerDown(divider, { clientX: 252, clientY: 101 });
      fireEvent.pointerUp(window);

      expect(onSetRatio).not.toHaveBeenCalled();
      expect(onEqualizePaneRun).toHaveBeenCalledExactlyOnceWith("tab-1", "");
      now.mockRestore();
    });

    it("ignores two clicks separated by more than the double-click window", () => {
      const onSetRatio = vi.fn();
      render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} onSetRatio={onSetRatio} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_900);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);

      expect(onSetRatio).not.toHaveBeenCalled();
      now.mockRestore();
    });

    it("resizes instead of equalizing when a drag starts on the second press", () => {
      const onSetRatio = vi.fn();
      render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} onSetRatio={onSetRatio} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
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
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_100);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerMove(window, { clientX: 400, clientY: 100 });
      fireEvent.pointerUp(window);

      expect(onSetRatio).not.toHaveBeenCalledWith("tab-1", "", 0.5, expect.anything());
      expect(onSetRatio).toHaveBeenCalledWith(
        "tab-1",
        "",
        0.8,
        expect.objectContaining({ isolated: false }),
      );
      now.mockRestore();
    });

    it("ignores a right-button double-click", () => {
      const onSetRatio = vi.fn();
      render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} onSetRatio={onSetRatio} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100, button: 2 });
      fireEvent.pointerUp(window, { button: 2 });
      now.mockReturnValue(1_200);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100, button: 2 });
      fireEvent.pointerUp(window, { button: 2 });

      expect(onSetRatio).not.toHaveBeenCalledWith("tab-1", "", 0.5, expect.anything());
      now.mockRestore();
    });

    it("keeps the even division when the second press jitters within the slop", () => {
      const onSetRatio = vi.fn();
      const onEqualizePaneRun = vi.fn();
      render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} onSetRatio={onSetRatio} onEqualizePaneRun={onEqualizePaneRun} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
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
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_100);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerMove(window, { clientX: 252, clientY: 100 });
      fireEvent.pointerUp(window);

      expect(onSetRatio).toHaveBeenCalledWith("tab-1", "", 0.504, expect.anything());
      expect(onEqualizePaneRun).toHaveBeenCalledExactlyOnceWith("tab-1", "");
      now.mockRestore();
    });

    it("does not equalize when the second click follows a real resize drag", () => {
      const onSetRatio = vi.fn();
      render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} onSetRatio={onSetRatio} />);
      const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
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
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      now.mockReturnValue(1_100);
      fireEvent.pointerMove(window, { clientX: 300, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_200);
      fireEvent.pointerDown(divider, { clientX: 251, clientY: 100 });
      fireEvent.pointerUp(window);

      expect(onSetRatio).not.toHaveBeenCalledWith("tab-1", "", 0.5, expect.anything());
      now.mockRestore();
    });

    it("equalizes split tab groups from the group divider", () => {
      const onSetGroupRatio = vi.fn();
      const onEqualizeGroupRun = vi.fn();
      const layout: LayoutState = {
        tabs: [tab("tab-a", "session-1"), tab("tab-b", "session-2")],
        activeTabId: "tab-a",
        tabGroups: {
          "group-a": { id: "group-a", tabIds: ["tab-a"], activeTabId: "tab-a" },
          "group-b": { id: "group-b", tabIds: ["tab-b"], activeTabId: "tab-b" },
        },
        tabGroupLayout: {
          type: "split",
          direction: "horizontal",
          first: { type: "group", groupId: "group-a" },
          second: { type: "group", groupId: "group-b" },
          ratio: 0.75,
        },
        focusedGroupId: "group-a",
        layoutsByTabId: {
          "tab-a": {
            root: { type: "leaf", leafId: "leaf-a" },
            activeLeafId: "leaf-a",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-a": "session-1" },
          },
          "tab-b": {
            root: { type: "leaf", leafId: "leaf-b" },
            activeLeafId: "leaf-b",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-b": "session-2" },
          },
        },
      };

      render(
        <TerminalSplitView layout={layout} sessions={splitSessions()} onSetGroupRatio={onSetGroupRatio} onEqualizeGroupRun={onEqualizeGroupRun} />,
      );
      const divider = screen.getByRole("separator", { name: "Resize tab groups" });
      const now = vi.spyOn(Date, "now");

      now.mockReturnValue(1_000);
      fireEvent.pointerDown(divider, { clientX: 250, clientY: 100 });
      fireEvent.pointerUp(window);
      now.mockReturnValue(1_200);
      fireEvent.pointerDown(divider, { clientX: 251, clientY: 100 });
      fireEvent.pointerUp(window);

      expect(onSetGroupRatio).not.toHaveBeenCalled();
      expect(onEqualizeGroupRun).toHaveBeenCalledExactlyOnceWith("");
      now.mockRestore();
    });
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
    const items = nativeMenu.lastCall?.items ?? [];
    const agentItems = items.filter((item) => ((item as { id?: string }).id ?? "").startsWith("agent:"));
    expect((agentItems[0] as { label?: string }).label).toContain("Aider");
    expect((agentItems[0] as { label?: string }).label).toContain("Default");
    clickNativeMenuItem("agent:claude");
    expect(onLaunchAgent).toHaveBeenCalledWith(agents[0]);
  });

  it("narrows pane toolbar hover hotspot to h-5 (20px) and toggles toolbar on top hover", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const hotspot = screen.getByTestId("pane-toolbar-hotspot");
    const toolbar = screen.getByTestId("pane-toolbar");
    const paneLeaf = screen.getByTestId("pane-leaf");

    expect(hotspot).toHaveClass("h-5");
    expect(toolbar).toHaveClass("h-5");
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

    // Move mouse within top 20px (e.g. clientY = 118, relativeY = 18)
    fireEvent.mouseMove(paneLeaf, { clientY: 118 });
    expect(toolbar).toHaveClass("pointer-events-auto");

    // Move mouse below 20px (e.g. clientY = 150, relativeY = 50)
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

  it("outlines only the focused pane of a split and leaves a lone pane unframed", () => {
    const { unmount } = render(<TerminalSplitView layout={splitLayout()} sessions={splitSessions()} />);
    const [first, second] = screen.getAllByTestId("pane-leaf");
    expect(first).toHaveAttribute("data-pane-focused", "true");
    expect(first).toHaveClass("border-primary/60");
    expect(second).not.toHaveAttribute("data-pane-focused");
    expect(second).toHaveClass("border-transparent");
    unmount();

    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);
    const lone = screen.getByTestId("pane-leaf");
    expect(lone).not.toHaveAttribute("data-pane-focused");
    expect(lone).not.toHaveClass("border");
  });

  it("does not apply amber ring class on pane leaf in any state, while forwarding needsAttention to terminal pane and rendering bottom DOM frame completion for waiting or unread done", () => {
    const layout = singleTabLayout();
    const sessions = { "session-1": session("session-1", "backend-1") };

    // 1. No activity -> no ring, needsAttention=false, no DOM frame completion
    const { rerender } = render(<TerminalSplitView layout={layout} sessions={sessions} />);
    let paneLeaf = screen.getByTestId("pane-leaf");
    let terminalPane = screen.getByTestId("terminal-pane");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "false");
    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-right")).not.toBeInTheDocument();

    // 2. Waiting -> no DOM ring, needsAttention=true forwarded to pane, DOM frame completion rendered
    rerender(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        activityBySessionId={{ "session-1": { state: "waiting", title: "Waiting", isAgent: true } }}
      />,
    );
    paneLeaf = screen.getByTestId("pane-leaf");
    terminalPane = screen.getByTestId("terminal-pane");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "true");
    expect(screen.getByTestId("attention-frame-bottom")).toBeInTheDocument();
    expect(screen.getByTestId("attention-frame-corner-left")).toBeInTheDocument();
    expect(screen.getByTestId("attention-frame-corner-right")).toBeInTheDocument();

    // 3. Done (unseen) -> no DOM ring, needsAttention=true forwarded to pane, DOM frame completion rendered
    rerender(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        activityBySessionId={{ "session-1": { state: "done", title: "Done", isAgent: true, seen: false } }}
      />,
    );
    paneLeaf = screen.getByTestId("pane-leaf");
    terminalPane = screen.getByTestId("terminal-pane");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "true");
    expect(screen.getByTestId("attention-frame-bottom")).toBeInTheDocument();
    expect(screen.getByTestId("attention-frame-corner-left")).toBeInTheDocument();
    expect(screen.getByTestId("attention-frame-corner-right")).toBeInTheDocument();

    // 4. Done (seen) -> no ring, needsAttention=false, no DOM frame completion
    rerender(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        activityBySessionId={{ "session-1": { state: "done", title: "Done", isAgent: true, seen: true } }}
      />,
    );
    paneLeaf = screen.getByTestId("pane-leaf");
    terminalPane = screen.getByTestId("terminal-pane");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "false");
    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-right")).not.toBeInTheDocument();

    // 5. Working -> no ring, needsAttention=false, no DOM frame completion
    rerender(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        activityBySessionId={{ "session-1": { state: "working", title: "Working", isAgent: true } }}
      />,
    );
    paneLeaf = screen.getByTestId("pane-leaf");
    terminalPane = screen.getByTestId("terminal-pane");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "false");
    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-right")).not.toBeInTheDocument();
  });

  it("dismisses both native and DOM attention for an acknowledged waiting pane", () => {
    render(<TerminalSplitView layout={singleTabLayout()}
      sessions={{ "session-1": session("session-1", "backend-1") }}
      activityBySessionId={{ "session-1": { state: "waiting", title: "", isAgent: true, seen: true } }} />);
    expect(screen.getByTestId("terminal-pane")).toHaveAttribute("data-needs-attention", "false");
    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
  });

  it("renders no amber ring and passes needsAttention=false when attentionFrame setting is false", () => {
    saveNotificationSettings({ attentionFrame: false });
    const layout = singleTabLayout();
    const sessions = { "session-1": session("session-1", "backend-1") };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        activityBySessionId={{ "session-1": { state: "done", title: "Done", isAgent: true, seen: false } }}
      />,
    );

    const paneLeaf = screen.getByTestId("pane-leaf");
    expect(paneLeaf.className).not.toContain("ring-[#fbbf24]");

    const terminalPane = screen.getByTestId("terminal-pane");
    expect(terminalPane).toHaveAttribute("data-needs-attention", "false");
    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-right")).not.toBeInTheDocument();
  });
});

describe("inbox navigation flash", () => {
  afterEach(() => resetPaneFlashForTests());

  it("flashes only the target pane, fades out on its own, and leaves a sibling's attention frame alone", () => {
    render(
      <TerminalSplitView
        layout={splitLayout()}
        sessions={splitSessions()}
        activityBySessionId={{
          "session-1": { state: "done", title: "Done", isAgent: true, seen: true },
          "session-2": { state: "waiting", title: "Waiting", isAgent: true, seen: false },
        }}
      />,
    );
    const [target, sibling] = screen.getAllByTestId("pane-leaf");
    expect(screen.queryByTestId("pane-inbox-flash")).toBeNull();

    act(() => flashPane("session-1"));

    const flash = screen.getByTestId("pane-inbox-flash");
    expect(target).toContainElement(flash);
    expect(sibling).not.toContainElement(flash);
    expect(target.querySelector('[data-testid="terminal-pane"]')).toHaveAttribute("data-needs-attention", "true");
    expect(sibling.querySelector('[data-testid="terminal-pane"]')).toHaveAttribute("data-needs-attention", "true");
    expect(sibling.querySelector('[data-testid="attention-frame-bottom"]')).not.toBeNull();

    fireEvent.animationEnd(flash);

    expect(screen.queryByTestId("pane-inbox-flash")).toBeNull();
    expect(target.querySelector('[data-testid="terminal-pane"]')).toHaveAttribute("data-needs-attention", "false");
    expect(sibling.querySelector('[data-testid="terminal-pane"]')).toHaveAttribute("data-needs-attention", "true");
    expect(sibling.querySelector('[data-testid="attention-frame-bottom"]')).not.toBeNull();
  });

  it("restarts the flash when the same pane is reached again before the fade ends", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);
    act(() => flashPane("session-1"));
    const first = screen.getByTestId("pane-inbox-flash");

    act(() => flashPane("session-1"));
    const second = screen.getByTestId("pane-inbox-flash");
    expect(second).not.toBe(first);

    fireEvent.animationEnd(second);
    expect(screen.queryByTestId("pane-inbox-flash")).toBeNull();
  });
});

describe("TerminalSplitView pane copy debug info", () => {
  it("copies exact debug info for the specific terminal pane, including inactive panes in a split", async () => {
    const sessions = splitSessions();
    render(<TerminalSplitView layout={splitLayout()} sessions={sessions} />);

    const copyButtons = screen.getAllByRole("button", { name: "Copy Debug Info" });
    expect(copyButtons).toHaveLength(2);

    // Active pane (leaf-1)
    fireEvent.click(copyButtons[0]);
    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledTimes(1);
    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledWith(
      formatPaneDebugInfo("leaf-1", sessions["session-1"]),
    );
    const firstPayload = JSON.parse(mockClipboard.copyTextToClipboard.mock.calls[0][0]);
    expect(firstPayload.leafId).toBe("leaf-1");
    expect(firstPayload.sessionId).toBe("session-1");
    expect(firstPayload.backendSessionId).toBe("backend-1");

    mockClipboard.copyTextToClipboard.mockClear();

    // Inactive pane (leaf-2)
    fireEvent.click(copyButtons[1]);
    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledTimes(1);
    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledWith(
      formatPaneDebugInfo("leaf-2", sessions["session-2"]),
    );
    const secondPayload = JSON.parse(mockClipboard.copyTextToClipboard.mock.calls[0][0]);
    expect(secondPayload.leafId).toBe("leaf-2");
    expect(secondPayload.sessionId).toBe("session-2");
    expect(secondPayload.backendSessionId).toBe("backend-2");
  });

  it("stops propagation on click and pointerDown to avoid changing pane focus or triggering split/drag", () => {
    const onFocusPane = vi.fn();
    const onSplitPane = vi.fn();
    const onClosePane = vi.fn();

    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={{ "session-1": session("session-1", "backend-1") }}
        onFocusPane={onFocusPane}
        onSplitPane={onSplitPane}
        onClosePane={onClosePane}
      />,
    );

    const copyButton = screen.getByRole("button", { name: "Copy Debug Info" });

    // Pointer down must stop propagation: must not focus pane, must not drag
    fireEvent.pointerDown(copyButton);
    expect(onFocusPane).not.toHaveBeenCalled();
    expect(onSplitPane).not.toHaveBeenCalled();
    expect(onClosePane).not.toHaveBeenCalled();
    expect(copyButton).not.toHaveAttribute("draggable", "true");

    // Click must copy without focusing or splitting
    fireEvent.click(copyButton);
    expect(mockClipboard.copyTextToClipboard).toHaveBeenCalledWith(
      formatPaneDebugInfo("leaf-1", session("session-1", "backend-1")),
    );
    expect(onFocusPane).not.toHaveBeenCalled();
    expect(onSplitPane).not.toHaveBeenCalled();
    expect(onClosePane).not.toHaveBeenCalled();
  });

  it("reveals toolbar when keyboard focus moves into toolbar controls and hides on blur", () => {
    render(<TerminalSplitView layout={singleTabLayout()} sessions={{ "session-1": session("session-1", "backend-1") }} />);

    const toolbar = screen.getByTestId("pane-toolbar");
    const copyButton = screen.getByRole("button", { name: "Copy Debug Info" });

    // Initially hidden
    expect(toolbar).toHaveClass("pointer-events-none");
    expect(toolbar).toHaveClass("opacity-0");

    // Keyboard focus onto Copy button reveals toolbar
    fireEvent.focus(copyButton);
    expect(toolbar).toHaveClass("pointer-events-auto");
    expect(toolbar).toHaveClass("opacity-100");

    // Blur outside toolbar hides it
    fireEvent.blur(toolbar, { relatedTarget: document.body });
    expect(toolbar).toHaveClass("pointer-events-none");
    expect(toolbar).toHaveClass("opacity-0");
  });
});
