import * as fs from "node:fs";
import * as path from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import { saveBrowserSettings } from "../lib/browserSettings";
import type { WorkspaceTab } from "../lib/types";
import { TabBar } from "./TabBar";
import { SortableTab } from "./tab-dnd/SortableTab";

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

const nativeMenu = vi.hoisted(() => ({
  lastCall: null as null | {
    command: string;
    items: Array<Record<string, unknown>>;
    position: { x: number; y: number };
    onAction: (id: string) => void;
  },
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: vi.fn(
    async (
      command: string,
      items: Array<Record<string, unknown>>,
      position: { x: number; y: number },
      onAction: (id: string) => void,
    ) => {
      nativeMenu.lastCall = { command, items, position, onAction };
      return () => undefined;
    },
  ),
}));

function menuItems() {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  return nativeMenu.lastCall.items;
}

function menuItem(idOrLabel: string) {
  const items = menuItems();
  const found = items.find(
    (item) =>
      (item as { id?: string }).id === idOrLabel ||
      ((item as { label?: string }).label ?? "").includes(idOrLabel),
  );
  if (!found) {
    throw new Error(`menu item not found: ${idOrLabel}; got ${JSON.stringify(items)}`);
  }
  return found as { id: string; label: string; enabled?: boolean };
}

function clickMenuItem(idOrLabel: string) {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  act(() => {
    nativeMenu.lastCall?.onAction(menuItem(idOrLabel).id);
  });
}

afterEach(() => {
  cleanup();
  nativeWindow.startDragging.mockClear();
  nativeMenu.lastCall = null;
  localStorage.clear();
});

function activity(overrides: Partial<ActivitySummary> = {}): ActivitySummary {
  return {
    workingCount: 0,
    waitingCount: 0,
    doneCount: 0,
    runningCount: 0,
    hasWorking: false,
    hasWaiting: false,
    hasDone: false,
    hasUnread: false,
    ...overrides,
  };
}

function terminalTab(id: string, label: string, pinned = false): WorkspaceTab {
  return { id, label, sessionId: `session-${id}`, pinned };
}

function getTab(label: string): HTMLElement {
  return screen.getByText(label).closest('[role="tab"]') as HTMLElement;
}

