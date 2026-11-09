import * as fs from "node:fs";
import * as path from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import { saveBrowserSettings } from "../lib/browserSettings";
import { clearSleepingSessions } from "../lib/sessionLifecycle";
import { saveShortcutOverrides } from "../lib/shortcutOverrides";
import { formatBindingLabel, isMacShortcutPlatform } from "../lib/shortcuts";
import type { SystemPermissionsStatus, WorkspaceTab } from "../lib/types";
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

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
}));

vi.mock("../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");
  return {
    ...actual,
    getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  };
});

/**
 * Reports `platform` as the HOST platform and resolves once TabBar has actually asked
 * for it, so tests await the exact IPC request instead of a fixed delay.
 */
function stubHostPlatform(platform: string) {
  return new Promise<void>((resolve) => {
    mockTauri.getSystemPermissionsStatus.mockImplementation(() => {
      resolve();
      return Promise.resolve({ platform } as SystemPermissionsStatus);
    });
  });
}

function menuItems() {
  if (!nativeMenu.lastCall) throw new Error("native menu was not opened");
  return nativeMenu.lastCall.items;
}

function findEntryRecursively(
  items: Array<Record<string, unknown>>,
  idOrLabel: string,
): { id: string; label: string; enabled?: boolean } | undefined {
  for (const item of items) {
    if (
      (item as { id?: string }).id === idOrLabel ||
      ((item as { label?: string }).label ?? "").includes(idOrLabel)
    ) {
      return item as { id: string; label: string; enabled?: boolean };
    }
    if (item.kind === "submenu" && Array.isArray(item.items)) {
      const found = findEntryRecursively(item.items as Array<Record<string, unknown>>, idOrLabel);
      if (found) return found;
    }
  }
  return undefined;
}

