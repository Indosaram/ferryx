import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
  }: {
    sessionId: string;
  }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      Mirrored terminal {sessionId}
    </div>
  ),
}));

function jsonResponse(body: unknown, ok = true, status: number | null = null): Response {
  return {
    ok,
    status: status ?? (ok ? 200 : 500),
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

const remoteState = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-main",
    activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
    terminalTabs: [
      { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
    ],
  },
};

const agentHistoryPage = {
  sessionId: "sess-main",
  items: [
    { ordinal: 0, role: "user", text: "what changed in the parser?", id: "u1" },
    { ordinal: 1, role: "assistant", text: "The parser now streams line by line.", id: "a1" },
  ],
  nextCursor: null,
  partial: false,
  warnings: [],
};

function routedFetch(historyResponse: Response) {
  return vi.fn<typeof fetch>(async (input) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/agent-history/")) return historyResponse;
    return jsonResponse(remoteState);
  });
}

async function selectPaneFromWorktreeSheet(name: RegExp) {
  const existing = screen.queryByRole("tablist", { name: /terminal tabs/i });
  let sheet = existing;
  if (!sheet) {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    });
    sheet = screen.getByRole("tablist", { name: /terminal tabs/i });
  }
  fireEvent.click(within(sheet).getByRole("tab", { name }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: /Close worktree list/i }));
  });
}

