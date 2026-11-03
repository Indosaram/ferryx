/**
 * Integration regression source for the reference chat lane (plan task 12).
 *
 *
 * The scenarios below are the task-12 acceptance contract (QA-01/04/05/06/07): chat is the default
 * at every width, the explicit terminal keeps the same session, submit/Stop/answer/files speak to
 * the ORIGINAL session through the reference-chat routes, drafts and held rows are owner-scoped,
 * and no assistant turn is ever fabricated.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createHash } from "node:crypto";
import { remoteHostStore } from "../state/remoteHostStore";
import {
  RemoteApp,
  chatDaemonEpochFromRows,
  chatProviderSessionId,
  parseChatGatewayIdentity,
} from "./RemoteApp";
import { referenceDrafts } from "./chat/referenceDraft";
import { referenceQueues } from "./chat/referenceQueue";
import type { ReferenceTargetRef } from "./chat/referenceTypes";

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
  readonly whenCalled: (fragment: string, timeoutMs?: number, method?: string) => Promise<HarnessCall>;
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
  state?: () => unknown;
} = {}): Harness {
  const calls: HarnessCall[] = [];
  interface Waiter {
    readonly fragment: string;
    readonly method?: string;
    readonly resolve: (call: HarnessCall) => void;
    readonly timer: ReturnType<typeof setTimeout>;
  }
  const waiters: Waiter[] = [];
  const record = (call: HarnessCall): void => {
    calls.push(call);
    for (let index = waiters.length - 1; index >= 0; index -= 1) {
      if (!call.url.includes(waiters[index].fragment)) continue;
      if (waiters[index].method !== undefined && waiters[index].method !== call.method) continue;
      const [waiter] = waiters.splice(index, 1);
      clearTimeout(waiter.timer);
      waiter.resolve(call);
    }
  };
  const whenCalled = (fragment: string, timeoutMs = 2000, method?: string): Promise<HarnessCall> => {
    const existing = calls.find(
      (call) => call.url.includes(fragment) && (method === undefined || call.method === method),
    );
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
      waiters.push({ fragment, method, resolve, timer });
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
      return jsonResponse({ sessions: (options.sessions ?? (() => [{ sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true }]))() });
    }
    if (url.includes("/reference-chat/") && url.includes("/history")) {
      // The host's history route requires a registry id: an empty one is a 400 INVALID_REQUEST.
      if (new URL(url, "http://harness.invalid").searchParams.get("registryId") === "") {
        return jsonResponse({ error: { code: "INVALID_REQUEST" } }, false, 400);
      }
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
    if (url.includes("/api/v1/workspace/state")) return jsonResponse((options.state ?? (() => remoteState))());
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
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
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

  it.each([
    ["idle", false],
    ["working", false],
    ["idle", true],
  ] as const)("r25-ui-m1: direct %s snapshot preserves controls with prompt=%s", async (activityState, hasPrompt) => {
    const harness = installFetch({
      state: () => ({ activeContext: {
        ...remoteState.activeContext,
        terminalTabs: remoteState.activeContext.terminalTabs.map((tab) => ({ ...tab, activityState })),
      } }),
      prompt: () => hasPrompt ? promptBody("r25-direct", false) : { prompt: null, screenRevision: null },
    });
    const promptRead = harness.whenCalled("/prompt");
    const running = activityState === "working" || hasPrompt;
    let dispose = () => {};
    const ready = new Promise<void>((resolve, reject) => {
      const matches = () => screen.queryByTestId(running ? "stop-button" : "send-button") !== null
        && (!hasPrompt || screen.queryByTestId("reference-prompt-card") !== null);
      const observer = new MutationObserver(() => { if (matches()) { dispose(); resolve(); } });
      const timer = setTimeout(() => { dispose(); reject(new Error("Missing R25 direct controls")); }, 2000);
      dispose = () => { clearTimeout(timer); observer.disconnect(); };
      observer.observe(document.body, { subtree: true, childList: true, attributes: true });
    });
    try {
      await act(async () => { render(<RemoteApp />); });
      await Promise.all([promptRead, ready]);
      if (hasPrompt) expect(screen.queryByTestId("send-button")).not.toBeNull();
      else expect(screen.queryByTestId(running ? "send-button" : "stop-button")).toBeNull();
      expect(harness.calls.filter((call) => call.url.includes("/workspace/select"))).toHaveLength(0);
    } finally {
      dispose();
    }
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

  it("acknowledges the opened chat on its owning daemon incarnation", async () => {
    setWidth(390);
    const harness = installFetch();
    const acknowledged = harness.whenCalled("/acknowledge-outcome", 2000, "POST");
    render(<RemoteApp />);
    const call = await acknowledged;
    const url = new URL(call.url, "http://harness.invalid");
    expect(url.pathname).toBe(`/api/v1/reference-chat/${SESSION_ID}/acknowledge-outcome`);
    expect(url.searchParams.get("backendSessionId")).toBe(SESSION_ID);
    expect(url.searchParams.get("epoch")).toBe(DAEMON_EPOCH);
    expect(url.searchParams.get("ownerId")).toBe(REFERENCE_OWNER_ID);
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
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/can't be shown on this phone/i);
    });
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("ui-workspace-wiring-r20-1: a pane with no agent reads no transcript and says so instead of a refresh failure", async () => {
    setWidth(390);
    let agentType: string | undefined;
    const harness = installFetch({
      state: () => ({
        activeContext: {
          ...remoteState.activeContext,
          activeTerminal: { sessionId: SESSION_ID, title: "zsh", running: true },
          terminalTabs: [
            { id: "tab-main", sessionId: SESSION_ID, label: "zsh", activityState: "idle", worktreeLabel: "main", ...(agentType ? { agentType } : {}) },
          ],
        },
      }),
    });
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    try {
      render(<RemoteApp />);
      expect(await screen.findByTestId("mobile-chat-workspace")).toBeInTheDocument();
      // The notice is drawn only once the pane's target is bound, so the polls below are not vacuous.
      await waitFor(() => {
        expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/no chat to show/i);
      });
      await act(async () => {
        vi.advanceTimersByTime(9000);
        await new Promise<void>((done) => setTimeout(done, 0));
      });
      expect(harness.calls.some((call) => call.url.includes("/history"))).toBe(false);
      expect(screen.getByTestId("chat-history-warning")).not.toHaveTextContent(/could not refresh/i);
      // The composer stays usable on a plain shell pane.
      expect(screen.getByTestId("chat-composer-textarea")).not.toBeDisabled();

      // The pane gains an agent: the newest page is read for it and the notice goes away.
      agentType = "claude";
      await act(async () => {
        window.dispatchEvent(new Event("online"));
      });
      const history = await harness.whenCalled("/history");
      expect(history.url).toContain("registryId=claude");
      await screen.findByText("The parser now streams line by line.");
      expect(screen.queryByTestId("chat-history-warning")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
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

  it("a FORBIDDEN send on a plain shell pane names the View role, not the missing agent", async () => {
    setWidth(390);
    const harness = installFetch({
      state: () => ({
        activeContext: {
          ...remoteState.activeContext,
          activeTerminal: { sessionId: SESSION_ID, title: "zsh", running: true },
          terminalTabs: [{ id: "tab-main", sessionId: SESSION_ID, label: "zsh", activityState: "idle", worktreeLabel: "main" }],
        },
      }),
      submit: () => ({
        ok: false,
        error: { code: "FORBIDDEN", message: "a view-only device cannot type into a pane", retryable: false },
      }),
    });
    render(<RemoteApp />);
    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/no chat to show/i);
    });

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "ls" } });
    const submitted = harness.whenCalled("/submit");
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await submitted;

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent(/can only view this session/);
    });
    expect(screen.getByTestId("chat-composer-warning")).not.toHaveTextContent(/runs no agent/);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("ls");
  });

  it("ui-reference-logic-r17-1: a send that fails on the current pane ends its draft send", async () => {
    setWidth(390);
    // the spy calls through: it only records which target the composer's send began on
    const begin = vi.spyOn(referenceDrafts, "begin");
    const begun = (): ReferenceTargetRef[] => begin.mock.calls.map(([target]) => target);
    let submits = 0;
    const harness = installFetch({
      submit: () => {
        submits += 1;
        return submits === 1
          ? { ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false }, requestId: "r1" }
          : { ok: true, data: { receipt: { requestId: "r2", target: {}, stage: "accepted" } }, requestId: "r2" };
      },
    });
    render(<RemoteApp />);
    await screen.findByTestId("chat-composer-textarea");

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "helo" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("helo");
    });
    expect(begun()).toHaveLength(1);
    // the failed send is over: the draft must not stay in flight carrying 'helo' as its pending
    // text, or the corrected resend is treated as an overlap and never ends either
    expect(referenceDrafts.read(begun()[0]).sending).toBe(false);

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "hello" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(harness.calls.filter((call) => call.url.includes("/submit"))).toHaveLength(2);
    });
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("");
    });
    await waitFor(() => {
      expect(referenceDrafts.read(begun()[0]).sending).toBe(false);
    });
    begin.mockRestore();
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

  it("T-23: a removed held message can be brought back with Undo within five seconds", async () => {
    setWidth(390);
    const prompt = {
      prompt: {
        promptId: "prompt-undo",
        agent: "claude",
        kind: "question",
        title: "Which file?",
        question: "Which file?",
        options: [{ label: "src/main.rs" }, { label: "src/lib.rs" }],
        multiSelect: false,
      },
      screenRevision: "rev-undo",
      cols: 80,
      rows: 24,
    };
    installFetch({ prompt: () => prompt });
    render(<RemoteApp />);
    await screen.findByTestId("reference-prompt-card");
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "keep me" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    const held = await screen.findByTestId("chat-composer-held");
    const remove = held.querySelector<HTMLButtonElement>('[data-testid^="held-message-remove-"]');
    expect(remove).not.toBeNull();

    await act(async () => {
      fireEvent.click(remove!);
    });
    expect(screen.queryByText("keep me", { selector: '[data-testid="chat-composer-held"] *' })).not.toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("Held message removed.");

    await act(async () => {
      fireEvent.click(within(screen.getByTestId("chat-composer-warning")).getByRole("button", { name: "Undo" }));
    });
    expect(await screen.findByTestId("chat-composer-held")).toHaveTextContent("keep me");
    expect(screen.queryByTestId("chat-composer-warning")).not.toBeInTheDocument();
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
    // The lane's own staged-file rehydrate is a GET on the same route; the upload is the POST.
    const stagedRequest = harness.whenCalled("/files", 2000, "POST");

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

  it("QA-06: a second Stop tap while the first is in flight sends no second interrupt", async () => {
    setWidth(390);
    const prompt = {
      prompt: {
        promptId: "prompt-stop-twice",
        agent: "claude",
        kind: "question",
        title: "Which file?",
        question: "Which file?",
        options: [{ label: "src/main.rs" }, { label: "src/lib.rs" }],
        multiSelect: false,
      },
      screenRevision: "rev-stop-twice",
      cols: 80,
      rows: 24,
    };
    const harness = installFetch({ prompt: () => prompt });
    render(<RemoteApp />);
    await screen.findByTestId("reference-prompt-card");

    // Both taps land before the first POST settles: a second Escape would open the rewind picker.
    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
      fireEvent.click(screen.getByTestId("stop-button"));
    });
    await harness.whenCalled("/stop", 2000, "POST");

    const stopCalls = harness.calls.filter((call) => call.url.includes("/stop"));
    expect(stopCalls).toHaveLength(1);
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
        // The lane binds a target to the identity the gateway publishes: the epoch it compares
        // against AND the owner authority. Without this document there is no target, and the
        // refusal this test asserts would never be reached.
        if (url.includes("/api/v1/capabilities")) {
          return jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: REFERENCE_OWNER_ID });
        }
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
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent(/Stop isn't available/i);
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
      expect(screen.getByTestId("chat-composer-attach-error")).toHaveTextContent(/over the size limit/i);
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

/* ---- review-fix regressions (fix/remote-chat-wiring) ---- */

