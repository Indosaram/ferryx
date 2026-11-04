import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { seriousAxeViolations } from "../test/axe";
import { remoteHostStore } from "../state/remoteHostStore";
import { clearStoredAccountSessionToken } from "./accountSession";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => <div data-testid="remote-terminal" data-session-id={sessionId} />,
}));

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

class SilentWebSocket {
  readonly url: string;
  close = vi.fn();
  send = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;
  constructor(url: string) {
    this.url = url;
  }
}

const state = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-1",
    activeTerminal: { sessionId: "sess-1", title: "Shell 1", running: true },
    terminalTabs: [
      { id: "tab-1", sessionId: "sess-1", label: "Shell 1", agentType: "shell", activityState: "working", worktreeLabel: "main" },
      { id: "tab-2", sessionId: "sess-2", label: "Agent Task", agentType: "claude", activityState: "waiting", worktreeLabel: "main" },
    ],
  },
};

describe("RemoteApp shell accessibility", () => {
  beforeEach(() => {
    remoteHostStore.reset();
    localStorage.clear();
    clearStoredAccountSessionToken();
  });

  afterEach(() => {
    cleanup();
    remoteHostStore.reset();
    localStorage.clear();
    window.history.replaceState(null, "", "/");
    vi.unstubAllGlobals();
  });

  it("the signed-out entry has no serious or critical axe violations", async () => {
    window.history.replaceState(null, "", "/");
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(new Response("{}", { status: 404 })));
    const { container } = await act(async () => render(<RemoteApp />));

    expect(screen.getByRole("heading", { name: /Sign in to Ferryx/i })).toBeInTheDocument();
    expect(await seriousAxeViolations(container)).toEqual([]);
  });

  it("the paired shell with its top bar has no serious or critical axe violations", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", SilentWebSocket);
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>(async (input) => {
        const url = String(input instanceof Request ? input.url : input);
        if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
        return jsonResponse(state);
      }),
    );
    const { container } = render(<RemoteApp />);

    await screen.findByTestId("mobile-top-bar");
    expect(await seriousAxeViolations(container)).toEqual([]);
  });
});