function menuItem(idOrLabel: string) {
  const items = menuItems();
  const found = findEntryRecursively(items, idOrLabel);
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

beforeEach(() => {
  // Unresolved by default: tests that do not care about the host platform must not
  // receive a state update outside act().
  mockTauri.getSystemPermissionsStatus.mockImplementation(() => new Promise(() => {}));
});

afterEach(() => {
  cleanup();
  nativeWindow.startDragging.mockClear();
  nativeMenu.lastCall = null;
  localStorage.clear();
  clearSleepingSessions();
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

  it("scrolls the strip so a tab activated off-screen becomes visible", () => {
    const tabs = [terminalTab("tab-a", "main"), terminalTab("tab-b", "feature"), terminalTab("tab-c", "far")];
    const rects: Record<string, { left: number; right: number }> = {
      tablist: { left: 0, right: 200 },
      main: { left: 0, right: 100 },
      feature: { left: 100, right: 200 },
      far: { left: 300, right: 400 },
    };
    const rectFor = (left: number, right: number) =>
      ({ left, right, top: 0, bottom: 30, width: right - left, height: 30, x: left, y: 0, toJSON: () => ({}) }) as DOMRect;
    const spy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.getAttribute("role") === "tablist") return rectFor(rects.tablist.left, rects.tablist.right);
      const label = this.textContent ?? "";
      const key = (["main", "feature", "far"] as const).find((name) => label.includes(name));
      return key ? rectFor(rects[key].left, rects[key].right) : rectFor(0, 0);
    });
    try {
      const { rerender } = render(<TabBar groupId="group-a" tabs={tabs} activeTabId="tab-a" onActivate={vi.fn()} onClose={vi.fn()} />);
      const tablist = screen.getByRole("tablist");
      expect(tablist.scrollLeft).toBe(0);
      rerender(<TabBar groupId="group-a" tabs={tabs} activeTabId="tab-c" onActivate={vi.fn()} onClose={vi.fn()} />);
      expect(tablist.scrollLeft).toBe(200);
    } finally {
      spy.mockRestore();
    }
  });

  it("renders the resolved display text and tooltip instead of the raw label", () => {
    render(
      <TabBar
        groupId="group-a"
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        tabDisplayById={{
          "tab-a": { text: "fix login bug", tooltip: "OmO - fix login bug - ferryx (main)" },
        }}
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(getTab("fix login bug")).toHaveAttribute(
      "title",
      "OmO - fix login bug - ferryx (main)",
    );
    expect(screen.queryByText("main")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close fix login bug" })).toBeInTheDocument();
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

  it("offers Move to New Window and Move to Window only when the move action is provided", async () => {
    const onMoveTabToWindow = vi.fn();
    const listOtherWindows = vi.fn(async () => ["main", "ws-abc123"]);
    const { rerender } = render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );
    fireEvent.contextMenu(getTab("main"));
    expect(findEntryRecursively(menuItems(), "move-window:new")).toBeUndefined();

    rerender(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        onMoveTabToWindow={onMoveTabToWindow}
        listOtherWindows={listOtherWindows}
      />,
    );
    await act(async () => {
      fireEvent.contextMenu(getTab("main"));
      await listOtherWindows.mock.results[0]?.value;
    });
    expect(menuItem("move-window:new").label).toBe("Move to New Window");
    expect(menuItem("move-window:ws-abc123").label).toBe("Window abc123");

    clickMenuItem("move-window:new");
    expect(onMoveTabToWindow).toHaveBeenLastCalledWith("tab-a", null);
    await act(async () => {
      fireEvent.contextMenu(getTab("main"));
      await listOtherWindows.mock.results[1]?.value;
    });
    clickMenuItem("move-window:ws-abc123");
    expect(onMoveTabToWindow).toHaveBeenLastCalledWith("tab-a", "ws-abc123");
  });

  it("does not offer a window move for a browser tab", async () => {
    const listOtherWindows = vi.fn(async () => []);
    render(
      <TabBar
        tabs={[{ id: "tab-b", kind: "browser", label: "browser", browserId: "browser-1", url: "https://example.com" }]}
        activeTabId="tab-b"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
        onMoveTabToWindow={vi.fn()}
        listOtherWindows={listOtherWindows}
      />,
    );
    fireEvent.contextMenu(getTab("browser"));
    expect(findEntryRecursively(menuItems(), "move-window:new")).toBeUndefined();
    expect(listOtherWindows).not.toHaveBeenCalled();
  });

  it("includes reset-agent-state in context menu and calls onResetAgentState for terminal tab", () => {
    const onResetAgentState = vi.fn();
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onResetAgentState={onResetAgentState}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"));
    const reset = menuItem("reset-agent-state");
    expect(reset.label).toBe("Reset Agent State");

    clickMenuItem("reset-agent-state");
    expect(onResetAgentState).toHaveBeenCalledWith("tab-a");
  });

  it("no longer offers copy-debug-info in the terminal tab menu", () => {
    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"));
    expect(findEntryRecursively(menuItems(), "copy-debug-info")).toBeUndefined();
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

  describe("new tab button tooltip", () => {
    function renderTabBar() {
      render(
        <TabBar
          tabs={[terminalTab("tab-a", "main")]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={vi.fn()}
        />,
      );
      return screen.getByRole("button", { name: "New tab" });
    }

    it("shows the default new terminal tab chord", () => {
      const chord = formatBindingLabel({ key: "t", mod: true }, isMacShortcutPlatform());
      expect(renderTabBar()).toHaveAttribute("title", `New tab (${chord})`);
    });

    it("shows a rebound chord and follows later rebinds", () => {
      const isMac = isMacShortcutPlatform();
      saveShortcutOverrides({ "tab.newTerminal": { key: "n", mod: true, alt: true } });
      const button = renderTabBar();
      expect(button).toHaveAttribute("title", `New tab (${formatBindingLabel({ key: "n", mod: true, alt: true }, isMac)})`);

      act(() => {
        saveShortcutOverrides({ "tab.newTerminal": { key: "y", mod: true, shift: true } });
      });
      expect(button).toHaveAttribute("title", `New tab (${formatBindingLabel({ key: "y", mod: true, shift: true }, isMac)})`);
    });

    it("shows only New tab when the command is unbound", () => {
      saveShortcutOverrides({ "tab.newTerminal": null });
      expect(renderTabBar()).toHaveAttribute("title", "New tab");
    });
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

  describe("pane chat entry points", () => {
    const agents = [
      { name: "claude", command: "claude", args: "" },
      { name: "aider", command: "aider", args: "" },
      { name: "codex", command: "codex", args: "--x", enabled: false },
      { name: "pi", command: "pi", args: "" },
    ];

    function newChatSubmenu() {
      return menuItems().find((item) => item.kind === "submenu" && item.label === "New Chat") as
        | { items: Array<{ id: string; label: string; shortcut?: string }> }
        | undefined;
    }

    it("lists only enabled chat-capable agents, last used first, next to New Terminal", () => {
      localStorage.setItem("ferryx.paneChat.lastAgent", "pi");
      const onNewChat = vi.fn();
      render(
        <TabBar
          tabs={[terminalTab("tab-a", "main")]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={vi.fn()}
          agents={agents}
          onNewChat={onNewChat}
        />,
      );

      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      const items = menuItems();
      expect(items[0]).toEqual(expect.objectContaining({ id: "new-terminal" }));
      expect(items[1]).toEqual(expect.objectContaining({ kind: "submenu", label: "New Chat" }));
      const submenu = newChatSubmenu()!;
      expect(submenu.items.map((item) => item.id)).toEqual(["new-chat:pi", "new-chat:claude"]);
      expect(submenu.items[0].shortcut).toBeTruthy();
      expect(submenu.items[1].shortcut).toBeUndefined();

      clickMenuItem("new-chat:claude");
      expect(onNewChat).toHaveBeenCalledWith(agents[0]);
    });

    it("hides New Chat when the host gates it off or no agent supports chat", () => {
      const { unmount } = render(
        <TabBar
          tabs={[terminalTab("tab-a", "main")]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={vi.fn()}
          agents={agents}
        />,
      );
      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      expect(newChatSubmenu()).toBeUndefined();
      unmount();

      render(
        <TabBar
          tabs={[terminalTab("tab-a", "main")]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={vi.fn()}
          agents={[{ name: "aider", command: "aider", args: "" }]}
          onNewChat={vi.fn()}
        />,
      );
      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      expect(newChatSubmenu()).toBeUndefined();
    });

    it("offers split chat right/down for terminal tabs only and forwards the agent", () => {
      const onSplitChat = vi.fn();
      render(
        <TabBar
          tabs={[terminalTab("tab-a", "main"), { id: "tab-b", kind: "browser", label: "web", browserId: "b", url: "about:blank", title: null, loading: false, canGoBack: false, canGoForward: false } as WorkspaceTab]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={vi.fn()}
          agents={agents}
          onSplitChat={onSplitChat}
        />,
      );

      fireEvent.contextMenu(getTab("main"));
      clickMenuItem("split-chat-down:claude");
      expect(onSplitChat).toHaveBeenCalledWith("tab-a", "vertical", agents[0]);
      fireEvent.contextMenu(getTab("main"));
      clickMenuItem("split-chat-right:pi");
      expect(onSplitChat).toHaveBeenLastCalledWith("tab-a", "horizontal", agents[3]);

      fireEvent.contextMenu(getTab("web"));
      expect(findEntryRecursively(menuItems(), "split-chat-right:claude")).toBeUndefined();
    });
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

  it("offers Windows terminal shell options in the new-tab menu on a Windows host and forwards selected shell to onAdd", async () => {
    // The browser OS is deliberately the opposite of the host: the menu must follow the host.
    const originalPlatform = navigator.platform;
    const originalUserAgent = navigator.userAgent;

    try {
      Object.defineProperty(navigator, "platform", { value: "MacIntel", configurable: true });
      Object.defineProperty(navigator, "userAgent", {
        value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        configurable: true,
      });

      const ready = stubHostPlatform("windows");
      const onAdd = vi.fn();
      render(
        <TabBar
          groupId="group-win"
          tabs={[terminalTab("tab-1", "main")]}
          activeTabId="tab-1"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={onAdd}
        />,
      );
      await act(async () => {
        await ready;
      });

      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      expect(nativeMenu.lastCall?.command).toBe("cmd_native_new_tab_menu");

      const items = menuItems();
      const shells = [
        { id: "new-terminal:pwsh", shell: "pwsh", label: "PowerShell" },
        { id: "new-terminal:powershell", shell: "powershell", label: "Windows PowerShell" },
        { id: "new-terminal:cmd", shell: "cmd", label: "Command Prompt" },
        { id: "new-terminal:wsl", shell: "wsl", label: "WSL" },
      ];

      for (const target of shells) {
        const found = findEntryRecursively(items, target.label);
        expect(found).toBeDefined();
        expect(found?.id).toBe(target.id);

        act(() => {
          nativeMenu.lastCall?.onAction(target.id);
        });
        expect(onAdd).toHaveBeenLastCalledWith(target.shell);
      }

      // Default generic action invokes onAdd without shell
      act(() => {
        nativeMenu.lastCall?.onAction("new-terminal");
      });
      expect(onAdd).toHaveBeenLastCalledWith();
    } finally {
      Object.defineProperty(navigator, "platform", { value: originalPlatform, configurable: true });
      Object.defineProperty(navigator, "userAgent", { value: originalUserAgent, configurable: true });
    }
  });

  it.each(["macos", "linux"])("does not offer Windows shell options in the new-tab menu on a %s host", async (hostPlatform) => {
    // The browser OS is deliberately the opposite of the host: the menu must follow the host.
    const originalPlatform = navigator.platform;
    const originalUserAgent = navigator.userAgent;

    try {
      Object.defineProperty(navigator, "platform", { value: "Win32", configurable: true });
      Object.defineProperty(navigator, "userAgent", {
        value: "Windows NT 10.0; Win64; x64",
        configurable: true,
      });

      const ready = stubHostPlatform(hostPlatform);
      const onAdd = vi.fn();
      render(
        <TabBar
          groupId="group-non-win"
          tabs={[terminalTab("tab-1", "main")]}
          activeTabId="tab-1"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onAdd={onAdd}
        />,
      );
      await act(async () => {
        await ready;
      });

      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      expect(nativeMenu.lastCall?.command).toBe("cmd_native_new_tab_menu");

      const items = menuItems();
      expect(findEntryRecursively(items, "Command Prompt")).toBeUndefined();
      expect(findEntryRecursively(items, "new-terminal:cmd")).toBeUndefined();
      expect(findEntryRecursively(items, "new-terminal:pwsh")).toBeUndefined();
      expect(findEntryRecursively(items, "new-terminal:powershell")).toBeUndefined();
      expect(findEntryRecursively(items, "new-terminal:wsl")).toBeUndefined();
    } finally {
      Object.defineProperty(navigator, "platform", { value: originalPlatform, configurable: true });
      Object.defineProperty(navigator, "userAgent", { value: originalUserAgent, configurable: true });
    }
  });
});
