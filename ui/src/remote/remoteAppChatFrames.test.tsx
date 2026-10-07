/**
 * Reference-chat frame regressions (plan task 12).
 *
 * Every route this lane reads is answered explicitly below, including `/api/v1/capabilities` -
 * the one answer that carries the published owner authority the reference-chat target is built
 * from. A route left to the catch-all leaves the lane with no target, and a scenario that then
 * fails describes this fixture rather than the product. This repair lane authored the fixture
 * without executing it: the file's first run is the post-merge batch gate, so nothing here is a
 * test receipt.
 *
 * This suite used to drive the legacy `/api/v1/agent-history` poll and the chat's own raw
 * terminal socket. Both are gone: the chat reads the frozen reference-chat history route and
 * never opens a terminal socket, so each scenario below keeps its INTENT against the route that
 * exists now. The raw socket is exercised by the explicit terminal mode, not by the chat.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    onBack,
  }: {
    sessionId: string;
    onBack?: () => void;
  }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      Mirrored terminal {sessionId}
      <button type="button" data-testid="remote-terminal-back" onClick={onBack}>
        Back to chat
      </button>
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

const DAEMON_EPOCH = "18446744073709551615";

// The owner authority the gateway publishes beside the epoch, on the same incarnation lifetime.
// Deliberately not the workspace id the state below carries ("ferryx"): the reference-chat target
// is built from the PUBLISHED owner, so a fixture that omits it leaves the lane with no target at
// all - which is how this suite failed before the answer in `installFetch` existed.
const REFERENCE_OWNER_ID = "owner-pub-1";

function sessionState(sessionId: string, extraTabs: { id: string; sessionId: string; label: string }[] = []) {
  return {
    activeContext: {
      workspaceId: "ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
      activeTabId: "tab-main",
      activeTerminal: { sessionId, title: "claude", running: true },
      terminalTabs: [
        { id: "tab-main", sessionId, label: "claude", agentType: "claude", activityState: "idle", worktreeLabel: "main" },
        ...extraTabs.map((tab) => ({ ...tab, agentType: "claude", activityState: "idle", worktreeLabel: "main" })),
      ],
    },
  };
}

/** A native reference page: the frozen shape the route answers with. */
function page(turns: unknown[], overrides: Record<string, unknown> = {}) {
  return {
    source: "claude-transcript",
    availability: "native",
    turns,
    cursor: null,
    hasMore: false,
    generation: "gen-1",
    unavailableReason: null,
    ...overrides,
  };
}

const turn = (role: "user" | "assistant", text: string, extra: Record<string, unknown> = {}) => ({
  role,
  parts: [{ kind: "text", text }],
  startedAt: "2026-10-06T00:00:00Z",
  ...extra,
});

function installFetch(handler: (url: string, init?: RequestInit) => Response | undefined) {
  const calls: string[] = [];
  const impl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input instanceof Request ? input.url : input);
    calls.push(url);
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    if (url.includes("/api/v1/capabilities")) {
      // The lane builds its target from this answer: the epoch the route compares a target
      // against, and the owner authority published on the same incarnation. A gateway that omits
      // either leaves the lane with no target, so it is answered here rather than by the
      // catch-all below.
      return jsonResponse({
        apiVersion: 1,
        machineId: "mach-1",
        daemonEpoch: DAEMON_EPOCH,
        referenceOwnerId: REFERENCE_OWNER_ID,
        platform: "linux",
      });
    }
    if (url.includes("/api/v1/sessions")) {
      // The host's inventory names every pane it serves, with the incarnation that serves it:
      // the lane binds a pane's target by that epoch, so an omitted pane cannot be read at all.
      return jsonResponse({
        sessions: [
          { sessionId: "sess-main", daemonEpoch: DAEMON_EPOCH, running: true },
          { sessionId: "sess-second", daemonEpoch: DAEMON_EPOCH, running: true },
        ],
      });
    }
    const handled = handler(url, init);
    if (handled) return handled;
    if (url.includes("/reference-chat/") && url.includes("/prompt")) {
      return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
    }
    return jsonResponse({});
  });
  vi.stubGlobal("fetch", impl as unknown as typeof fetch);
  return { calls };
}