describe("remoteAppChatFrames", () => {
  // RemoteApp loads the chat workspace lazily. Its first cold import can outlast findBy's
  // window, so resolve the chunk up front instead of racing the transform.
  beforeAll(async () => {
    await import("../remote/chat/MobileChatWorkspace");
  });

  let originalInnerWidth: number;

  beforeEach(() => {
    originalInnerWidth = window.innerWidth;
    StubWebSocket.instances = [];
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", StubWebSocket);
  });

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", { value: originalInnerWidth, configurable: true, writable: true });
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("renders the conversation from the agent-history API, one bubble per turn", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const request = routedFetch(jsonResponse(agentHistoryPage));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    const userBubble = await screen.findByTestId("user-message-bubble");
    expect(userBubble.textContent).toContain("what changed in the parser?");

    const assistantBody = await screen.findByTestId("assistant-message-body");
    expect(assistantBody.textContent).toContain("The parser now streams line by line.");

    const historyCalls = request.mock.calls.filter(([input]) =>
      String(input instanceof Request ? input.url : input).includes("/api/v1/agent-history/"),
    );
    expect(historyCalls.length).toBeGreaterThan(0);
  });

  it("terminal output never becomes a chat message", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const request = routedFetch(jsonResponse(agentHistoryPage));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await screen.findByTestId("user-message-bubble");

    await waitFor(() => {
      const socket = StubWebSocket.instances.find((s) => s.url.includes("/api/v1/terminal/"));
      expect(socket).toBeDefined();
      expect(socket?.binaryType).toBe("arraybuffer");
    });

    const socket = StubWebSocket.instances.find((s) => s.url.includes("/api/v1/terminal/"))!;

    const bytes = new TextEncoder().encode(
      "\x1b]777;ferryx;" +
        JSON.stringify({ kind: "output", sequence: "1" }) +
        "\x07" +
        "PTY_NOISE_MARKER",
    );
    const blob = new Blob([bytes]);

    await act(async () => {
      socket.onmessage?.({ data: blob } as MessageEvent);
    });

    expect(document.body.textContent).not.toContain("PTY_NOISE_MARKER");
    expect((await screen.findByTestId("assistant-message-body")).textContent).toContain(
      "The parser now streams line by line.",
    );
  });

  it("a missing transcript renders a quiet empty state, not an error", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    vi.stubGlobal(
      "fetch",
      ticketed(routedFetch(jsonResponse({ error: "TRANSCRIPT_NOT_FOUND" }, false, 404))),
    );

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    const workspace = await screen.findByTestId("mobile-chat-workspace");
    expect(workspace).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(document.body.textContent).not.toMatch(/error|failed/i);
  });

  it("the terminal socket is still opened for input and the running signal", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    vi.stubGlobal("fetch", ticketed(routedFetch(jsonResponse(agentHistoryPage))));

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await waitFor(() => {
      const socket = StubWebSocket.instances.find((s) => s.url.includes("/api/v1/terminal/"));
      expect(socket).toBeDefined();
      expect(socket?.binaryType).toBe("arraybuffer");
    });
  });

  it("the poll appends only new turns without duplicating or dropping earlier ones", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const first = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "first user turn", id: "u0" },
        { ordinal: 1, role: "assistant", text: "first assistant turn", id: "a1" },
      ],
      nextCursor: 0,
      partial: true,
      warnings: [],
    };
    const extended = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "first user turn", id: "u0" },
        { ordinal: 1, role: "assistant", text: "first assistant turn", id: "a1" },
        { ordinal: 2, role: "user", text: "second user turn", id: "u2" },
        { ordinal: 3, role: "assistant", text: "second assistant turn", id: "a3" },
      ],
      nextCursor: 0,
      partial: true,
      warnings: [],
    };
    let historyCalls = 0;
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) {
        historyCalls += 1;
        return jsonResponse(historyCalls === 1 ? first : extended);
      }
      return jsonResponse(remoteState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});

      expect(historyCalls).toBe(1);
      const initialBubbles = screen.getAllByTestId("user-message-bubble");
      expect(initialBubbles).toHaveLength(1);
      expect(initialBubbles[0].textContent).toContain("first user turn");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(historyCalls).toBe(2);
      const userBubbles = screen.getAllByTestId("user-message-bubble");
      expect(userBubbles).toHaveLength(2);
      expect(userBubbles[0].textContent).toContain("first user turn");
      expect(userBubbles[1].textContent).toContain("second user turn");

      const assistantBodies = screen.getAllByTestId("assistant-message-body");
      expect(assistantBodies).toHaveLength(2);
      expect(assistantBodies[0].textContent).toContain("first assistant turn");
      expect(assistantBodies[1].textContent).toContain("second assistant turn");

      expect(screen.getAllByText("first user turn")).toHaveLength(1);
      expect(screen.getAllByText("first assistant turn")).toHaveLength(1);
      expect(screen.getAllByText("second user turn")).toHaveLength(1);
      expect(screen.getAllByText("second assistant turn")).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a response carrying partial: true and a non-null nextCursor must NOT cause a backwards request", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const paged = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "oldest turn", id: "u0" },
        { ordinal: 1, role: "assistant", text: "newest turn", id: "a1" },
      ],
      nextCursor: 0,
      partial: true,
      warnings: [],
    };
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) return jsonResponse(paged);
      return jsonResponse(remoteState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
        await vi.advanceTimersByTimeAsync(3000);
      });

      const historyRequests = request.mock.calls
        .map(([input]) => String(input instanceof Request ? input.url : input))
        .filter((url) => url.includes("/api/v1/agent-history/"));
      expect(historyRequests.length).toBeGreaterThanOrEqual(3);
      for (const url of historyRequests) {
        expect(url).not.toContain("cursor=");
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it("switching sessions does not retain the previous conversation", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sessionAItems = [
      { ordinal: 0, role: "user", text: "A0", id: "a0" },
      { ordinal: 1, role: "assistant", text: "A1", id: "a1" },
      { ordinal: 2, role: "user", text: "A2", id: "a2" },
      { ordinal: 3, role: "assistant", text: "A3", id: "a3" },
    ];
    const sessionBItems = [
      { ordinal: 0, role: "user", text: "B0", id: "b0" },
      { ordinal: 1, role: "assistant", text: "B1", id: "b1" },
    ];
    const twoTabState = {
      activeContext: {
        workspaceId: "ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-main",
        activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
        terminalTabs: [
          { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
          { id: "tab-second", sessionId: "sess-second", label: "second", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
        ],
      },
    };
    let historyCalls = 0;
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) {
        historyCalls += 1;
        if (url.includes("sess-second")) {
          return jsonResponse({ sessionId: "sess-second", items: sessionBItems, nextCursor: null, partial: false, warnings: [] });
        }
        return jsonResponse({ sessionId: "sess-main", items: sessionAItems, nextCursor: null, partial: false, warnings: [] });
      }
      return jsonResponse(twoTabState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});

      expect(historyCalls).toBe(1);
      const initialText = document.body.textContent;
      expect(initialText).toContain("A0");
      expect(initialText).toContain("A1");
      expect(initialText).toContain("A2");
      expect(initialText).toContain("A3");
      expect(screen.getAllByTestId("user-message-bubble")).toHaveLength(2);
      expect(screen.getAllByTestId("assistant-message-body")).toHaveLength(2);

      await selectPaneFromWorktreeSheet(/second/i);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(historyCalls).toBeGreaterThanOrEqual(3);
      const switchedText = document.body.textContent;
      expect(switchedText).toContain("B0");
      expect(switchedText).toContain("B1");
      expect(switchedText).not.toContain("A0");
      expect(switchedText).not.toContain("A1");
      expect(switchedText).not.toContain("A2");
      expect(switchedText).not.toContain("A3");
      expect(screen.getAllByTestId("user-message-bubble")).toHaveLength(1);
      expect(screen.getAllByTestId("user-message-bubble")[0].textContent).toContain("B0");
      expect(screen.getAllByTestId("assistant-message-body")).toHaveLength(1);
      expect(screen.getAllByTestId("assistant-message-body")[0].textContent).toContain("B1");
    } finally {
      vi.useRealTimers();
    }
  });

  it("an optimistic prompt from the previous session does not leak into the new one", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const twoTabState = {
      activeContext: {
        workspaceId: "ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-main",
        activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
        terminalTabs: [
          { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
          { id: "tab-second", sessionId: "sess-second", label: "second", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
        ],
      },
    };
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) {
        if (url.includes("sess-second")) {
          return jsonResponse({ sessionId: "sess-second", items: [], nextCursor: null, partial: false, warnings: [] });
        }
        return jsonResponse({
          sessionId: "sess-main",
          items: [
            { ordinal: 0, role: "user", text: "A0", id: "a0" },
            { ordinal: 1, role: "assistant", text: "A1", id: "a1" },
          ],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      return jsonResponse(twoTabState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});
      expect(screen.getAllByTestId("user-message-bubble")).toHaveLength(1);

      fireEvent.change(screen.getByTestId("chat-composer-textarea"), {
        target: { value: "LEAK_PROBE_PROMPT" },
      });
      await act(async () => {});
      fireEvent.click(screen.getByTestId("send-button"));
      await act(async () => {});
      expect(document.body.textContent).toContain("LEAK_PROBE_PROMPT");
      expect(screen.queryByTestId("stop-button")).not.toBeNull();

      await selectPaneFromWorktreeSheet(/second/i);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(document.body.textContent).not.toContain("LEAK_PROBE_PROMPT");
      expect(screen.queryAllByTestId("user-message-bubble")).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
  it("an optimistic prompt sent after switching sessions survives the new session's polls", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const twoTabState = {
      activeContext: {
        workspaceId: "ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-main",
        activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
        terminalTabs: [
          { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
          { id: "tab-second", sessionId: "sess-second", label: "second", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
        ],
      },
    };
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) {
        if (url.includes("sess-second")) {
          return jsonResponse({ sessionId: "sess-second", items: [], nextCursor: null, partial: false, warnings: [] });
        }
        return jsonResponse({
          sessionId: "sess-main",
          items: [
            { ordinal: 0, role: "user", text: "A0", id: "a0" },
            { ordinal: 1, role: "assistant", text: "A1", id: "a1" },
          ],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      return jsonResponse(twoTabState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});

      await selectPaneFromWorktreeSheet(/second/i);
      expect(screen.queryAllByTestId("user-message-bubble")).toHaveLength(0);

      fireEvent.change(screen.getByTestId("chat-composer-textarea"), {
        target: { value: "SURVIVES_PROBE_PROMPT" },
      });
      await act(async () => {});
      fireEvent.click(screen.getByTestId("send-button"));
      await act(async () => {});

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      const bubbles = screen.getAllByTestId("user-message-bubble");
      expect(bubbles).toHaveLength(1);
      expect(bubbles[0].textContent).toContain("SURVIVES_PROBE_PROMPT");
    } finally {
      vi.useRealTimers();
    }
  });
  it("switching sessions does not carry the running state into the new session", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const twoTabState = {
      activeContext: {
        workspaceId: "ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-main",
        activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
        terminalTabs: [
          { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
          { id: "tab-second", sessionId: "sess-second", label: "second", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
        ],
      },
    };
    const request = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/agent-history/")) {
        if (url.includes("sess-second")) {
          return jsonResponse({ sessionId: "sess-second", items: [], nextCursor: null, partial: false, warnings: [] });
        }
        return jsonResponse({
          sessionId: "sess-main",
          items: [
            { ordinal: 0, role: "user", text: "A0", id: "a0" },
            { ordinal: 1, role: "assistant", text: "A1", id: "a1" },
          ],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      return jsonResponse(twoTabState);
    });
    vi.stubGlobal("fetch", ticketed(request));

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      fireEvent.click(screen.getByTestId("remote-view-mode-chat"));
      await act(async () => {});

      fireEvent.change(screen.getByTestId("chat-composer-textarea"), {
        target: { value: "RUNNING_PROBE_PROMPT" },
      });
      await act(async () => {});
      fireEvent.click(screen.getByTestId("send-button"));
      await act(async () => {});
      expect(screen.queryByTestId("stop-button")).not.toBeNull();

      await selectPaneFromWorktreeSheet(/second/i);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(screen.queryByTestId("stop-button")).toBeNull();
      expect(screen.queryByTestId("send-button")).not.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("distinguishes composition: toolResult records belong in worked-for disclosure rather than assistant message bodies", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const syntheticTranscript = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "what is the diff?", id: "u0" },
        { ordinal: 1, role: "toolResult", text: "git diff --stat: src/main.rs +10 -2", id: "t1" },
        { ordinal: 2, role: "toolResult", text: "cargo check: 0 errors", id: "t2" },
        { ordinal: 3, role: "toolResult", text: "codesign -dvv dump", id: "t3" },
        { ordinal: 4, role: "toolResult", text: "web search: results 1..5", id: "t4" },
        { ordinal: 5, role: "toolResult", text: "plist comment xml", id: "t5" },
        { ordinal: 6, role: "assistant", text: "Here is the summary of changes.", id: "a6" },
        { ordinal: 7, role: "assistant", text: "Everything builds cleanly.", id: "a7" },
      ],
      nextCursor: null,
      partial: false,
      warnings: [],
    };
    const request = routedFetch(jsonResponse(syntheticTranscript));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    // 1. User message bubble
    const userBubbles = await screen.findAllByTestId("user-message-bubble");
    expect(userBubbles).toHaveLength(1);
    expect(userBubbles[0].textContent).toContain("what is the diff?");

    // 2. Assistant message bodies count equals 1 (the final assistant prose record), earlier prose moves inside the fold
    const assistantBodies = await screen.findAllByTestId("assistant-message-body");
    expect(assistantBodies).toHaveLength(1);
    expect(assistantBodies[0].textContent).toContain("Everything builds cleanly.");
    expect(assistantBodies[0].textContent).not.toContain("Here is the summary of changes.");

    // 3. Composition check: none of the 5 toolResult strings appear inside assistant message bodies
    const toolTexts = [
      "git diff --stat: src/main.rs +10 -2",
      "cargo check: 0 errors",
      "codesign -dvv dump",
      "web search: results 1..5",
      "plist comment xml",
    ];
    for (const body of assistantBodies) {
      for (const toolText of toolTexts) {
        expect(body.textContent).not.toContain(toolText);
      }
    }

    // 4. worked-for-toggle exists on the turn
    const toggle = await screen.findByTestId("worked-for-toggle");
    expect(toggle).toBeInTheDocument();
    expect(toggle.textContent).toMatch(/Worked for/);

    // 5. Tool text and folded earlier prose are NOT visible in the DOM before expanding the disclosure
    for (const toolText of toolTexts) {
      expect(screen.queryByText(toolText)).not.toBeInTheDocument();
    }
    expect(screen.queryByText("Here is the summary of changes.")).not.toBeInTheDocument();

    // 6. Tool text and folded earlier prose ARE retrievable after expanding the disclosure
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(await screen.findByText("Here is the summary of changes.")).toBeInTheDocument();
    const workRows = await screen.findAllByTestId("work-row");
    for (const row of workRows) {
      if (row.tagName === "BUTTON" && row.getAttribute("aria-expanded") === "false") {
        fireEvent.click(row);
      }
    }
    for (const toolText of toolTexts) {
      expect(await screen.findByText(toolText)).toBeInTheDocument();
    }
  });

  it("drops assistant records with empty text so they do not render empty bubbles", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const emptyProseTranscript = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "run checks", id: "u0" },
        { ordinal: 1, role: "assistant", text: "", id: "a1" },
        { ordinal: 2, role: "assistant", text: "   ", id: "a2" },
        { ordinal: 3, role: "toolResult", text: "checks passed", id: "t3" },
        { ordinal: 4, role: "assistant", text: "Finished successfully.", id: "a4" },
      ],
      nextCursor: null,
      partial: false,
      warnings: [],
    };
    const request = routedFetch(jsonResponse(emptyProseTranscript));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));

    // Exactly 1 prose body ("Finished successfully."), empty assistant records (a1, a2) did not produce empty bubbles
    const assistantBodies = await screen.findAllByTestId("assistant-message-body");
    expect(assistantBodies).toHaveLength(1);
    expect(assistantBodies[0].textContent).toContain("Finished successfully.");
    expect(screen.getByTestId("worked-for-toggle")).toBeInTheDocument();
  });

  it("derives worked-for duration from record timestamps across toolResults and assistant turn", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const timedTranscript = {
      sessionId: "sess-main",
      items: [
        { ordinal: 0, role: "user", text: "please run tests", id: "u0" },
        {
          ordinal: 1,
          role: "toolResult",
          text: "test results: 12 passed",
          id: "t1",
          timestamp: "2026-09-25T02:00:00.000Z",
        },
        {
          ordinal: 2,
          role: "assistant",
          text: "All 12 tests passed successfully.",
          id: "a2",
          timestamp: "2026-09-25T02:01:30.000Z",
        },
      ],
      nextCursor: null,
      partial: false,
      warnings: [],
    };
    const request = routedFetch(jsonResponse(timedTranscript));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));

    const assistantBodies = await screen.findAllByTestId("assistant-message-body");
    expect(assistantBodies).toHaveLength(1);
    expect(assistantBodies[0].textContent).toContain("All 12 tests passed successfully.");

    const toggle = await screen.findByTestId("worked-for-toggle");
    expect(toggle).toBeInTheDocument();
    expect(toggle.textContent).toContain("Worked for 1m 30s");
    expect(toggle.textContent).not.toContain("Worked for 0s");
  });
});