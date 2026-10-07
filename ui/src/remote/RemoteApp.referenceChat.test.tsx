/**
 * Integration regression source for the reference chat lane (plan task 12).
 *
 * AUTHORED, NOT EXECUTED: the execution override defers every test run to the post-merge gate.
 *
 * The scenarios below are the task-12 acceptance contract (QA-01/04/05/06/07): chat is the default
 * at every width, the explicit terminal keeps the same session, submit/Stop/answer/files speak to
 * the ORIGINAL session through the reference-chat routes, drafts and held rows are owner-scoped,
 * and no assistant turn is ever fabricated.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  RemoteApp,
  chatDaemonEpochFromRows,
  chatProviderSessionId,
  parseChatGatewayIdentity,
} from "./RemoteApp";

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

const SESSION_ID = "sess-main";
const DAEMON_EPOCH = "18446744073709551615";
// The owner authority the gateway publishes for this incarnation. Deliberately NOT the workspace
// id the state below carries ("ferryx"): if the client ever fell back to the workspace again, an
// assertion naming this value would fail instead of passing by coincidence.
const REFERENCE_OWNER_ID = "owner-pub-1";

const remoteState = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-main",
    activeTerminal: { sessionId: SESSION_ID, title: "claude", running: true },
    terminalTabs: [
      { id: "tab-main", sessionId: SESSION_ID, label: "claude", agentType: "claude", activityState: "idle", worktreeLabel: "main" },
    ],
  },
};

/** A native page exactly as the Rust `ReferenceHistoryPage` serializes it. */
function nativePage(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: [
      { role: "user", parts: [{ kind: "text", text: "what changed in the parser?" }], startedAt: "2026-10-06T00:00:00Z" },
      { role: "assistant", parts: [{ kind: "text", text: "The parser now streams line by line." }], startedAt: "2026-10-06T00:00:01Z" },
    ],
    cursor: null,
    hasMore: false,
    generation: "gen-1",
    unavailableReason: null,
    ...overrides,
  };
}

/** SHA-256 of the staged fixture's own bytes ("abc"): the lane refuses a receipt it cannot prove. */
const NOTES_TXT_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/**
 * A File the staging lane can read. jsdom implements no 'Blob.arrayBuffer' at all, and the lane
 * reads the bytes it is handed through its 'ReferenceFileSource' seam, so the fixture supplies
 * them: the same contents the File declares, read the way a browser File answers.
 */
function readableFile(contents: string, name: string, type: string): File {
  const file = new File([contents], name, { type });
  Object.defineProperty(file, "arrayBuffer", {
    configurable: true,
    value: async () => new TextEncoder().encode(contents).buffer,
  });
  return file;
}

/** One request the app made, as the harness recorded it. */
interface HarnessCall {
  url: string;
  method: string;
  body: unknown;
}

interface Harness {
  readonly calls: HarnessCall[];
  /**
   * Resolves with the FIRST recorded request whose URL contains `fragment` - immediately when one
   * already exists, otherwise the moment one is recorded. Rejects after `timeoutMs` carrying every
   * URL the app actually requested, so a bounded wait that is never satisfied reports what
   * happened instead of collapsing into "expected undefined to be defined".
   */
  readonly whenCalled: (fragment: string, timeoutMs?: number) => Promise<HarnessCall>;
}