const historyFor = (body: unknown) => (url: string) =>
  url.includes("/reference-chat/") && url.includes("/history") ? jsonResponse(body) : undefined;

function setWidth(width: number): void {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true, writable: true });
}

describe("remoteAppChatFrames", () => {
  let originalInnerWidth: number;

  beforeEach(() => {
    originalInnerWidth = window.innerWidth;
    StubWebSocket.instances = [];
    localStorage.clear();
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", StubWebSocket);
  });

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", { value: originalInnerWidth, configurable: true, writable: true });
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("renders the conversation from the reference history route, one bubble per turn", async () => {
    setWidth(390);
    const harness = installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(page([turn("user", "what changed in the parser?"), turn("assistant", "The parser now streams line by line.")]))(url);
    });
    render(<RemoteApp />);

    expect(await screen.findByTestId("user-message-bubble")).toHaveTextContent("what changed in the parser?");
    // a reference turn's prose is drawn by the reference body, never by the legacy body
    expect(await screen.findByTestId("assistant-reference-body")).toHaveTextContent("The parser now streams line by line.");
    expect(harness.calls.some((url) => url.includes("/api/v1/agent-history/"))).toBe(false);
  });

  it("the chat lane opens no raw terminal socket", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(page([turn("assistant", "steady")]))(url);
    });
    render(<RemoteApp />);
    await screen.findByTestId("assistant-reference-body");

    await waitFor(() => {
      // the workspace event socket is not a terminal socket
      expect(StubWebSocket.instances.some((socket) => socket.url.includes("/api/v1/terminal/"))).toBe(false);
    });
  });

  it("a non-native page is disclosed rather than rendered as native history", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(
        page([], { source: "scrollback", availability: "scrollback", unavailableReason: "no native reader" }),
      )(url);
    });
    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/terminal output/i);
    });
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("an identity refusal clears the transcript and never browses another one", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      if (url.includes("/reference-chat/") && url.includes("/history")) {
        return jsonResponse({ error: { code: "UNAUTHORIZED" } }, false, 401);
      }
      return undefined;
    });
    render(<RemoteApp />);

    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/not available/i);
    });
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("the newest read appends new turns without duplicating or dropping earlier ones", async () => {
    setWidth(390);
    let reads = 0;
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      if (url.includes("/reference-chat/") && url.includes("/history")) {
        reads += 1;
        return jsonResponse(
          reads === 1
            ? page([turn("user", "first user turn"), turn("assistant", "first assistant turn")])
            : page([
                turn("user", "first user turn"),
                turn("assistant", "first assistant turn"),
                turn("user", "second user turn"),
                turn("assistant", "second assistant turn"),
              ]),
        );
      }
      return undefined;
    });

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      await act(async () => {});
      expect(reads).toBe(1);
      expect(screen.getAllByTestId("user-message-bubble")).toHaveLength(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(reads).toBe(2);
      const userBubbles = screen.getAllByTestId("user-message-bubble");
      expect(userBubbles).toHaveLength(2);
      expect(userBubbles[0]).toHaveTextContent("first user turn");
      expect(userBubbles[1]).toHaveTextContent("second user turn");
      expect(screen.getAllByText("first user turn")).toHaveLength(1);
      expect(screen.getAllByText("first assistant turn")).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a paginated page does not make the newest read fetch backwards on its own", async () => {
    setWidth(390);
    const harness = installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      if (url.includes("/reference-chat/") && url.includes("/history")) {
        return jsonResponse(
          page([turn("user", "oldest turn"), turn("assistant", "newest turn")], {
            cursor: { streamId: "dev:ino", offset: 4096 },
            hasMore: true,
          }),
        );
      }
      return undefined;
    });

    vi.useFakeTimers();
    try {
      await act(async () => {
        render(<RemoteApp />);
      });
      await act(async () => {});
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
        await vi.advanceTimersByTimeAsync(3000);
      });

      const paged = harness.calls.filter((url) => url.includes("cursor="));
      expect(paged).toHaveLength(0);
      // the control offers the older page instead of fetching it silently
      expect(screen.getByTestId("reference-older-button")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("switching panes does not retain the previous conversation", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) {
        return jsonResponse(sessionState("sess-main", [{ id: "tab-second", sessionId: "sess-second", label: "second" }]));
      }
      if (url.includes("/reference-chat/") && url.includes("/history")) {
        return jsonResponse(
          url.includes("sess-second")
            ? page([turn("user", "B0"), turn("assistant", "B1")])
            : page([turn("user", "A0"), turn("assistant", "A1")]),
        );
      }
      return undefined;
    });
    render(<RemoteApp />);
    expect(await screen.findByText("A0")).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    });
    const sheet = screen.getByRole("tablist", { name: /terminal tabs/i });
    await act(async () => {
      fireEvent.click(within(sheet).getByRole("tab", { name: /second/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Close worktree list/i }));
    });

    await waitFor(() => {
      expect(screen.getByText("B0")).toBeInTheDocument();
    });
    expect(document.body.textContent).not.toContain("A0");
  });

  it("tool parts render in the work disclosure, not as assistant prose", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(
        page([
          {
            role: "assistant",
            startedAt: "2026-10-06T00:00:00Z",
            parts: [
              { kind: "tool", name: "bash", summary: "git diff --stat", input: "git diff --stat", output: "src/main.rs +10 -2" },
              { kind: "text", text: "Here is the summary of changes." },
            ],
          },
        ]),
      )(url);
    });
    render(<RemoteApp />);

    expect(await screen.findByTestId("assistant-reference-body")).toHaveTextContent("Here is the summary of changes.");
    const toolRow = screen.getByTestId("reference-tool-part");
    expect(toolRow).toHaveTextContent("git diff --stat");
    // the tool output lives inside the row, not in the turn's prose
    expect(screen.getByTestId("assistant-reference-body").textContent).not.toContain("src/main.rs +10 -2");
  });

  it("a turn's skill part is drawn by the turn's skill list, outside the work rows", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(
        page([
          {
            role: "assistant",
            startedAt: "2026-10-06T00:00:00Z",
            parts: [
              { kind: "skill", skill: { name: "superwiki-mail-otp", evidence: "invocation", status: "loaded" } },
              { kind: "text", text: "Read the skill." },
            ],
          },
        ]),
      )(url);
    });
    render(<RemoteApp />);

    const skills = await screen.findByTestId("reference-turn-skills");
    expect(skills).toHaveTextContent("superwiki-mail-otp");
    expect(skills).toHaveTextContent(/Skill invoked/);
    // the chip is not repeated inside the inline part list
    expect(screen.getAllByTestId("reference-skill")).toHaveLength(1);
  });

  it("abandoned branches are disclosed once for the page, never once per turn", async () => {
    setWidth(390);
    installFetch((url) => {
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(sessionState("sess-main"));
      return historyFor(
        page([
          turn("assistant", "kept turn one", { abandoned: { count: 1, branches: 1 } }),
          turn("assistant", "kept turn two", { abandoned: { count: 2, branches: 2 } }),
        ]),
      )(url);
    });
    render(<RemoteApp />);

    await screen.findByText("kept turn two");
    const disclosures = screen.getAllByTestId("reference-abandoned");
    expect(disclosures).toHaveLength(1);
    expect(disclosures[0]).toHaveTextContent(/3 earlier turns on 3 branches/);
  });
});
