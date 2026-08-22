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
    fireEvent.click(screen.getByRole("button", { name: /New Terminal/i }));
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
    expect(screen.getByRole("menuitem", { name: "Move Tab to Split Left" })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Split terminal right/i })).toBeEnabled();
    expect(screen.getByRole("menuitem", { name: /Split terminal down/i })).toBeDisabled();

    fireEvent.click(screen.getByRole("menuitem", { name: "Move Tab to Split Left" }));
    expect(onMoveTabToSplit).toHaveBeenCalledWith("tab-a", "left");
    expect(onSplitRight).not.toHaveBeenCalled();

    fireEvent.contextMenu(getTab("main"));
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
    expect(screen.getByRole("menuitem", { name: /Split terminal right/i })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: "Move Tab to Split Right" })).toBeEnabled();
    fireEvent.click(screen.getByRole("menuitem", { name: "Move Tab to Split Right" }));
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
});
