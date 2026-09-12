import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationCenterPopover, POPOVER_WIDTH } from "./NotificationCenterPopover";
import { createNotificationCenterStore } from "../../lib/notificationCenter/notificationCenterStore";
import { resolveAgentLogo } from "../../lib/agentIcon";

describe("NotificationCenterPopover", () => {
  let store: ReturnType<typeof createNotificationCenterStore>;
  let origDispatchEvent: typeof HTMLButtonElement.prototype.dispatchEvent;

  beforeEach(() => {
    localStorage.clear();
    store = createNotificationCenterStore({ storage: null });

    // Emulate HTML specification for native button keyboard activation in JSDOM:
    // When Enter or Space keydown is dispatched on a button and not default-prevented,
    // the native default action is to trigger button.click().
    origDispatchEvent = HTMLButtonElement.prototype.dispatchEvent;
    HTMLButtonElement.prototype.dispatchEvent = function (event: Event) {
      const result = origDispatchEvent.call(this, event);
      if (
        event.type === "keydown" &&
        ((event as KeyboardEvent).key === "Enter" || (event as KeyboardEvent).key === " ") &&
        !event.defaultPrevented
      ) {
        this.click();
      }
      return result;
    };
  });

  afterEach(() => {
    HTMLButtonElement.prototype.dispatchEvent = origDispatchEvent;
    cleanup();
  });

  it("renders empty state when there are no notifications", () => {
    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);
    expect(screen.getByText("No new notifications")).toBeInTheDocument();
    expect(screen.getByTestId("notification-empty-state")).toBeInTheDocument();
  });

  it("closes on backdrop click", () => {
    const onClose = vi.fn();
    render(<NotificationCenterPopover onClose={onClose} store={store} />);

    const backdrop = screen.getByTestId("notification-center-backdrop");
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on Escape key and restores focus", async () => {
    const onClose = vi.fn();
    const trigger = document.createElement("button");
    trigger.textContent = "Trigger";
    document.body.appendChild(trigger);
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    const { unmount } = render(<NotificationCenterPopover onClose={onClose} store={store} />);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();

    unmount();
    expect(document.activeElement).toBe(trigger);
    document.body.removeChild(trigger);
  });

  it("traps focus between first and last focusable elements on Tab / Shift+Tab", async () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { terminalTitle: "Term 1" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    const dialog = screen.getByRole("dialog", { name: "Notifications" });
    const focusable = dialog.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    expect(focusable.length).toBeGreaterThan(1);

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    // Focus last element and press Tab -> should wrap to first
    last.focus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    // Focus first element and press Shift+Tab -> should wrap to last
    first.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);
  });

  it("does not auto-mark read when popover opens", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { terminalTitle: "Term 1" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    const snapshot = store.getSnapshot();
    expect(snapshot.entries[0].read).toEqual({ unread: true });
  });

  it("renders row details: agent icon, StatusDot, title, location, relative time, and unread indicator", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: {
        workspaceLabel: "orca-lite",
        worktreeLabel: "feature-branch",
        agentLabel: "claude",
        terminalTitle: "Build task",
      },
      subject: "agent",
      occurredAt: Date.now() - 30_000, // 30s ago
      observed: false,
      previousState: "working",
      state: "waiting",
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    expect(screen.getByText("Build task")).toBeInTheDocument();
    expect(screen.getByText("orca-lite / feature-branch")).toBeInTheDocument();
    expect(screen.getByText("just now")).toBeInTheDocument();

    // Agent icon
    const icon = screen.getByTestId("notification-agent-icon");
    expect(icon).toHaveAttribute("src", resolveAgentLogo("claude"));

    // StatusDot for waiting reason
    const dot = screen.getByTestId("notification-status-dot");
    expect(dot.querySelector('[data-status-state="waiting"]')).toBeInTheDocument();

    // Unread indicator
    expect(screen.getByTestId("unread-indicator")).toBeInTheDocument();
  });

  it("deduplicates location string when workspace and worktree are equal or one is missing", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: {
        workspaceLabel: "main",
        worktreeLabel: "main",
        terminalTitle: "Term 1",
      },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    store.recordActivity({
      workspaceId: "ws-2",
      sessionId: "s-2",
      labels: {
        workspaceLabel: "only-project",
        terminalTitle: "Term 2",
      },
      subject: "terminal",
      occurredAt: 2000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.getByText("only-project")).toBeInTheDocument();
  });

  it("renders StatusDot for done and bell reasons", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-done",
      labels: { terminalTitle: "Done Task" },
      subject: "agent",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    store.recordBell({
      workspaceId: "ws-1",
      sessionId: "s-bell",
      labels: { terminalTitle: "Bell Task" },
      subject: "terminal",
      occurredAt: 2000,
      observed: false,
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    const dots = screen.getAllByTestId("notification-status-dot");
    expect(dots.some((d) => d.querySelector('[data-status-state="done"]'))).toBe(true);
    expect(dots.some((d) => d.querySelector('[data-status-state="unread"]'))).toBe(true);

    // Bell indicator
    expect(screen.getByTestId("bell-indicator")).toBeInTheDocument();
  });

  it("calls onNavigateToSession on row click and closes, but does NOT mark read", () => {
    const onNavigateToSession = vi.fn();
    const onClose = vi.fn();

    store.recordActivity({
      workspaceId: "ws-nav",
      sessionId: "s-nav",
      labels: { terminalTitle: "Navigable Session" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    const entry = store.getSnapshot().entries[0];

    render(
      <NotificationCenterPopover
        onClose={onClose}
        onNavigateToSession={onNavigateToSession}
        store={store}
      />,
    );

    const row = screen.getByTestId(`notification-row-${entry.id}`);
    fireEvent.click(row);

    expect(onNavigateToSession).toHaveBeenCalledWith({
      workspaceId: "ws-nav",
      sessionId: "s-nav",
      revision: entry.revision,
    });
    expect(onClose).toHaveBeenCalledOnce();

    // Entry MUST remain unread in store (App owns mark-read after focus)
    expect(store.getSnapshot().entries[0].read).toEqual({ unread: true });
  });

  it("renders closed-session rows as muted 'Session ended' and non-clickable", () => {
    const onNavigateToSession = vi.fn();
    const onClose = vi.fn();

    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "closed-session-123",
      labels: { terminalTitle: "Closed Session" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    render(
      <NotificationCenterPopover
        onClose={onClose}
        onNavigateToSession={onNavigateToSession}
        isSessionNavigable={(sessionId: string) => sessionId !== "closed-session-123"}
        store={store}
      />,
    );

    expect(screen.getByText("Session ended")).toBeInTheDocument();

    const row = screen.getByTestId(/notification-row-/);
    expect(row).toHaveAttribute("aria-disabled", "true");

    fireEvent.click(row);
    expect(onNavigateToSession).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("provides keyboard-accessible hover clear button that removes the row", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { terminalTitle: "Term to clear" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    const onNavigate = vi.fn();
    render(
      <NotificationCenterPopover
        onClose={vi.fn()}
        onNavigateToSession={onNavigate}
        store={store}
      />,
    );

    const clearButton = screen.getByRole("button", { name: "Clear notification" });
    expect(clearButton).toBeInTheDocument();

    // Clicking clear dismisses entry without navigating
    fireEvent.click(clearButton);
    expect(onNavigate).not.toHaveBeenCalled();
    expect(screen.queryByText("Term to clear")).toBeNull();
    expect(store.getSnapshot().entries).toHaveLength(0);
  });

  it("B1: pressing Enter or Space on clear button dismisses entry without navigating or closing popover", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-enter",
      labels: { terminalTitle: "Term Enter" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-space",
      labels: { terminalTitle: "Term Space" },
      subject: "terminal",
      occurredAt: 2000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    const onNavigateToSession = vi.fn();
    const onClose = vi.fn();

    render(
      <NotificationCenterPopover
        onClose={onClose}
        onNavigateToSession={onNavigateToSession}
        store={store}
      />,
    );

    const clearButtons = screen.getAllByRole("button", { name: "Clear notification" });
    expect(clearButtons).toHaveLength(2);

    // Press Enter on first clear button
    fireEvent.keyDown(clearButtons[0], { key: "Enter" });
    expect(onNavigateToSession).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByText("Term Space")).toBeNull();
    expect(store.getSnapshot().entries).toHaveLength(1);

    // Press Space on second clear button
    fireEvent.keyDown(clearButtons[1], { key: " " });
    expect(onNavigateToSession).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByText("Term Enter")).toBeNull();
    expect(store.getSnapshot().entries).toHaveLength(0);
  });

  it("B1: row keyboard activation still navigates and closes when Enter or Space is pressed directly on the row", () => {
    store.recordActivity({
      workspaceId: "ws-nav",
      sessionId: "s-nav",
      labels: { terminalTitle: "Term Nav" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });

    const onNavigateToSession = vi.fn();
    const onClose = vi.fn();

    render(
      <NotificationCenterPopover
        onClose={onClose}
        onNavigateToSession={onNavigateToSession}
        store={store}
      />,
    );

    const entry = store.getSnapshot().entries[0];
    const row = screen.getByTestId(`notification-row-${entry.id}`);

    // Enter on row
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onNavigateToSession).toHaveBeenCalledWith({
      workspaceId: "ws-nav",
      sessionId: "s-nav",
      revision: entry.revision,
    });
    expect(onClose).toHaveBeenCalledOnce();

    onNavigateToSession.mockClear();
    onClose.mockClear();

    // Space on row
    fireEvent.keyDown(row, { key: " " });
    expect(onNavigateToSession).toHaveBeenCalledWith({
      workspaceId: "ws-nav",
      sessionId: "s-nav",
      revision: entry.revision,
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("B2: clamps popover horizontally within narrow viewport so right edge does not overflow", () => {
    const originalInnerWidth = window.innerWidth;
    window.innerWidth = 430;

    const anchorEl = document.createElement("button");
    anchorEl.getBoundingClientRect = () =>
      ({
        top: 600,
        bottom: 630,
        left: 390,
        right: 420,
        width: 30,
        height: 30,
        x: 390,
        y: 600,
        toJSON: () => {},
      }) as DOMRect;
    const anchorRef = { current: anchorEl };

    render(
      <NotificationCenterPopover
        open={true}
        anchorRef={anchorRef}
        onClose={vi.fn()}
        store={store}
      />,
    );

    const popover = screen.getByTestId("notification-center-popover");
    const computedLeft = parseInt(popover.style.left, 10);
    const popoverWidth = POPOVER_WIDTH;
    const margin = 8;

    // Popover right edge (left + width + margin) must be within viewport width (430)
    expect(computedLeft + popoverWidth + margin).toBeLessThanOrEqual(430);
    expect(computedLeft).toBe(42);

    window.innerWidth = originalInnerWidth;
  });

  it("B2: re-anchors popover on window resize while open", () => {
    let currentLeft = 200;
    const anchorEl = document.createElement("button");
    anchorEl.getBoundingClientRect = () =>
      ({
        top: 500,
        bottom: 530,
        left: currentLeft,
        right: currentLeft + 30,
        width: 30,
        height: 30,
        x: currentLeft,
        y: 500,
        toJSON: () => {},
      }) as DOMRect;
    const anchorRef = { current: anchorEl };

    render(
      <NotificationCenterPopover
        open={true}
        anchorRef={anchorRef}
        onClose={vi.fn()}
        store={store}
      />,
    );

    const popover = screen.getByTestId("notification-center-popover");
    expect(popover.style.left).toBe("200px");

    // Move anchor and trigger resize
    currentLeft = 50;
    fireEvent(window, new Event("resize"));

    expect(popover.style.left).toBe("50px");
  });

  it("header: 'Mark all read' and 'Clear all' buttons function correctly", () => {
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-1",
      labels: { terminalTitle: "Term 1" },
      subject: "terminal",
      occurredAt: 1000,
      observed: false,
      previousState: "working",
      state: "done",
    });
    store.recordActivity({
      workspaceId: "ws-1",
      sessionId: "s-2",
      labels: { terminalTitle: "Term 2" },
      subject: "terminal",
      occurredAt: 2000,
      observed: false,
      previousState: "working",
      state: "waiting",
    });

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    expect(screen.getByText("2 unread")).toBeInTheDocument();

    // Mark all read
    fireEvent.click(screen.getByRole("button", { name: "Mark all read" }));
    expect(screen.queryByText("2 unread")).toBeNull();
    expect(store.getSnapshot().entries.every((e) => "seen" in e.read)).toBe(true);

    // Clear all
    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(screen.getByText("No new notifications")).toBeInTheDocument();
    expect(store.getSnapshot().entries).toHaveLength(0);
  });

  it("reflects 201st row eviction in render", () => {
    // Record 201 distinct sessions
    for (let i = 1; i <= 201; i++) {
      store.recordActivity({
        workspaceId: "ws-1",
        sessionId: `session-${i}`,
        labels: { terminalTitle: `Term ${i}` },
        subject: "terminal",
        occurredAt: 1000 + i,
        observed: i === 1, // first one is observed (seen), so it will be evicted first
        previousState: "working",
        state: "done",
      });
    }

    render(<NotificationCenterPopover onClose={vi.fn()} store={store} />);

    const rows = screen.getAllByTestId(/^notification-row-/);
    expect(rows).toHaveLength(200);

    // Session 1 (the oldest seen row) should have been evicted
    expect(screen.queryByText("Term 1")).toBeNull();
    // Session 201 (the newest) should be rendered
    expect(screen.getByText("Term 201")).toBeInTheDocument();
  });
});
