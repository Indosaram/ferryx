import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    showTitleBar = true,
    onBack,
  }: {
    sessionId: string;
    showTitleBar?: boolean;
    onBack?: () => void;
  }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      {showTitleBar ? (
        <button type="button" data-testid="remote-terminal-back" onClick={onBack}>
          Back to chat
        </button>
      ) : null}
      Mirrored terminal {sessionId}
    </div>
  ),
}));

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
    text: vi.fn(async () => JSON.stringify(body)),
  } as unknown as Response;
}

function ticketed(inner: typeof fetch): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    return inner(input, init);
  }) as unknown as typeof fetch;
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

const baseState = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-1",
    activeTerminal: { sessionId: "sess-1", title: "Shell 1", running: true },
    terminalTabs: [
      {
        id: "tab-1",
        sessionId: "sess-1",
        label: "Shell 1",
        agentType: "shell",
        activityState: "working",
        worktreeLabel: "main",
      },
      {
        id: "tab-2",
        sessionId: "sess-2",
        label: "Agent Task",
        agentType: "claude",
        activityState: "waiting",
        worktreeLabel: "main",
      },
    ],
  },
};

describe("RemoteApp shell states", () => {
  beforeEach(() => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("terminal mode with no running terminal offers New terminal instead of a blank body", async () => {
    const empty = { activeContext: { ...baseState.activeContext, activeTabId: null, activeTerminal: null, terminalTabs: [] } };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(empty));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));

    const emptyState = await screen.findByTestId("remote-terminal-empty");
    fireEvent.click(within(emptyState).getByRole("button", { name: "New terminal" }));
    await waitFor(() => {
      const select = fetchMock.mock.calls.find(([input]) => String(input).includes("/api/v1/workspace/select"));
      expect(select).toBeDefined();
      expect(JSON.parse(String(select?.[1]?.body))).toMatchObject({ workspaceId: "ferryx", createTerminal: true });
    });
  });

  it("New worktree opens an in-app sheet instead of a browser prompt and posts the typed name as a slug", async () => {
    const promptSpy = vi.fn();
    vi.stubGlobal("prompt", promptSpy);
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    fireEvent.click(screen.getByRole("button", { name: "More actions" }));
    const actions = await screen.findByRole("dialog", { name: "Actions" });
    fireEvent.click(within(actions).getByTestId("overflow-action-new-worktree"));

    const sheet = await screen.findByRole("dialog", { name: "New worktree" });
    const input = within(sheet).getByLabelText("Worktree name");
    expect(input).toHaveClass("text-base");
    fireEvent.change(input, { target: { value: "Fix Login Bug" } });
    expect(within(sheet).getByText("Will be created as fix-login-bug")).toBeInTheDocument();
    fireEvent.click(within(sheet).getByRole("button", { name: "Create worktree" }));

    await waitFor(() => {
      const create = fetchMock.mock.calls.find(([input]) => String(input).includes("/api/v1/workspace/worktrees"));
      expect(create).toBeDefined();
      expect(JSON.parse(String(create?.[1]?.body)).worktree).toEqual({ wsId: "ferryx", slug: "fix-login-bug" });
    });
    expect(promptSpy).not.toHaveBeenCalled();
  });

  it("a tapped alert for a session that never appears is cleared and reported", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      window.history.replaceState(null, "", "/#session=gone-session");
      vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));
      render(<RemoteApp />);
      await screen.findByTestId("mobile-top-bar");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });

      expect(await screen.findByText("That session isn't available anymore")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
      window.history.replaceState(null, "", "/");
    }
  });
});
