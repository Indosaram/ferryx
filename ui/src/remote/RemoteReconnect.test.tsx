import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => <div data-testid="session">{sessionId}</div>,
}));

class EventSocket {
  static instances: EventSocket[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  close = vi.fn();
  constructor(readonly url: string) {
    EventSocket.instances.push(this);
  }
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  localStorage.clear();
  EventSocket.instances = [];
});

function ticketed(inner: typeof fetch): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return new Response(JSON.stringify({ ticket: "ui-test-ticket", expiresAt: 9999999999 }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }
    return inner(input, init);
  }) as unknown as typeof fetch;
}

it("reconnects events and refreshes missed focus without losing pairing", async () => {
  // Given a paired browser that has loaded one desktop selection.
  vi.useFakeTimers();
  localStorage.setItem(`ferryx_remote_token_local:${window.location.origin}`, "paired");
  vi.stubGlobal("WebSocket", EventSocket);
  let sessionId: string | null = "before-outage";
  const fetcher = vi.fn(async () => new Response(JSON.stringify({
    activeContext: { workspaceId: "workspace", sessionId, terminalTabs: [] },
    projects: [],
    sessions: [],
  })));
  vi.stubGlobal("fetch", ticketed(fetcher));
  let unmount = () => {};
  await act(async () => { unmount = render(<RemoteApp />).unmount; });
  expect(screen.getByTestId("session").textContent).toBe("before-outage");
  const first = EventSocket.instances[0];
  if (!first) throw new Error("Missing initial event socket");

  // When the socket is lost and the desktop clears focus while disconnected.
  await act(async () => { first.onclose?.(); });
  sessionId = null;
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(EventSocket.instances).toHaveLength(2);
  const recovered = EventSocket.instances[1];
  if (!recovered) throw new Error("Missing recovered event socket");
  await act(async () => { recovered.onopen?.(); });

  // Then reconnect itself refreshes state, even without a selection event.
  expect(screen.queryByTestId("session")).toBeNull();
  expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBe("paired");
  await act(async () => { recovered.onclose?.(); unmount(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
  expect(EventSocket.instances).toHaveLength(2);
});
