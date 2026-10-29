/**
 * The phone's Inbox (plan todo 9): one needs-you row per waiting session with the badge count,
 * an inline answer through the prompt route, and a row answered elsewhere dropped on the pushed
 * `agentPromptChanged` frame without waiting for any timer.
 */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => <div data-testid="remote-terminal">{sessionId}</div>,
}));

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
    text: vi.fn(async () => JSON.stringify(body)),
  } as unknown as Response;
}

class StubWebSocket {
  static instances: StubWebSocket[] = [];
  readonly url: string;
  binaryType = "blob";
  readyState = 1;
  close = vi.fn();
  send = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(url: string) {
    this.url = url;
    StubWebSocket.instances.push(this);
  }
}

const DAEMON_EPOCH = "7";

const state = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-main",
    activeTerminal: { sessionId: "sess-main", title: "shell", running: true },
    terminalTabs: [
      { id: "tab-main", sessionId: "sess-main", label: "shell", activityState: "working", worktreeLabel: "main" },
      { id: "tab-a", sessionId: "sess-a", label: "alpha", agentType: "claude", activityState: "waiting", worktreeLabel: "main" },
      { id: "tab-b", sessionId: "sess-b", label: "bravo", agentType: "claude", activityState: "waiting", worktreeLabel: "main" },
    ],
  },
};

function promptOf(session: string, kind = "question"): Record<string, unknown> {
  return {
    prompt: {
      promptId: `p-${session}`,
      agent: "claude",
      kind,
      title: `Pick for ${session}`,
      question: `Pick for ${session}`,
      options: [{ label: "first" }, { label: "second" }],
      multiSelect: false,
    },
    screenRevision: `rev-${session}`,
    cols: 80,
    rows: 24,
  };
}

type Call = { url: string; method: string; body: unknown };

function installFetch(prompts: Record<string, Record<string, unknown>>, workspaceState: unknown = state): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input instanceof Request ? input.url : input);
      const method = (init?.method ?? "GET").toUpperCase();
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : null;
      calls.push({ url, method, body });
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "t", expiresAt: 9999999999 });
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: "owner-1" });
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({
          sessions: ["sess-main", "sess-a", "sess-b"].map((sessionId) => ({ sessionId, daemonEpoch: DAEMON_EPOCH, running: true })),
        });
      }
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(workspaceState);
      if (url.includes("/reference-chat/") && url.includes("/prompt")) {
        const session = Object.keys(prompts).find((id) => url.includes(`/reference-chat/${id}/`));
        return jsonResponse(session ? prompts[session] : { prompt: null, screenRevision: "rev-0", cols: 80, rows: 24 });
      }
      if (url.includes("/reference-chat/") && url.includes("/answer")) return jsonResponse({ ok: true, data: {}, requestId: "r1" });
      return jsonResponse({});
    }) as unknown as typeof fetch,
  );
  return calls;
}

async function openInbox(): Promise<HTMLElement> {
  const button = await screen.findByTestId("mobile-inbox-button");
  await act(async () => {
    fireEvent.click(button);
  });
  return screen.getByRole("dialog", { name: "Inbox" });
}

