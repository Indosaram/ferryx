import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, onTestFailed, vi } from "vitest";

/**
 * Failure-only diagnostics for the post-merge run: the fetch order (method + PATHNAME only) and the
 * terminal-related selector state at the point of failure. Pathnames only - query strings can carry
 * tickets and tokens - and no header, body or DOM dump is ever read or printed. Bounded to 40 calls
 * per mock. Registered through onTestFailed, so a passing test prints nothing and the original
 * assertion error is untouched.
 */
/**
 * Writes a diagnostic line straight to the process stdout. The JSON reporter does not implement
 * onUserConsoleLog, so a console.log never reaches a --reporter=json receipt; process.stdout does.
 */
function emitLine(line: string): void {
  try {
    process.stdout.write(`${line}\n`);
  } catch {
    /* a diagnostic must never change the outcome of the test it reports on */
  }
}

function reportFetchOrder(label: string, ...mocks: unknown[]): void {
  try {
    mocks.forEach((mock, mockIndex) => {
      const calls = (mock as { mock?: { calls?: unknown[][] } })?.mock?.calls ?? [];
      const shown = calls.slice(0, 40).map((args, index) => {
        const raw = String(args[0] instanceof Request ? args[0].url : args[0]);
        let pathname = "(unparseable-url)";
        try {
          pathname = new URL(raw, "http://localhost").pathname;
        } catch {
          /* keep the marker: the raw value is never printed */
        }
        const init = args[1] as RequestInit | undefined;
        return `${index + 1} ${(init?.method ?? "GET").toUpperCase()} ${pathname}`;
      });
      const more = calls.length > 40 ? ` (+${calls.length - 40} more)` : "";
      emitLine(
        `[ui-diag] ${label} | mock${mockIndex + 1} order (${calls.length}): ${shown.join(" | ") || "(none)"}${more}`,
      );
    });
    const selectors = ["remote-view-mode-terminal", "remote-terminal", "remote-terminal-grid", "mobile-chat-workspace"]
      .map((id) => `${id}=${document.querySelector(`[data-testid="${id}"]`) ? "present" : "absent"}`)
      .join(", ");
    const trigger = document.querySelector('button[aria-label="Change workspace context"]') ? "present" : "absent";
    emitLine(`[ui-diag] ${label} | selectors: ${selectors}, context-trigger=${trigger}`);
  } catch {
    // A diagnostic must never change the outcome of the test it reports on.
  }
}

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
  onTestFailed(() => reportFetchOrder("reconnect keeps pairing", fetcher));
  let unmount = () => {};
  await act(async () => { unmount = render(<RemoteApp />).unmount; });
  // Chat is the default surface: the mirrored terminal is asked for explicitly.
  // Timer-free readiness: flush the mocked state read, then read the switch synchronously.
  await act(async () => {});
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
  });
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
