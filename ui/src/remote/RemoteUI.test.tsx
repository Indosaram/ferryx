import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useId } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resolveAgentLogo } from "../lib/agentIcon";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { PairingPage } from "./PairingPage";
import { RemoteApp } from "./RemoteApp";
import { normalizeRemoteWorkspaceState } from "./RemoteSessionList";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    onSocketLifecycle,
  }: {
    sessionId: string;
    onSocketLifecycle?: (sessionId: string, state: "open" | "closed") => void;
  }) => (
    <div
      data-testid="remote-terminal"
      data-session-id={sessionId}
      data-instance-id={useId()}
      onClick={() => onSocketLifecycle?.(sessionId, "closed")}
      onDoubleClick={() => onSocketLifecycle?.(sessionId, "open")}
    >
      Mirrored terminal {sessionId}
    </div>
  ),
}));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
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

const focusedState = {
  activeContext: {
    workspaceId: "ferryx-ui",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTerminal: {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      running: true,
    },
  },
  projects: [
    {
      workspaceId: "ferryx-ui",
      repoRoot: "/Users/alice/secret/ferryx-ui",
      worktrees: [
        {
          slug: "main",
          label: "main",
          path: "/Users/alice/secret/ferryx-ui",
        },
      ],
    },
    {
      workspaceId: "api-service",
      repoRoot: "/Volumes/private/api-service",
      worktrees: [
        {
          slug: "feature/remote-safe",
          label: "feature/remote-safe",
          path: "/Volumes/private/api-service-worktree",
        },
      ],
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
    {
      sessionId: "background-terminal",
      title: "Hidden /Users/alice/secret shell",
      workspaceId: "api-service",
      worktreeLabel: "private-background",
      running: true,
    },
  ],
};

const currentNativeState = {
  activeWorkspaceId: "ferryx-ui",
  projects: [
    { workspaceId: "ferryx-ui", repoRoot: "/Users/alice/secret/ferryx-ui" },
    { workspaceId: "api-service", repoRoot: "/Volumes/private/api-service" },
  ],
  worktrees: [
    {
      path: "/Users/alice/secret/ferryx-ui",
      branch: "refs/heads/orca/ferryx-ui/main",
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
  ],
};

const confirmedNoFocusState = {
  activeContext: {
    workspaceId: "api-service",
    worktreeSlug: "feature/remote-safe",
    worktreeLabel: "feature/remote-safe",
    activeTerminal: null,
  },
  projects: focusedState.projects,
  sessions: focusedState.sessions,
};

const secondFocusedState = {
  activeContext: {
    workspaceId: "api-service",
    worktreeSlug: "feature/remote-safe",
    worktreeLabel: "feature/remote-safe",
    sessionId: "new-focused-terminal",
  },
  activeWorkspaceId: "api-service",
  projects: focusedState.projects,
  worktrees: [{ worktreeSlug: "feature/remote-safe", worktreeLabel: "feature/remote-safe" }],
  sessions: [
    {
      sessionId: "new-focused-terminal",
      workspaceId: "api-service",
      worktreeLabel: "feature/remote-safe",
      running: true,
    },
  ],
};

const safeServerWorkspaceState = {
  activeContext: {
    workspaceId: "remote-e2e",
    worktreeSlug: "mobile-control",
    worktreeLabel: "mobile-control",
    sessionId: "focused-terminal",
  },
  activeWorkspaceId: "remote-e2e",
  projects: [
    {
      workspaceId: "remote-e2e",
      worktrees: [{ worktreeSlug: "mobile-control", worktreeLabel: "mobile-control" }],
    },
    {
      workspaceId: "other-project",
      worktrees: [{ worktreeSlug: "other-worktree", worktreeLabel: "other-worktree" }],
    },
  ],
  worktrees: [{ worktreeSlug: "mobile-control", worktreeLabel: "mobile-control" }],
  sessions: [
    {
      sessionId: "focused-terminal",
      workspaceId: "remote-e2e",
      worktreeLabel: "mobile-control",
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

describe("Remote UI Components", () => {
  it("PairingPage renders the Ferryx Desktop PIN flow", () => {
    render(<PairingPage onPaired={vi.fn()} />);

    const input = screen.getByPlaceholderText(/6-digit PIN/i);
    expect(input).toHaveAttribute("maxLength", "6");
    expect(screen.getByText(/Ferryx Desktop settings/i)).toBeInTheDocument();
  });

  it("renders the native active-only state as one mirrored terminal without exposing paths", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(currentNativeState)),
    );

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
  });

  it("renders the current safe server workspace contract without local paths", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(safeServerWorkspaceState)),
    );

    render(<RemoteApp />);

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "remote-e2e / mobile-control",
    );
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    expect(
      within(selector).getByRole("button", { name: /other-project.*other-worktree/i }),
    ).toBeEnabled();
    expect(document.body.textContent).not.toMatch(/\/(Users|private|Volumes)\//);
  });

  it("mirrors only the server-declared terminal and safely confirms a context selection", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.queryByText(/background-terminal|private-background|Hidden/i)).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
    expect(document.body).not.toHaveTextContent("/Volumes/private");

    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));

    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    const target = within(selector).getByRole("button", {
      name: /api-service.*feature\/remote-safe/i,
    });
    fireEvent.click(target);

    // Picking a worktree dismisses the selector instead of leaving it stuck open.
    expect(screen.queryByRole("dialog", { name: /Workspace context/i })).toBeNull();
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/workspace/select?token=test-token",
      expect.objectContaining({
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          workspaceId: "api-service",
          worktreeSlug: "feature/remote-safe",
        }),
      }),
    );

    await act(async () => {
      selectionResponse.resolve(jsonResponse({ accepted: true }));
      await selectionResponse.promise;
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
        "api-service / feature/remote-safe",
      );
    });
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByText("No focused terminal")).toBeInTheDocument();
    expect(screen.getByText(/mirror it here/i)).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
  });

  it("waits for the desktop active-selection event when the first confirmation read is stale", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");
    expect(eventSocket().url).toMatch(/\/api\/v1\/events\?token=test-token$/);
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    fireEvent.click(
      within(selector).getByRole("button", {
        name: /api-service.*feature\/remote-safe/i,
      }),
    );

    // The selection POST is in flight; the desktop has not confirmed yet.
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
              sessionId: "new-focused-terminal",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
        "api-service / feature/remote-safe",
      );
    });
  });

  it("recovers from a desktop that never confirms so the picker stays usable", async () => {
    vi.useFakeTimers();
    try {
      localStorage.setItem("ferryx_remote_token", "test-token");
      // The desktop accepts the request over HTTP but never republishes a
      // matching selection, which is exactly what a stale/unreachable desktop
      // listener looks like from the phone.
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(jsonResponse(focusedState))
        .mockResolvedValueOnce(jsonResponse({ accepted: true }))
        .mockResolvedValue(jsonResponse(focusedState));
      vi.stubGlobal("fetch", fetchMock);
      vi.stubGlobal("WebSocket", EventWebSocket);

      render(<RemoteApp />);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
      fireEvent.click(
        within(screen.getByRole("dialog", { name: /Workspace context/i })).getByRole("button", {
          name: /api-service.*feature\/remote-safe/i,
        }),
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.anything(),
      );

      // A selection that is never confirmed must not strand the UI forever:
      // the pending lock has to expire so the picker becomes usable again.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });

      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
      expect(
        within(screen.getByRole("dialog", { name: /Workspace context/i })).getByRole("button", {
          name: /api-service.*feature\/remote-safe/i,
        }),
      ).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("refreshes the mirrored terminal after an unsolicited desktop focus change", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse(secondFocusedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
              sessionId: "new-focused-terminal",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "new-focused-terminal",
      );
    });
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "api-service / feature/remote-safe",
    );
  });

  it("keeps the newest desktop focus when focus events arrive during a refresh", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const firstFocusRefresh = deferred<Response>();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockImplementationOnce(() => firstFocusRefresh.promise)
      .mockResolvedValueOnce(jsonResponse(secondFocusedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main" },
          }),
        }),
      );
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "api-service", worktreeSlug: "feature/remote-safe" },
          }),
        }),
      );
    });

    await act(async () => {
      firstFocusRefresh.resolve(jsonResponse(focusedState));
      await firstFocusRefresh.promise;
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "new-focused-terminal",
      );
    });
  });

  it("clears the mirrored terminal when desktop no longer focuses a terminal", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: null,
          }),
        }),
      );
    });

    expect(await screen.findByText("No focused terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
  });

  it("shows no focused terminal when only undeclared background sessions are present", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(
        jsonResponse({
          ...confirmedNoFocusState,
          sessions: [focusedState.sessions[1]],
        }),
      ),
    );

    render(<RemoteApp />);

    expect(await screen.findByText("No focused terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
    expect(document.body).not.toHaveTextContent("Hidden /Users/alice/secret shell");
  });

  it("MobileKeyDock dispatches primary key actions and latches modifiers", () => {
    const handleSendKey = vi.fn();
    render(<MobileKeyDock onSendKey={handleSendKey} />);

    fireEvent.click(screen.getByText("Ctrl-C"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-c");

    fireEvent.click(screen.getByText("Ctrl"));
    fireEvent.click(screen.getByText("Tab"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-tab");
  });

  it("retains legacy authentication without rendering old Orca branding", async () => {
    localStorage.setItem("rorca_remote_token", "legacy-token");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(confirmedNoFocusState)));

    render(<RemoteApp />);

    const header = await screen.findByRole("banner");
    expect(header).toHaveTextContent("Ferryx Remote");
    expect(header.textContent?.toLowerCase()).not.toContain("orca");
  });

  it("sets browser document.title to active tab or terminal title on initial authenticated load", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithTabs)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await waitFor(() => {
      expect(document.title).toBe("Dev Server - Ferryx");
    });
  });

  it("does not treat a worktree row id as a terminal tab id", () => {
    const parsed = normalizeRemoteWorkspaceState({
      projects: [
        {
          id: "workspace-1",
          worktrees: [{ id: "worktree-row-1", slug: "feature/fast-switch" }],
        },
      ],
    });

    expect(parsed.options).toContainEqual({
      workspaceId: "workspace-1",
      worktreeSlug: "feature/fast-switch",
      worktreeLabel: "feature/fast-switch",
    });
  });

  it("updates document.title on unsolicited desktop focus and active tab change and falls back to Ferryx", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const firstState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };
    const secondState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(firstState))
      .mockResolvedValueOnce(jsonResponse(secondState))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    const { unmount } = render(<RemoteApp />);

    await waitFor(() => {
      expect(document.title).toBe("Editor - Ferryx");
    });

    // Unsolicited desktop tab change
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
      expect(document.title).toBe("Dev Server - Ferryx");
    });

    // Desktop unfocuses terminal/tab
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: null,
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(document.title).toBe("Ferryx");
    });

    unmount();
    expect(document.title).toBe("Ferryx");
  });

  it("resets document.title to Ferryx when unpaired or disconnected", async () => {
    render(<RemoteApp />);
    expect(document.title).toBe("Ferryx");
  });

  it("allows sequential traversal using previous and next terminal tab controls and ordinal indicator", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const multiTabState1 = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        sessionId: "session-tab-1",
        activeTerminal: {
          sessionId: "session-tab-1",
          title: "Editor",
          running: true,
        },
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Build" },
          { id: "tab-3", label: "Server" },
        ],
      },
      sessions: [
        { sessionId: "session-tab-1", title: "Editor", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
        { sessionId: "session-tab-2", title: "Build", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
        { sessionId: "session-tab-3", title: "Server", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
      ],
    };
    const multiTabState2 = {
      ...multiTabState1,
      activeContext: {
        ...multiTabState1.activeContext,
        tabId: "tab-2",
        sessionId: "session-tab-2",
        activeTerminal: {
          sessionId: "session-tab-2",
          title: "Build",
          running: true,
        },
      },
    };
    const multiTabState3 = {
      ...multiTabState1,
      activeContext: {
        ...multiTabState1.activeContext,
        tabId: "tab-3",
        sessionId: "session-tab-3",
        activeTerminal: {
          sessionId: "session-tab-3",
          title: "Server",
          running: true,
        },
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(multiTabState1))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState2))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState3))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState2));

    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-tab-1");

    // Ordinal indicator
    expect(screen.getByText("1 / 3")).toBeInTheDocument();

    const prevBtn = screen.getByRole("button", { name: /Previous terminal tab/i });
    const nextBtn = screen.getByRole("button", { name: /Next terminal tab/i });
    expect(prevBtn).toBeDisabled();
    expect(nextBtn).toBeEnabled();

    // Traverse to next tab (tab-2)
    fireEvent.click(nextBtn);

    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/v1/workspace/select?token=test-token",
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

    // Simulate desktop focus event
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
              sessionId: "session-tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-2",
      );
    });
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeEnabled();

    // Traverse to next tab (tab-3)
    fireEvent.click(nextBtn);

    expect(fetchMock).toHaveBeenNthCalledWith(
      4,
      "/api/v1/workspace/select?token=test-token",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          workspaceId: "ferryx-ui",
          worktreeSlug: "main",
          tabId: "tab-3",
        }),
      }),
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-3",
              sessionId: "session-tab-3",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-3",
      );
    });
    expect(screen.getByText("3 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeDisabled();

    // Traverse back to previous tab (tab-2)
    fireEvent.click(prevBtn);

    expect(fetchMock).toHaveBeenNthCalledWith(
      6,
      "/api/v1/workspace/select?token=test-token",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          workspaceId: "ferryx-ui",
          worktreeSlug: "main",
          tabId: "tab-2",
        }),
      }),
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
              sessionId: "session-tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-2",
      );
    });
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeEnabled();
  });

  it("renders safe tab items under active worktree and dispatches tab switch request", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "/Users/secret/path/run" },
        ],
      },
    };

    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithTabs));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    expect(tablist).toBeInTheDocument();

    const editorTab = within(tablist).getByRole("tab", { name: /editor/i });
    const devServerTab = within(tablist).getByRole("tab", { name: /dev server/i });
    expect(editorTab).toHaveAttribute("aria-selected", "true");
    expect(devServerTab).toHaveAttribute("aria-selected", "false");

    expect(within(tablist).queryByText("/Users/secret/path/run")).not.toBeInTheDocument();
    expect(within(tablist).getByText("Terminal")).toBeInTheDocument();

    fireEvent.click(devServerTab);

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

  it("cycles published terminal tabs only after Desktop confirms the selected tab", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const editorState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "focused-terminal",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };
    const devServerState = {
      ...editorState,
      activeContext: {
        ...editorState.activeContext,
        activeTerminal: {
          sessionId: "dev-server-terminal",
          title: "Dev Server",
          running: true,
        },
        tabId: "tab-2",
      },
      sessions: [
        {
          sessionId: "dev-server-terminal",
          workspaceId: "ferryx-ui",
          worktreeLabel: "main",
          running: true,
        },
      ],
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(editorState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(devServerState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(editorState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );
    expect(screen.getByLabelText("Terminal position: Tab 1 of 3")).toHaveTextContent("1 / 3");

    fireEvent.click(screen.getByRole("button", { name: "Next terminal tab" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "dev-server-terminal",
      );
    });
    expect(screen.getByLabelText("Terminal position: Tab 2 of 3")).toHaveTextContent("2 / 3");

    fireEvent.click(screen.getByRole("button", { name: "Previous terminal tab" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-1",
          }),
        }),
      );
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-1" },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "focused-terminal",
      );
    });
    expect(screen.getByLabelText("Terminal position: Tab 1 of 3")).toHaveTextContent("1 / 3");
  });

  it("retains authorization across normal page reload when server returns transient error", async () => {
    localStorage.setItem("ferryx_remote_token", "paired-device-token");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse({ error: "gateway busy" }, false));
    vi.stubGlobal("fetch", fetchMock);

    render(<RemoteApp />);

    // Wait for the fetch attempt
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());

    // Authorization token must NOT be cleared from localStorage on non-401/403 failure
    expect(localStorage.getItem("ferryx_remote_token")).toBe("paired-device-token");
    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
  });

  it("clears authorization and returns to PairingPage when token is revoked (401)", async () => {
    localStorage.setItem("ferryx_remote_token", "revoked-device-token");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue({
      ok: false,
      status: 401,
      json: vi.fn(async () => ({ error: "Invalid or revoked token" })),
    } as unknown as Response);
    vi.stubGlobal("fetch", fetchMock);

    render(<RemoteApp />);

    // Must navigate to pairing page and clear token
    expect(await screen.findByPlaceholderText(/6-digit PIN/i)).toBeInTheDocument();
    expect(localStorage.getItem("ferryx_remote_token")).toBeNull();
  });

  it("normalizeRemoteWorkspaceState parses activityState, agentType, and attention and drops invalid values defensively", () => {
    const rawState = {
      activeWorkspaceId: "project-1",
      activeContext: {
        workspaceId: "project-1",
        worktreeSlug: "wt-main",
        worktreeLabel: "main",
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Claude Agent", activityState: "working", agentType: "claude" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting", agentType: "codex" },
          { id: "tab-3", label: "Omo Agent", activityState: "done", agentType: "omo" },
          { id: "tab-4", label: "Invalid State", activityState: "unknown_state", agentType: "/bin/sh" },
          { id: "tab-5", label: "Invalid State 2", activityState: "starting", agentType: "copilot" },
        ],
      },
      projects: [
        {
          workspaceId: "project-1",
          worktrees: [
            { worktreeSlug: "wt-main", worktreeLabel: "main", attention: "waiting" },
            { worktreeSlug: "wt-feature", worktreeLabel: "feature", attention: "working" },
            { worktreeSlug: "wt-done", worktreeLabel: "done-wt", attention: "done" },
            { worktreeSlug: "wt-invalid", worktreeLabel: "invalid-wt", attention: "unsupported" },
          ],
        },
      ],
      worktrees: [
        { worktreeSlug: "wt-main", worktreeLabel: "main", attention: "waiting" },
      ],
      sessions: [],
    };

    const model = normalizeRemoteWorkspaceState(rawState);
    const tabs = model.context.terminalTabs!;
    expect(tabs).toHaveLength(5);
    expect(tabs[0]).toEqual({ id: "tab-1", label: "Claude Agent", activityState: "working", agentType: "claude" });
    expect(tabs[1]).toEqual({ id: "tab-2", label: "Codex Agent", activityState: "waiting", agentType: "codex" });
    expect(tabs[2]).toEqual({ id: "tab-3", label: "Omo Agent", activityState: "done", agentType: "omo" });
    expect(tabs[3]).toEqual({ id: "tab-4", label: "Invalid State" });
    expect(tabs[4]).toEqual({ id: "tab-5", label: "Invalid State 2", agentType: "copilot" });

    const mainOpt = model.options.find((opt) => opt.worktreeSlug === "wt-main");
    expect(mainOpt?.attention).toBe("waiting");

    const featOpt = model.options.find((opt) => opt.worktreeSlug === "wt-feature");
    expect(featOpt?.attention).toBe("working");

    const doneOpt = model.options.find((opt) => opt.worktreeSlug === "wt-done");
    expect(doneOpt?.attention).toBe("done");

    const invalidOpt = model.options.find((opt) => opt.worktreeSlug === "wt-invalid");
    expect(invalidOpt?.attention).toBeUndefined();
  });

  it("tab strip renders state indicators for waiting and working tabs discoverable by accessible name", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithActivity = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Dev Server", activityState: "waiting" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithActivity)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    expect(tablist).toBeInTheDocument();

    const workingTab = within(tablist).getByRole("tab", { name: /editor.*working/i });
    expect(workingTab).toBeInTheDocument();
    expect(within(workingTab).getByTestId("tab-working-indicator")).toBeInTheDocument();

    const waitingTab = within(tablist).getByRole("tab", { name: /dev server.*waiting/i });
    expect(waitingTab).toBeInTheDocument();
    expect(within(waitingTab).getByTestId("tab-waiting-indicator")).toBeInTheDocument();
  });

  it("lists a published terminal pane even when the desktop has nothing focused", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const inventoryWithoutFocus = {
      ...focusedState,
      activeContext: {
        workspaceId: "ferryx-ui",
        worktreeSlug: "main",
        worktreeLabel: "main",
        terminalTabs: [{ id: "tab-1", label: "Editor", worktreeSlug: "main", worktreeLabel: "main" }],
      },
    };
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(inventoryWithoutFocus)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const tablist = await screen.findByRole("tablist", { name: /terminal tabs/i });
    // One entry is enough to render the list, and no mirrored terminal is required to browse it.
    expect(within(tablist).getAllByRole("tab")).toHaveLength(1);
    expect(within(tablist).getByRole("tab", { name: /editor/i })).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByText("No focused terminal")).toBeInTheDocument();
  });

  it("selects a pane from another worktree using that pane's own worktree", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const crossWorktreeState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", worktreeSlug: "main", worktreeLabel: "main" },
          {
            id: "tab-2::leaf-b",
            label: "Build (2)",
            worktreeSlug: "feature/remote-safe",
            worktreeLabel: "feature/remote-safe",
          },
        ],
      },
    };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(crossWorktreeState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const tablist = await screen.findByRole("tablist", { name: /terminal tabs/i });
    fireEvent.click(within(tablist).getByRole("tab", { name: /build.*feature\/remote-safe/i }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/workspace/select?token=test-token",
        expect.objectContaining({
          method: "POST",
          // The pane's own worktree travels with the request; the mirrored context is not assumed.
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "feature/remote-safe",
            tabId: "tab-2::leaf-b",
          }),
        }),
      ),
    );
  });

  it("tab strip renders brand logo image for supported agentType and fallback terminal icon for unknown/missing agentType", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithAgents = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Claude Agent", agentType: "claude" },
          { id: "tab-2", label: "Unknown Agent", agentType: "unsupported-tool" },
          { id: "tab-3", label: "Plain Terminal" },
        ],
      },
    };
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithAgents)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    const claudeTab = within(tablist).getByRole("tab", { name: /claude agent/i });
    const unknownTab = within(tablist).getByRole("tab", { name: /unknown agent/i });
    const plainTab = within(tablist).getByRole("tab", { name: /plain terminal/i });

    // Claude tab has img with claude logo
    const claudeImg = within(claudeTab).getByTestId("tab-agent-icon");
    expect(claudeImg).toHaveAttribute("src", resolveAgentLogo("claude")!);
    expect(claudeImg).toHaveAttribute("data-agent-type", "claude");
    expect(within(claudeTab).queryByTestId("tab-terminal-icon")).not.toBeInTheDocument();

    // Unknown agent and plain tab do not have agent logo img, they have terminal fallback icon
    const unknownIcon = within(unknownTab).getByTestId("tab-terminal-icon");
    expect(unknownIcon).toBeInTheDocument();
    expect(within(unknownTab).queryByTestId("tab-agent-icon")).not.toBeInTheDocument();

    const plainIcon = within(plainTab).getByTestId("tab-terminal-icon");
    expect(plainIcon).toBeInTheDocument();
    expect(within(plainTab).queryByTestId("tab-agent-icon")).not.toBeInTheDocument();
  });

  it("context selector exposes worktree attention in its accessible name", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithAttention = {
      ...focusedState,
      projects: [
        {
          workspaceId: "ferryx-ui",
          worktrees: [
            { worktreeSlug: "main", worktreeLabel: "main", attention: "waiting" },
          ],
        },
        {
          workspaceId: "api-service",
          worktrees: [
            { worktreeSlug: "feature/remote-safe", worktreeLabel: "feature/remote-safe", attention: "working" },
          ],
        },
      ],
    };
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithAttention)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });

    // The worktree with waiting attention exposes "waiting" in its button accessible name
    const waitingOption = within(selector).getByRole("button", {
      name: /ferryx-ui.*main.*waiting/i,
    });
    expect(waitingOption).toBeInTheDocument();

    const workingOption = within(selector).getByRole("button", {
      name: /api-service.*feature\/remote-safe.*working/i,
    });
    expect(workingOption).toBeInTheDocument();
  });

  it("immediately remounts RemoteTerminal to session when selecting a tab with sessionId before confirmation", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: {
          sessionId: "session-dev",
          title: "Dev Server",
          running: true,
        },
        tabId: "tab-2",
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(switchedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-editor");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    const devTab = within(tablist).getByRole("tab", { name: /dev server/i });

    // Click dev server tab
    fireEvent.click(devTab);

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    expect(optimisticTerminal).toHaveAttribute("data-session-id", "session-dev");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.doubleClick(optimisticTerminal);

    // Tab buttons should be disabled during pending selection
    expect(devTab).toBeDisabled();

    // Resolve POST request
    await act(async () => {
      selectionResponse.resolve(jsonResponse({ accepted: true }));
      await selectionResponse.promise;
    });

    // RemoteTerminal is still on session-dev
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Desktop confirms selection via WebSocket event
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
      expect(devTab).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );
  });

  it("remounts the optimistic terminal after confirmation when its socket closed before opening", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(screen.getByRole("tablist", { name: /terminal tabs/i })).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.click(optimisticTerminal);

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
        "data-instance-id",
        optimisticInstanceId,
      );
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
  });

  it("remounts on confirmation when the optimistic socket opened and then closed", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(screen.getByRole("tablist", { name: /terminal tabs/i })).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.doubleClick(optimisticTerminal);
    fireEvent.click(optimisticTerminal);

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
        "data-instance-id",
        optimisticInstanceId,
      );
    });
  });

  it("remounts when the optimistic socket reports its failed handshake after confirmation", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(screen.getByRole("tablist", { name: /terminal tabs/i })).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.getByRole("tab", { name: /dev server/i })).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );

    fireEvent.click(screen.getByTestId("remote-terminal"));
    expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
  });

  it("does not retry again when the confirmed replacement socket also closes", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState)));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(screen.getByRole("tablist", { name: /terminal tabs/i })).getByRole("tab", { name: /dev server/i }));
    const firstInstanceId = screen.getByTestId("remote-terminal").getAttribute("data-instance-id");

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });
    await waitFor(() => expect(screen.getByRole("tab", { name: /dev server/i })).toBeEnabled());

    fireEvent.click(screen.getByTestId("remote-terminal"));
    const replacement = screen.getByTestId("remote-terminal");
    expect(replacement).not.toHaveAttribute("data-instance-id", firstInstanceId);
    const replacementInstanceId = replacement.getAttribute("data-instance-id");

    fireEvent.click(replacement);
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      replacementInstanceId,
    );
  });

  it("does not perform immediate post-POST workspace/state refresh on selection", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        tabId: "tab-2",
        activeTerminal: {
          sessionId: "session-dev",
          running: true,
        },
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    // Wait for POST to complete
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      expect.stringContaining("/api/v1/workspace/select"),
      expect.anything(),
    );

    // Crucial check: selection flow must NOT immediately call /api/v1/workspace/state
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Desktop confirms selection via WebSocket event
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

    // Now confirmation fetch is triggered
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(3);
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      expect.stringContaining("/api/v1/workspace/state"),
    );
  });

  it("clears optimistic session override and reverts when confirmation times out", async () => {
    vi.useFakeTimers();
    try {
      localStorage.setItem("ferryx_remote_token", "test-token");
      const stateWithSessions = {
        ...focusedState,
        activeContext: {
          ...focusedState.activeContext,
          activeTerminal: {
            sessionId: "session-editor",
            title: "Editor",
            running: true,
          },
          tabId: "tab-1",
          terminalTabs: [
            { id: "tab-1", label: "Editor", sessionId: "session-editor" },
            { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
          ],
        },
      };
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(jsonResponse(stateWithSessions))
        .mockResolvedValueOnce(jsonResponse({ accepted: true }));
      vi.stubGlobal("fetch", fetchMock);
      vi.stubGlobal("WebSocket", EventWebSocket);

      render(<RemoteApp />);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
      fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

      // Optimistically shows session-dev
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

      // Advance past confirmation timeout (6000ms)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(7000);
      });

      // Optimistic override should clear and revert to authoritative session-editor
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
      expect(within(tablist).getByRole("tab", { name: /dev server/i })).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears optimistic session override when selection request fails", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ error: "gateway busy" }, false));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-editor");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    const devTab = within(tablist).getByRole("tab", { name: /dev server/i });

    fireEvent.click(devTab);

    // After failure resolves, optimistic override reverts and lock is released
    await waitFor(() => {
      expect(devTab).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
  });

  it("clears optimistic session override when user disconnects", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockImplementationOnce(() => selectionResponse.promise);
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Click Disconnect
    fireEvent.click(screen.getByRole("button", { name: /Disconnect/i }));

    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Confirm disconnect/i }));

    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText(/6-digit PIN/i)).toBeInTheDocument();
  });

  it("requires confirmation before Disconnect removes the pairing", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    fireEvent.click(screen.getByRole("button", { name: /^Disconnect$/i }));

    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Confirm disconnect/i })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/i }));

    expect(screen.getByRole("button", { name: /^Disconnect$/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Confirm disconnect/i })).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
  });

  it("Cancel keeps the remote session paired", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    fireEvent.click(screen.getByRole("button", { name: /^Disconnect$/i }));
    expect(screen.getByRole("button", { name: /Confirm disconnect/i })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Cancel$/i }));

    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
  });

  it("clears optimistic session override when a different authoritative state arrives", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithThreeTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
          { id: "tab-3", label: "Tests", sessionId: "session-tests" },
        ],
      },
    };
    const differentAuthoritativeState = {
      ...stateWithThreeTabs,
      activeContext: {
        ...stateWithThreeTabs.activeContext,
        activeTerminal: {
          sessionId: "session-tests",
          title: "Tests",
          running: true,
        },
        tabId: "tab-3",
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithThreeTabs))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(differentAuthoritativeState));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await screen.findByTestId("remote-terminal");

    const tablist = screen.getByRole("tablist", { name: /terminal tabs/i });
    // User requested tab-2 (session-dev)
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Desktop unexpectedly switches to tab-3 (session-tests) instead
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-3",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tests",
      );
    });
  });
});
