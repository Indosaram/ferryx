import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import type { WorkspaceTab } from "../lib/types";
import { TabBar } from "./TabBar";

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

afterEach(() => {
  cleanup();
  nativeWindow.startDragging.mockClear();
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

function createDataTransfer() {
  let payload = "";
  return {
    setData: vi.fn((_type: string, value: string) => {
      payload = value;
    }),
    getData: vi.fn(() => payload),
    effectAllowed: "",
    dropEffect: "",
  };
}

describe("TabBar", () => {
  it("routes activate, close, and new-tab actions through real callbacks", () => {
    const onActivate = vi.fn();
    const onClose = vi.fn();
    const onAdd = vi.fn();
    render(
      <TabBar
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
    fireEvent.click(screen.getByRole("button", { name: /New Terminal/i }));
    expect(onAdd).toHaveBeenCalledOnce();
  });

  it("supports keyboard tab activation", () => {
    const onActivate = vi.fn();
    render(
      <TabBar
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

  it("opens context menu with wired Orca-parity actions", () => {
    const onCloseOthers = vi.fn();
    const onCloseToRight = vi.fn();
    const onSplitRight = vi.fn();
    const onRenameTab = vi.fn();
    const onTogglePin = vi.fn();

    render(
      <TabBar
        tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature"), terminalTab("tab-c", "docs")]}
        activeTabId="tab-a"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onCloseOthers={onCloseOthers}
        onCloseToRight={onCloseToRight}
        onSplitRight={onSplitRight}
        onRenameTab={onRenameTab}
        onTogglePin={onTogglePin}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("main"), { clientX: 100, clientY: 100 });

    expect(screen.getByRole("menu", { name: "Tab context menu" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Split terminal right/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Split terminal down/i })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: /Pin Tab/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Change Title/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Close Others/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Close Tabs To The Right/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Close Tabs To The Left/i })).toBeDisabled();

    fireEvent.click(screen.getByRole("menuitem", { name: /Split terminal right/i }));
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
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin Tab" }));
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
    expect(screen.getByRole("menuitem", { name: /^Close(?:\s*⌘W)?$/i })).toBeDisabled();
    fireEvent.click(screen.getByRole("menuitem", { name: "Unpin Tab" }));
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
    fireEvent.click(screen.getByRole("menuitem", { name: "Change Title" }));
    let input = screen.getByDisplayValue("main");
    fireEvent.change(input, { target: { value: "  discarded  " } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onRenameTab).not.toHaveBeenCalled();

    fireEvent.contextMenu(getTab("main"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Change Title" }));
    input = screen.getByDisplayValue("main");
    fireEvent.change(input, { target: { value: "  custom title  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onRenameTab).toHaveBeenCalledWith("tab-a", "custom title");
  });

  it("uses left/right drop edges to compute the actual tab insertion index", () => {
    const onReorderTabs = vi.fn();

    const rectSpy = vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const text = this.textContent || "";
      if (text.includes("main")) return { left: 0, right: 100, width: 100, top: 0, bottom: 32, height: 32, x: 0, y: 0, toJSON: () => ({}) };
      if (text.includes("feature")) return { left: 100, right: 200, width: 100, top: 0, bottom: 32, height: 32, x: 100, y: 0, toJSON: () => ({}) };
      if (text.includes("docs")) return { left: 200, right: 300, width: 100, top: 0, bottom: 32, height: 32, x: 200, y: 0, toJSON: () => ({}) };
      return { left: 0, right: 0, width: 0, top: 0, bottom: 0, height: 0, x: 0, y: 0, toJSON: () => ({}) };
    });

    try {
      render(
        <TabBar
          tabs={[terminalTab("tab-a", "main"), terminalTab("tab-b", "feature"), terminalTab("tab-c", "docs")]}
          activeTabId="tab-a"
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onReorderTabs={onReorderTabs}
          onAdd={vi.fn()}
        />,
      );

      let transfer = createDataTransfer();
      fireEvent.dragStart(getTab("main"), { dataTransfer: transfer });
      fireEvent.dragOver(getTab("feature"), { clientX: 180, dataTransfer: transfer });
      expect(getTab("feature").className).toContain("after:bg-blue-500");
      fireEvent.drop(getTab("feature"), { clientX: 180, dataTransfer: transfer });
      expect(onReorderTabs).toHaveBeenCalledWith("tab-a", 1);

      onReorderTabs.mockClear();
      transfer = createDataTransfer();
      fireEvent.dragStart(getTab("main"), { dataTransfer: transfer });
      fireEvent.dragOver(getTab("feature"), { clientX: 120, dataTransfer: transfer });
      expect(getTab("feature").className).toContain("before:bg-blue-500");
      fireEvent.drop(getTab("feature"), { clientX: 120, dataTransfer: transfer });
      expect(onReorderTabs).not.toHaveBeenCalled();

      transfer = createDataTransfer();
      fireEvent.dragStart(getTab("docs"), { dataTransfer: transfer });
      fireEvent.dragOver(getTab("main"), { clientX: 80, dataTransfer: transfer });
      expect(getTab("main").className).toContain("after:bg-blue-500");
      fireEvent.drop(getTab("main"), { clientX: 80, dataTransfer: transfer });
      expect(onReorderTabs).toHaveBeenCalledWith("tab-c", 1);
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("forwards typed URLs from the new-tab popover and keeps popovers out of window dragging", () => {
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
    const input = screen.getByRole("textbox", { name: "New tab query input" });
    fireEvent.pointerDown(input, { button: 0 });
    expect(nativeWindow.startDragging).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Open query" }));
    expect(onAddBrowser).toHaveBeenCalledWith("https://example.com");

    fireEvent.pointerDown(screen.getByRole("button", { name: "Action" }), { button: 0 });
    expect(nativeWindow.startDragging).not.toHaveBeenCalled();
  });

  it("disables terminal split actions for browser tabs", () => {
    const onSplitRight = vi.fn();
    render(
      <TabBar
        tabs={[{ id: "browser", kind: "browser", label: "Browser", browserId: "browser-1", url: "https://example.com" }]}
        activeTabId="browser"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onSplitRight={onSplitRight}
        onAdd={vi.fn()}
      />,
    );

    fireEvent.contextMenu(getTab("Browser"));
    expect(screen.getByRole("menuitem", { name: /Split terminal right/i })).toBeDisabled();
    fireEvent.click(screen.getByRole("menuitem", { name: /Split terminal right/i }));
    expect(onSplitRight).not.toHaveBeenCalled();
  });

  it("marks tab strip as Tauri drag region and starts native drag only on background", () => {
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
    expect(tabStrip).toHaveAttribute("data-tauri-drag-region");
    expect(tabStrip).toHaveClass("drag-region");
    expect(getTab("main")).toHaveClass("no-drag");

    fireEvent.pointerDown(tabStrip, { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();

    fireEvent.pointerDown(getTab("main"), { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();
  });
});