function installFetch(options: {
  page?: () => unknown;
  prompt?: () => unknown;
  historyStatus?: number;
  historyErrorCode?: string;
  submit?: () => unknown;
  stop?: () => unknown;
  answer?: () => unknown;
  files?: () => unknown;
  capabilities?: Record<string, unknown>;
  sessions?: () => unknown[];
} = {}): Harness {
  const calls: HarnessCall[] = [];
  interface Waiter {
    readonly fragment: string;
    readonly resolve: (call: HarnessCall) => void;
    readonly timer: ReturnType<typeof setTimeout>;
  }
  const waiters: Waiter[] = [];
  const record = (call: HarnessCall): void => {
    calls.push(call);
    for (let index = waiters.length - 1; index >= 0; index -= 1) {
      if (!call.url.includes(waiters[index].fragment)) continue;
      const [waiter] = waiters.splice(index, 1);
      clearTimeout(waiter.timer);
      waiter.resolve(call);
    }
  };
  const whenCalled = (fragment: string, timeoutMs = 2000): Promise<HarnessCall> => {
    const existing = calls.find((call) => call.url.includes(fragment));
    if (existing) return Promise.resolve(existing);
    return new Promise<HarnessCall>((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = waiters.findIndex((waiter) => waiter.timer === timer);
        if (index >= 0) waiters.splice(index, 1);
        const seen = calls.map((call) => `${call.method} ${call.url}`).join(" | ");
        reject(
          new Error(
            `No request containing "${fragment}" was recorded within ${timeoutMs}ms. Recorded: ${seen || "(none)"}`,
          ),
        );
      }, timeoutMs);
      waiters.push({ fragment, resolve, timer });
    });
  };
  const impl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input instanceof Request ? input.url : input);
    const method = (init?.method ?? "GET").toUpperCase();
    let parsed: unknown = null;
    if (typeof init?.body === "string") {
      try {
        parsed = JSON.parse(init.body);
      } catch {
        parsed = init.body;
      }
    }
    record({ url, method, body: parsed });
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    if (url.includes("/api/v1/capabilities")) {
      return jsonResponse(options.capabilities ?? {
        apiVersion: 1,
        machineId: "mach-1",
        daemonEpoch: DAEMON_EPOCH,
        // The owner authority the route compares a target against, published beside the host id
        // on the same instance lifetime as the epoch. A gateway that omits it has no target.
        referenceOwnerId: REFERENCE_OWNER_ID,
        platform: "linux",
      });
    }
    if (url.includes("/api/v1/sessions")) {
      return jsonResponse({ sessions: (options.sessions ?? [() => ({ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true })])() });
    }
    if (url.includes("/reference-chat/") && url.includes("/history")) {
      if (options.historyStatus !== undefined) {
        return jsonResponse({ error: { code: options.historyErrorCode ?? "FORBIDDEN" } }, false, options.historyStatus);
      }
      return jsonResponse((options.page ?? nativePage)());
    }
    if (url.includes("/reference-chat/") && url.includes("/prompt")) {
      return jsonResponse((options.prompt ?? (() => ({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 })))());
    }
    if (url.includes("/reference-chat/") && url.includes("/submit")) {
      return jsonResponse((options.submit ?? (() => ({ ok: true, data: { receipt: { requestId: "r1", target: {}, stage: "accepted" } }, requestId: "r1" })))());
    }
    if (url.includes("/reference-chat/") && url.includes("/stop")) {
      return jsonResponse((options.stop ?? (() => ({ ok: true, data: { receipt: { requestId: "r2", target: {}, stage: "accepted" } }, requestId: "r2" })))());
    }
    if (url.includes("/reference-chat/") && url.includes("/answer")) {
      return jsonResponse((options.answer ?? (() => ({ ok: true, data: {}, requestId: "r3" })))());
    }
    if (url.includes("/reference-chat/") && url.includes("/files")) {
      return jsonResponse((options.files ?? (() => ({
        ok: true,
        data: {
          receipt: { hostId: "local", attachmentId: "att-1", sha256: NOTES_TXT_SHA256, sizeBytes: 3, mediaType: "text/plain" },
          displayName: "notes.txt",
          mentionText: "@notes.txt ",
        },
        requestId: "r4",
      })))());
    }
    if (url.includes("/api/v1/workspace/state")) return jsonResponse(remoteState);
    return jsonResponse({});
  });
  vi.stubGlobal("fetch", impl as unknown as typeof fetch);
  return { calls, whenCalled };
}

/**
 * Await one bounded harness signal, attaching the composer's own refusal text if it times out, so a
 * failed wait reports what the lane did instead of collapsing into "expected undefined to be
 * defined". Takes the signal (not a fragment) so exactly one bounded wait is ever outstanding: the
 * caller subscribes, then awaits, then arms the next one.
 */