describe("phone Inbox (todo 9)", () => {
  beforeEach(() => {
    StubWebSocket.instances = [];
    localStorage.clear();
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", StubWebSocket);
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    // Only the polls are faked: none may fire, so a row can only leave on the answer or the push.
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  it("lists one needs-you row per waiting session and shows their count on the badge", async () => {
    installFetch({ "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") });
    render(<RemoteApp />);
    await waitFor(() => expect(screen.getByTestId("mobile-inbox-badge")).toHaveTextContent("2"));
    const sheet = await openInbox();
    const rows = within(sheet).getAllByTestId("attention-row");
    expect(rows).toHaveLength(2);
    expect(rows.every((row) => row.getAttribute("data-attention-state") === "needs-you")).toBe(true);
  });

  it("lists waiting sessions of other worktrees too, not only the desktop's active selection", async () => {
    const hostState = {
      ...state,
      contexts: [
        { workspaceId: "ferryx", worktreeSlug: "feat-c", worktreeLabel: "feat-c", sessionId: "sess-c", attention: "waiting" },
        { workspaceId: "docs", worktreeSlug: "feat-d", worktreeLabel: "feat-d", sessionId: "sess-d", attention: "waiting" },
      ],
    };
    const calls = installFetch({ "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") }, hostState);
    render(<RemoteApp />);
    await waitFor(() => expect(screen.getByTestId("mobile-inbox-badge")).toHaveTextContent("4"));
    const sheet = await openInbox();
    const rows = within(sheet).getAllByTestId("attention-row");
    expect(rows).toHaveLength(4);
    expect(within(sheet).getByText("ferryx / feat-c")).toBeInTheDocument();
    expect(within(sheet).getByText("docs / feat-d")).toBeInTheDocument();
    expect(calls.some((call) => call.url.includes("/workspace/select"))).toBe(false);
  });

  it("answers inline through the prompt route and removes the row", async () => {
    const calls = installFetch({ "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") });
    render(<RemoteApp />);
    await waitFor(() => expect(screen.getByTestId("mobile-inbox-badge")).toHaveTextContent("2"));
    const sheet = await openInbox();
    await within(sheet).findByText("Pick for sess-a");
    const panel = within(sheet).getByText("Pick for sess-a").closest("[data-testid='attention-prompt']") as HTMLElement;
    await act(async () => {
      fireEvent.click(within(panel).getAllByTestId("attention-prompt-option")[1]);
    });

    const answer = calls.find((call) => call.method === "POST" && call.url.includes("/reference-chat/sess-a/answer"));
    expect(answer?.body).toMatchObject({
      target: { backendSessionId: "sess-a", ownerId: "owner-1", epoch: DAEMON_EPOCH },
      registryId: "claude",
      params: { promptId: "p-sess-a", screenRevision: "rev-sess-a", answer: { optionIndex: 1 } },
    });
    await waitFor(() => expect(within(sheet).getAllByTestId("attention-row")).toHaveLength(1));
    expect(within(sheet).queryByText("Pick for sess-a")).toBeNull();
    expect(screen.getByTestId("mobile-inbox-badge")).toHaveTextContent("1");
    expect(calls.some((call) => call.url.includes("/workspace/select"))).toBe(false);
  });

  it("drops a row answered elsewhere on the pushed agentPromptChanged frame", async () => {
    const prompts = { "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") };
    const calls = installFetch(prompts);
    render(<RemoteApp />);
    const sheet = await openInbox();
    await within(sheet).findByText("Pick for sess-b");

    // Answered on the desktop: the pane's screen no longer shows the menu.
    prompts["sess-b"] = { prompt: null, screenRevision: "rev-after", cols: 80, rows: 24 };
    await act(async () => {
      for (const socket of StubWebSocket.instances) {
        socket.onmessage?.({
          data: JSON.stringify({ type: "agentPromptChanged", sessionId: "sess-b", promptId: null, screenRevision: "rev-after" }),
        } as MessageEvent);
      }
    });

    expect(within(sheet).queryByText("Pick for sess-b")).toBeNull();
    expect(within(sheet).getAllByTestId("attention-row")).toHaveLength(1);
    expect(screen.getByTestId("mobile-inbox-badge")).toHaveTextContent("1");
    expect(calls.some((call) => call.url.includes("/workspace/select"))).toBe(false);
  });
  it("asks to confirm a menu pick before anything is sent, then posts the picked option", async () => {
    const calls = installFetch({ "sess-a": promptOf("sess-a", "menu") });
    render(<RemoteApp />);
    const sheet = await openInbox();
    await within(sheet).findByText("Pick for sess-a");
    const panel = within(sheet).getByText("Pick for sess-a").closest("[data-testid='attention-prompt']") as HTMLElement;
    await act(async () => {
      fireEvent.click(within(panel).getAllByTestId("attention-prompt-option")[1]);
    });
    expect(within(panel).getByRole("button", { name: "Confirm" })).toBeInTheDocument();
    expect(calls.some((call) => call.url.includes("/answer"))).toBe(false);

    await act(async () => {
      fireEvent.click(within(panel).getByRole("button", { name: "Confirm" }));
    });
    const answer = calls.find((call) => call.method === "POST" && call.url.includes("/reference-chat/sess-a/answer"));
    expect(answer?.body).toMatchObject({ params: { promptId: "p-sess-a", answer: { optionIndex: 1 } } });
  });

  it("re-reads every row when the events socket reopens, dropping one answered while it was down", async () => {
    const prompts = { "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") };
    installFetch(prompts);
    render(<RemoteApp />);
    const sheet = await openInbox();
    await within(sheet).findByText("Pick for sess-b");
    const events = () => StubWebSocket.instances.filter((socket) => socket.url.includes("/api/v1/events"));
    const opened = events().length;
    expect(opened).toBeGreaterThan(0);

    // Answered on the desktop while the socket was down: no push ever reaches the phone.
    prompts["sess-b"] = { prompt: null, screenRevision: "rev-after", cols: 80, rows: 24 };
    await act(async () => {
      for (const socket of events()) socket.onclose?.();
    });
    await act(async () => {
      window.dispatchEvent(new Event("online"));
    });
    await waitFor(() => expect(events().length).toBeGreaterThan(opened));

    await waitFor(() => expect(within(sheet).queryByText("Pick for sess-b")).toBeNull());
    expect(within(sheet).getAllByTestId("attention-row")).toHaveLength(1);
  });

  it("opens a row on the phone only, never moving the desktop's focus", async () => {
    const calls = installFetch({ "sess-a": promptOf("sess-a"), "sess-b": promptOf("sess-b") });
    render(<RemoteApp />);
    const sheet = await openInbox();
    await within(sheet).findByText("Pick for sess-a");
    const row = within(sheet).getAllByTestId("attention-row").find((candidate) => candidate.textContent?.includes("alpha"));
    await act(async () => {
      fireEvent.click(row as HTMLElement);
    });

    await waitFor(() => expect(within(screen.getByTestId("mobile-top-bar")).getByText("alpha")).toBeInTheDocument());
    expect(calls.some((call) => call.url.includes("/workspace/select"))).toBe(false);
  });
});