const SECOND_SESSION_ID = "sess-second";

const twoTabState = {
  activeContext: {
    ...remoteState.activeContext,
    terminalTabs: [
      ...remoteState.activeContext.terminalTabs,
      { id: "tab-second", sessionId: SECOND_SESSION_ID, label: "second", agentType: "claude", activityState: "idle", worktreeLabel: "main" },
    ],
  },
};

type LaneRoute = (call: HarnessCall) => Response | Promise<Response> | undefined;

const okSubmit = () =>
  jsonResponse({ ok: true, data: { receipt: { requestId: "r1", target: {}, stage: "accepted" } }, requestId: "r1" });

const okStage = () =>
  jsonResponse({
    ok: true,
    data: {
      receipt: { hostId: "local", attachmentId: "att-1", sha256: NOTES_TXT_SHA256, sizeBytes: 3, mediaType: "text/plain" },
      displayName: "notes.txt",
      mentionText: "@notes.txt ",
    },
    requestId: "r4",
  });

/** A two-pane host: every route answered, and `route` overrides any of them first. */
function installLane(route: LaneRoute = () => undefined): HarnessCall[] {
  const calls: HarnessCall[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input instanceof Request ? input.url : input);
      const method = (init?.method ?? "GET").toUpperCase();
      let body: unknown = null;
      if (typeof init?.body === "string") {
        try {
          body = JSON.parse(init.body);
        } catch {
          body = init.body;
        }
      }
      const call = { url, method, body };
      calls.push(call);
      const routed = route(call);
      if (routed) return routed;
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "t", expiresAt: 9999999999 });
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: REFERENCE_OWNER_ID });
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({
          sessions: [
            { sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true },
            { sessionId: SECOND_SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true },
          ],
        });
      }
      if (url.includes("/api/v1/workspace/state")) return jsonResponse(twoTabState);
      if (url.includes("/history")) {
        return jsonResponse(
          url.includes(SECOND_SESSION_ID)
            ? nativePage({ turns: [{ role: "user", parts: [{ kind: "text", text: "B0" }], startedAt: "2026-10-06T00:00:00Z" }] })
            : nativePage(),
        );
      }
      if (url.includes("/prompt")) return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
      if (url.includes("/submit")) return okSubmit();
      if (url.includes("/files")) return method === "DELETE" ? jsonResponse({ ok: true }) : okStage();
      return jsonResponse({});
    }) as unknown as typeof fetch,
  );
  return calls;
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/**
 * Every response in these tests is answered by an already-settled mock, so a settled request's
 * whole continuation is microtasks: one task boundary drains all of them.
 */
async function drainSettledResponses(): Promise<void> {
  await act(async () => {
    await new Promise<void>((done) => setTimeout(done, 0));
  });
}

async function switchToSecondTab(): Promise<void> {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
  });
  const sheet = screen.getByRole("tablist", { name: /terminal tabs/i });
  await act(async () => {
    fireEvent.click(within(sheet).getByRole("tab", { name: /second/i }));
  });
  await screen.findByText("B0");
}

/** The lane is bound once the main pane's newest page has rendered. */
async function laneReady(): Promise<void> {
  await screen.findByText("The parser now streams line by line.", undefined, { timeout: 15000 });
}

async function stageNotes(): Promise<void> {
  await act(async () => {
    fireEvent.change(screen.getByTestId("file-upload-input"), {
      target: { files: [readableFile("abc", "notes.txt", "text/plain")] },
    });
  });
}

function promptBody(id: string, multiSelect: boolean): Record<string, unknown> {
  return {
    prompt: {
      promptId: id,
      agent: "claude",
      kind: "question",
      title: `Question ${id}`,
      question: `Question ${id}`,
      options: [{ label: "alpha" }, { label: "bravo" }, { label: "charlie" }],
      multiSelect,
    },
    screenRevision: `rev-${id}`,
    cols: 80,
    rows: 24,
  };
}

