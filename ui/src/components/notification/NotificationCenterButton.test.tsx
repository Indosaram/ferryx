import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationCenterButton } from "./NotificationCenterButton";
import { createNotificationCenterStore } from "../../lib/notificationCenter/notificationCenterStore";

describe("NotificationCenterButton", () => {
  let store: ReturnType<typeof createNotificationCenterStore>;

  beforeEach(() => {
    localStorage.clear();
    store = createNotificationCenterStore({ storage: null });
  });

  afterEach(cleanup);

  it("renders bell IconButton with no-drag class and data-testid", () => {
    render(<NotificationCenterButton store={store} />);
    const button = screen.getByTestId("notification-center-button");
    expect(button).toBeInTheDocument();
    expect(button).toHaveClass("no-drag");
    expect(button).toHaveAttribute("aria-label", "Notifications");
  });

  it("shows no badge when unread count is 0", () => {
    render(<NotificationCenterButton store={store} />);
    expect(screen.queryByTestId("notification-center-badge")).toBeNull();
  });

  it("shows unread badge count matching the store", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { workspaceLabel: "Project A", worktreeLabel: "main", terminalTitle: "Term 1" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-2",
      labels: { workspaceLabel: "Project A", worktreeLabel: "feat", terminalTitle: "Term 2" },
      subject: "terminal",
      occurredAt: 2000,
      observed: false,
      previousState: "working",
      state: "waiting",
    });

    render(<NotificationCenterButton store={store} />);
    const badge = screen.getByTestId("notification-center-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("2");
  });

  it("stops propagation on pointerdown and click", () => {
    const onParentClick = vi.fn();
    const onParentPointerDown = vi.fn();
    render(
      <div onClick={onParentClick} onPointerDown={onParentPointerDown}>
        <NotificationCenterButton store={store} />
      </div>,
    );

    const button = screen.getByTestId("notification-center-button");
    fireEvent.pointerDown(button);
    expect(onParentPointerDown).not.toHaveBeenCalled();

    fireEvent.click(button);
    expect(onParentClick).not.toHaveBeenCalled();
  });

  it("toggles the popover when clicked", () => {
    render(<NotificationCenterButton store={store} />);
    const button = screen.getByTestId("notification-center-button");

    expect(screen.queryByRole("dialog", { name: "Notifications" })).toBeNull();
    fireEvent.click(button);
    expect(screen.getByRole("dialog", { name: "Notifications" })).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.queryByRole("dialog", { name: "Notifications" })).toBeNull();
  });

  it("clears the badge when mark all read is clicked", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { workspaceLabel: "Project A", terminalTitle: "Term 1" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    render(<NotificationCenterButton store={store} />);
    expect(screen.getByTestId("notification-center-badge")).toHaveTextContent("1");

    // Open popover
    fireEvent.click(screen.getByTestId("notification-center-button"));
    // Click Mark all read
    fireEvent.click(screen.getByRole("button", { name: "Mark all read" }));

    // Badge should be cleared
    expect(screen.queryByTestId("notification-center-badge")).toBeNull();
  });
});
