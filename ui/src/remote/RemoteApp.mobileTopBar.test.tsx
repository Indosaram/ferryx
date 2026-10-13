import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

describe("RemoteApp - MobileTopBar and Sheets (Todo 7)", () => {
  beforeEach(() => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("renders exactly one mobile-top-bar and hides mobile-chat-header under compact matchMedia", async () => {
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));

    render(<RemoteApp />);

    expect(await screen.findByTestId("mobile-top-bar")).toBeInTheDocument();
    expect(screen.getAllByTestId("mobile-top-bar")).toHaveLength(1);
    expect(screen.queryByTestId("mobile-chat-header")).not.toBeInTheDocument();
  });

  it("renders terminal mode without remote-terminal-back under compact mode", async () => {
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));

    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));

    expect(await screen.findByTestId("remote-terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-back")).not.toBeInTheDocument();
  });

  it("opens the switcher sheet and selecting the second tab POSTs /api/v1/workspace/select with that tabId", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState));
    vi.stubGlobal("fetch", ticketed(fetchMock));

    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");

    const sessionsBtn = screen.getByRole("button", { name: /sessions/i });
    expect(sessionsBtn).toBeInTheDocument();
    fireEvent.click(sessionsBtn);

    expect(await screen.findByRole("dialog", { name: "Sessions" })).toBeInTheDocument();

    const tab2 = screen.getByTestId("session-switcher-tab-tab-2");
    expect(tab2).toBeInTheDocument();
    fireEvent.click(tab2);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
  });

  it("opens the overflow sheet, triggers New terminal with createTerminal, and Escape closes it", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState));
    vi.stubGlobal("fetch", ticketed(fetchMock));

    render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");

    const menuBtn = screen.getByRole("button", { name: "More actions" });
    fireEvent.click(menuBtn);

    expect(await screen.findByRole("dialog", { name: "Actions" })).toBeInTheDocument();

    const newTerminalBtn = screen.getByText("New terminal");
    fireEvent.click(newTerminalBtn);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx",
            worktreeSlug: "main",
            createTerminal: true,
          }),
        }),
      );
    });

    // Re-open and verify Escape closes it
    fireEvent.click(screen.getByRole("button", { name: "More actions" }));
    const dialog = await screen.findByRole("dialog", { name: "Actions" });
    expect(dialog).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.animationEnd(dialog);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Actions" })).not.toBeInTheDocument();
    });
  });

  it("renders desktop global header and omits mobile-top-bar when matchMedia matches:true", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: true,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }));

    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(baseState))));

    render(<RemoteApp />);

    expect(screen.queryByTestId("mobile-top-bar")).not.toBeInTheDocument();

    const trigger = await screen.findByRole("button", { name: "Change workspace context" });
    expect(trigger).toBeInTheDocument();

    const header = trigger.closest("header");
    expect(header).toBeInTheDocument();
    expect(header).toHaveClass("h-7");
  });
});