describe("TabBar", () => {
  it("routes activate, close, and new-tab actions through real callbacks", () => {
    const onActivate = vi.fn();
    const onClose = vi.fn();
    const onAdd = vi.fn();
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature")]}
        activeTabId="tab-a"
        onActivate={onActivate}
        onClose={onClose}
        onAdd={onAdd}
      />,
    );

    fireEvent.click(getTab("feature"));
    expect(onActivate).toHaveBeenCalledWith("tab-b");

    fireEvent.click(screen.getByRole("button", { name: "Close main" }));
    expect(onClose).toHaveBeenCalledWith("tab-a");

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    clickMenuItem("new-terminal");
    expect(onAdd).toHaveBeenCalledOnce();
  });

  it("supports keyboard tab activation", () => {
    const onActivate = vi.fn();
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature")]}
        activeTabId="tab-a"
        onActivate={onActivate}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const feature = getTab("feature");
    expect(feature).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(feature, { key: "Enter" });
    fireEvent.keyDown(feature, { key: " " });
    expect(onActivate).toHaveBeenCalledTimes(2);
    expect(onActivate).toHaveBeenLastCalledWith("tab-b");
  });

  it("renders leading working, waiting, and unread indicators with attention precedence", () => {
    render(
      <TabBar
        tabs={[
          terminalTab("tab-working", "working"),
          terminalTab("tab-waiting", "waiting"),
          terminalTab("tab-unread", "unread"),
          { id: "tab-browser", kind: "browser", label: "browser", browserId: "browser-1", url: "https://example.com" },
        ]}
        activeTabId="tab-working"
        unreadTabIds={{ "tab-working": true, "tab-unread": true }}
        activityByTabId={{
          "tab-working": activity({ workingCount: 1, runningCount: 1, hasWorking: true, hasUnread: true }),
          "tab-waiting": activity({ workingCount: 1, waitingCount: 1, runningCount: 1, hasWorking: true, hasWaiting: true }),
          "tab-unread": activity({ doneCount: 1, hasDone: true, hasUnread: true }),
          "tab-browser": activity({ waitingCount: 1, hasWaiting: true }),
        }}
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(screen.getByTestId("tab-working-indicator")).toHaveAttribute("aria-label", "Agent working");
    expect(screen.getByTestId("tab-working-indicator").querySelector('[data-status-state="working"]')).toHaveClass("animate-spin");
    expect(screen.getByTestId("tab-waiting-indicator")).toHaveAttribute("aria-label", "Agent waiting");
    expect(screen.getByTestId("tab-unread-dot").querySelector('[data-status-state="unread"]')).toBeInTheDocument();
    expect(screen.queryByTestId("tab-done-indicator")).not.toBeInTheDocument();
    expect(screen.getByText("browser").previousElementSibling?.tagName.toLowerCase()).toBe("svg");
  });

  it("keeps whole-tab split actions separate from terminal-pane split actions", () => {
    const onSplitRight = vi.fn();
    const onMoveTabToSplit = vi.fn();
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature"), terminalTab("tab-c", "docs")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onCloseOthers={vi.fn()}
        onCloseToRight={vi.fn()}
        onSplitRight={onSplitRight}
        onMoveTabToSplit={onMoveTabToSplit}
        onRenameTab={vi.fn()}
        onTogglePin={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"), { clientX: 100, clientY: 100 });
    expect(menuItem("move:left").enabled).not.toBe(false);
    expect(menuItem("split-right").enabled).not.toBe(false);

    clickMenuItem("move:left");
    expect(onMoveTabToSplit).toHaveBeenCalledWith("tab-a", "left");
    expect(onSplitRight).not.toHaveBeenCalled();

    fireEvent.contextMenu(getTab("main"));
    clickMenuItem("split-right");
    expect(onSplitRight).toHaveBeenCalledWith("tab-a");
  });

  it("keeps pin state controlled by the workspace model and blocks pinned-tab close", () => {
    const onTogglePin = vi.fn();
    const onClose = vi.fn();
    const { rerender } = render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={onClose}
        onTogglePin={onTogglePin}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"));
    clickMenuItem("pin");
    expect(onTogglePin).toHaveBeenCalledWith("tab-a", true);

    rerender(
      <TabBar
        tabs={[terminalTab("tab-a", "main", true)]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={onClose}
        onTogglePin={onTogglePin}
        onAdd={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("Pinned tab")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close main" })).not.toBeInTheDocument();

    fireEvent.contextMenu(getTab("main"));
    expect(menuItem("close").enabled).toBe(false);
    expect(menuItem("pin").label).toBe("Unpin tab");
    clickMenuItem("pin");
    expect(onTogglePin).toHaveBeenLastCalledWith("tab-a", false);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("commits trimmed title changes and cancels rename on Escape", () => {
    const onRenameTab = vi.fn();
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onRenameTab={onRenameTab}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"));
    clickMenuItem("rename");
    let input = screen.getByDisplayValue("main");
    fireEvent.change(input, { target: { value: "  discarded  " } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onRenameTab).not.toHaveBeenCalled();

    fireEvent.contextMenu(getTab("main"));
    clickMenuItem("rename");
    input = screen.getByDisplayValue("main");
    fireEvent.change(input, { target: { value: "  custom title  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onRenameTab).toHaveBeenCalledWith("tab-a", "custom title");
  });

  it("exposes dnd-kit sortable metadata scoped to its tab group instead of global DOM hit testing", () => {
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature"), terminalTab("tab-c", "docs")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(getTab("main")).toHaveAttribute("data-dnd-type", "tab");
    expect(getTab("main")).toHaveAttribute("data-tab-group-id", "group-a");
    expect(getTab("main")).toHaveAttribute("data-tab-index", "0");
    expect(getTab("feature")).toHaveAttribute("data-tab-index", "1");
    expect(getTab("docs")).toHaveAttribute("data-tab-index", "2");
    expect(getTab("main")).toHaveAttribute("draggable", "false");
  });

  it("opens browser tabs from the action-only menu and keeps popovers out of window dragging", () => {
    const onAddBrowser = vi.fn();
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        onAddBrowser={onAddBrowser}
        actions={<button type="button">Action</button>}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    clickMenuItem("new-browser");
    expect(onAddBrowser).toHaveBeenCalledWith("about:blank");

    fireEvent.pointerDown(screen.getByRole("button", { name: "Action" }), { button: 0 });
    expect(nativeWindow.startDragging).not.toHaveBeenCalled();
  });

  it("passes the configured homepage when opening a browser tab from the new-tab menu", () => {
    saveBrowserSettings({ homePage: "https://example.com/start" });
    const onAddBrowser = vi.fn();
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        onAddBrowser={onAddBrowser}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    clickMenuItem("new-browser");
    expect(onAddBrowser).toHaveBeenCalledWith("https://example.com/start");
  });

  it("disables terminal split for browser tabs while preserving whole-tab split", () => {
    const onSplitRight = vi.fn();
    const onMoveTabToSplit = vi.fn();
    render(
      <TabBar
        tabs={[{ id: "browser", kind: "browser", label: "Browser", browserId: "browser-1", url: "https://example.com" }]}
        activeTabId="browser"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onSplitRight={onSplitRight}
        onMoveTabToSplit={onMoveTabToSplit}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("Browser"));
    expect(menuItem("split-right").enabled).toBe(false);
    expect(menuItem("move:right").enabled).not.toBe(false);
    clickMenuItem("move:right");
    expect(onMoveTabToSplit).toHaveBeenCalledWith("browser", "right");
    expect(onSplitRight).not.toHaveBeenCalled();
  });

  it("keeps Tauri window dragging on strip background without making tabs native drag regions", () => {
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const tabStrip = screen.getByTestId("tab-strip");
    expect(tabStrip).not.toHaveAttribute("data-tauri-drag-region");
    expect(tabStrip).not.toHaveClass("drag-region");
    expect(getTab("main")).toHaveClass("no-drag");
    expect(getTab("main")).toHaveAttribute("draggable", "false");

    fireEvent.pointerDown(tabStrip, { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();

    fireEvent.pointerDown(getTab("main"), { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();
  });

  it("renders new tab trigger and popover outside the horizontal overflow tablist container to avoid vertical clipping", () => {
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    const tablist = screen.getByRole("tablist");
    const newTabButton = screen.getByRole("button", { name: "New tab" });

    // The add trigger must NOT be a descendant of the overflow-x-auto [role=tablist] container
    expect(tablist.contains(newTabButton)).toBe(false);

    // The menu is a native OS popup (no DOM dialog); opening it must not mount any in-app overlay
    fireEvent.click(newTabButton);
    expect(nativeMenu.lastCall?.command).toBe("cmd_native_new_tab_menu");
    expect(screen.queryByRole("dialog", { name: "New tab menu" })).not.toBeInTheDocument();
  });

  it("forwards launchable agents with brand icons and triggers onLaunchAgent from the new tab menu", () => {
    const onLaunchAgent = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "aider", command: "aider", args: "" },
    ];
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(nativeMenu.lastCall?.command).toBe("cmd_native_new_tab_menu");
    expect(menuItems()).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "agent:claude", label: "Claude", icon: "agent:claude" }),
      expect.objectContaining({ id: "agent:aider", label: "Aider", icon: "agent:aider" }),
    ]));
    clickMenuItem("agent:claude");
    expect(onLaunchAgent).toHaveBeenCalledWith(agents[0]);
  });

  it("forwards defaultAgentId so the usable default is first with a Default label", () => {
    const onLaunchAgent = vi.fn();
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "aider", command: "aider", args: "" },
    ];
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        agents={agents}
        onLaunchAgent={onLaunchAgent}
        defaultAgentId="aider"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    const agentIds = menuItems()
      .map((item) => (item as { id?: string }).id ?? "")
      .filter((id) => id.startsWith("agent:"));
    expect(agentIds[0]).toBe("agent:aider");
    expect(menuItem("agent:aider").label).toContain("Default");
    clickMenuItem("agent:claude");
    expect(onLaunchAgent).toHaveBeenCalledWith(agents[0]);
  });

  it("memoizes SortableTab with React.memo", () => {
    const isMemo = typeof SortableTab === "object" && (SortableTab as any)?.$$typeof === Symbol.for("react.memo");
    const source = fs.readFileSync(path.resolve(__dirname, "tab-dnd/SortableTab.tsx"), "utf8");
    const sourceHasMemo = /export const SortableTab = (?:React\.)?memo\(/.test(source);
    expect(isMemo || sourceHasMemo).toBe(true);
    expect(sourceHasMemo).toBe(true);
  });

  it("stabilizes onCancelRename callback across all rendered tabs", () => {
    const tabBarSource = fs.readFileSync(path.resolve(__dirname, "TabBar.tsx"), "utf8");
    expect(tabBarSource).toMatch(/const handleCancelRename = useCallback\(/);
    expect(tabBarSource).toMatch(/onCancelRename=\{handleCancelRename\}/);
    expect(tabBarSource).not.toMatch(/onCancelRename=\{\(\)\s*=>/);
  });

  it("memoizes SortableContext items array across re-renders when tabs are stable", () => {
    const tabBarSource = fs.readFileSync(path.resolve(__dirname, "TabBar.tsx"), "utf8");
    expect(tabBarSource).toMatch(/const sortableItems = useMemo\(\(\) => tabs\.map\(/);
    expect(tabBarSource).toMatch(/<SortableContext\s+items=\{sortableItems\}/);
  });
});
