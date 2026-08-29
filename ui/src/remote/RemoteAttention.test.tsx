import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    onSwipeNextTab,
    onSwipePreviousTab,
  }: {
    sessionId: string;
    onSwipeNextTab?: () => void;
    onSwipePreviousTab?: () => void;
  }) => (
    <div
      data-testid="remote-terminal"
      data-session-id={sessionId}
      onTouchStart={(e) => {
        const firstTouch = e.touches[0];
        if (firstTouch) {
          e.currentTarget.setAttribute("data-touch-start-x", String(firstTouch.clientX));
        }
      }}
      onTouchEnd={(e) => {
        const rawStartX = e.currentTarget.getAttribute("data-touch-start-x");
        const changedTouch = e.changedTouches[0];
        if (rawStartX !== null && changedTouch) {
          const startX = parseFloat(rawStartX);
          const deltaX = changedTouch.clientX - startX;
          if (deltaX < -40) {
            onSwipeNextTab?.();
          } else if (deltaX > 40) {
            onSwipePreviousTab?.();
          }
        }
      }}
    >
      Mirrored terminal {sessionId}
    </div>
  ),
}));

function swipe(element: HTMLElement, deltaX: number) {
  fireEvent.touchStart(element, {
    touches: [{ clientX: 100, clientY: 100 }],
  });
  fireEvent.touchEnd(element, {
    changedTouches: [{ clientX: 100 + deltaX, clientY: 100 }],
  });
}

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
  } as unknown as Response;
}

class EventWebSocket {
  static latest: EventWebSocket | null = null;
  readonly url: string;
  close = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    EventWebSocket.latest = this;
  }
}

function eventSocket(): EventWebSocket {
  const socket = EventWebSocket.latest;
  if (!socket) throw new Error("Expected active-selection event socket");
  return socket;
}

const baseState = {
  activeContext: {
    workspaceId: "ferryx-ui",
    worktreeSlug: "main",
    worktreeLabel: "main",
    sessionId: "tab-1-session",
    tabId: "tab-1",
    terminalTabs: [
      { id: "tab-1", label: "Editor", activityState: "working" },
      { id: "tab-2", label: "Dev Server" },
    ],
  },
  projects: [
    {
      workspaceId: "ferryx-ui",
      worktrees: [{ slug: "main", label: "main" }],
    },
  ],
  sessions: [
    {
      sessionId: "tab-1-session",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
  ],
};

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  EventWebSocket.latest = null;
  vi.unstubAllGlobals();
});

describe("RemoteAttention Affordance", () => {
  it("renders attention affordance with accessible name when a background tab enters waiting state", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithWaiting = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting" },
        ],
      },
    };

    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithWaiting)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const attentionAffordance = await screen.findByRole("button", {
      name: /codex agent.*waiting/i,
    });
    expect(attentionAffordance).toBeInTheDocument();
  });

  it("activating the attention affordance issues exactly one context selection carrying tabId, workspaceId, and worktreeSlug", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithWaiting = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting" },
        ],
      },
    };

    const targetSwitchedState = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        sessionId: "tab-2-session",
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting" },
        ],
      },
      sessions: [
        {
          sessionId: "tab-2-session",
          workspaceId: "ferryx-ui",
          worktreeLabel: "main",
          running: true,
        },
      ],
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithWaiting))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(targetSwitchedState));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const attentionAffordance = await screen.findByRole("button", {
      name: /codex agent.*waiting/i,
    });
    fireEvent.click(attentionAffordance);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });

    // The selection is in flight and not yet confirmed by the desktop.

    // Socket confirms active selection
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "tab-2-session",
      );
    });
  });

  it("is absent from the DOM when no tab is waiting", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateNoWaiting = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Dev Server", activityState: "done" },
        ],
      },
    };

    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateNoWaiting)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    expect(screen.queryByTestId("remote-attention-badge")).not.toBeInTheDocument();
  });

  it("is absent from the DOM when the waiting tab is already the active tab", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateActiveWaiting = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting" },
        ],
      },
    };

    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateActiveWaiting)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    expect(screen.queryByTestId("remote-attention-badge")).not.toBeInTheDocument();
  });

  it("targets the first waiting tab in published order and states the waiting count when multiple tabs are waiting", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateMultipleWaiting = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "First Waiting", activityState: "waiting" },
          { id: "tab-3", label: "Second Waiting", activityState: "waiting" },
          { id: "tab-4", label: "Third Waiting", activityState: "waiting" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateMultipleWaiting))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(stateMultipleWaiting));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const attentionAffordance = await screen.findByRole("button", {
      name: /first waiting.*3 waiting/i,
    });
    expect(attentionAffordance).toBeInTheDocument();
    expect(attentionAffordance).toHaveTextContent("First Waiting");
    expect(attentionAffordance).toHaveTextContent("3");

    fireEvent.click(attentionAffordance);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
  });

  it("identifies worktree in accessible name when the waiting target belongs to a different worktree", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateDifferentWorktree = {
      ...baseState,
      activeContext: {
        workspaceId: "ferryx-ui",
        worktreeSlug: "main",
        worktreeLabel: "main",
        sessionId: "tab-1-session",
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
        ],
      },
      contexts: [
        {
          workspaceId: "ferryx-ui",
          worktreeSlug: "feature-branch",
          worktreeLabel: "feature-branch",
          attention: "waiting",
        },
      ],
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateDifferentWorktree))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(stateDifferentWorktree));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const attentionAffordance = await screen.findByRole("button", {
      name: /feature-branch.*waiting/i,
    });
    expect(attentionAffordance).toBeInTheDocument();

    fireEvent.click(attentionAffordance);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "feature-branch",
          }),
        }),
      );
    });
  });

  it("horizontal swipe left on embedded terminal advances to next tab using existing selection path", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithThreeTabs = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithThreeTabs))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(stateWithThreeTabs));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");

    // Swipe left (next tab)
    swipe(terminal, -60);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
  });

  it("horizontal swipe right on embedded terminal retreats to previous tab using existing selection path", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateOnTabTwo = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        sessionId: "tab-2-session",
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateOnTabTwo))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(stateOnTabTwo));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");

    // Swipe right (previous tab)
    swipe(terminal, 60);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-1",
          }),
        }),
      );
    });
  });

  it("swiping past the last tab or before the first tab is a no-op issuing no selection request", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateOnFirstTab = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(jsonResponse(stateOnFirstTab));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    const { unmount } = render(<RemoteApp />);

    const terminalFirst = await screen.findByTestId("remote-terminal");

    // Clear initial load fetch calls
    fetchMock.mockClear();

    // Swipe right (previous) on first tab -> no-op
    swipe(terminalFirst, 60);

    expect(fetchMock).not.toHaveBeenCalled();

    unmount();

    // Now test on last tab
    const stateOnLastTab = {
      ...baseState,
      activeContext: {
        ...baseState.activeContext,
        sessionId: "tab-2-session",
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };

    const fetchMockLast = vi
      .fn<typeof fetch>()
      .mockResolvedValue(jsonResponse(stateOnLastTab));

    vi.stubGlobal("fetch", fetchMockLast);

    render(<RemoteApp />);

    const terminalLast = await screen.findByTestId("remote-terminal");

    fetchMockLast.mockClear();

    // Swipe left (next) on last tab -> no-op
    swipe(terminalLast, -60);

    expect(fetchMockLast).not.toHaveBeenCalled();
  });
});