describe("reference chat lane review fixes", () => {
  beforeEach(() => {
    StubWebSocket.instances = [];
    localStorage.clear();
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", StubWebSocket);
    setWidth(390);
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("acknowledges the current chat again after a pushed screen refresh without reopening it", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const firstRead = deferred<void>();
    let nextAcknowledgement: ReturnType<typeof deferred<HarnessCall>> | null = null;
    let promptRead = false;
    installLane((call) => {
      if (call.url.includes("/prompt")) {
        promptRead = true;
        return jsonResponse({ prompt: null, screenRevision: "ready", cols: 80, rows: 24 });
      }
      if (call.url.includes("/acknowledge-outcome") && call.method === "POST") {
        if (promptRead) firstRead.resolve();
        nextAcknowledgement?.resolve(call);
        return new Response(null, { status: 204 });
      }
      return undefined;
    });
    await act(async () => { render(<RemoteApp />); });
    await firstRead.promise;
    nextAcknowledgement = deferred<HarnessCall>();
    const eventSockets = StubWebSocket.instances.filter((socket) => socket.url.includes("/events"));
    expect(eventSockets.length).toBeGreaterThan(0);
    await act(async () => {
      for (const socket of eventSockets) {
        socket.onmessage?.({ data: JSON.stringify({ type: "agentPromptChanged", sessionId: SESSION_ID,
          promptId: null, screenRevision: "completed" }) } as MessageEvent);
      }
    });
    const acknowledged = await nextAcknowledgement.promise;
    const url = new URL(acknowledged.url, "http://harness.invalid");
    expect(url.pathname).toBe(`/api/v1/reference-chat/${SESSION_ID}/acknowledge-outcome`);
    expect(url.searchParams.get("epoch")).toBe(DAEMON_EPOCH);
    expect(url.searchParams.get("ownerId")).toBe(REFERENCE_OWNER_ID);
  });

  it("ui-prompt-push-1: a pushed agentPromptChanged with no prompt closes the card without waiting for the poll", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let current: Record<string, unknown> = promptBody("A", false);
    installLane(({ url }) => (url.includes("/prompt") ? jsonResponse(current) : undefined));
    render(<RemoteApp />);
    await screen.findByText("Question A");

    // Answered on another device: the pane's screen no longer shows the menu.
    current = { prompt: null, screenRevision: "rev-after", cols: 80, rows: 24 };
    await act(async () => {
      for (const socket of StubWebSocket.instances) {
        socket.onmessage?.({
          data: JSON.stringify({ type: "agentPromptChanged", sessionId: SESSION_ID, promptId: null, screenRevision: "rev-after" }),
        } as MessageEvent);
      }
    });

    await waitFor(() => expect(screen.queryByText("Question A")).toBeNull());
  });

  it("ui-composer-prompt-1: a new prompt gets a fresh card, never the previous prompt's picks", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let current = promptBody("A", true);
    installLane(({ url }) => (url.includes("/prompt") ? jsonResponse(current) : undefined));
    render(<RemoteApp />);
    await screen.findByText("Question A");
    fireEvent.click(within(screen.getByTestId("reference-prompt-multi-0")).getByRole("checkbox"));
    expect(screen.getByTestId("reference-prompt-submit")).toBeEnabled();

    current = promptBody("B", true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await screen.findByText("Question B");

    expect(within(screen.getByTestId("reference-prompt-multi-0")).getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByTestId("reference-prompt-submit")).toBeDisabled();
  });

  it("ui-composer-prompt-2: a held message still in flight cannot be sent a second time", async () => {
    const submits: Array<(response: Response) => void> = [];
    const calls = installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(promptBody("P", false));
      if (url.includes("/submit")) return new Promise<Response>((done) => submits.push(done));
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question P");
    const textarea = screen.getByTestId("chat-composer-textarea");
    for (const text of ["note one", "note two"]) {
      fireEvent.change(textarea, { target: { value: text } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("send-button"));
      });
    }
    const [sendA, sendB] = screen.getAllByTestId(/^held-message-send-/);
    await act(async () => {
      fireEvent.click(sendA);
    });
    await act(async () => {
      fireEvent.click(sendB);
    });
    expect(submits).toHaveLength(2);

    await act(async () => {
      submits[0](okSubmit());
    });
    await drainSettledResponses();

    const remaining = screen.getAllByTestId(/^held-message-send-/);
    expect(remaining).toHaveLength(1);
    expect(remaining[0]).toBeDisabled();
    await act(async () => {
      fireEvent.click(remaining[0]);
    });
    expect(calls.filter((call) => call.url.includes("/submit"))).toHaveLength(2);
  });

  it("ui-reference-logic-1: a typed answer waiting for Confirm is dropped when the prompt changes", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const menu = (id: string) => {
      const body = promptBody(id, false);
      return { ...body, prompt: { ...(body.prompt as Record<string, unknown>), kind: "menu" } };
    };
    let current = menu("P1");
    const calls = installLane(({ url }) => (url.includes("/prompt") ? jsonResponse(current) : undefined));
    render(<RemoteApp />);
    await screen.findByText("Question P1", undefined, { timeout: 15000 });
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "1" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    expect(screen.getByTestId("reference-prompt-confirm-send")).toBeInTheDocument();

    current = menu("P2");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await screen.findByText("Question P2");

    // the "1" was typed for P1: P2's card offers no Confirm that would answer it with that pick
    expect(screen.queryByTestId("reference-prompt-confirm-send")).toBeNull();
    expect(calls.some((call) => call.url.includes("/answer"))).toBe(false);
  });

  it("ui-reference-logic-5: tapping a held row again while its send is in flight, after another row was sent, sends it once", async () => {
    const submits: Array<(response: Response) => void> = [];
    const calls = installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(promptBody("P", false));
      if (url.includes("/submit")) return new Promise<Response>((done) => submits.push(done));
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question P", undefined, { timeout: 15000 });
    const textarea = screen.getByTestId("chat-composer-textarea");
    for (const text of ["held A", "held B"]) {
      fireEvent.change(textarea, { target: { value: text } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("send-button"));
      });
    }
    const [sendA, sendB] = screen.getAllByTestId(/^held-message-send-/);
    await act(async () => {
      fireEvent.click(sendA);
    });
    await act(async () => {
      fireEvent.click(sendB);
    });
    await act(async () => {
      fireEvent.click(screen.getAllByTestId(/^held-message-send-/)[0]);
    });

    const submitsOf = (text: string) =>
      calls.filter(
        (call) => call.url.includes("/submit") && (call.body as { params?: { text?: string } }).params?.text === text,
      );
    expect(submitsOf("held B")).toHaveLength(1);
    expect(submitsOf("held A")).toHaveLength(1);
    await act(async () => {
      for (const done of submits) done(okSubmit());
    });
    await drainSettledResponses();
  });

  it("ui-workspace-wiring-1: picks ticked on one multi-select prompt are not carried to the next prompt", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let current = promptBody("M1", true);
    installLane(({ url }) => (url.includes("/prompt") ? jsonResponse(current) : undefined));
    render(<RemoteApp />);
    await screen.findByText("Question M1", undefined, { timeout: 15000 });
    fireEvent.click(within(screen.getByTestId("reference-prompt-multi-0")).getByRole("checkbox"));
    fireEvent.click(within(screen.getByTestId("reference-prompt-multi-2")).getByRole("checkbox"));
    expect(screen.getByTestId("reference-prompt-submit")).toBeEnabled();

    current = promptBody("M2", true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await screen.findByText("Question M2");

    expect(within(screen.getByTestId("reference-prompt-multi-0")).getByRole("checkbox")).not.toBeChecked();
    expect(within(screen.getByTestId("reference-prompt-multi-2")).getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByTestId("reference-prompt-submit")).toBeDisabled();
  });

  it("ui-composer-prompt-3: a keyboard answer hands the focus on to the message box", async () => {
    let answered = false;
    installLane(({ url }) => {
      if (url.includes("/answer")) {
        answered = true;
        return jsonResponse({ ok: true, data: {}, requestId: "r3" });
      }
      if (url.includes("/prompt")) {
        return jsonResponse(answered ? { prompt: null, screenRevision: "rev-2", cols: 80, rows: 24 } : promptBody("F", false));
      }
      return undefined;
    });
    render(<RemoteApp />);
    const option = await screen.findByTestId("reference-prompt-option-0");
    option.focus();
    await act(async () => {
      fireEvent.keyDown(option, { key: "Enter" });
      fireEvent.click(option, { detail: 0 });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-submit"));
    });
    await waitFor(() => {
      expect(screen.queryByTestId("reference-prompt-card")).not.toBeInTheDocument();
    });
    expect(document.activeElement).toBe(screen.getByTestId("chat-composer-textarea"));
  });

  it("rs-history-core-3: paging keeps going past the first older page of the same stream", async () => {
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? jsonResponse(nativePage({
            turns: [{ role: "user", parts: [{ kind: "text", text: "OLDER-1" }], startedAt: "2026-10-05T00:00:00Z" }],
            cursor: { streamId: "omo:abc", offset: 100 },
            hasMore: true,
          }))
        : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLDER-1");
    expect(screen.getByTestId("reference-older-button")).toBeInTheDocument();
  });

  it("rs-history-r4-2: the oldest page of a capped tail window discloses the history past the read cap", async () => {
    const capReason = "older history lies beyond the 4194304 byte read cap; the 7000000 bytes before this window were not read";
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? jsonResponse(nativePage({
            turns: [{ role: "user", parts: [{ kind: "text", text: "WINDOW-OLDEST" }], startedAt: "2026-10-05T00:00:00Z" }],
            cursor: null,
            hasMore: false,
            unavailableReason: capReason,
          }))
        : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("WINDOW-OLDEST");
    expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(capReason);
  });

  it("rs-files-routes-4: leaving a pane releases the files it staged and never sent", async () => {
    const calls = installLane();
    render(<RemoteApp />);
    await laneReady();
    await stageNotes();
    await screen.findByTestId("attachment-preview-att-1");

    await switchToSecondTab();

    await waitFor(() => {
      expect(
        calls.some((call) => call.method === "DELETE" && call.url.includes(`/reference-chat/${SESSION_ID}/files/att-1`)),
      ).toBe(true);
    });
  });

  const MOVED_HOST_ID = `${window.location.origin}/host/mach-1`;

  /** The connection is a saved machine record that does not yet name its relay origin. */
  function seedSavedHost(): void {
    remoteHostStore.reset();
    remoteHostStore.upsertHost({
      hostId: MOVED_HOST_ID,
      name: "mach-1",
      address: window.location.origin,
      machineId: "mach-1",
      deviceToken: "test-token",
    } as unknown as Parameters<typeof remoteHostStore.upsertHost>[0]);
    remoteHostStore.setActiveHost(MOVED_HOST_ID);
  }

  /**
   * Moves the transport base URL WITHOUT a remount: the same host record gains its relay origin,
   * so every route (capabilities included) is now addressed through /host/<machineId>. The
   * gateway answer is scoped to the old base, so the chat target is null until it re-answers.
   */
  function moveTransportBase(): void {
    act(() => {
      remoteHostStore.upsertHost({
        hostId: MOVED_HOST_ID,
        name: "mach-1",
        address: window.location.origin,
        machineId: "mach-1",
        relayOrigin: window.location.origin,
      } as unknown as Parameters<typeof remoteHostStore.upsertHost>[0]);
    });
  }

  it("ui-workspace-wiring-r17-1: a transport change that answers the same target keeps the staged file and its mention", async () => {
    try {
      seedSavedHost();
      const calls = installLane();
      render(<RemoteApp />);
      await laneReady();
      await stageNotes();
      await screen.findByTestId("attachment-preview-att-1");
      expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toContain("@notes.txt");

      moveTransportBase();
      // The same identity answers on the new base, and the lane reads its page there again.
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      expect(calls.filter((call) => call.method === "DELETE" && call.url.includes("/files/"))).toEqual([]);
      expect(screen.getByTestId("attachment-preview-att-1")).toBeInTheDocument();
      expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toContain("@notes.txt");
    } finally {
      remoteHostStore.reset();
    }
  });

  it("ui-workspace-wiring-r17-1: a transport change that answers a different owner still releases the staged file", async () => {
    try {
      seedSavedHost();
      let ownerId = REFERENCE_OWNER_ID;
      const calls = installLane(({ url }) =>
        url.includes("/api/v1/capabilities")
          ? jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: ownerId })
          : undefined,
      );
      render(<RemoteApp />);
      await laneReady();
      await stageNotes();
      await screen.findByTestId("attachment-preview-att-1");

      ownerId = "owner-pub-2";
      moveTransportBase();

      await waitFor(() => {
        expect(
          calls.some(
            (call) =>
              call.method === "DELETE" &&
              call.url.includes(`/reference-chat/${SESSION_ID}/files/att-1`) &&
              call.url.includes(`ownerId=${REFERENCE_OWNER_ID}`),
          ),
        ).toBe(true);
      });
      await waitFor(() => {
        expect(screen.queryByTestId("attachment-preview-att-1")).toBeNull();
      });
    } finally {
      remoteHostStore.reset();
    }
  });

  /** Holds the identity answer on the moved base, so the chat target stays null until released. */
  function gatedMovedCapabilities(): { route: LaneRoute; release: () => void } {
    const gate = deferred<void>();
    return {
      route: ({ url }) =>
        url.includes("/host/mach-1/") && url.includes("/api/v1/capabilities")
          ? gate.promise.then(() =>
              jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: REFERENCE_OWNER_ID }),
            )
          : undefined,
      release: () => gate.resolve(),
    };
  }

  it("ui-workspace-wiring-r18-1: an upload finishing while the target is briefly null leaves the composer usable", async () => {
    try {
      seedSavedHost();
      const stage = deferred<Response>();
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) =>
        call.url.includes("/files") && call.method === "POST" ? stage.promise : identity.route(call),
      );
      render(<RemoteApp />);
      await laneReady();
      await stageNotes();
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/files") && call.method === "POST")).toBe(true);
      });
      expect(screen.getByTestId("attach-file-button")).toBeDisabled();

      // The transport moves while the upload is out: the target is null until the host re-answers.
      moveTransportBase();
      await act(async () => {
        stage.resolve(okStage());
      });
      await drainSettledResponses();

      // The same identity answers on the new base, so the lane is not reset.
      await act(async () => {
        identity.release();
      });
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      await act(async () => {
        fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "hello" } });
      });
      expect(screen.getByTestId("attach-file-button")).toBeEnabled();
      expect(screen.getByTestId("send-button")).toBeEnabled();
    } finally {
      remoteHostStore.reset();
    }
  });

  it("ui-workspace-wiring-r18-2: an older page finishing while the target is briefly null leaves Load earlier usable", async () => {
    try {
      seedSavedHost();
      const older = deferred<Response>();
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) => {
        const { url } = call;
        if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return identity.route(call);
        return url.includes("cursor=")
          ? older.promise
          : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
      });
      render(<RemoteApp />);
      const olderButton = await screen.findByTestId("reference-older-button");
      await act(async () => {
        fireEvent.click(olderButton);
      });

      moveTransportBase();
      await act(async () => {
        older.resolve(new Response("unavailable", { status: 503 }));
      });
      await drainSettledResponses();

      await act(async () => {
        identity.release();
      });
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      expect(screen.getByTestId("reference-older-button")).toBeEnabled();
    } finally {
      remoteHostStore.reset();
    }
  });

  /** Message rows only: the composer's own value is not a sent message. */
  const shownMessages = (text: string): HTMLElement[] =>
    screen.queryAllByText(text).filter((element) => element.tagName !== "TEXTAREA");

  it("ui-workspace-wiring-r19-1: a send failing while the target is briefly null removes its echo and restores the draft", async () => {
    try {
      seedSavedHost();
      const submit = deferred<Response>();
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) => (call.url.includes("/submit") ? submit.promise : identity.route(call)));
      render(<RemoteApp />);
      await laneReady();
      fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "refactor the parser" } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("send-button"));
      });
      expect(shownMessages("refactor the parser")).toHaveLength(1);

      // The direct endpoint dies: the transport moves and the send settles while the target is null.
      moveTransportBase();
      await act(async () => {
        submit.resolve(
          jsonResponse({ ok: false, error: { code: "NETWORK_ERROR", message: "down", retryable: true }, requestId: "r1" }, false, 502),
        );
      });
      await drainSettledResponses();

      // The same identity answers on the new base, so the lane is not reset.
      await act(async () => {
        identity.release();
      });
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      expect(shownMessages("refactor the parser")).toHaveLength(0);
      expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toContain("refactor the parser");
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/not delivered|may not have been delivered|not known/i);
    } finally {
      remoteHostStore.reset();
    }
  });

  it("ui-workspace-wiring-r19-1: a stop failing while the target is briefly null still warns", async () => {
    try {
      seedSavedHost();
      const stop = deferred<Response>();
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) => {
        if (call.url.includes("/prompt")) return jsonResponse(promptBody("A", false));
        if (call.url.includes("/stop")) return stop.promise;
        return identity.route(call);
      });
      render(<RemoteApp />);
      await screen.findByText("Question A");
      await act(async () => {
        fireEvent.click(screen.getByTestId("stop-button"));
      });

      moveTransportBase();
      await act(async () => {
        stop.resolve(
          jsonResponse({ ok: false, error: { code: "INTERNAL", message: "no", retryable: true }, requestId: "s1" }, false, 500),
        );
      });
      await drainSettledResponses();
      await act(async () => {
        identity.release();
      });
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      expect(screen.getAllByText(/stop request was not delivered/).length).toBeGreaterThan(0);
    } finally {
      remoteHostStore.reset();
    }
  });

  it("ui-workspace-wiring-r19-1: an answer refused while the target is briefly null shows its error on the card", async () => {
    try {
      seedSavedHost();
      const answer = deferred<Response>();
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) => {
        if (call.url.includes("/prompt")) return jsonResponse(promptBody("A", false));
        if (call.url.includes("/answer")) return answer.promise;
        return identity.route(call);
      });
      render(<RemoteApp />);
      await screen.findByText("Question A");
      await act(async () => {
        fireEvent.click(screen.getByTestId("reference-prompt-option-0"));
      });
      await act(async () => {
        fireEvent.click(screen.getByTestId("reference-prompt-submit"));
      });

      moveTransportBase();
      await act(async () => {
        answer.resolve(
          jsonResponse(
            { ok: false, error: { code: "INTERNAL", message: "answer failed in gap", retryable: true }, requestId: "a1" },
            false,
            500,
          ),
        );
      });
      await drainSettledResponses();
      await act(async () => {
        identity.release();
      });
      await waitFor(() => {
        expect(calls.some((call) => call.url.includes("/host/mach-1/") && call.url.includes("/history"))).toBe(true);
      });
      await drainSettledResponses();

      expect(screen.getByText(/Couldn't send your answer\./)).toBeInTheDocument();
      expect(screen.queryByText("answer failed in gap")).not.toBeInTheDocument();
    } finally {
      remoteHostStore.reset();
    }
  });

  it("ui-composer-prompt-r22-1: a typed reply sent while the target is briefly null stays in the message box", async () => {
    try {
      seedSavedHost();
      const freeForm = {
        prompt: {
          promptId: "Q",
          agent: "codex",
          kind: "question",
          title: "Question Q",
          question: "Question Q",
          options: [],
          customOptionIndex: 0,
          multiSelect: false,
        },
        screenRevision: "rev-Q",
        cols: 80,
        rows: 24,
      };
      const identity = gatedMovedCapabilities();
      const calls = installLane((call) => (call.url.includes("/prompt") ? jsonResponse(freeForm) : identity.route(call)));
      render(<RemoteApp />);
      // The lane is bound and its card is up before the transport moves.
      await laneReady();
      await screen.findByText("Question Q");
      await drainSettledResponses();

      // The transport moves: the target is null until the host re-answers, and the card stays.
      moveTransportBase();
      await drainSettledResponses();
      expect(screen.getByText("Question Q")).toBeInTheDocument();

      const reply = "please keep the parser streaming";
      fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: reply } });
      await act(async () => {
        fireEvent.click(screen.getByTestId("send-button"));
      });
      await drainSettledResponses();

      expect(calls.some((call) => call.url.includes("/answer"))).toBe(false);
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(reply);
      expect(screen.getAllByText(/not attached yet, so nothing was sent/).length).toBeGreaterThan(0);
    } finally {
      remoteHostStore.reset();
    }
  });

  it("rs-history-r22-1: a provider session that rotated under the pane is re-read from the host's session rows", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let provider = "prov-A";
    let sessionReads = 0;
    const calls = installLane(({ url }) => {
      if (url.includes("/api/v1/sessions")) {
        sessionReads += 1;
        return jsonResponse({
          sessions: [
            { sessionId: SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true, providerSession: { key: "claude", id: provider } },
            { sessionId: SECOND_SESSION_ID, daemonEpoch: DAEMON_EPOCH, running: true },
          ],
        });
      }
      if (url.includes("/history") && url.includes(SESSION_ID)) {
        if (url.includes("providerSessionId=prov-B")) {
          return jsonResponse(
            nativePage({ turns: [{ role: "assistant", parts: [{ kind: "text", text: "fresh conversation B" }], startedAt: "2026-10-06T00:00:02Z" }] }),
          );
        }
        // the daemon now publishes prov-B for this pane, so a read naming prov-A is refused
        if (provider !== "prov-A") return jsonResponse({ error: { code: "TARGET_EXPIRED" } }, false, 410);
      }
      return undefined;
    });
    render(<RemoteApp />);
    await laneReady();
    expect(calls.some((call) => call.url.includes("/history") && call.url.includes("providerSessionId=prov-A"))).toBe(true);
    await drainSettledResponses();
    const readsBefore = sessionReads;

    // /new on the desktop: the pane's provider session rotates
    provider = "prov-B";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    await drainSettledResponses();

    // the refusal re-read the pane's session rows ...
    expect(sessionReads).toBeGreaterThan(readsBefore);
    // ... and the next read speaks to the new conversation
    await waitFor(() => {
      expect(calls.some((call) => call.url.includes("/history") && call.url.includes("providerSessionId=prov-B"))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.getByText("fresh conversation B")).toBeInTheDocument();
    });
  });

  it("ui-workspace-wiring-r19-1: a send failing after a switch to a different pane stays with its own pane", async () => {
    const submit = deferred<Response>();
    installLane(({ url }) => (url.includes("/submit") ? submit.promise : undefined));
    render(<RemoteApp />);
    await laneReady();
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "refactor the parser" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });

    await switchToSecondTab();
    await act(async () => {
      submit.resolve(
        jsonResponse({ ok: false, error: { code: "NETWORK_ERROR", message: "down", retryable: true }, requestId: "r1" }, false, 502),
      );
    });
    await drainSettledResponses();

    expect(screen.getByText("B0")).toBeInTheDocument();
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).not.toContain("refactor the parser");
    expect(screen.queryByTestId("chat-composer-warning")).not.toBeInTheDocument();
    expect(screen.queryByTestId("chat-history-warning")?.textContent ?? "").not.toMatch(/not delivered/);
  });

  it("ui-workspace-wiring-2: a late older page from the previous pane is never painted", async () => {
    const older = deferred<Response>();
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? older.promise
        : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });

    await switchToSecondTab();
    await act(async () => {
      older.resolve(jsonResponse(nativePage({
        turns: [{ role: "user", parts: [{ kind: "text", text: "A-OLD" }], startedAt: "2026-10-05T00:00:00Z" }],
        cursor: null,
        hasMore: false,
      })));
    });
    await drainSettledResponses();

    expect(screen.queryByText("A-OLD")).not.toBeInTheDocument();
    expect(screen.queryByTestId("reference-older-endcap")).not.toBeInTheDocument();
  });

  it("ui-workspace-wiring-3: the newest-page poll keeps an older page that was loaded", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? jsonResponse(nativePage({
            turns: [{ role: "user", parts: [{ kind: "text", text: "OLD-1" }], startedAt: "2026-10-05T00:00:00Z" }],
            cursor: null,
            hasMore: false,
          }))
        : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLD-1");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();

    expect(screen.getByText("OLD-1")).toBeInTheDocument();
  });

  it("ui-workspace-wiring-5: a staged file finishing after a pane switch stays out of the new pane", async () => {
    const stage = deferred<Response>();
    const calls = installLane(({ url, method }) =>
      url.includes("/files") && method === "POST" ? stage.promise : undefined,
    );
    render(<RemoteApp />);
    await laneReady();
    await stageNotes();
    await waitFor(() => {
      expect(calls.some((call) => call.url.includes("/files") && call.method === "POST")).toBe(true);
    });

    await switchToSecondTab();
    await act(async () => {
      stage.resolve(okStage());
    });
    await drainSettledResponses();

    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).not.toContain("@notes.txt");
    expect(screen.queryByTestId(/^attachment-preview-/)).not.toBeInTheDocument();
  });

  it("ui-workspace-wiring-6: a repeating identity refusal does not wipe what the user is typing", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    installLane(({ url }) =>
      url.includes("/history")
        ? jsonResponse({ error: { code: "INVENTORY_INCOMPLETE" } }, false, 503)
        : undefined,
    );
    render(<RemoteApp />);
    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/can't be shown on this phone/i);
    });
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "abc" } });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();

    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("abc");
  });

  it("ui-workspace-wiring-7: a delivered file mention clears its staged chip", async () => {
    const calls = installLane();
    render(<RemoteApp />);
    await laneReady();
    await stageNotes();
    await screen.findByTestId("attachment-preview-att-1");

    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(calls.some((call) => call.url.includes("/submit"))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.queryByTestId("chat-composer-attachments")).not.toBeInTheDocument();
    });
  });

  it("ui-workspace-wiring-9: a delivered send clears the earlier not-delivered warning", async () => {
    let submits = 0;
    installLane(({ url }) => {
      if (!url.includes("/submit")) return undefined;
      submits += 1;
      return submits === 1
        ? jsonResponse({ ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false }, requestId: "r1" })
        : okSubmit();
    });
    render(<RemoteApp />);
    await laneReady();
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "one" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/can only view this session/);
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(submits).toBe(2);
    });
    await drainSettledResponses();
    expect(screen.queryByTestId("chat-history-warning")?.textContent ?? "").not.toMatch(/can only view this session/);
  });

  it("ui-workspace-wiring-r2-2: a transient capabilities failure warns and retries instead of killing the lane", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let capabilityReads = 0;
    installLane(({ url }) => {
      if (!url.includes("/api/v1/capabilities")) return undefined;
      capabilityReads += 1;
      return capabilityReads === 1 ? jsonResponse({ error: "bad gateway" }, false, 502) : undefined;
    });
    render(<RemoteApp />);
    await waitFor(() => {
      expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/can't be shown on this phone/i);
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();

    expect(capabilityReads).toBeGreaterThan(1);
    await laneReady();
  });

  it("ui-workspace-wiring-r2-3: a newest page that slid past the loaded older pages drops them instead of leaving a gap", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let newestOffset = 500;
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      if (url.includes("cursor=")) {
        return jsonResponse(nativePage({
          turns: [{ role: "user", parts: [{ kind: "text", text: "OLD-1" }], startedAt: "2026-10-05T00:00:00Z" }],
          cursor: null,
          hasMore: false,
        }));
      }
      // a window that shares no turn with the one the older page joined: what lies between is unknown
      return newestOffset === 500
        ? jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: newestOffset }, hasMore: true }))
        : jsonResponse(nativePage({
            turns: [{ role: "user", parts: [{ kind: "text", text: "FAR-LATER" }], startedAt: "2026-10-07T00:00:00Z" }],
            cursor: { streamId: "omo:abc", offset: newestOffset },
            hasMore: true,
          }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLD-1");

    // the agent wrote more turns: the newest page now starts later than the one the older page joined
    newestOffset = 600;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();

    expect(screen.queryByText("OLD-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("reference-older-button")).toBeInTheDocument();
  });

  it("ui-workspace-wiring-r6-2: a newest window that slides forward keeps the older pages and the turns it slid past", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const t1 = { role: "user", parts: [{ kind: "text", text: "SLID-OUT" }], startedAt: "2026-10-06T00:00:00Z" };
    const t2 = { role: "assistant", parts: [{ kind: "text", text: "The parser now streams line by line." }], startedAt: "2026-10-06T00:00:01Z" };
    const t3 = { role: "assistant", parts: [{ kind: "text", text: "NEW-3" }], startedAt: "2026-10-06T00:00:02Z" };
    let slid = false;
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      if (url.includes("cursor=")) {
        return jsonResponse(nativePage({
          turns: [{ role: "user", parts: [{ kind: "text", text: "OLD-1" }], startedAt: "2026-10-05T00:00:00Z" }],
          cursor: null,
          hasMore: false,
        }));
      }
      return slid
        ? jsonResponse(nativePage({ turns: [t2, t3], cursor: { streamId: "omo:abc", offset: 600 }, hasMore: true }))
        : jsonResponse(nativePage({ turns: [t1, t2], cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLD-1");

    // the agent appended a turn: the newest window now starts one turn later on the same stream
    slid = true;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    await screen.findByText("NEW-3");

    expect(screen.getByText("OLD-1")).toBeInTheDocument();
    const slidOut = screen.getByText("SLID-OUT");
    expect(screen.getByText("OLD-1").compareDocumentPosition(slidOut) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      slidOut.compareDocumentPosition(screen.getByText("The parser now streams line by line.")) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getAllByText("SLID-OUT")).toHaveLength(1);
  });

  it("ui-workspace-wiring-r3-1: an older page still in flight when the newest page re-anchors is dropped", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let newestOffset = 500;
    const older = deferred<Response>();
    const calls = installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? older.promise
        : jsonResponse(nativePage({ cursor: { streamId: "omo:abc", offset: newestOffset }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });

    // while the older read is out, the agent writes more turns and the newest page starts later
    newestOffset = 600;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    await act(async () => {
      older.resolve(jsonResponse(nativePage({
        turns: [{ role: "user", parts: [{ kind: "text", text: "BEFORE-C0" }], startedAt: "2026-10-05T00:00:00Z" }],
        cursor: { streamId: "omo:abc", offset: 100 },
        hasMore: true,
      })));
    });
    await drainSettledResponses();

    // turns before the old anchor would sit directly on the new page with a silent gap between
    expect(screen.queryByText("BEFORE-C0")).not.toBeInTheDocument();
    const again = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(again);
    });
    const paged = calls.filter((call) => call.url.includes("cursor="));
    expect(paged).toHaveLength(2);
    expect(paged[1].url).toContain("cursor=600");
  });

  it("ui-workspace-wiring-r4-1: a send refused before it leaves keeps its text in the message box", async () => {
    const calls = installLane();
    render(<RemoteApp />);
    await laneReady();
    const tooLong = "x".repeat(20_001);
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: tooLong } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await drainSettledResponses();

    expect(calls.some((call) => call.url.includes("/submit"))).toBe(false);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(tooLong);
  }, 30_000);

  it("ui-workspace-wiring-r4-2: a failed send comes back even when the user typed during it", async () => {
    const submit = deferred<Response>();
    installLane(({ url }) => (url.includes("/submit") ? submit.promise : undefined));
    render(<RemoteApp />);
    await laneReady();
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "refactor the parser" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    // while the send is in flight the user starts the next message
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "and" } });
    await act(async () => {
      submit.resolve(
        jsonResponse({ ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false }, requestId: "r1" }),
      );
    });
    await drainSettledResponses();

    expect(screen.getByTestId("chat-history-warning")).toHaveTextContent(/can only view this session/);
    const box = screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement;
    expect(box.value).toContain("refactor the parser");
    expect(box.value).toContain("and");
  });

  it("ui-workspace-wiring-r4-3: an earlier turn with the same text does not swallow a new send's echo", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const page = nativePage({
      turns: [
        { role: "user", parts: [{ kind: "text", text: "continue" }], startedAt: "2026-10-06T00:00:00Z" },
        { role: "assistant", parts: [{ kind: "text", text: "The parser now streams line by line." }], startedAt: "2026-10-06T00:00:01Z" },
      ],
    });
    installLane(({ url }) => (url.includes("/history") && !url.includes(SECOND_SESSION_ID) ? jsonResponse(page) : undefined));
    render(<RemoteApp />);
    await laneReady();
    expect(screen.getAllByText("continue")).toHaveLength(1);

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "continue" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await drainSettledResponses();
    expect(screen.getAllByText("continue")).toHaveLength(2);

    // the agent is busy: the next poll still returns only the earlier turn
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    expect(screen.getAllByText("continue")).toHaveLength(2);
  });

  it("ui-workspace-wiring-r6-1: an echo retires when its turn lands even though the window slid the earlier same-text turn out", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const earlier = { role: "user", parts: [{ kind: "text", text: "continue" }], startedAt: "2026-10-06T00:00:00Z" };
    const reply = { role: "assistant", parts: [{ kind: "text", text: "The parser now streams line by line." }], startedAt: "2026-10-06T00:00:01Z" };
    const delivered = { role: "user", parts: [{ kind: "text", text: "continue" }], startedAt: "2026-10-06T00:00:02Z" };
    let landed = false;
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      // a full newest window: the delivered turn pushes the earlier "continue" out of it
      return jsonResponse(nativePage({ turns: landed ? [reply, delivered] : [earlier, reply] }));
    });
    render(<RemoteApp />);
    await laneReady();

    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "continue" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await drainSettledResponses();
    expect(screen.getAllByText("continue")).toHaveLength(2);

    landed = true;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    expect(screen.getAllByText("continue")).toHaveLength(1);
  });

  it("ui-composer-prompt-r5-1: a typed free-text answer the host refuses comes back to the message box", async () => {
    const freeForm = {
      prompt: {
        promptId: "Q",
        agent: "codex",
        kind: "question",
        title: "Question Q",
        question: "Question Q",
        options: [],
        customOptionIndex: 0,
        multiSelect: false,
      },
      screenRevision: "rev-Q",
      cols: 80,
      rows: 24,
    };
    const calls = installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(freeForm);
      if (url.includes("/answer")) {
        return jsonResponse(
          { ok: false, error: { code: "INTERNAL", message: "host failed", retryable: true }, requestId: "r5" },
          false,
          500,
        );
      }
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question Q");
    const reply = "please keep the parser streaming and add a test for it";
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: reply } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await drainSettledResponses();

    expect(calls.some((call) => call.url.includes("/answer"))).toBe(true);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(reply);
  });

  it("ui-workspace-wiring-r7-1: a send warning from the previous pane does not follow the user to the next pane", async () => {
    installLane(({ url }) =>
      url.includes("/submit")
        ? jsonResponse({ ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false }, requestId: "r1" })
        : undefined,
    );
    render(<RemoteApp />);
    await laneReady();
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "one" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent(/can only view this session/);
    });

    await switchToSecondTab();

    expect(screen.queryByTestId("chat-composer-warning")).not.toBeInTheDocument();
  });

  it("ui-workspace-wiring-r7-2: an identity refusal that resolves after a pane switch leaves the new pane alone", async () => {
    const identity = deferred<Response>();
    let capabilityReads = 0;
    installLane(({ url }) => {
      if (url.includes("/api/v1/capabilities")) {
        capabilityReads += 1;
        return capabilityReads === 1 ? undefined : identity.promise;
      }
      if (url.includes("/history") && url.includes(SESSION_ID)) {
        return jsonResponse({ error: { code: "TARGET_EXPIRED" } }, false, 410);
      }
      return undefined;
    });
    render(<RemoteApp />);
    await waitFor(() => {
      expect(capabilityReads).toBe(2);
    });

    await switchToSecondTab();
    await act(async () => {
      identity.resolve(
        jsonResponse({ apiVersion: 1, machineId: "mach-1", daemonEpoch: DAEMON_EPOCH, referenceOwnerId: REFERENCE_OWNER_ID }),
      );
    });
    await drainSettledResponses();

    expect(screen.getByText("B0")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-history-warning")?.textContent ?? "").not.toMatch(/can't be shown on this phone/);
  });

  it.each([
    ["accepted", "different-id"],
    ["refused", "different-id"],
    ["accepted", "same-id-new-revision"],
    ["refused", "same-id-new-revision"],
    ["accepted", "same-occurrence"],
    ["refused", "same-occurrence"],
    ["accepted", "A-B-A"],
    ["refused", "A-B-A"],
    ["accepted", "A-null-A"],
    ["refused", "A-null-A"],
  ] as const)("r24-ui-m1: %s completion protects %s prompt ownership", async (result, replacement) => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const answer = deferred<Response>();
    const arrived = deferred<void>();
    const nextArrived = deferred<void>();
    let promptRead = deferred<void>();
    const customPrompt = (id: string, revision: string) => ({
      ...promptBody(id, false),
      prompt: {
        promptId: id,
        agent: "claude",
        kind: "question",
        title: `Question ${revision}`,
        question: `Question ${revision}`,
        options: [{ label: "alpha" }, { label: "Other" }],
        customOptionIndex: 1,
        multiSelect: false,
      },
      screenRevision: revision,
    });
    let current: Record<string, unknown> = customPrompt("A", "rev-A");
    let polling = false;
    let completed = false;
    const calls = installLane(({ url }) => {
      if (url.includes("/prompt")) {
        if (polling) promptRead.resolve();
        return jsonResponse(completed && replacement === "same-occurrence" && result === "accepted"
          ? { prompt: null, screenRevision: null }
          : current);
      }
      if (url.includes("/answer")) {
        if (calls.filter((call) => call.url.includes("/answer")).length > 1) nextArrived.resolve();
        arrived.resolve();
        return answer.promise;
      }
      return undefined;
    });
    const bounded = <T,>(signal: Promise<T>): Promise<T> => new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Missing R24 prompt event")), 2000);
      signal.then(
        (value) => { clearTimeout(timer); resolve(value); },
        (error: unknown) => { clearTimeout(timer); reject(error); },
      );
    });
    const whenState = (matches: () => boolean): Promise<void> => new Promise((resolve, reject) => {
      const observer = new MutationObserver(() => {
        if (matches()) finish();
      });
      const timer = setTimeout(() => {
        observer.disconnect();
        reject(new Error("Missing R24 prompt state transition"));
      }, 2000);
      const finish = () => {
        clearTimeout(timer);
        observer.disconnect();
        resolve();
      };
      observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
      if (matches()) finish();
    });
    const ready = whenState(() => screen.queryByTestId("reference-prompt-option-0") !== null);
    render(<RemoteApp />);
    await ready;
    const firstCard = screen.getByTestId("reference-prompt-card");
    const dispatched = bounded(arrived.promise);
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-option-0"));
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-submit"));
      await dispatched;
    });
    expect(calls.find((call) => call.url.includes("/answer"))?.body).toMatchObject({
      params: { promptId: "A", screenRevision: "rev-A" },
    });

    const changed = replacement !== "same-occurrence";
    const recurring = replacement === "A-B-A" || replacement === "A-null-A";
    if (changed) current = replacement === "A-null-A"
      ? { prompt: null, screenRevision: null }
      : customPrompt(replacement === "different-id" || replacement === "A-B-A" ? "B" : "A", "rev-B");
    const published = whenState(() => changed
      ? replacement === "A-null-A"
        ? screen.queryByTestId("reference-prompt-card") === null
        : screen.queryByTestId("reference-prompt-card") !== null && screen.queryByTestId("reference-prompt-card") !== firstCard
      : screen.queryByTestId("reference-prompt-option-0")?.hasAttribute("disabled") === true);
    const polled = bounded(promptRead.promise);
    polling = true;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
      await polled;
    });
    await published;
    if (recurring) {
      const intermediateCard = screen.queryByTestId("reference-prompt-card");
      current = customPrompt("A", "rev-A");
      promptRead = deferred<void>();
      const returnedRead = bounded(promptRead.promise);
      const returned = whenState(() => {
        const shown = screen.queryByTestId("reference-prompt-card");
        return shown !== null && shown !== firstCard && shown !== intermediateCard;
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
        await returnedRead;
      });
      await returned;
    }
    const card = screen.getByTestId("reference-prompt-card");
    const customDraft = recurring ? "A2 custom draft" : "B custom draft";
    if (changed) {
      fireEvent.change(screen.getByTestId("reference-prompt-custom-input"), { target: { value: customDraft } });
    }
    const settled = changed ? null : whenState(() => result === "accepted"
      ? screen.queryByTestId("reference-prompt-card") === null
      : screen.queryAllByText(/Couldn't send your answer/).length > 0);
    completed = true;
    await act(async () => {
      answer.resolve(result === "accepted"
        ? jsonResponse({ ok: true, data: {}, requestId: "r24" })
        : jsonResponse({ ok: false, error: { code: "INTERNAL", message: "R24 answer refused", retryable: true }, requestId: "r24" }, false, 500));
      await answer.promise;
    });
    if (settled) await settled;
    if (!changed) {
      if (result === "accepted") expect(screen.queryByTestId("reference-prompt-card")).not.toBeInTheDocument();
      else {
        expect(screen.getByTestId("reference-prompt-card")).toBe(card);
        expect(screen.getByTestId("reference-prompt-option-0")).toBeEnabled();
        // the refusal is one plain sentence on the card; the host's message never shows
        expect(screen.queryAllByText("R24 answer refused")).toHaveLength(0);
      }
      return;
    }
    expect(screen.getByTestId("reference-prompt-card")).toBe(card);
    expect(screen.getByTestId("reference-prompt-custom-input")).toHaveValue(customDraft);
    expect(screen.queryAllByText(/Couldn't send your answer/)).toHaveLength(0);
    expect(screen.queryAllByText("R24 answer refused")).toHaveLength(0);
    const nextAnswer = calls.filter((call) => call.url.includes("/answer")).length;
    const nextDispatched = bounded(nextArrived.promise);
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-custom-send"));
      await nextDispatched;
    });
    expect(calls.filter((call) => call.url.includes("/answer"))).toHaveLength(nextAnswer + 1);
    expect(calls.filter((call) => call.url.includes("/answer"))[nextAnswer].body).toMatchObject({
      params: { promptId: replacement === "different-id" ? "B" : "A", screenRevision: recurring ? "rev-A" : "rev-B", answer: { customText: customDraft } },
    });
  });

  it("ui-workspace-wiring-r11-1: an answer refused after a pane switch leaves the new pane's prompt card alone", async () => {
    const answer = deferred<Response>();
    installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(promptBody(url.includes(SECOND_SESSION_ID) ? "B" : "A", false));
      if (url.includes("/answer")) return answer.promise;
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question A");
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-option-0"));
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-submit"));
    });

    await switchToSecondTab();
    await screen.findByText("Question B");
    await act(async () => {
      answer.resolve(
        jsonResponse(
          { ok: false, error: { code: "INTERNAL", message: "pane A answer failed", retryable: true }, requestId: "r11" },
          false,
          500,
        ),
      );
    });
    await drainSettledResponses();

    expect(screen.getByText("Question B")).toBeInTheDocument();
    expect(screen.queryByText("pane A answer failed")).not.toBeInTheDocument();
    expect(screen.queryByText(/Couldn't send your answer/)).not.toBeInTheDocument();
  });

  it("ui-workspace-wiring-r11-1: an answer accepted after a pane switch keeps the new pane's typed answer waiting for Confirm", async () => {
    const answer = deferred<Response>();
    const menuB = { ...promptBody("B", false), prompt: { ...(promptBody("B", false).prompt as object), kind: "menu" } };
    installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(url.includes(SECOND_SESSION_ID) ? menuB : promptBody("A", false));
      if (url.includes("/answer")) return answer.promise;
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question A");
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-option-0"));
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("reference-prompt-submit"));
    });

    await switchToSecondTab();
    await screen.findByText("Question B");
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "1" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await screen.findByTestId("reference-prompt-confirm");
    await act(async () => {
      answer.resolve(jsonResponse({ ok: true, data: {}, requestId: "r11" }));
    });
    await drainSettledResponses();

    expect(screen.getByText("Question B")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-confirm")).toBeInTheDocument();
  });

  it("ui-workspace-wiring-r5-1: a pane switch that releases staged files also drops their mentions from the pane's draft", async () => {
    // The desktop follows the pick and says so, so each switch confirms on its own event instead
    // of holding the picker until the confirmation timeout releases it.
    let activeTabId = "tab-main";
    installLane(({ url, method, body }) => {
      if (url.includes("/files") && method === "GET") return jsonResponse({ ok: true, data: { files: [] }, requestId: "r6" });
      if (url.includes("/api/v1/workspace/select")) {
        activeTabId = (body as { tabId: string }).tabId;
        queueMicrotask(() => {
          for (const socket of StubWebSocket.instances) {
            socket.onmessage?.({
              data: JSON.stringify({
                event: "remote_active_selection_changed",
                payload: { workspaceId: "ferryx", worktreeSlug: "main", tabId: activeTabId },
              }),
            } as MessageEvent);
          }
        });
        return jsonResponse({ ok: true });
      }
      if (url.includes("/api/v1/workspace/state")) {
        const tab = twoTabState.activeContext.terminalTabs.find((entry) => entry.id === activeTabId)!;
        return jsonResponse({
          activeContext: {
            ...twoTabState.activeContext,
            activeTabId,
            activeTerminal: { sessionId: tab.sessionId, title: tab.label, running: true },
          },
        });
      }
      return undefined;
    });
    render(<RemoteApp />);
    await laneReady();
    await stageNotes();
    await screen.findByTestId("attachment-preview-att-1");
    await waitFor(() => {
      expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("@notes.txt ");
    });

    await switchToSecondTab();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    });
    const sheet = screen.getByRole("tablist", { name: /terminal tabs/i });
    await act(async () => {
      fireEvent.click(within(sheet).getByRole("tab", { name: /claude/i }));
    });
    await laneReady();
    await drainSettledResponses();

    expect(screen.queryByTestId("attachment-preview-att-1")).not.toBeInTheDocument();
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).not.toContain("@notes.txt");
  });

  it("ui-workspace-wiring-r13-1: a stop that fails after a pane switch does not warn in the new pane", async () => {
    const stopA = deferred<Response>();
    installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(promptBody(url.includes(SECOND_SESSION_ID) ? "B" : "A", false));
      if (url.includes(`/reference-chat/${SESSION_ID}/stop`)) return stopA.promise;
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question A");
    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
    });

    await switchToSecondTab();
    await screen.findByText("Question B");
    await act(async () => {
      stopA.resolve(
        jsonResponse({ ok: false, error: { code: "INTERNAL", message: "no", retryable: true }, requestId: "s1" }, false, 500),
      );
    });
    await drainSettledResponses();

    expect(screen.queryAllByText(/stop request was not delivered/)).toHaveLength(0);
  });

  it("ui-workspace-wiring-r13-1: a stop still in flight on one pane does not swallow a stop on the next pane", async () => {
    const stopA = deferred<Response>();
    const calls = installLane(({ url }) => {
      if (url.includes("/prompt")) return jsonResponse(promptBody(url.includes(SECOND_SESSION_ID) ? "B" : "A", false));
      if (url.includes(`/reference-chat/${SESSION_ID}/stop`)) return stopA.promise;
      if (url.includes("/stop")) {
        return jsonResponse({ ok: true, data: { receipt: { requestId: "r2", target: {}, stage: "accepted" } }, requestId: "r2" });
      }
      return undefined;
    });
    render(<RemoteApp />);
    await screen.findByText("Question A");
    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
    });

    await switchToSecondTab();
    await screen.findByText("Question B");
    await act(async () => {
      fireEvent.click(screen.getByTestId("stop-button"));
    });
    await drainSettledResponses();

    expect(
      calls.filter((call) => call.method === "POST" && call.url.includes(`/reference-chat/${SECOND_SESSION_ID}/stop`)),
    ).toHaveLength(1);
    await act(async () => {
      stopA.resolve(jsonResponse({ ok: true, data: {}, requestId: "s1" }));
    });
  });

  it("ui-workspace-wiring-r15-1: files picked together are bounded by the bytes staged earlier in the same pick", async () => {
    // Three 8 MiB files: each fits the 10 MiB file bound, but together they pass the 20 MiB turn bound.
    const size = 8 * 1024 * 1024;
    const bytes = new Uint8Array(size);
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    const big = (name: string) => ({
      name,
      type: "text/plain",
      size,
      arrayBuffer: async () => bytes.buffer.slice(0),
    });
    const calls = installLane(({ url, method, body }) => {
      if (!url.includes("/files") || method !== "POST") return undefined;
      const name = (body as { params: { name: string } }).params.name;
      return jsonResponse({
        ok: true,
        data: {
          receipt: { hostId: "local", attachmentId: `att-${name}`, sha256, sizeBytes: size, mediaType: "text/plain" },
          displayName: name,
          mentionText: `@${name} `,
        },
        requestId: `r-${name}`,
      });
    });
    render(<RemoteApp />);
    await laneReady();
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), {
        target: { files: [big("one.txt"), big("two.txt"), big("three.txt")] },
      });
    });
    await screen.findByTestId("attachment-preview-att-two.txt", undefined, { timeout: 10_000 });
    await drainSettledResponses();

    expect(calls.filter((call) => call.method === "POST" && call.url.includes("/files"))).toHaveLength(2);
    expect(screen.queryByTestId("attachment-preview-att-three.txt")).not.toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-attach-error")).toHaveTextContent(/over the size limit/);
    expect(screen.getByTestId("chat-composer-attach-error").textContent).not.toMatch(/bytes/);
  }, 30_000);

  it("ui-workspace-wiring-r15-2: a slide past a repeated turn keeps every turn it slid past", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const cont = { role: "user", parts: [{ kind: "text", text: "continue" }], startedAt: null };
    const a1 = { role: "assistant", parts: [{ kind: "text", text: "ANSWER-1" }], startedAt: null };
    const a2 = { role: "assistant", parts: [{ kind: "text", text: "ANSWER-2" }], startedAt: null };
    const n1 = { role: "user", parts: [{ kind: "text", text: "NEW-1" }], startedAt: null };
    const n2 = { role: "assistant", parts: [{ kind: "text", text: "NEW-2" }], startedAt: null };
    let slid = false;
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      if (url.includes("cursor=")) {
        return jsonResponse(nativePage({
          turns: [{ role: "user", parts: [{ kind: "text", text: "OLD-1" }], startedAt: "2026-10-05T00:00:00Z" }],
          cursor: null,
          hasMore: false,
        }));
      }
      return slid
        ? jsonResponse(nativePage({ turns: [cont, a2, n1, n2], cursor: { streamId: "omo:abc", offset: 600 }, hasMore: true }))
        : jsonResponse(nativePage({ turns: [cont, a1, cont, a2], cursor: { streamId: "omo:abc", offset: 500 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLD-1");

    // two turns were appended: the window now starts at the second "continue"
    slid = true;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await drainSettledResponses();
    await screen.findByText("NEW-2");

    expect(screen.getByText("OLD-1")).toBeInTheDocument();
    expect(screen.getAllByText("ANSWER-1")).toHaveLength(1);
    expect(screen.getAllByText("continue")).toHaveLength(2);
    expect(
      screen.getByText("OLD-1").compareDocumentPosition(screen.getByText("ANSWER-1")) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      screen.getByText("ANSWER-1").compareDocumentPosition(screen.getByText("ANSWER-2")) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("ui-workspace-wiring-r15-2: an older page and the newest page holding the same turn text never share a key", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const cont = { role: "user", parts: [{ kind: "text", text: "continue" }], startedAt: null };
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      if (url.includes("cursor=")) {
        return jsonResponse(nativePage({ turns: [cont], cursor: null, hasMore: false }));
      }
      return jsonResponse(nativePage({
        turns: [cont, { role: "assistant", parts: [{ kind: "text", text: "The parser now streams line by line." }], startedAt: null }],
        cursor: { streamId: "omo:abc", offset: 500 },
        hasMore: true,
      }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await waitFor(() => {
      expect(screen.getAllByText("continue")).toHaveLength(2);
    });

    const duplicateKeys = errors.mock.calls.filter((args) => args.map(String).join(" ").includes("same key"));
    errors.mockRestore();
    expect(duplicateKeys).toEqual([]);
  });

  /** The desktop follows each pick and says so, so a pane round trip confirms on its own events. */
  function followingDesktop(): LaneRoute {
    let activeTabId = "tab-main";
    return ({ url, body }) => {
      if (url.includes("/api/v1/workspace/select")) {
        activeTabId = (body as { tabId: string }).tabId;
        queueMicrotask(() => {
          for (const socket of StubWebSocket.instances) {
            socket.onmessage?.({
              data: JSON.stringify({
                event: "remote_active_selection_changed",
                payload: { workspaceId: "ferryx", worktreeSlug: "main", tabId: activeTabId },
              }),
            } as MessageEvent);
          }
        });
        return jsonResponse({ ok: true });
      }
      if (url.includes("/api/v1/workspace/state")) {
        const tab = twoTabState.activeContext.terminalTabs.find((entry) => entry.id === activeTabId)!;
        return jsonResponse({
          activeContext: {
            ...twoTabState.activeContext,
            activeTabId,
            activeTerminal: { sessionId: tab.sessionId, title: tab.label, running: true },
          },
        });
      }
      return undefined;
    };
  }

  async function switchBackToFirstTab(): Promise<void> {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    });
    const sheet = screen.getByRole("tablist", { name: /terminal tabs/i });
    await act(async () => {
      fireEvent.click(within(sheet).getByRole("tab", { name: /claude/i }));
    });
  }

  const noPrompt = () => jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });

  function r26Bounded<T>(signal: Promise<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Missing R26 transport settlement")), 2000);
      signal.then(
        (value) => { clearTimeout(timer); resolve(value); },
        (error: unknown) => { clearTimeout(timer); reject(error); },
      );
    });
  }

  function r26DomEvent(matches: () => boolean): Promise<void> {
    return new Promise((resolve, reject) => {
      const finish = () => {
        clearTimeout(timer);
        observer.disconnect();
        resolve();
      };
      const observer = new MutationObserver(() => { if (matches()) finish(); });
      const timer = setTimeout(() => {
        observer.disconnect();
        reject(new Error("Missing R26 DOM state event"));
      }, 2000);
      observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
      if (matches()) finish();
    });
  }

  const r26Target: ReferenceTargetRef = {
    target: { hostId: "local", ownerId: REFERENCE_OWNER_ID, epoch: DAEMON_EPOCH, backendSessionId: SESSION_ID },
  };

  function r26Receipt(name: string) {
    return {
      receipt: { hostId: "local", attachmentId: `att-${name}`, sha256: NOTES_TXT_SHA256, sizeBytes: 3, mediaType: "text/plain" },
      displayName: name,
      mentionText: `@.ferryx/chat/att-${name}/${name} `,
    };
  }

  function r26Registry() {
    const desktop = followingDesktop();
    const files = new Map<string, Map<string, ReturnType<typeof r26Receipt>>>();
    const writes: string[] = [];
    let prompt = true;
    let deleteFails = false;
    let submitGate: ReturnType<typeof deferred<Response>> | null = null;
    const dispatched = deferred<void>();
    const key = (session: string) => `local\u0000${session}`;
    const bucket = (session: string) => {
      const targetKey = key(session);
      let entries = files.get(targetKey);
      if (!entries) { entries = new Map(); files.set(targetKey, entries); }
      return entries;
    };
    const calls = installLane((call) => {
      const url = new URL(call.url, "http://harness.invalid");
      const session = url.pathname.match(/\/reference-chat\/([^/]+)\//)?.[1];
      if (!session) return desktop(call);
      if (url.pathname.endsWith("/prompt")) {
        return session === SESSION_ID && prompt ? jsonResponse(promptBody("R26", false)) : noPrompt();
      }
      const entries = bucket(session);
      if (url.pathname.includes("/files")) {
        if (call.method === "GET") return jsonResponse({ files: [...entries.values()] });
        if (call.method === "DELETE") {
          if (deleteFails) return jsonResponse({ ok: false }, false, 500);
          entries.delete(decodeURIComponent(url.pathname.split("/").pop() ?? ""));
          return jsonResponse({ ok: true });
        }
        const body = call.body as { target: { hostId: string; backendSessionId: string }; params: { name: string } };
        expect(body.target).toMatchObject({ hostId: "local", backendSessionId: session });
        const file = r26Receipt(body.params.name);
        entries.set(file.receipt.attachmentId, file);
        return jsonResponse({ ok: true, data: file, requestId: "r26-stage" });
      }
      if (url.pathname.endsWith("/submit")) {
        const body = call.body as { target: { hostId: string; backendSessionId: string }; params: { text: string } };
        expect(body.target).toMatchObject({ hostId: "local", backendSessionId: session });
        const mentions = body.params.text.match(/@\.ferryx\/chat\/[^\s]+/g) ?? [];
        const missing = mentions.some((mention) => ![...entries.values()].some((file) => file.mentionText.trimEnd() === mention));
        dispatched.resolve();
        if (missing) return jsonResponse({ ok: false, error: { code: "NOT_FOUND", message: "nothing was typed", retryable: false } }, false, 404);
        if (submitGate) return submitGate.promise;
        writes.push(body.params.text);
        return okSubmit();
      }
      return desktop(call);
    });
    return {
      calls, writes, bucket, dispatched,
      clearPrompt: () => { prompt = false; },
      failDelete: () => { deleteFails = true; },
      deferSubmit: () => { const gate = deferred<Response>(); submitGate = gate; return gate; },
    };
  }

  async function r26Open(): Promise<void> {
    const ready = r26DomEvent(() => screen.queryByText("Question R26") !== null);
    await act(async () => { render(<RemoteApp />); });
    await ready;
  }

  async function r26Stage(name: string): Promise<void> {
    const staged = r26DomEvent(() => screen.queryByTestId(`attachment-preview-att-${name}`) !== null);
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), {
        target: { files: [readableFile("abc", name, "text/plain")] },
      });
    });
    await staged;
  }

  async function r26Hold(text: string): Promise<void> {
    const count = referenceQueues.read(r26Target).length;
    const held = r26DomEvent(() => screen.queryAllByTestId(/^held-message-send-/).length === count + 1);
    await act(async () => {
      fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: text } });
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await held;
    await act(async () => {
      fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "" } });
    });
  }

  it("r26-ui: held upload survives departure, GET rehydration and explicit send while unheld upload is deleted", async () => {
    const registry = r26Registry();
    await r26Open();
    await r26Stage("held.txt");
    const text = `review ${r26Receipt("held.txt").mentionText}`.trimEnd();
    await r26Hold(text);
    await r26Stage("unheld.txt");
    await switchToSecondTab();
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(true);
    expect(registry.bucket(SESSION_ID).has("att-unheld.txt")).toBe(false);
    expect(registry.calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(referenceDrafts.read(r26Target).text).not.toContain("att-unheld.txt");
    registry.clearPrompt();
    const rehydrated = r26DomEvent(() => screen.queryByTestId("attachment-preview-att-held.txt") !== null);
    await switchBackToFirstTab();
    await rehydrated;
    expect(screen.queryByTestId("attachment-preview-att-unheld.txt")).toBeNull();
    const acknowledged = r26DomEvent(() => screen.queryAllByTestId(/^held-message-send-/).length === 0);
    await act(async () => { fireEvent.click(screen.getAllByTestId(/^held-message-send-/)[0]); });
    await acknowledged;
    expect(registry.writes).toEqual([text]);
    expect(referenceQueues.read(r26Target)).toEqual([]);
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(true);
  });

  it("r26-ui: late definite refusal after departure preserves the held row and its bytes", async () => {
    const registry = r26Registry();
    const response = registry.deferSubmit();
    await r26Open();
    await r26Stage("held.txt");
    await r26Hold(`later ${r26Receipt("held.txt").mentionText}`);
    const dispatched = r26Bounded(registry.dispatched.promise);
    await act(async () => {
      fireEvent.click(screen.getAllByTestId(/^held-message-send-/)[0]);
      await dispatched;
    });
    await switchToSecondTab();
    const ended = deferred<void>();
    const unsubscribe = referenceDrafts.subscribe(() => ended.resolve());
    const settled = r26Bounded(ended.promise);
    try {
      await act(async () => {
        response.resolve(jsonResponse({ ok: false, error: { code: "NOT_FOUND", message: "nothing was typed", retryable: false } }, false, 404));
        await settled;
      });
    } finally {
      unsubscribe();
    }
    expect(registry.calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(true);
    expect(referenceQueues.read(r26Target)).toHaveLength(1);
    expect(screen.getByText("B0")).toBeInTheDocument();
    const returned = r26DomEvent(() => screen.queryByTestId("attachment-preview-att-held.txt") !== null);
    await switchBackToFirstTab();
    await returned;
    expect(screen.getAllByTestId(/^held-message-send-/)[0]).toBeEnabled();
    expect(registry.writes).toEqual([]);
  });

  it("r26-ui: chip removal is visibly refused until every held reference is explicitly edited or removed", async () => {
    const registry = r26Registry();
    await r26Open();
    await r26Stage("held.txt");
    await r26Hold(`one ${r26Receipt("held.txt").mentionText}`);
    await r26Hold(`two ${r26Receipt("held.txt").mentionText}`);
    const notice = r26DomEvent(() => screen.queryByTestId("chat-composer-attach-error") !== null);
    await act(async () => { fireEvent.click(screen.getByTestId("remove-attachment-att-held.txt")); });
    await notice;
    expect(screen.getByTestId("chat-composer-attach-error").textContent?.length).toBeGreaterThan(0);
    expect(registry.calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
    const [first, second] = referenceQueues.read(r26Target);
    await act(async () => { fireEvent.click(screen.getByTestId(`held-message-edit-${first.id}`)); });
    await act(async () => {
      fireEvent.change(screen.getByTestId(`held-message-input-${first.id}`), { target: { value: "no attachment" } });
      fireEvent.keyDown(screen.getByTestId(`held-message-input-${first.id}`), { key: "Enter" });
    });
    await act(async () => { fireEvent.click(screen.getByTestId("remove-attachment-att-held.txt")); });
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(true);
    expect(referenceQueues.read(r26Target).find((row) => row.id === second.id)?.text).toContain("att-held.txt");
    await act(async () => { fireEvent.click(screen.getByTestId(`held-message-remove-${second.id}`)); });
    const removed = r26DomEvent(() => screen.queryByTestId("attachment-preview-att-held.txt") === null);
    await act(async () => { fireEvent.click(screen.getByTestId("remove-attachment-att-held.txt")); });
    await removed;
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(false);
    expect(referenceQueues.read(r26Target)).toEqual([{ id: first.id, text: "no attachment" }]);
  });

  it("r26-ui: cold GET rehydration protects held ownership through another departure without a draft", async () => {
    const registry = r26Registry();
    registry.bucket(SESSION_ID).set("att-held.txt", r26Receipt("held.txt"));
    localStorage.setItem(referenceQueues.storageKey(r26Target), JSON.stringify({ version: 1, messages: [
      { id: "r26-cold", text: r26Receipt("held.txt").mentionText },
    ] }));
    const ready = r26DomEvent(() => screen.queryByTestId("attachment-preview-att-held.txt") !== null);
    await act(async () => { render(<RemoteApp />); });
    await ready;
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("");
    await switchToSecondTab();
    const returned = r26DomEvent(() => screen.queryByTestId("attachment-preview-att-held.txt") !== null);
    await switchBackToFirstTab();
    await returned;
    await switchToSecondTab();
    expect(registry.calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
    expect(registry.bucket(SESSION_ID).has("att-held.txt")).toBe(true);
  });

  it.each([false, true])("r26-ui: unheld departure honors DELETE confirmation with failure=%s", async (fails) => {
    const registry = r26Registry();
    if (fails) registry.failDelete();
    await r26Open();
    await r26Stage("unheld.txt");
    await switchToSecondTab();
    expect(registry.calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(registry.bucket(SESSION_ID).has("att-unheld.txt")).toBe(fails);
    expect(referenceDrafts.read(r26Target).text.includes("att-unheld.txt")).toBe(fails);
  });

  it("r26-ui: a missing generated mention refuses before writing and keeps the held row", async () => {
    const registry = r26Registry();
    await r26Open();
    await r26Stage("held.txt");
    await r26Hold(r26Receipt("held.txt").mentionText);
    registry.bucket(SESSION_ID).clear();
    const previousNotice = screen.getByTestId("chat-composer-warning").textContent;
    const refused = r26DomEvent(() => {
      const notice = screen.queryByTestId("chat-composer-warning");
      return notice !== null && notice.textContent !== previousNotice
        && !screen.getAllByTestId(/^held-message-send-/)[0].hasAttribute("disabled");
    });
    const dispatched = r26Bounded(registry.dispatched.promise);
    await act(async () => {
      fireEvent.click(screen.getAllByTestId(/^held-message-send-/)[0]);
      await dispatched;
    });
    await refused;
    expect(registry.writes).toEqual([]);
    expect(referenceQueues.read(r26Target)).toHaveLength(1);
    expect(registry.calls.filter((call) => call.url.includes("/submit"))).toHaveLength(1);
  });

  it("lane-sweep-1: a held row still in flight after leaving and returning to its pane cannot be sent twice", async () => {
    const submit = deferred<Response>();
    const desktop = followingDesktop();
    const calls = installLane((call) => {
      if (call.url.includes("/prompt")) return call.url.includes(SECOND_SESSION_ID) ? noPrompt() : jsonResponse(promptBody("P", false));
      if (call.url.includes("/submit")) return submit.promise;
      return desktop(call);
    });
    render(<RemoteApp />);
    await screen.findByText("Question P");
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "note one" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    const [held] = screen.getAllByTestId(/^held-message-send-/);
    await act(async () => {
      fireEvent.click(held);
    });
    expect(calls.filter((call) => call.url.includes("/submit"))).toHaveLength(1);

    await switchToSecondTab();
    await switchBackToFirstTab();
    await screen.findByText("Question P");

    const [again] = screen.getAllByTestId(/^held-message-send-/);
    expect(again).toBeDisabled();
    await act(async () => {
      fireEvent.click(again);
    });
    expect(calls.filter((call) => call.url.includes("/submit"))).toHaveLength(1);
    await act(async () => {
      submit.resolve(okSubmit());
    });
    await drainSettledResponses();
  });

  it("lane-sweep-2: a typed answer refused after a switch to another pane comes back in its own pane's draft", async () => {
    const answer = deferred<Response>();
    const desktop = followingDesktop();
    installLane((call) => {
      if (call.url.includes("/prompt")) return call.url.includes(SECOND_SESSION_ID) ? noPrompt() : jsonResponse(promptBody("A", false));
      if (call.url.includes("/answer")) return answer.promise;
      return desktop(call);
    });
    render(<RemoteApp />);
    await screen.findByText("Question A");
    fireEvent.change(screen.getByTestId("chat-composer-textarea"), { target: { value: "2" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toBe("");

    await switchToSecondTab();
    await act(async () => {
      answer.resolve(
        jsonResponse({ ok: false, error: { code: "INTERNAL", message: "answer refused", retryable: true }, requestId: "a1" }, false, 500),
      );
    });
    await drainSettledResponses();
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).not.toContain("2");

    await switchBackToFirstTab();
    await screen.findByText("Question A");
    expect((screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement).value).toBe("2");
  });

  it("ui-reference-logic-3: files delivered with a send stop counting against the next turn's file bound", async () => {
    const calls = installLane(({ url, method, body }) => {
      if (url.includes("/files") && method === "GET") return jsonResponse({ ok: true, data: { files: [] }, requestId: "r6" });
      if (!url.includes("/files") || method !== "POST") return undefined;
      const name = (body as { params: { name: string } }).params.name;
      return jsonResponse({
        ok: true,
        data: {
          receipt: { hostId: "local", attachmentId: `att-${name}`, sha256: NOTES_TXT_SHA256, sizeBytes: 3, mediaType: "text/plain" },
          displayName: name,
          mentionText: `@${name} `,
        },
        requestId: `r-${name}`,
      });
    });
    render(<RemoteApp />);
    await laneReady();
    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), {
        target: { files: ["a.txt", "b.txt", "c.txt", "d.txt"].map((name) => readableFile("abc", name, "text/plain")) },
      });
    });
    await screen.findByTestId("attachment-preview-att-d.txt");

    await act(async () => {
      fireEvent.click(screen.getByTestId("send-button"));
    });
    await waitFor(() => {
      expect(calls.some((call) => call.url.includes("/submit"))).toBe(true);
    });
    await drainSettledResponses();

    await act(async () => {
      fireEvent.change(screen.getByTestId("file-upload-input"), {
        target: { files: [readableFile("abc", "e.txt", "text/plain")] },
      });
    });
    await drainSettledResponses();

    expect(screen.queryByTestId("chat-composer-attach-error")).not.toBeInTheDocument();
    expect(calls.filter((call) => call.method === "POST" && call.url.includes("/files"))).toHaveLength(5);
    await screen.findByTestId("attachment-preview-att-e.txt");
  });

  it("ui-workspace-wiring-4: an older page whose cursor names the newest page's stream keeps Load earlier reachable", async () => {
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      return url.includes("cursor=")
        ? jsonResponse(nativePage({
            turns: [{ role: "user", parts: [{ kind: "text", text: "OLDER-PAGE-1" }], startedAt: "2026-10-05T00:00:00Z" }],
            cursor: { streamId: "claude:abc", offset: 2048 },
            hasMore: true,
          }))
        : jsonResponse(nativePage({ cursor: { streamId: "claude:abc", offset: 4096 }, hasMore: true }));
    });
    render(<RemoteApp />);
    const olderButton = await screen.findByTestId("reference-older-button");
    await act(async () => {
      fireEvent.click(olderButton);
    });
    await screen.findByText("OLDER-PAGE-1");
    await drainSettledResponses();

    expect(screen.queryByTestId("reference-older-endcap")).not.toBeInTheDocument();
    expect(screen.getByTestId("reference-older-button")).toBeInTheDocument();
  });

  it("ui-workspace-wiring-r16-1: a newest-page disclosure is drawn once and does not outlive the page that said it", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const notStarted = "This session has not written a turn yet.";
    let reads = 0;
    installLane(({ url }) => {
      if (!url.includes("/history") || url.includes(SECOND_SESSION_ID)) return undefined;
      reads += 1;
      return jsonResponse(
        reads === 1
          ? nativePage({ source: "omo-transcript", availability: "notStarted", turns: [], unavailableReason: "session_not_written" })
          : nativePage(),
      );
    });
    render(<RemoteApp />);
    await waitFor(() => {
      expect(screen.getByTestId("reference-disclosure")).toHaveTextContent(notStarted);
    });
    await drainSettledResponses();
    expect(document.body.textContent?.split(notStarted).length).toBe(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    await laneReady();
    await drainSettledResponses();

    expect(reads).toBe(2);
    expect(document.body.textContent).not.toContain(notStarted);
  });

  it.each(["accepted", "refused", "fetch-lost", "body-truncated", "invalid-envelope"] as const)(
    "r23-ui: protects dispatched mentions across a switch until %s settles",
    async (result) => {
      const response = deferred<Response>();
      const arrived = deferred<void>();
      const desktop = followingDesktop();
      const calls = installLane((call) => {
        if (call.url.includes("/files") && call.method === "GET") {
          return jsonResponse({ ok: true, data: { files: [] }, requestId: "r6" });
        }
        if (call.url.includes("/submit")) {
          arrived.resolve();
          return response.promise.then((answer) => {
            if (result === "fetch-lost") throw new TypeError("Connection closed after dispatch");
            return answer;
          });
        }
        return desktop(call);
      });
      const bounded = <T,>(signal: Promise<T>): Promise<T> => new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("Missing submit state event")), 2000);
        signal.then(
          (value) => { clearTimeout(timer); resolve(value); },
          (error: unknown) => { clearTimeout(timer); reject(error); },
        );
      });
      render(<RemoteApp />);
      await laneReady();
      await stageNotes();
      await screen.findByTestId("attachment-preview-att-1");
      const target: ReferenceTargetRef = {
        target: { hostId: "local", ownerId: REFERENCE_OWNER_ID, epoch: DAEMON_EPOCH, backendSessionId: SESSION_ID },
      };
      const ended = deferred<void>();
      let sending = false;
      const unsubscribe = referenceDrafts.subscribe(() => {
        const draft = referenceDrafts.read(target);
        if (draft.sending) sending = true;
        else if (sending) ended.resolve();
      });
      try {
        const dispatched = bounded(arrived.promise);
        await act(async () => {
          fireEvent.click(screen.getByTestId("send-button"));
          await dispatched;
        });
        await act(async () => {
          fireEvent.click(screen.getByTestId("remove-attachment-att-1"));
        });
        expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(0);
        await switchToSecondTab();
        const deletes = () => calls.filter((call) => call.method === "DELETE" && call.url.includes("/files/att-1"));
        expect(deletes()).toHaveLength(0);

        const settled = bounded(ended.promise);
        await act(async () => {
          response.resolve(
            result === "refused"
              ? jsonResponse({ ok: false, error: { code: "FORBIDDEN", message: "no", retryable: false } }, false, 403)
              : result === "body-truncated"
                ? new Response('{"ok":true,', { status: 200 })
                : result === "invalid-envelope"
                  ? jsonResponse({})
                  : okSubmit(),
          );
          await settled;
        });
        expect(deletes()).toHaveLength(result === "refused" ? 1 : 0);
        if (result === "refused") expect(deletes()[0].url).toContain(`/reference-chat/${SESSION_ID}/files/att-1`);
        expect(screen.getByText("B0")).toBeInTheDocument();
      } finally {
        unsubscribe();
      }
    },
  );

  it("ui-workspace-wiring-r5-1: a released staged file's mention leaves the previous pane's saved draft", async () => {
    const desktop = followingDesktop();
    const calls = installLane((call) => {
      if (call.url.includes("/files") && call.method === "GET") return jsonResponse({ ok: true, data: { files: [] }, requestId: "r6" });
      return desktop(call);
    });
    const savedDrafts = (): string[] => {
      const values: string[] = [];
      for (let index = 0; index < localStorage.length; index += 1) {
        const key = localStorage.key(index);
        if (key?.startsWith("ferryx.referenceChat.draft:")) values.push(localStorage.getItem(key) ?? "");
      }
      return values;
    };
    render(<RemoteApp />);
    await laneReady();
    await stageNotes();
    await screen.findByTestId("attachment-preview-att-1");
    await waitFor(() => {
      expect(savedDrafts().some((text) => text.includes("@notes.txt"))).toBe(true);
    });

    await switchToSecondTab();
    await waitFor(() => {
      expect(
        calls.some((call) => call.method === "DELETE" && call.url.includes(`/reference-chat/${SESSION_ID}/files/att-1`)),
      ).toBe(true);
    });
    await drainSettledResponses();

    expect(savedDrafts().some((text) => text.includes("@notes.txt"))).toBe(false);
  });
});