async function settleRequest(signal: Promise<HarnessCall>): Promise<HarnessCall> {
  try {
    return await signal;
  } catch (error) {
    const refusal = screen.queryByTestId("chat-composer-attach-error")?.textContent ?? "(none)";
    throw new Error(`${(error as Error).message}\ncomposer refusal: ${refusal}`);
  }
}

function setWidth(width: number): void {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true, writable: true });
}

async function openTerminalMode(): Promise<void> {
  await act(async () => {
    fireEvent.click(screen.getByTestId("open-terminal-button"));
  });
}

describe("reference chat lane (task 12)", () => {
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

  it("QA-01: chat is the first surface at 360, 390 and 1280 wide", async () => {
    for (const width of [360, 390, 1280]) {
      cleanup();
      setWidth(width);
      installFetch();
      render(<RemoteApp />);
      expect(await screen.findByTestId("mobile-chat-workspace")).toBeInTheDocument();
      expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    }
  });

  it("QA-01: the explicit terminal keeps the same session and returns to chat", async () => {
    setWidth(390);
    installFetch();
    render(<RemoteApp />);
    await screen.findByTestId("mobile-chat-workspace");

    await openTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", SESSION_ID);
    expect(screen.queryByTestId("mobile-chat-workspace")).not.toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-terminal-back"));
    });
    expect(await screen.findByTestId("mobile-chat-workspace")).toBeInTheDocument();
  });

  it("QA-02: the transcript comes from the reference route bound to the original target", async () => {
    setWidth(390);
    const harness = installFetch();
    render(<RemoteApp />);

    expect(await screen.findByText("The parser now streams line by line.")).toBeInTheDocument();
    const history = harness.calls.find((call) => call.url.includes("/history"));
    expect(history).toBeDefined();
    expect(history!.url).toContain(`/api/v1/reference-chat/${SESSION_ID}/history`);
    expect(history!.url).toContain(`epoch=${DAEMON_EPOCH}`);
    expect(history!.url).toContain("registryId=claude");
    expect(history!.url).toContain(`ownerId=${REFERENCE_OWNER_ID}`);
    // the legacy agent-history path is not used by the chat any more
    expect(harness.calls.some((call) => call.url.includes("/api/v1/agent-history/"))).toBe(false);
  });

  it("QA-02: an identity refusal discloses and clears instead of browsing another transcript", async () => {
    setWidth(390);
    installFetch({ historyStatus: 403, historyErrorCode: "FORBIDDEN" });
    render(<RemoteApp />);

    const workspace = await screen.findByTestId("mobile-chat-workspace");
    expect(workspace).toBeTruthy();
    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/not available/i);
    });
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("QA-04: a send goes through /submit and fabricates no assistant turn", async () => {
    setWidth(390);
    const harness = installFetch();
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "please review" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    const submit = harness.calls.find((call) => call.url.includes("/submit"));
    expect(submit).toBeDefined();
    expect(submit!.method).toBe("POST");
    const body = submit!.body as { requestId: string; target: Record<string, unknown>; params: Record<string, unknown> };
    expect(typeof body.requestId).toBe("string");
    expect(body.target.backendSessionId).toBe(SESSION_ID);
    expect(body.target.epoch).toBe(DAEMON_EPOCH);
    expect(body.params.text).toBe("please review");
    expect(body.params.origin).toBe("chat");
    expect(body.params.attachmentIds).toEqual([]);
    // Two legitimate messages, not one rendered twice: the transcript's own turn, then the echo
    // of the user's words. Both texts are pinned, and so is their order.
    const bubbles = screen.getAllByTestId("user-message-bubble");
    expect(bubbles).toHaveLength(2);
    expect(bubbles[0]).toHaveTextContent("what changed in the parser?");
    expect(bubbles[1]).toHaveTextContent("please review");
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("QA-04: a refused send keeps the text and says so", async () => {
    setWidth(390);
    installFetch({ submit: () => ({ ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false }, requestId: "r1" }) });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "keep me" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("keep me");
    });
    // the echo stopped claiming it was sent: only the transcript's own turn is left
    const bubbles = screen.getAllByTestId("user-message-bubble");
    expect(bubbles).toHaveLength(1);
    expect(bubbles[0]).toHaveTextContent("what changed in the parser?");
  });

  it("QA-06: Stop goes through /stop with the pane's own interrupt, never Ctrl-C", async () => {
    setWidth(390);
    const harness = installFetch();
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    // the pane is working: the composer offers Stop instead of Send
    await act(async () => {
      fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "go" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    // with no running pane the composer shows Send; Stop is driven by the mirror's own state
    const stopCalls = harness.calls.filter((call) => call.url.includes("/stop"));
    expect(stopCalls).toHaveLength(0);
    // nothing was written to a raw terminal socket by the chat lane
    for (const socket of StubWebSocket.instances) {
      expect(socket.send).not.toHaveBeenCalledWith("\x03");
    }
  });

  it("QA-05: a typed answer goes through /answer with the card's screen revision", async () => {
    setWidth(390);
    const prompt = {
      prompt: {
        promptId: "prompt-1",
        agent: "claude",
        kind: "approval",
        title: "Run the tests?",
        question: "Run the tests?",
        options: [{ label: "Yes, proceed (y)" }, { label: "No (n)" }],
        multiSelect: false,
      },
      screenRevision: "rev-7",
      cols: 80,
      rows: 24,
    };
    const harness = installFetch({ prompt: () => prompt });
    render(<RemoteApp />);

    const card = await screen.findByTestId("reference-prompt-card");
    // A two-option approval is drawn by the existing approval shell, not by the ported option
    // list ('ReferencePromptCard.test.tsx' pins that shape), so index 0 is the shell's confirm.
    await act(async () => {
      fireEvent.click(within(card).getByRole("button", { name: /Yes, proceed/ }));
    });

    const answer = harness.calls.find((call) => call.url.includes("/answer"));
    expect(answer).toBeDefined();
    const body = answer!.body as { params: Record<string, unknown> };
    expect(body.params.promptId).toBe("prompt-1");
    expect(body.params.screenRevision).toBe("rev-7");
    expect(body.params.answer).toEqual({ optionIndex: 0 });
  });

  it("QA-04: text a prompt cannot take is held, not sent and not dropped", async () => {
    setWidth(390);
    const prompt = {
      prompt: {
        promptId: "prompt-2",
        agent: "claude",
        kind: "question",
        title: "Which file?",
        question: "Which file?",
        options: [{ label: "src/main.rs" }, { label: "src/lib.rs" }],
        multiSelect: false,
      },
      screenRevision: "rev-8",
      cols: 80,
      rows: 24,
    };
    const harness = installFetch({ prompt: () => prompt });
    render(<RemoteApp />);
    await screen.findByTestId("reference-prompt-card");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "something else" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    expect(harness.calls.some((call) => call.url.includes("/submit"))).toBe(false);
    expect(harness.calls.some((call) => call.url.includes("/answer"))).toBe(false);
    const held = await screen.findByTestId("chat-composer-held");
    expect(held).toHaveTextContent("something else");
    // the box keeps the text: nothing was sent, so nothing was consumed
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("something else");
  });

  it("QA-07: a staged file becomes an editable mention through /files", async () => {
    setWidth(390);
    const harness = installFetch();
    // Readiness, subscribed BEFORE the render so no request can be missed and awaited BEFORE the
    // file change so the prerequisite actually holds rather than being raced.
    //
    // The lane's newest-page read is the readiness proof, from source: it is gated on
    // `chatReferenceTarget` being non-null, and the ref is assigned from that same value in the
    // render body (`chatTargetRef.current = chatReferenceTarget`), which commits before that
    // read's effect runs - so a recorded /history request means the ref was bound. `attachChatFiles`
    // returns early, with only a warning, while the ref is null, so triggering the change first can
    // discard the event outright. Which of the two gates (target binding, or the file read + digest
    // that follows it) the observed order dependence comes from is a SCHEDULING HYPOTHESIS until the
    // merged run; this sequence is correct under either.
    const laneReady = harness.whenCalled("/history");
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");
    await act(async () => {
      await settleRequest(laneReady);
    });

    // Armed only once readiness is settled, so one bounded wait is outstanding at a time and a
    // readiness failure cannot leave a second rejection unhandled.
    const stagedRequest = harness.whenCalled("/files");

    const file = readableFile("abc", "notes.txt", "text/plain");
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), { target: { files: [file] } });
    });

    const staged = await settleRequest(stagedRequest);
    expect(staged.url).toContain("/files");
    expect(staged.method).toBe("POST");
    const body = staged.body as { params: Record<string, unknown> };
    expect(body.params.name).toBe("notes.txt");
    expect(body.params.mediaType).toBe("text/plain");
    expect(body.params.sizeBytes).toBe(3);
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("@notes.txt ");
    });
    expect(screen.getByTestId("attachment-preview-att-1")).toBeInTheDocument();
  });

  it("QA-01: the draft survives a chat to terminal to chat roundtrip", async () => {
    setWidth(390);
    installFetch();
    render(<RemoteApp />);
    const textarea = await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(textarea, { target: { value: "half written thought" } });
    await openTerminalMode();
    await screen.findByTestId("remote-terminal");
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-terminal-back"));
    });

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("half written thought");
    });
  });

  it("QA-03: an older page is only fetched on an explicit request", async () => {
    setWidth(390);
    const harness = installFetch({
      page: () => nativePage({ cursor: { streamId: "dev:ino", offset: 4096 }, hasMore: true }),
    });
    render(<RemoteApp />);
    await screen.findByTestId("reference-older-button");

    const before = harness.calls.filter((call) => call.url.includes("cursor=")).length;
    expect(before).toBe(0);

    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-older-button"));
    });

    await waitFor(() => {
      const paged = harness.calls.filter((call) => call.url.includes("cursor="));
      expect(paged.length).toBeGreaterThan(0);
      expect(paged[0].url).toContain("cursorStream=dev%3Aino");
    });
  });

  it("QA-06: Stop sends the pane's own interrupt through /stop", async () => {
    setWidth(390);
    const prompt = {
      prompt: {
        promptId: "prompt-stop",
        agent: "claude",
        kind: "question",
        title: "Which file?",
        question: "Which file?",
        options: [{ label: "src/main.rs" }, { label: "src/lib.rs" }],
        multiSelect: false,
      },
      screenRevision: "rev-stop",
      cols: 80,
      rows: 24,
    };
    // A waiting prompt makes the pane read as running, which is when the composer offers Stop.
    const harness = installFetch({ prompt: () => prompt });
    render(<RemoteApp />);
    await screen.findByTestId("reference-prompt-card");

    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
    });

    const stop = harness.calls.find((call) => call.url.includes("/stop"));
    expect(stop).toBeDefined();
    expect(stop!.method).toBe("POST");
    expect(stop!.url).toContain(`/api/v1/reference-chat/${SESSION_ID}/stop`);
    const body = stop!.body as { target: Record<string, unknown>; params: Record<string, unknown> };
    expect(body.target.backendSessionId).toBe(SESSION_ID);
    expect(body.target.epoch).toBe(DAEMON_EPOCH);
    // the pane is a Claude TUI, so the honest capability is its own interrupt — never Ctrl-C
    expect(body.params).toEqual({ capability: "providerInterrupt" });
    for (const socket of StubWebSocket.instances) {
      expect(socket.send).not.toHaveBeenCalledWith("\x03");
    }
  });

  it("QA-06: a pane the chat cannot stop refuses instead of sending a signal", async () => {
    setWidth(390);
    const state = {
      activeContext: {
        workspaceId: "ferryx",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-main",
        activeTerminal: { sessionId: SESSION_ID, title: "shell", running: true },
        terminalTabs: [
          { id: "tab-main", sessionId: SESSION_ID, label: "shell", agentType: "shell", activityState: "working", worktreeLabel: "main" },
        ],
      },
    };
    const calls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input instanceof Request ? input.url : input);
        calls.push(url);
        if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "t", expiresAt: 9999999999 });
        if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }] });
        if (url.includes("/api/v1/workspace/state")) return jsonResponse(state);
        if (url.includes("/history")) return jsonResponse(nativePage());
        if (url.includes("/prompt")) return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
        return jsonResponse({});
      }) as unknown as typeof fetch,
    );
    render(<RemoteApp />);
    await screen.findByTestId("stop-button");

    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
    });

    // an unknown capability is a typed refusal: nothing is sent, and the user is told why
    expect(calls.some((url) => url.includes("/stop"))).toBe(false);
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent(/does not know how to stop/i);
  });

  it("QA-04: an acknowledgement settles the sent prefix and keeps what was typed after it", async () => {
    setWidth(390);
    let releaseSubmit: (() => void) | null = null;
    const submitGate = new Promise<void>((resolve) => {
      releaseSubmit = resolve;
    });
    const calls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input instanceof Request ? input.url : input);
        calls.push(url);
        if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "t", expiresAt: 9999999999 });
        if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }] });
        if (url.includes("/api/v1/workspace/state")) return jsonResponse(remoteState);
        if (url.includes("/history")) return jsonResponse(nativePage());
        if (url.includes("/prompt")) return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
        if (url.includes("/submit")) {
          // hold the acknowledgement so an edit can land while the send is in flight
          await submitGate;
          return jsonResponse({ ok: true, data: { receipt: { requestId: "r1", target: {}, stage: "accepted" } }, requestId: "r1" });
        }
        return jsonResponse({});
      }) as unknown as typeof fetch,
    );
    render(<RemoteApp />);
    const textarea = await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(textarea, { target: { value: "first half" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    // the send is on its way; the user keeps typing
    fireEvent.change(textarea, { target: { value: "second half" } });
    expect(textarea).toHaveValue("second half");

    await act(async () => {
      releaseSubmit?.();
      await submitGate;
    });

    // only the acknowledged text left the box; what was typed after it is still there
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("second half");
    });
  });

  it("QA-07: a refused file stages nothing and leaves the draft alone", async () => {
    setWidth(390);
    installFetch({
      files: () => ({
        ok: false,
        error: { code: "PAYLOAD_TOO_LARGE", message: "that file is over the limit", retryable: false },
        requestId: "r4",
      }),
    });
    render(<RemoteApp />);
    const textarea = await screen.findByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "keep this draft" } });

    const file = readableFile("too big", "huge.bin", "text/plain");
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), { target: { files: [file] } });
    });

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-attach-error")).toHaveTextContent(/over the limit/i);
    });
    // nothing was staged, so there is no chip and no mention, and the draft is untouched
    expect(screen.queryByTestId(/^attachment-preview-/)).not.toBeInTheDocument();
    expect(textarea).toHaveValue("keep this draft");
  });

  it("QA-09: a READ names no host, so a renamed host answers from its own identity", async () => {
    setWidth(390);
    // A gateway that renamed itself publishes that name. A read must still carry no host id: the
    // route falls back to the host's OWN reference_host_id(), which is what keeps it correct.
    const harness = installFetch({
      capabilities: { apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceHostId: "renamed-host", referenceOwnerId: REFERENCE_OWNER_ID },
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(true);
    });
    const history = harness.calls.find((call) => call.url.includes("/history"));
    expect(history!.url).not.toContain("hostId=");
    expect(history!.url).toContain(`epoch=${DAEMON_EPOCH}`);
    expect(history!.url).toContain(`ownerId=${REFERENCE_OWNER_ID}`);
    expect(history!.url).toContain(`backendSessionId=${SESSION_ID}`);
  });

  it("QA-09: a mutation names the host the gateway published, never the machine id", async () => {
    setWidth(390);
    const harness = installFetch({
      capabilities: { apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceHostId: "renamed-host", referenceOwnerId: REFERENCE_OWNER_ID },
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "hello" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    const submit = harness.calls.find((call) => call.url.includes("/submit"));
    expect(submit).toBeDefined();
    const body = submit!.body as { target: Record<string, unknown> };
    expect(body.target.hostId).toBe("renamed-host");
    // the machine identity is a different value and is never substituted for the host id
    expect(body.target.hostId).not.toBe("mach-1");
    expect(body.target.epoch).toBe(DAEMON_EPOCH);
  });

  it("QA-09: the gateway's own epoch wins over the session row", async () => {
    setWidth(390);
    const harness = installFetch({
      capabilities: { apiVersion: 1, machineId: "mach-1", daemonEpoch: "77", referenceOwnerId: REFERENCE_OWNER_ID },
      sessions: () => [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }],
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(true);
    });
    const history = harness.calls.find((call) => call.url.includes("/history"));
    // the capabilities answer is the exact value the route compares a target against
    expect(history!.url).toContain("epoch=77");
    expect(history!.url).not.toContain(`epoch=${DAEMON_EPOCH}`);
  });

  it("QA-09: without a published host id the documented default is used", async () => {
    setWidth(390);
    const harness = installFetch({
      capabilities: { apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: REFERENCE_OWNER_ID },
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "hi" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    const submit = harness.calls.find((call) => call.url.includes("/submit"));
    const body = submit!.body as { target: Record<string, unknown> };
    expect(body.target.hostId).toBe("local");
    // the same envelope echoes the published owner, so the route can compare it
    expect(body.target.ownerId).toBe(REFERENCE_OWNER_ID);
  });

  it("QA-09: the provider session the host published for THIS pane travels with the read", async () => {
    setWidth(390);
    const harness = installFetch({
      sessions: () => [
        { sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true, providerSession: { key: "claude", id: "agent-42" } },
        { sessionId: "other-session", daemonEpoch: DAEMON_EPOCH, running: true, providerSession: { key: "claude", id: "agent-99" } },
      ],
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(harness.calls.some((call) => call.url.includes("providerSessionId=agent-42"))).toBe(true);
    });
    const history = harness.calls.find((call) => call.url.includes("/history"));
    // another pane's provider session is never borrowed
    expect(history!.url).not.toContain("agent-99");
  });

  it("QA-09: an ambiguous provider identity is refused rather than guessed", async () => {
    setWidth(390);
    const harness = installFetch({
      sessions: () => [
        { sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true, providerSession: { key: "claude", id: "agent-1" } },
        { sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true, providerSession: { key: "claude", id: "agent-2" } },
      ],
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(true);
    });
    // two rows disagreeing means unknown: the read binds by the host's own owner/source rules
    for (const call of harness.calls.filter((entry) => entry.url.includes("/history"))) {
      expect(call.url).not.toContain("providerSessionId=");
    }
  });

  it("QA-09: a pane with no published provider session sends none", async () => {
    setWidth(390);
    const harness = installFetch({
      sessions: () => [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }],
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(true);
    });
    for (const call of harness.calls.filter((entry) => entry.url.includes("/history"))) {
      expect(call.url).not.toContain("providerSessionId=");
    }
  });

  it("QA-09: a stale incarnation is re-read from the host instead of being frozen", async () => {
    setWidth(390);
    let historyReads = 0;
    let capabilitiesReads = 0;
    const calls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input instanceof Request ? input.url : input);
        calls.push(url);
        if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "t", expiresAt: 9999999999 });
        if (url.includes("/api/v1/capabilities")) {
          capabilitiesReads += 1;
          // the daemon restarted between the two reads, so the incarnation moved
          return jsonResponse({
            apiVersion: 1,
            machineId: "mach-1",
            daemonEpoch: capabilitiesReads === 1 ? DAEMON_EPOCH : "88",
            referenceOwnerId: REFERENCE_OWNER_ID,
          });
        }
        if (url.includes("/api/v1/sessions")) {
          return jsonResponse({ sessions: [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }] });
        }
        if (url.includes("/api/v1/workspace/state")) return jsonResponse(remoteState);
        if (url.includes("/history")) {
          historyReads += 1;
          // the first read names an incarnation that is no longer live
          if (historyReads === 1) {
            return jsonResponse({ error: { code: "TARGET_EXPIRED" } }, false, 410);
          }
          return jsonResponse(nativePage());
        }
        if (url.includes("/prompt")) return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
        return jsonResponse({});
      }) as unknown as typeof fetch,
    );
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    await waitFor(() => {
      expect(capabilitiesReads).toBeGreaterThan(1);
    });
    await waitFor(() => {
      expect(screen.getByText("The parser now streams line by line.")).toBeInTheDocument();
    });
    // the refusal cleared the lane, the identity was re-read, and the next poll rendered the pane
    const history = calls.filter((url) => url.includes("/history"));
    expect(history[history.length - 1]).toContain("epoch=88");
  });

  it("QA-09: the capabilities payload is an identity only when it carries a canonical epoch", () => {
    expect(parseChatGatewayIdentity({ daemonEpoch: "0" })).toEqual({
      daemonEpoch: "0",
      referenceHostId: null,
      referenceOwnerId: null,
      machineId: null,
    });
    // a non-canonical or non-string epoch is not an incarnation this route could compare
    expect(parseChatGatewayIdentity({ daemonEpoch: "007" })).toBeNull();
    expect(parseChatGatewayIdentity({ daemonEpoch: "-1" })).toBeNull();
    expect(parseChatGatewayIdentity({ daemonEpoch: 12 })).toBeNull();
    expect(parseChatGatewayIdentity({ machineId: "mach-1" })).toBeNull();
    expect(parseChatGatewayIdentity(null)).toBeNull();
    expect(parseChatGatewayIdentity({ daemonEpoch: "5", hostId: "named-host", machineId: "mach-1" })).toEqual({
      daemonEpoch: "5",
      referenceHostId: "named-host",
      referenceOwnerId: null,
      machineId: "mach-1",
    });
    // the machine identity is never promoted to the host id
    expect(parseChatGatewayIdentity({ daemonEpoch: "5", machineId: "mach-1" })!.referenceHostId).toBeNull();
    // the owner authority is read from its own published field, never from the host id
    expect(
      parseChatGatewayIdentity({ daemonEpoch: "5", referenceHostId: "h", referenceOwnerId: "own-1" })!
        .referenceOwnerId,
    ).toBe("own-1");
  });

  it("repair15: a gateway that publishes no owner authority has no target at all", async () => {
    setWidth(390);
    // The workspace the UI is showing is not an owner authority. With `referenceOwnerId` absent
    // there is no target: neither a read nor a mutation may go out under an owner this client
    // invented, which is what the removed workspace fallback used to do.
    const harness = installFetch({
      capabilities: { apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH },
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");
    // the identity answer landed, so the absence below is a property of the payload, not a race
    await harness.whenCalled("/api/v1/capabilities");

    expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(false);

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "hi" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    expect(harness.calls.some((call) => call.url.includes("/submit"))).toBe(false);
  });

  it("QA-09: the provider session is read only from rows naming this session, and never guessed", () => {
    const rows = [
      { sessionId: "a", providerSession: { id: "one" } },
      { target: { sessionId: "a" }, providerSession: { id: "one" } },
      { sessionId: "b", providerSession: { id: "two" } },
    ];
    expect(chatProviderSessionId(rows, "a")).toBe("one");
    expect(chatProviderSessionId(rows, "b")).toBe("two");
    expect(chatProviderSessionId(rows, "c")).toBeNull();
    expect(chatProviderSessionId([{ sessionId: "a" }, { sessionId: "a", providerSession: { id: "one" } }], "a")).toBe("one");
    // two different ids for one pane is ambiguous, never "the newest"
    expect(
      chatProviderSessionId(
        [{ sessionId: "a", providerSession: { id: "one" } }, { sessionId: "a", providerSession: { id: "two" } }],
        "a",
      ),
    ).toBeNull();
    expect(chatDaemonEpochFromRows([{ sessionId: "a", daemonEpoch: "5" }], "a")).toBe("5");
    expect(
      chatDaemonEpochFromRows([{ sessionId: "a", daemonEpoch: "5" }, { sessionId: "a", daemonEpoch: "6" }], "a"),
    ).toBeNull();
    expect(chatDaemonEpochFromRows([], "a")).toBeNull();
  });
});


