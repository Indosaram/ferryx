import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteTerminal } from "./RemoteTerminal";
import { REMOTE_TERMINAL_FRAME_PREFIX, REMOTE_TERMINAL_FRAME_TERMINATOR } from "./remoteTerminalFrames";
import { FALLBACK_PREFERENCES } from "../lib/terminalSettings";

class MockWebSocket {
  static readonly OPEN = 1;
  static latest: MockWebSocket | null = null;
  static instances: MockWebSocket[] = [];
  static creationResolvers: ((socket: MockWebSocket) => void)[] = [];
  static nextCreation(): Promise<MockWebSocket> {
    return new Promise(resolve => { MockWebSocket.creationResolvers.push(resolve); });
  }
  readonly url: string;
  binaryType = "arraybuffer";
  readyState = MockWebSocket.OPEN;
  send = vi.fn();
  close = vi.fn();
  private _onopen: ((event?: any) => void) | null = null;
  private _onclose: ((event?: any) => void) | null = null;
  private _onmessage: ((event: any) => void) | null = null;
  private _onerror: ((event?: any) => void) | null = null;
  private _installedResolvers: (() => void)[] = [];

  get onopen() {
    return this._onopen;
  }
  set onopen(val) {
    this._onopen = val;
    this._checkInstalled();
  }

  get onclose() {
    return this._onclose;
  }
  set onclose(val) {
    this._onclose = val;
    this._checkInstalled();
  }

  get onmessage() {
    return this._onmessage;
  }
  set onmessage(val) {
    this._onmessage = val;
    this._checkInstalled();
  }

  get onerror() {
    return this._onerror;
  }
  set onerror(val) {
    this._onerror = val;
    this._checkInstalled();
  }

  private _checkInstalled() {
    if (this._onopen && this._onclose && this._onmessage && this._onerror) {
      const resolvers = this._installedResolvers;
      this._installedResolvers = [];
      resolvers.forEach((r) => r());
    }
  }

  waitForHandlers(): Promise<void> {
    if (this._onopen && this._onclose && this._onmessage && this._onerror) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this._installedResolvers.push(resolve);
    });
  }

  constructor(url: string) {
    this.url = url;
    MockWebSocket.latest = this;
    MockWebSocket.instances.push(this);
    const resolvers = MockWebSocket.creationResolvers.splice(0);
    resolvers.forEach(resolve => resolve(this));
  }
}

class SynchronousCloseWebSocket extends MockWebSocket {
  override close = vi.fn(() => this.onclose?.());
}

function socket(): MockWebSocket {
  const value = MockWebSocket.latest;
  if (!value) throw new Error("Expected remote terminal WebSocket");
  return value;
}

function surface(): HTMLElement {
  return screen.getByTestId("remote-terminal-grid");
}

function mockLegacyTicketFetch() {
  let ticket = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const path = new URL(String(input), window.location.origin).pathname;
    if (path === "/api/v1/socket-ticket") {
      return Response.json({ ticket: `legacy-ticket-${++ticket}` });
    }
    if (path === "/api/v1/terminal/preferences") return Response.json(FALLBACK_PREFERENCES);
    throw new Error(`Unexpected test fetch: ${path}`);
  }));
}

function rect(width: number, height: number): DOMRect {
  return {
    x: 0,
    y: 0,
    width,
    height,
    top: 0,
    right: width,
    bottom: height,
    left: 0,
    toJSON: () => ({}),
  } as DOMRect;
}

const hiddenCursor = {
  x: 0,
  y: 0,
  visible: false,
  blinking: false,
  wideTail: false,
  visualStyle: "block",
};

function connectionStatus() {
  return screen.getByTestId("remote-terminal-connection-status");
}

function establishReady(ws = socket()) {
  act(() => {
    ws.onopen?.();
    ws.onmessage?.({
      data: JSON.stringify({
        type: "grid",
        cols: 80,
        rows: 20,
        cursor: hiddenCursor,
        lines: [],
      }),
    });
  });
}

describe("remote terminal grid contract", () => {
  beforeEach(() => {
    // Pin jitter to zero so backoff tests stay deterministic (jitter = 1 - 0.5 * 0 = 1).
    vi.spyOn(Math, "random").mockReturnValue(0);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });
  });

  afterEach(() => {
    cleanup();
    MockWebSocket.latest = null;
    MockWebSocket.instances = [];
    MockWebSocket.creationResolvers = [];
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("includes viewport geometry in the initial grid socket request", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    render(<RemoteTerminal sessionId="session-123" token="token-abc" title="Remote Shell" />);

    expect(screen.getByText("Remote Shell")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");
    expect(socket().url).toMatch(/\/api\/v1\/terminal\/session-123\?token=token-abc&render=grid&cols=80&rows=20$/);

    act(() => {
      socket().onopen?.();
      socket().onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(socket().send).not.toHaveBeenCalled();
  });

  it("fences owner outcomes and coalesces Retry through the socket owner", async () => {
    vi.useFakeTimers();
    const sockets: MockWebSocket[] = [];
    const createWebSocket = vi.fn(() => {
      const ws = new SynchronousCloseWebSocket("mock://outcome");
      sockets.push(ws);
      return ws;
    });
    const view = render(<RemoteTerminal sessionId="paired-1" token="unused" isAccountSession daemonEpoch="epoch-1" createWebSocket={createWebSocket} />);
    await sockets[0].waitForHandlers();
    const frame = { type: "terminalOutcome", target: { sessionId: "paired-1", backendSessionId: "paired-1", daemonEpoch: "epoch-1" }, incarnation: "incarnation-1", generation: "8", state: "blocked", blockedReason: "permission_denied", exitCode: null };
    act(() => sockets[0].onmessage?.({ data: JSON.stringify({ ...frame, target: { ...frame.target, daemonEpoch: "old" } }) }));
    expect(screen.queryByTestId("remote-terminal-outcome")).toBeNull();
    act(() => sockets[0].onmessage?.({ data: JSON.stringify(frame) }));
    expect(screen.getByTestId("remote-terminal-outcome")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
    expect(createWebSocket).toHaveBeenCalledTimes(1);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    });
    expect(createWebSocket).toHaveBeenCalledTimes(2);
    await sockets[1].waitForHandlers();
    act(() => sockets[0].onmessage?.({ data: JSON.stringify(frame) }));
    expect(screen.queryByTestId("remote-terminal-outcome")).toBeNull();
    act(() => sockets[1].onmessage?.({ data: JSON.stringify({ ...frame, generation: null, state: "affirmative_exit", exitCode: 7 }) }));
    expect(screen.getByTestId("remote-terminal-outcome")).toHaveTextContent("This terminal has exited (code 7).");
    expect(screen.queryByTestId("remote-terminal-outcome-unknown")).toBeNull();
    act(() => sockets[1].onmessage?.({ data: JSON.stringify({ ...frame, generation: null, state: "recovering", exitCode: null }) }));
    expect(screen.getByTestId("remote-terminal-outcome")).toHaveTextContent("This terminal has exited (code 7).");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
    await sockets[2].waitForHandlers();
    act(() => sockets[2].onmessage?.({ data: JSON.stringify({ ...frame, generation: null, state: "recovering", exitCode: null }) }));
    expect(screen.getByTestId("remote-terminal-outcome-unknown")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-outcome")).toBeNull();
    view.unmount();
  });

  it.each(["blocked", "affirmative_exit"] as const)("ignores trailing presentation and control frames after %s until Retry", async (state) => {
    // Given: a live legacy owner receives an authoritative terminal outcome.
    mockLegacyTicketFetch();
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal("__task9Wire", {});
    const onInputDrop = vi.fn();
    const sessionId = `trailing-${state}`;
    const created = MockWebSocket.nextCreation();
    render(<RemoteTerminal sessionId={sessionId} token="token" daemonEpoch="epoch" onInputDrop={onInputDrop} />);
    const ws = await created;
    await ws.waitForHandlers();
    establishReady(ws);
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    const target = { sessionId, daemonEpoch: "epoch" };
    const outcome = { type: "terminalOutcome", target: { ...target, backendSessionId: sessionId },
      incarnation: "owner", generation: null, state, blockedReason: "permission_denied", exitCode: 0 };
    act(() => ws.onmessage?.({ data: JSON.stringify(outcome) }));
    const presentationSequence = qa.snapshot(target).presentationSequence;
    const grid = JSON.stringify({ type: "grid", cols: 80, rows: 20, cursor: hiddenCursor,
      lines: [{ index: 0, runs: [{ text: "trailing-presentation", fg: null, bg: null, attrs: 0 }] }] });
    const trailing = [grid,
      ...["output", "replay", "replayGap"].map(kind =>
        `${REMOTE_TERMINAL_FRAME_PREFIX}${JSON.stringify({ kind })}${REMOTE_TERMINAL_FRAME_TERMINATOR}${grid}`),
      JSON.stringify({ type: "attached", target, generation: "9" }),
      JSON.stringify({ ...outcome, state: "ready" }),
      JSON.stringify({ ...outcome, state: "recovering" }),
    ];

    // When: queued presentation/control frames arrive on that same socket.
    for (const data of trailing) {
      act(() => ws.onmessage?.({ data }));
      expect(connectionStatus()).not.toHaveTextContent("Live");
      expect(qa.snapshot(target)).toMatchObject({ outcome: state, presentationSequence, pendingTimers: 0 });
    }
    ws.send.mockClear();
    fireEvent.keyDown(surface(), { key: "x" });

    // Then: neither presentation nor input resumes without an explicit Retry.
    expect(screen.queryByText("trailing-presentation")).toBeNull();
    expect(onInputDrop).toHaveBeenCalledWith(1);
    expect(ws.send).not.toHaveBeenCalled();
    const retryCreated = MockWebSocket.nextCreation();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
    const next = await retryCreated;
    expect(next).not.toBe(ws);
    await next.waitForHandlers();
    establishReady(next);
    expect(connectionStatus()).toHaveTextContent("Live");
    expect(qa.snapshot(target)).toMatchObject({ outcome: "ready" });
  });

  it.each([
    ["blocked", "close"], ["blocked", "error"],
    ["affirmative_exit", "close"], ["affirmative_exit", "error"],
  ] as const)("does not roll back transport after %s followed by socket %s", async (state, callback) => {
    // Given: a live direct-transport owner settles authoritatively.
    mockLegacyTicketFetch();
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onTransportFailure = vi.fn();
    const created = MockWebSocket.nextCreation();
    render(<RemoteTerminal sessionId="settled" token="token" daemonEpoch="epoch" onTransportFailure={onTransportFailure} />);
    const ws = await created;
    await ws.waitForHandlers();
    establishReady(ws);
    act(() => ws.onmessage?.({ data: JSON.stringify({ type: "terminalOutcome",
      target: { sessionId: "settled", backendSessionId: "settled", daemonEpoch: "epoch" },
      incarnation: "owner", generation: null, state, blockedReason: "permission_denied", exitCode: 0 }) }));

    // When: the socket reports its terminal callback and foreground wake follows.
    await act(async () => {
      if (callback === "close") ws.onclose?.();
      else ws.onerror?.();
      window.dispatchEvent(new Event("online"));
      await vi.advanceTimersByTimeAsync(20000);
    });

    // Then: normal settlement is not a transport outage or an automatic redial.
    expect(onTransportFailure).not.toHaveBeenCalled();
    expect(MockWebSocket.instances).toHaveLength(1);
    expect(connectionStatus()).not.toHaveTextContent("Live");
    expect(screen.getByTestId("remote-terminal-outcome")).toBeInTheDocument();
  });

  it("observes ready only after a full legacy grid despite an earlier ready outcome", async () => {
    // Given: a quiet legacy socket is still awaiting its first full grid.
    mockLegacyTicketFetch();
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal("__task9Wire", {});
    const onInputDrop = vi.fn();
    const created = MockWebSocket.nextCreation();
    render(<RemoteTerminal sessionId="quiet" token="token" daemonEpoch="epoch" onInputDrop={onInputDrop} />);
    const ws = await created;
    await ws.waitForHandlers();
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    const target = { sessionId: "quiet", daemonEpoch: "epoch" };

    // When: outcome metadata says ready before presentation exists.
    act(() => ws.onmessage?.({ data: JSON.stringify({ type: "terminalOutcome",
      target: { ...target, backendSessionId: "quiet" }, incarnation: "owner", generation: "1", state: "ready" }) }));

    // Then: observation and input agree with the pending presentation handshake.
    expect(qa.snapshot(target)).toMatchObject({ outcome: "retryable", pendingTimers: 1, presentationSequence: 0 });
    expect(connectionStatus()).not.toHaveTextContent("Live");
    fireEvent.keyDown(surface(), { key: "x" });
    expect(onInputDrop).toHaveBeenCalledWith(1);
    expect(ws.send).not.toHaveBeenCalled();
    establishReady(ws);
    expect(qa.snapshot(target)).toMatchObject({ outcome: "ready", pendingTimers: 0, presentationSequence: 1 });
    expect(connectionStatus()).toHaveTextContent("Live");
    expect(ws.send).not.toHaveBeenCalled();
    fireEvent.keyDown(surface(), { key: "y" });
    expect(ws.send).toHaveBeenCalledWith(new TextEncoder().encode("y"));
  });

  it.each(["blocked", "affirmative_exit"] as const)("stops handshake and close recovery for a validated null-generation %s", async (state) => {
    // Given: a current socket has not completed its presentation handshake.
    vi.stubGlobal("__task9Wire", {});
    vi.useFakeTimers();
    const sockets: MockWebSocket[] = [];
    const createWebSocket = vi.fn(() => {
      const ws = new SynchronousCloseWebSocket("mock://null-generation-outcome");
      sockets.push(ws);
      return ws;
    });
    const sessionId = `paired-null-${state}`;
    const view = render(<RemoteTerminal sessionId={sessionId} token="unused" isAccountSession daemonEpoch="epoch-null" createWebSocket={createWebSocket} />);
    await sockets[0].waitForHandlers();
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    const target = { sessionId, daemonEpoch: "epoch-null" };

    // When: the exact owner reports a terminal outcome without a controller.
    act(() => sockets[0].onmessage?.({ data: JSON.stringify({
      type: "terminalOutcome", target: { ...target, backendSessionId: sessionId },
      incarnation: "incarnation-null", generation: null, state,
      blockedReason: state === "blocked" ? "permission_denied" : null,
      exitCode: state === "affirmative_exit" ? 7 : null,
    }) }));

    // Then: no handshake timer, close callback, or foreground wake can redial.
    expect(qa.snapshot(target)).toMatchObject({ outcome: state, incarnation: "incarnation-null", pendingTimers: 0 });
    await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
    expect(createWebSocket).toHaveBeenCalledTimes(1);
    expect(sockets[0].close).not.toHaveBeenCalled();
    await act(async () => {
      sockets[0].onclose?.();
      window.dispatchEvent(new Event("online"));
      document.dispatchEvent(new Event("visibilitychange"));
      await vi.advanceTimersByTimeAsync(20000);
    });
    expect(createWebSocket).toHaveBeenCalledTimes(1);
    expect(qa.snapshot(target)).toMatchObject({ outcome: state, pendingTimers: 0 });
    view.unmount();
  });

  it("reports the latest validated affirmative exit through the owner observer", async () => {
    vi.stubGlobal("__task9Wire", {});
    const sockets: MockWebSocket[] = [];
    const createWebSocket = vi.fn(() => {
      const ws = new SynchronousCloseWebSocket("mock://observer-outcome");
      sockets.push(ws);
      return ws;
    });
    const view = render(<RemoteTerminal sessionId="paired-exit" token="unused" isAccountSession daemonEpoch="epoch-exit" createWebSocket={createWebSocket} />);
    await sockets[0].waitForHandlers();
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    const target = { sessionId: "paired-exit", daemonEpoch: "epoch-exit" };
    act(() => sockets[0].onmessage?.({
      data: JSON.stringify({
        type: "terminalOutcome",
        target: { ...target, backendSessionId: "paired-exit" },
        incarnation: "incarnation-exit",
        generation: "9",
        state: "affirmative_exit",
        exitCode: 7,
      }),
    }));
    expect(qa.snapshot(target)).toMatchObject({
      outcome: "affirmative_exit",
      incarnation: "incarnation-exit",
      exitSettlements: 1,
    });
    view.unmount();
  });

  it("omits viewport geometry from the grid socket request when followHostSize is true", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        title="Remote Shell"
        followHostSize={true}
      />,
    );

    expect(screen.getByText("Remote Shell")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");
    expect(socket().url).toMatch(/\/api\/v1\/terminal\/session-123\?token=token-abc&render=grid$/);
    expect(socket().url).not.toContain("cols=");
    expect(socket().url).not.toContain("rows=");

    act(() => {
      socket().onopen?.();
      socket().onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(socket().send).not.toHaveBeenCalled();
  });

  it("builds account tunnel socket URL with daemonEpoch and render=grid but no client geometry", async () => {
    let capturedPath: string | null = null;
    const mockSocket = new MockWebSocket("mock://socket");
    const createWebSocket = vi.fn((pathAndQuery: string) => {
      capturedPath = pathAndQuery;
      return mockSocket;
    });

    render(
      <RemoteTerminal
        sessionId="session-account-123"
        token="token-unused"
        title="Account Terminal"
        isAccountSession={true}
        daemonEpoch="1790310731270"
        createWebSocket={createWebSocket}
      />,
    );

    await waitFor(() => {
      expect(createWebSocket).toHaveBeenCalledTimes(1);
    });

    expect(capturedPath).not.toBeNull();
    expect(capturedPath).toContain("daemonEpoch=1790310731270");
    // The machine API renders the grid server-side: request it, but never geometry.
    expect(capturedPath).toContain("render=grid");
    expect(capturedPath).not.toContain("cols=");
    expect(capturedPath).not.toContain("rows=");
    expect(capturedPath).toBe(
      "/api/v1/terminal/session-account-123?daemonEpoch=1790310731270&render=grid",
    );
  });

  it("reports failure and avoids sending an empty or guessed value when daemonEpoch is genuinely unknown in account mode", async () => {
    const createWebSocket = vi.fn();
    const onTransportFailure = vi.fn();
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    render(
      <RemoteTerminal
        sessionId="session-unknown-epoch"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch={undefined}
        createWebSocket={createWebSocket}
        onTransportFailure={onTransportFailure}
      />,
    );

    await waitFor(() => {
      expect(onTransportFailure).toHaveBeenCalled();
    });

    expect(createWebSocket).not.toHaveBeenCalled();
    expect(consoleErrorSpy).toHaveBeenCalledWith(
      expect.stringContaining("daemonEpoch is missing for session session-unknown-epoch"),
    );
  });

  it("reports whether a socket opened or closed before opening", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();
    const first = render(
      <RemoteTerminal
        sessionId="session-closed"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    act(() => socket().onclose?.());
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-closed", "closed");

    first.unmount();
    render(
      <RemoteTerminal
        sessionId="session-open"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );
    act(() => {
      socket().onopen?.();
      socket().onclose?.();
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-open", "open");
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-open", "closed");
    expect(onSocketLifecycle).toHaveBeenCalledTimes(3);
  });

  it("does not report an intentional component teardown as a socket failure", () => {
    vi.stubGlobal("WebSocket", SynchronousCloseWebSocket);
    const onSocketLifecycle = vi.fn();
    const view = render(
      <RemoteTerminal
        sessionId="session-intentional-close"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    view.unmount();

    expect(onSocketLifecycle).not.toHaveBeenCalled();
  });

  it("automatically re-dials with exponential backoff on abnormal close (1s, 2s, 4s)", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    expect(MockWebSocket.instances).toHaveLength(1);
    const firstSocket = MockWebSocket.instances[0];

    // Initial abnormal close (without prior onopen)
    act(() => {
      firstSocket.onclose?.();
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-123", "closed");
    expect(MockWebSocket.instances).toHaveLength(1);

    // After 999ms, no new socket yet
    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(MockWebSocket.instances).toHaveLength(1);

    // After 1000ms total, first reconnect dial occurs (1s delay)
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockWebSocket.instances).toHaveLength(2);
    const secondSocket = MockWebSocket.instances[1];
    expect(secondSocket.url).toBe(firstSocket.url);

    // Second failure: onclose on the second socket
    act(() => {
      secondSocket.onclose?.();
    });
    expect(MockWebSocket.instances).toHaveLength(2);

    // After 1999ms, no new socket yet
    act(() => {
      vi.advanceTimersByTime(1999);
    });
    expect(MockWebSocket.instances).toHaveLength(2);

    // After 2000ms total, second reconnect dial occurs (2s backoff)
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockWebSocket.instances).toHaveLength(3);
    const thirdSocket = MockWebSocket.instances[2];
    expect(thirdSocket.url).toBe(firstSocket.url);

    // Third failure: onclose on the third socket
    act(() => {
      thirdSocket.onclose?.();
    });
    expect(MockWebSocket.instances).toHaveLength(3);

    // Advance 4s for next backoff (4s)
    act(() => {
      vi.advanceTimersByTime(3999);
    });
    expect(MockWebSocket.instances).toHaveLength(3);
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockWebSocket.instances).toHaveLength(4);
  });

  it("successful reconnect reports open lifecycle again and resets backoff to 1s", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    const firstSocket = MockWebSocket.instances[0];

    // First socket opens and then closes
    act(() => {
      firstSocket.onopen?.();
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-123", "open");

    act(() => {
      firstSocket.onclose?.();
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-123", "closed");

    // Advance 1s -> 2nd socket connects
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(MockWebSocket.instances).toHaveLength(2);
    const secondSocket = MockWebSocket.instances[1];

    // 2nd socket fails immediately -> backoff becomes 2s
    act(() => {
      secondSocket.onclose?.();
    });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(MockWebSocket.instances).toHaveLength(3);
    const thirdSocket = MockWebSocket.instances[2];

    // 3rd socket successfully opens and establishes readiness -> resets backoff!
    act(() => {
      thirdSocket.onopen?.();
      thirdSocket.onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
    });
    expect(onSocketLifecycle).toHaveBeenLastCalledWith("session-123", "open");

    // 3rd socket closes abnormally later
    act(() => {
      thirdSocket.onclose?.();
    });
    expect(onSocketLifecycle).toHaveBeenLastCalledWith("session-123", "closed");

    // Because backoff was reset to 0, next reconnect must happen after 1s (not 4s or 8s)
    act(() => {
      vi.advanceTimersByTime(999);
    });
    expect(MockWebSocket.instances).toHaveLength(3);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(MockWebSocket.instances).toHaveLength(4);
  });

  it("unmount does not schedule a re-dial or fire lifecycle events", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();

    const view = render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    expect(MockWebSocket.instances).toHaveLength(1);
    const firstSocket = MockWebSocket.instances[0];

    // Unmount the component
    view.unmount();
    expect(firstSocket.close).toHaveBeenCalled();

    // Trigger onclose on the unmounted socket (simulate browser close event)
    act(() => {
      firstSocket.onclose?.();
    });

    // Advance time significantly
    act(() => {
      vi.advanceTimersByTime(30000);
    });

    // No new WebSockets should be created, no lifecycle events fired
    expect(MockWebSocket.instances).toHaveLength(1);
    expect(onSocketLifecycle).not.toHaveBeenCalled();
  });

  it("changing session cancels any pending reconnect timer and connects only to the new session", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();

    const view = render(
      <RemoteTerminal
        sessionId="session-a"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    const socketA = MockWebSocket.instances[0];
    expect(socketA.url).toContain("session-a");

    // Close socket A abnormally to start reconnect timer (1s)
    act(() => {
      socketA.onclose?.();
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-a", "closed");

    // Switch to session B before the 1s timer elapses (e.g. at 500ms)
    act(() => {
      vi.advanceTimersByTime(500);
    });
    view.rerender(
      <RemoteTerminal
        sessionId="session-b"
        token="token-abc"
        onSocketLifecycle={onSocketLifecycle}
      />,
    );

    // Session B is dialed immediately
    expect(MockWebSocket.instances).toHaveLength(2);
    const socketB = MockWebSocket.instances[1];
    expect(socketB.url).toContain("session-b");

    // Advance beyond the remaining 500ms of session A's timer
    act(() => {
      vi.advanceTimersByTime(10000);
    });

    // No extra dials for session A occurred
    expect(MockWebSocket.instances).toHaveLength(2);
  });

  it("guards against double-dialing so only one reconnect timer runs at a time", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);

    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const firstSocket = MockWebSocket.instances[0];

    // Trigger multiple close callbacks on the same socket
    act(() => {
      firstSocket.onclose?.();
      firstSocket.onclose?.();
    });

    // Advance 1s: exactly one new socket is dialed
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(MockWebSocket.instances).toHaveLength(2);
  });

  it("caps backoff delay at 10s and continues retrying indefinitely", () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", MockWebSocket);

    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    // 1st close -> 1s
    act(() => { MockWebSocket.instances[0].onclose?.(); });
    act(() => { vi.advanceTimersByTime(1000); });
    expect(MockWebSocket.instances).toHaveLength(2);

    // 2nd close -> 2s
    act(() => { MockWebSocket.instances[1].onclose?.(); });
    act(() => { vi.advanceTimersByTime(2000); });
    expect(MockWebSocket.instances).toHaveLength(3);

    // 3rd close -> 4s
    act(() => { MockWebSocket.instances[2].onclose?.(); });
    act(() => { vi.advanceTimersByTime(4000); });
    expect(MockWebSocket.instances).toHaveLength(4);

    // 4th close -> 8s
    act(() => { MockWebSocket.instances[3].onclose?.(); });
    act(() => { vi.advanceTimersByTime(8000); });
    expect(MockWebSocket.instances).toHaveLength(5);

    // 5th close -> 10s (capped)
    act(() => { MockWebSocket.instances[4].onclose?.(); });
    act(() => { vi.advanceTimersByTime(9999); });
    expect(MockWebSocket.instances).toHaveLength(5);
    act(() => { vi.advanceTimersByTime(1); });
    expect(MockWebSocket.instances).toHaveLength(6);

    // 6th close -> 10s (still capped)
    act(() => { MockWebSocket.instances[5].onclose?.(); });
    act(() => { vi.advanceTimersByTime(9999); });
    expect(MockWebSocket.instances).toHaveLength(6);
    act(() => { vi.advanceTimersByTime(1); });
    expect(MockWebSocket.instances).toHaveLength(7);
  });

  it("reattaches to the newly focused session and ignores callbacks from the old socket", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const view = render(<RemoteTerminal sessionId="session-a" token="token-abc" />);
    const firstSocket = socket();

    act(() => firstSocket.onopen?.());
    view.rerender(<RemoteTerminal sessionId="session-b" token="token-abc" />);
    const secondSocket = socket();
    expect(firstSocket.close).toHaveBeenCalledOnce();
    expect(secondSocket.url).toMatch(/\/api\/v1\/terminal\/session-b\?token=token-abc&render=grid&cols=80&rows=20$/);

    act(() => {
      secondSocket.onopen?.();
      secondSocket.onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
      firstSocket.onclose?.();
      firstSocket.onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 12,
          rows: 1,
          cursor: hiddenCursor,
          lines: [{ index: 0, runs: [{ text: "stale session output", fg: null, bg: null, attrs: 0 }] }],
        }),
      } as MessageEvent);
    });

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(surface()).not.toHaveTextContent("stale session output");
  });

  it("renders a full grid frame and patches only named diff lines", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    act(() => {
      socket().onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 12,
          rows: 2,
          cursor: hiddenCursor,
          lines: [
            { index: 0, runs: [{ text: "first line", fg: null, bg: null, attrs: 0 }] },
            { index: 1, runs: [{ text: "second line", fg: null, bg: null, attrs: 0 }] },
          ],
        }),
      } as MessageEvent);
    });

    await waitFor(() => {
      expect(surface().querySelector('[data-grid-line="0"]')).toHaveTextContent("first line");
      expect(surface().querySelector('[data-grid-line="1"]')).toHaveTextContent("second line");
    });

    act(() => {
      socket().onmessage?.({
        data: JSON.stringify({
          type: "gridDiff",
          cols: 12,
          rows: 2,
          cursor: { ...hiddenCursor, x: 4, y: 1 },
          lines: [{ index: 1, runs: [{ text: "changed", fg: null, bg: null, attrs: 0 }] }],
        }),
      } as MessageEvent);
    });

    await waitFor(() => {
      expect(surface().querySelector('[data-grid-line="0"]')).toHaveTextContent("first line");
      expect(surface().querySelector('[data-grid-line="1"]')).toHaveTextContent("changed");
      expect(surface()).not.toHaveTextContent("second line");
    });
  });

  it("preserves keyboard, control-signal, destructive-editing, Enter, and paste encodings", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const target = surface();

    fireEvent.keyDown(target, { key: "ArrowUp" });
    fireEvent.keyDown(target, { key: "a", ctrlKey: true });
    fireEvent.keyDown(target, { key: "c", ctrlKey: true });
    fireEvent.keyDown(target, { key: "Backspace" });
    fireEvent.keyDown(target, { key: "Delete" });
    fireEvent.keyDown(target, { key: "Enter" });
    fireEvent.paste(target, { clipboardData: { getData: () => "pasted text" } });

    expect(socket().send).toHaveBeenNthCalledWith(1, new TextEncoder().encode("\u001b[A"));
    expect(socket().send).toHaveBeenNthCalledWith(2, new Uint8Array([1]));
    expect(socket().send).toHaveBeenNthCalledWith(3, JSON.stringify({ type: "signal", signal: "interrupt" }));
    expect(socket().send).toHaveBeenNthCalledWith(4, new TextEncoder().encode("\u007f"));
    expect(socket().send).toHaveBeenNthCalledWith(5, new TextEncoder().encode("\u001b[3~"));
    expect(socket().send).toHaveBeenNthCalledWith(6, new TextEncoder().encode("\r"));
    expect(socket().send).toHaveBeenNthCalledWith(7, new TextEncoder().encode("pasted text"));
  });

  it("wraps multiline paste in bracketed paste mode markers to prevent prompt splitting", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const target = surface();

    fireEvent.paste(target, {
      clipboardData: { getData: () => "line 1\r\nline 2\nline 3" },
    });

    expect(socket().send).toHaveBeenCalledWith(
      new TextEncoder().encode("\x1b[200~line 1\nline 2\nline 3\x1b[201~"),
    );
  });

  it("does not shatter IME jamo keydowns into individual PTY writes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "ㄱ" });
    fireEvent.keyDown(surface(), { key: "ㅏ" });
    fireEvent.keyDown(surface(), { key: "ㄴ", isComposing: true });
    fireEvent.keyDown(surface(), { key: "Enter", isComposing: true });
    fireEvent.keyDown(surface(), { key: "Process" });

    expect(socket().send).not.toHaveBeenCalled();
  });

  it("commits IME composition through the input sink as one write", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const sink = screen.getByTestId("remote-terminal-input-sink");

    fireEvent.compositionStart(sink);
    fireEvent.compositionUpdate(sink, { data: "ㄱ" });
    fireEvent.input(sink, { target: { value: "ㄱ" } });
    fireEvent.compositionUpdate(sink, { data: "가" });
    fireEvent.input(sink, { target: { value: "가" } });
    fireEvent.compositionEnd(sink, { data: "가" });
    fireEvent.compositionStart(sink);
    fireEvent.input(sink, { target: { value: "나" } });
    fireEvent.compositionEnd(sink, { data: "나" });

    expect(socket().send).toHaveBeenCalledTimes(2);
    expect(socket().send).toHaveBeenNthCalledWith(1, new TextEncoder().encode("가"));
    expect(socket().send).toHaveBeenNthCalledWith(2, new TextEncoder().encode("나"));
    expect(sink).toHaveValue("");
  });

  it("renders and clears a local preedit overlay while composing", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const sink = screen.getByTestId("remote-terminal-input-sink");

    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    fireEvent.compositionStart(sink);
    fireEvent.input(sink, { target: { value: "ㄱ" } });
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("ㄱ");
    fireEvent.compositionEnd(sink, { data: "ㄱ" });
    // A commit made only of jamo is unfinished text: the syllable can still grow, so it stays in the
    // overlay instead of being shipped a jamo at a time (see the mobile IME suite).
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("ㄱ");
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.input(sink, { target: { value: " " } });
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("ㄱ "));
  });

  it("shows the build stamp in the options sheet, not in the phone header", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    expect(screen.queryByTestId("remote-terminal-build-stamp")).toBeNull();
    fireEvent.click(screen.getByTestId("remote-terminal-menu-button"));
    expect(screen.getByTestId("remote-terminal-build-stamp").textContent).not.toBe("");
  });

  it("keeps the build stamp in the header of the desktop compatibility renderer", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" showKeyDock={false} />);
    expect(screen.getByTestId("remote-terminal-build-stamp").textContent).not.toBe("");
  });

  it("sends non-composing sink input as text without keydown duplication", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const sink = screen.getByTestId("remote-terminal-input-sink");

    fireEvent.input(sink, { target: { value: "ls" } });
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("ls"));

    fireEvent.keyDown(surface(), { key: "a" });
    expect(socket().send).toHaveBeenNthCalledWith(2, new TextEncoder().encode("a"));
    expect(sink).toHaveValue("");
  });

  it("snaps Hangul runs and the cursor overlay onto exact cell boundaries", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    act(() => {
      socket().onmessage?.({
        data: JSON.stringify({
          type: "gridDiff",
          cols: 12,
          rows: 2,
          cursor: { ...hiddenCursor, x: 4, y: 0, visible: true },
          lines: [{ index: 0, runs: [{ text: "한글", fg: null, bg: null, attrs: 0, cells: 4 }] }],
        }),
      } as MessageEvent);
    });

    const runSpan = surface().querySelector('[data-grid-line="0"] > span');
    expect(runSpan).toHaveStyle({ display: "inline-block", width: "40px" });
    expect(surface().querySelector('[data-terminal-cursor="true"]')).toHaveStyle({
      transform: "translate(40px, 0px)",
    });
  });

  it("preserves control modifiers for physical navigation keys", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "ArrowUp", ctrlKey: true });

    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001b[1;5A"));
  });

  it("keeps MobileKeyDock modified navigation wiring", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    // Ctrl as a one-shot modifier, then the dock's Left arrow: the pair sends Ctrl-Left.
    fireEvent.click(screen.getByRole("button", { name: "Ctrl" }));
    fireEvent.click(screen.getByRole("button", { name: "Left" }));

    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001b[1;5D"));
  });

  it("resizes after the initial handshake only when viewport geometry changes", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    let surfaceWidth = 800;
    let resizeCallback: ResizeObserverCallback | null = null;

    class MockResizeObserver {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback;
      }
      observe() {}
      disconnect() {}
      unobserve() {}
    }

    vi.stubGlobal("ResizeObserver", MockResizeObserver);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(surfaceWidth, 400);
      return rect(0, 0);
    });

    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    expect(socket().url).toMatch(/&cols=80&rows=20$/);
    act(() => socket().onopen?.());
    expect(socket().send).not.toHaveBeenCalled();

    act(() => resizeCallback?.([], {} as ResizeObserver));
    expect(socket().send).not.toHaveBeenCalled();

    surfaceWidth = 640;
    act(() => resizeCallback?.([], {} as ResizeObserver));
    await waitFor(() => {
      expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "resize", cols: 64, rows: 20 }));
    });
  });

  it("renders surface with overflow-hidden to prevent layout scrollbars", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    expect(surface()).toHaveClass("overflow-hidden");
    expect(surface()).not.toHaveClass("overflow-auto");
  });

  it("sends clamped scroll message on wheel events over an open socket", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    act(() => socket().onopen?.());
    socket().send.mockClear();

    // Positive deltaY (scrolling down toward newer content)
    fireEvent.wheel(surface(), { deltaY: 60 });
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: 3 }));

    // Two negative half-row pixel events produce one older-content row.
    fireEvent.wheel(surface(), { deltaY: -10 });
    expect(socket().send).toHaveBeenCalledTimes(1);
    fireEvent.wheel(surface(), { deltaY: -10 });
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: -1 }));

    // Large deltaY clamped to max 10
    fireEvent.wheel(surface(), { deltaY: 500 });
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: 10 }));

    // Large negative deltaY clamped to min -10
    fireEvent.wheel(surface(), { deltaY: -500 });
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: -10 }));

    // Zero deltaY sends nothing
    socket().send.mockClear();
    fireEvent.wheel(surface(), { deltaY: 0 });
    expect(socket().send).not.toHaveBeenCalled();
  });

  it.each([
    [1, 1, 7, 3],
    [1, -1, 7, -3],
    [1, 100, 7, 10],
    [1, -100, 7, -10],
    [2, 1, 7, 7],
    [2, -1, 7, -7],
    [2, 1, 24, 10],
    [2, -1, 24, -10],
  ])("normalizes wheel mode %i delta %i using frame rows %i to %i wire rows", (deltaMode, deltaY, frameRows, rows) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();
    render(<RemoteTerminal sessionId="session-123" token="token-abc" onSocketLifecycle={onSocketLifecycle} />);
    const current = socket();
    if (!current.onopen || !current.onmessage) throw new Error("Expected socket event subscriptions");
    act(() => {
      current.onopen?.();
      current.onmessage?.(new MessageEvent("message", { data: JSON.stringify({
        type: "grid", cols: 80, rows: frameRows, cursor: hiddenCursor, lines: [],
      }) }));
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-123", "open");
    expect(surface().querySelectorAll("[data-grid-line]")).toHaveLength(frameRows);

    fireEvent.wheel(surface(), { deltaMode, deltaY });

    expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows })]]);
  });

  it.each([1, -1])("accumulates fractional pixels in direction %i without zero or horizontal input consuming them", (sign) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    act(() => socket().onopen?.());

    fireEvent.wheel(surface(), { deltaY: sign * 9.5 });
    fireEvent.wheel(surface(), { deltaY: 0, deltaMode: 2 });
    fireEvent.wheel(surface(), { deltaX: 100, deltaY: 0, deltaMode: 1 });
    fireEvent.wheel(surface(), { deltaY: sign * 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: sign * 0.5 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: sign })]]);
    socket().send.mockClear();
    fireEvent.wheel(surface(), { deltaY: sign * 500 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: sign * 10 })]]);
    socket().send.mockClear();
    fireEvent.wheel(surface(), { deltaY: sign * 1 });
    expect(socket().send).not.toHaveBeenCalled();
  });

  it.each(["session", "socket", "visibility"])("discards fractional wheel remainder across a %s transition", (transition) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const view = render(<RemoteTerminal sessionId="session-a" token="token-abc" />);
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    socket().send.mockClear();

    if (transition === "session") {
      view.rerender(<RemoteTerminal sessionId="session-b" token="token-abc" />);
      act(() => socket().onopen?.());
    } else if (transition === "socket") {
      view.rerender(<RemoteTerminal sessionId="session-a" token="token-abc" transportUrl="http://127.0.0.1" />);
      act(() => socket().onopen?.());
    } else {
      const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      fireEvent(document, new Event("visibilitychange"));
      fireEvent.wheel(surface(), { deltaY: 10 });
      expect(socket().send).not.toHaveBeenCalled();
      visibility.mockReturnValue("visible");
      fireEvent(document, new Event("visibilitychange"));
    }

    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: 1 })]]);
  });

  it.each([0, 2, 3])("does not accumulate or send wheel input with socket readyState %i", (readyState) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    socket().send.mockClear();
    socket().readyState = readyState;
    fireEvent.wheel(surface(), { deltaY: 10 });
    fireEvent.wheel(surface(), { deltaY: 1, deltaMode: 1 });
    expect(socket().send).not.toHaveBeenCalled();
    socket().readyState = MockWebSocket.OPEN;
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: 1 })]]);
  });

  it("auto-focuses its input sink on mount and when activeTabId changes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const view = render(
      <RemoteTerminal sessionId="session-123" token="token-abc" activeTabId="tab-a" />,
    );

    const sink = screen.getByTestId("remote-terminal-input-sink");
    expect(document.activeElement).toBe(sink);

    // Move focus elsewhere, then switch tabs: focus must return to the sink.
    sink.blur();
    expect(document.activeElement).not.toBe(sink);

    view.rerender(
      <RemoteTerminal sessionId="session-123" token="token-abc" activeTabId="tab-b" />,
    );
    expect(document.activeElement).toBe(screen.getByTestId("remote-terminal-input-sink"));
  });

  it("re-focuses its input sink when re-rendered with a new sessionId", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const view = render(
      <RemoteTerminal sessionId="session-1" token="token-abc" activeTabId="tab-1" />,
    );

    const sink = screen.getByTestId("remote-terminal-input-sink");
    expect(document.activeElement).toBe(sink);

    sink.blur();
    expect(document.activeElement).not.toBe(sink);

    view.rerender(
      <RemoteTerminal sessionId="session-2" token="token-abc" activeTabId="tab-2" />,
    );
    expect(document.activeElement).toBe(screen.getByTestId("remote-terminal-input-sink"));
  });

  it("does not send scroll message on wheel event when socket is not open", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    // Socket readyState is CONNECTING (0) before onopen
    socket().readyState = 0;
    fireEvent.wheel(surface(), { deltaY: 60 });
    expect(socket().send).not.toHaveBeenCalledWith(expect.stringContaining('"type":"scroll"'));
  });

  it("pastes literal text that collides with key-command names verbatim, not as key commands", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const target = surface();

    const literals = ["up", "tab", "delete", "ctrl-c", "alt-x", "ctrl-d"];
    for (const literal of literals) {
      socket().send.mockClear();
      fireEvent.paste(target, { clipboardData: { getData: () => literal } });
      expect(socket().send).toHaveBeenCalledTimes(1);
      expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(literal));
    }
  });

  it("pastes multiline text whose lines collide with key names as verbatim bracketed paste", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.paste(surface(), {
      clipboardData: { getData: () => "up\r\ntab\rdelete" },
    });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(
      new TextEncoder().encode("\x1b[200~up\ntab\ndelete\x1b[201~"),
    );
  });

  it("pastes Object.prototype key names verbatim without leaking inherited members", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();
    const target = surface();

    for (const literal of ["toString", "constructor", "hasOwnProperty"]) {
      socket().send.mockClear();
      fireEvent.paste(target, { clipboardData: { getData: () => literal } });
      expect(socket().send).toHaveBeenCalledTimes(1);
      expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(literal));
    }
  });

  it("pastes non-ASCII unicode literals verbatim without control-byte mangling", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.paste(surface(), { clipboardData: { getData: () => "안녕 ㅊ" } });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("안녕 ㅊ"));
  });

  it("maps Korean hardware Ctrl+C (key jamo, code KeyC) to a structured interrupt", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "ㅊ", code: "KeyC", ctrlKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", signal: "interrupt" }),
    );
  });

  it("maps Korean hardware Ctrl+D / Ctrl+L to physical ASCII control codes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "ㅇ", code: "KeyD", ctrlKey: true });
    expect(socket().send).toHaveBeenNthCalledWith(1, new Uint8Array([4]));

    fireEvent.keyDown(surface(), { key: "ㅣ", code: "KeyL", ctrlKey: true });
    expect(socket().send).toHaveBeenNthCalledWith(2, new Uint8Array([12]));
  });

  it("lets hardware Ctrl+V fall through to the browser paste event instead of sending a control byte", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const event = fireEvent.keyDown(surface(), { key: "ㅍ", code: "KeyV", ctrlKey: true });

    expect(socket().send).not.toHaveBeenCalled();
    // Not prevented -> the browser's native paste event can still run.
    expect(event).toBe(true);
  });

  it("preserves Ctrl+Shift+C for browser copy instead of sending an interrupt", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const event = fireEvent.keyDown(surface(), {
      key: "C",
      code: "KeyC",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(socket().send).not.toHaveBeenCalled();
    expect(event).toBe(true);
  });

  it("does not send CR for Enter while an IME composition is active", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const sink = screen.getByTestId("remote-terminal-input-sink");

    fireEvent.compositionStart(sink);
    // Platform where nativeEvent.isComposing is not reflected on the keydown:
    // the ref must still guard Enter from emitting a premature carriage return.
    fireEvent.keyDown(surface(), { key: "Enter" });

    expect(socket().send).not.toHaveBeenCalled();
  });

  it("does not send CR for Enter reported with IME keyCode 229", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "Enter", keyCode: 229 });

    expect(socket().send).not.toHaveBeenCalled();
  });

  it("encodes hardware Alt+letter as an ESC-prefixed meta chord instead of dropping it", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "x", code: "KeyX", altKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bx"));
  });

  it("encodes Korean hardware Alt+letter via the physical code, not the remapped jamo", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "ㅌ", code: "KeyX", altKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bx"));
  });

  it("preserves Shift case for Alt+letter meta chords (Alt+Shift+X -> ESC X)", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "X", code: "KeyX", altKey: true, shiftKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bX"));
  });

  it("hands AltGraph printable keys to the input sink so the glyph is emitted once", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    // getModifierState('AltGraph') is the authoritative printable-glyph signal.
    // jsdom ignores getModifierState passed via the init dict, so define it on a
    // real event. ctrlKey is false here (a valid AltGr-only report).
    const event = new KeyboardEvent("keydown", {
      key: "@",
      code: "Digit2",
      altKey: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(event, "getModifierState", {
      value: (mod: string) => mod === "AltGraph",
    });
    fireEvent(surface(), event);

    // No synthesized chord and no direct send: the focused sink's InputEvent owns
    // the glyph, and the keystroke is not canceled.
    expect(socket().send).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(screen.getByTestId("remote-terminal-input-sink"));
  });

  it("encodes Shift+Tab as a CSI back-tab sequence rather than a plain Tab", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    fireEvent.keyDown(surface(), { key: "Tab", shiftKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001b[Z"));
  });

  it("never wraps an unsupported Ctrl+digit chord into a junk control byte", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    establishReady();

    // Ctrl+9 has no ASCII control code; it must emit nothing, not a garbage byte.
    fireEvent.keyDown(surface(), { key: "9", code: "Digit9", ctrlKey: true });
    expect(socket().send).not.toHaveBeenCalled();

    // Ctrl+3 maps to ESC (0x1b) in the standard control table.
    fireEvent.keyDown(surface(), { key: "3", code: "Digit3", ctrlKey: true });
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new Uint8Array([27]));
  });

  it("observably rejects input and displays overflow status when outbound buffer limit is exceeded while connecting", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onInputOverflow = vi.fn();
    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onInputOverflow={onInputOverflow}
      />,
    );

    // Establish terminal readiness first
    establishReady();

    // Socket is in CONNECTING state (readyState = 0, before onopen)
    socket().readyState = 0;

    // Fill buffer up to MAX_OUTBOUND_BUFFER_BYTES (4096 bytes)
    const chunk4096 = "x".repeat(4096);
    fireEvent.paste(surface(), { clipboardData: { getData: () => chunk4096 } });

    // Verify buffered indicator is displayed and overflow indicator is not displayed yet
    expect(screen.getByTestId("remote-terminal-buffered-indicator")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).not.toBeInTheDocument();
    expect(onInputOverflow).not.toHaveBeenCalled();

    // Drive RemoteTerminal's own queue past the bound with an extra keystroke
    fireEvent.keyDown(surface(), { key: "a" });

    // Assert that overflow is now observable via both UI indicator and callback
    expect(screen.getByTestId("remote-terminal-overflow-indicator")).toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal-overflow-indicator")).toHaveTextContent("Some typing was dropped");
    expect(onInputOverflow).toHaveBeenCalledWith(1);

    // Assert that socket send was not called while connecting
    expect(socket().send).not.toHaveBeenCalled();

    // When the socket transitions to OPEN, only the buffered 4096 bytes are flushed; the overflowed byte was dropped
    socket().readyState = MockWebSocket.OPEN;
    act(() => socket().onopen?.());
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(chunk4096));
    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).not.toBeInTheDocument();
  });

  it("tracks generation from remoteStatus, emits generation-fenced remoteResize on status and resize, and resets on reconnect", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    let resizeObserverCallback: (() => void) | null = null;
    class MockResizeObserver {
      constructor(cb: () => void) {
        resizeObserverCallback = cb;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    render(<RemoteTerminal sessionId="session-ssh" token="token-abc" />);

    // Initial WebSocket request contains initial geometry 80x20
    const ws1 = socket();
    act(() => {
      ws1.onopen?.();
      ws1.onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
    });
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Live"));

    // Server sends remoteStatus with generation "101"
    act(() => {
      ws1.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "101",
        }),
      } as MessageEvent);
    });

    // RemoteTerminal should immediately send generation-fenced remoteResize with the measured geometry (80x20)
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "101",
        cols: 80,
        rows: 20,
      }),
    );

    // Viewport resizes to 1000x500 (100 cols, 25 rows)
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(1000, 500);
      return rect(0, 0);
    });

    act(() => {
      resizeObserverCallback?.();
    });

    // Verify remoteResize is sent with the new geometry and generation 101
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "101",
        cols: 100,
        rows: 25,
      }),
    );

    // Generation updates to "102"
    act(() => {
      ws1.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "102",
        }),
      } as MessageEvent);
    });

    // Should immediately send remoteResize with generation "102" and latest measured geometry (100x25)
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "102",
        cols: 100,
        rows: 25,
      }),
    );
  });

  it("routes typed input and ctrl-c dock key through generation-fenced remoteWrite on SSH sessions while preserving raw binary input on local sessions", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    const sshView = render(<RemoteTerminal sessionId="session-ssh" token="token-abc" />);
    const sshWs = socket();
    act(() => {
      sshWs.onopen?.();
      sshWs.onmessage?.({
        data: JSON.stringify({
          type: "grid",
          cols: 80,
          rows: 20,
          cursor: hiddenCursor,
          lines: [],
        }),
      });
    });
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Live"));

    // Server sends remoteStatus with generation "202"
    act(() => {
      sshWs.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "202",
        }),
      } as MessageEvent);
    });

    sshWs.send.mockClear();

    // Simulate typed character "a" on SSH session
    fireEvent.keyDown(surface(), { key: "a" });
    expect(sshWs.send).toHaveBeenCalledTimes(1);
    expect(sshWs.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteWrite",
        generation: "202",
        data: "a",
      }),
    );

    sshWs.send.mockClear();

    // Assert the ctrl-c dock key sends data: "\u0003" as remoteWrite in the same generation state
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    expect(sshWs.send).toHaveBeenCalledTimes(1);
    expect(sshWs.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteWrite",
        generation: "202",
        data: "\u0003",
      }),
    );

    sshView.unmount();

    // Without any remoteStatus (local session), typed character is still sent as raw bytes
    render(<RemoteTerminal sessionId="session-local" token="token-abc" />);
    const localWs = socket();
    establishReady(localWs);
    await waitFor(() => expect(connectionStatus()).toHaveTextContent("Live"));

    localWs.send.mockClear();
    fireEvent.keyDown(surface(), { key: "a" });
    expect(localWs.send).toHaveBeenCalledTimes(1);
    expect(localWs.send).toHaveBeenCalledWith(new TextEncoder().encode("a"));
  });

  it("renders T3-style header with icon back chevron, Terminal title, and title subtitle in both embedded and standard modes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onBack = vi.fn();
    const { rerender } = render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        title="ferryx-ui · Local"
        onBack={onBack}
        embedded={true}
      />,
    );

    const backButton = screen.getByTestId("remote-terminal-back");
    expect(backButton).toHaveAttribute("aria-label", "Back to chat");
    fireEvent.click(backButton);
    expect(onBack).toHaveBeenCalledTimes(1);

    expect(screen.getByText("Terminal")).toBeInTheDocument();
    expect(screen.getByText("ferryx-ui · Local")).toBeInTheDocument();

    rerender(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onBack={onBack}
        embedded={false}
      />,
    );
    expect(screen.getByText("Terminal")).toBeInTheDocument();
    expect(screen.queryByText("Desktop terminal")).not.toBeInTheDocument();
  });

  it("degrades safely when onBack is not provided in both embedded and standard modes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    const { rerender } = render(
      <RemoteTerminal
        sessionId="session-edge-1"
        token="token-abc"
        embedded={false}
      />,
    );

    expect(screen.getByText("Terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-back")).toBeNull();
    expect(screen.queryByText("Desktop terminal")).not.toBeInTheDocument();

    rerender(
      <RemoteTerminal
        sessionId="session-edge-1"
        token="token-abc"
        embedded={true}
      />,
    );

    expect(screen.getByText("Terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-back")).toBeNull();
    expect(screen.queryByText("Desktop terminal")).not.toBeInTheDocument();
  });

  it("preserves terminal grid container full-bleed layout and embedded indicator wiring", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    const { rerender } = render(
      <RemoteTerminal
        sessionId="session-grid-1"
        token="token-abc"
        embedded={true}
      />,
    );

    const grid = screen.getByTestId("remote-terminal-grid");
    expect(grid).toBeInTheDocument();
    expect(grid.className).toContain("relative");
    expect(grid.className).toContain("min-h-0");
    expect(grid.className).toContain("flex-1");
    expect(grid.className).toContain("overflow-hidden");
    expect(grid.className).toContain("bg-terminal");

    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).toBeNull();
    expect(screen.queryByTestId("remote-terminal-buffered-indicator")).toBeNull();

    rerender(
      <RemoteTerminal
        sessionId="session-grid-1"
        token="token-abc"
        embedded={false}
      />,
    );

    const standardGrid = screen.getByTestId("remote-terminal-grid");
    expect(standardGrid).toBeInTheDocument();
    expect(standardGrid.className).toContain("relative");
    expect(standardGrid.className).toContain("min-h-0");
    expect(standardGrid.className).toContain("flex-1");
    expect(standardGrid.className).toContain("overflow-hidden");
    expect(standardGrid.className).toContain("bg-terminal");

    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).toBeNull();
    expect(screen.queryByTestId("remote-terminal-buffered-indicator")).toBeNull();
  });

  it("enables horizontal scrolling container styles when followHostSize is true", () => {
    render(
      <RemoteTerminal
        sessionId="session-grid-follow"
        token="token-abc"
        embedded={true}
        followHostSize={true}
      />,
    );

    const followGrid = screen.getByTestId("remote-terminal-grid");
    expect(followGrid).toBeInTheDocument();
    expect(followGrid.className).toContain("overflow-x-auto");
    expect(followGrid.className).toContain("overflow-y-hidden");
    expect(followGrid.style.touchAction).toBe("pan-x");
  });

  function attachedFrame(generation: unknown, epoch = "epoch-1"): string {
    return JSON.stringify({
      type: "attached",
      target: { machineId: "mach-1", sessionId: "session-machine-1", daemonEpoch: epoch },
      generation,
      cols: 80,
      rows: 20,
      startSequence: 0,
      endSequence: 0,
      replayGap: null,
    });
  }

  function renderMachineTerminal(epoch: string, props?: Partial<React.ComponentProps<typeof RemoteTerminal>>) {
    const sockets: MockWebSocket[] = [];
    const paths: string[] = [];
    const createWebSocket = vi.fn((pathAndQuery: string) => {
      paths.push(pathAndQuery);
      const ws = new MockWebSocket("mock://machine");
      sockets.push(ws);
      return ws;
    });
    const view = render(
      <RemoteTerminal
        sessionId="session-machine-1"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch={epoch}
        createWebSocket={createWebSocket}
        {...props}
      />,
    );
    return { createWebSocket, paths, sockets, ...view };
  }

  it("observes actual handshake and retry handles without accepting missing lifetime evidence", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("__task9Wire", {});
    const target = { machineId: "mach-1", sessionId: "session-machine-1", daemonEpoch: "epoch-qa-timers" };
    const view = renderMachineTerminal(target.daemonEpoch);
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    expect(qa.snapshot(target)).toMatchObject({ pendingTimers: 1, inFlight: true,
      identityConfirmed: false, exitSettlements: null, disposed: false });
    expect(() => qa.currentTarget()).toThrow("WEB_OWNER_CURRENT_BINDING_EVIDENCE_REQUIRED");
    expect(() => qa.snapshot({ ...target, incarnation: "not-observed" }))
      .toThrow("WEB_OWNER_TARGET_EVIDENCE_REQUIRED");
    act(() => view.sockets[0].onerror?.());
    expect(qa.snapshot(target)).toMatchObject({ pendingTimers: 1, inFlight: false });
    view.unmount();
    expect(qa.snapshot(target)).toMatchObject({ pendingTimers: 0, inFlight: false, disposed: true });
    expect(() => qa.snapshot({ ...target, sessionId: "absent" }))
      .toThrow("WEB_OWNER_TARGET_EVIDENCE_REQUIRED");
  });

  it("retains exact disposed owner rejection before and after stale completion subscription", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("__task9Wire", {});
    const target = { sessionId: "session-machine-1", daemonEpoch: "epoch-qa-late" };
    let deliver: ((ws: MockWebSocket) => void) | undefined;
    const completion = new Promise<MockWebSocket>(resolve => { deliver = resolve; });
    const view = renderMachineTerminal(target.daemonEpoch, { createWebSocket: () => completion });
    const qa = window.__ferryxRecoveryQA;
    if (!qa || !deliver) throw new Error("Expected held owner completion");
    view.unmount();
    const rejection = qa.waitForStaleCompletion(target);
    const late = new MockWebSocket("mock://qa-late");
    await act(async () => { deliver?.(late); await completion; });
    expect(await rejection).toMatchObject({ staleCompletionsRejected: 1,
      cancelledAttemptIds: ["1"], pendingTimers: 0, disposed: true });
    expect(late.close).toHaveBeenCalledTimes(1);
    expect(await qa.waitForStaleCompletion(target)).toMatchObject({ staleCompletionsRejected: 1 });
  });

  it("keeps an exact retired-attempt observer alive across disposal until late completion", async () => {
    vi.stubGlobal("__task9Wire", {});
    const target = { sessionId: "session-machine-1", daemonEpoch: "epoch-retired-observer" };
    let deliver: ((ws: MockWebSocket) => void) | undefined;
    const completion = new Promise<MockWebSocket>(resolve => { deliver = resolve; });
    const view = renderMachineTerminal(target.daemonEpoch, { createWebSocket: () => completion });
    const qa = window.__ferryxRecoveryQA;
    if (!qa || !deliver) throw new Error("Expected retained-owner observer facade");
    const observed = qa.waitForRetiredStaleCompletion(target, "1", 1000);
    view.unmount();
    expect(qa.snapshot(target)).toMatchObject({ disposed: true, pendingTimers: 0, inFlight: false,
      staleCompletionsRejected: 0, cancelledAttemptIds: ["1"] });
    const late = new MockWebSocket("mock://retired-late");
    await act(async () => { deliver?.(late); await completion; });
    await expect(observed).resolves.toMatchObject({ disposed: true, pendingTimers: 0, inFlight: false,
      staleCompletionsRejected: 1, cancelledAttemptIds: ["1"], rejectedAttemptIds: ["1"] });
    expect(late.close).toHaveBeenCalledTimes(1);
  });

  it("rejects pending observation subscriptions on exact owner disposal", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({}), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    })));
    vi.stubGlobal("__task9Wire", {});
    const target = { sessionId: "session-machine-1", daemonEpoch: "epoch-qa-dispose" };
    const view = renderMachineTerminal(target.daemonEpoch);
    const qa = window.__ferryxRecoveryQA;
    if (!qa) throw new Error("Expected QA owner facade");
    vi.useFakeTimers();
    expect(vi.getTimerCount()).toBe(0);
    let rejection: unknown;
    const rejected = qa.waitForStaleCompletion(target).catch(error => { rejection = error; });
    expect(vi.getTimerCount()).toBe(1);
    view.unmount();
    await rejected;
    expect(rejection).toMatchObject({ message: "WEB_OWNER_OBSERVATION_DISPOSED" });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("R1: closes a socket handed over after the pane unmounted instead of leaking it", async () => {
    let deliver!: (ws: MockWebSocket) => void;
    const createWebSocket = vi.fn(() => new Promise<MockWebSocket>((resolve) => { deliver = resolve; }));
    const view = render(
      <RemoteTerminal
        sessionId="session-machine-late"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch="epoch-1"
        createWebSocket={createWebSocket as never}
      />,
    );
    await waitFor(() => expect(createWebSocket).toHaveBeenCalledTimes(1));
    view.unmount();
    const late = new MockWebSocket("mock://late");
    await act(async () => { deliver(late); });
    expect(late.close).toHaveBeenCalledTimes(1);
    expect(late.onopen).toBeNull();
  });

  it("holds machine controls until attached, fences them with its generation, and keeps PTY input binary", async () => {
    const { createWebSocket, paths, sockets } = renderMachineTerminal("epoch-1");
    await waitFor(() => expect(createWebSocket).toHaveBeenCalledTimes(1));
    expect(paths[0]).toBe("/api/v1/terminal/session-machine-1?daemonEpoch=epoch-1&render=grid");

    const ws = sockets[0];
    act(() => ws.onopen?.());
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");

    // Before the boundary, no unfenced control may reach the machine wire.
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    fireEvent.wheel(surface(), { deltaY: -20 });
    expect(ws.send).not.toHaveBeenCalled();

    act(() => {
      ws.onmessage?.({ data: attachedFrame("7") } as MessageEvent);
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));

    // The boundary drives resize with the attached generation and measured geometry.
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "7", cols: 80, rows: 20 }),
    );
    // Held interrupt and scroll flush fenced with the same generation.
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", generation: "7", signal: "interrupt" }),
    );
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "scroll", generation: "7", rows: -1 }),
    );
    // Machine generation never turns PTY input into legacy remoteWrite.
    expect(ws.send).not.toHaveBeenCalledWith(expect.stringContaining("remoteWrite"));

    ws.send.mockClear();
    fireEvent.keyDown(surface(), { key: "a" });
    expect(ws.send).toHaveBeenCalledTimes(1);
    const [payload] = ws.send.mock.calls[0];
    expect(Object.prototype.toString.call(payload)).toBe("[object Uint8Array]");
    expect(Array.from(payload as Uint8Array)).toEqual(Array.from(new TextEncoder().encode("a")));

    // Post-boundary controls stay fenced and scroll keeps its wheel clamp.
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", generation: "7", signal: "interrupt" }),
    );
    fireEvent.wheel(surface(), { deltaY: -20 });
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "scroll", generation: "7", rows: -1 }),
    );
  });

  it("stays deferred when attached carries a non-canonical generation", async () => {
    const { sockets } = renderMachineTerminal("epoch-1");
    await waitFor(() => expect(sockets).toHaveLength(1));
    const ws = sockets[0];
    act(() => ws.onopen?.());
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");

    const boundary = (generation: unknown) =>
      JSON.stringify({
        type: "attached",
        target: { machineId: "mach-1", sessionId: "session-machine-1", daemonEpoch: "epoch-1" },
        generation,
        cols: 80,
        rows: 20,
        startSequence: 0,
        endSequence: 0,
        replayGap: null,
      });

    act(() => ws.onmessage?.({ data: boundary(7) } as MessageEvent));
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    expect(ws.send).not.toHaveBeenCalled();

    act(() => ws.onmessage?.({ data: boundary("07") } as MessageEvent));
    expect(ws.send).not.toHaveBeenCalled();

    act(() => ws.onmessage?.({ data: boundary("11") } as MessageEvent));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "11", cols: 80, rows: 20 }),
    );
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", generation: "11", signal: "interrupt" }),
    );
  });

  it("ignores an attached boundary addressed to another session or epoch", async () => {
    const { sockets } = renderMachineTerminal("epoch-1");
    await waitFor(() => expect(sockets).toHaveLength(1));
    const ws = sockets[0];
    act(() => ws.onopen?.());
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");

    const wrongSession = JSON.stringify({
      type: "attached",
      target: { machineId: "mach-1", sessionId: "session-OTHER", daemonEpoch: "epoch-1" },
      generation: "5",
      cols: 80,
      rows: 20,
      startSequence: 0,
      endSequence: 0,
      replayGap: null,
    });
    const wrongEpoch = attachedFrame("6", "epoch-9");

    act(() => ws.onmessage?.({ data: wrongSession } as MessageEvent));
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    expect(ws.send).not.toHaveBeenCalled();

    act(() => ws.onmessage?.({ data: wrongEpoch } as MessageEvent));
    expect(ws.send).not.toHaveBeenCalled();

    act(() => ws.onmessage?.({ data: attachedFrame("7") } as MessageEvent));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "7", cols: 80, rows: 20 }),
    );
    expect(ws.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", generation: "7", signal: "interrupt" }),
    );
    expect(ws.send).not.toHaveBeenCalledWith(expect.stringContaining("\"generation\":\"5\""));
    expect(ws.send).not.toHaveBeenCalledWith(expect.stringContaining("\"generation\":\"6\""));
  });

  it("drops the replaced epoch's machine generation instead of fencing the new target's socket", async () => {
    const { createWebSocket, paths, sockets, rerender } = renderMachineTerminal("epoch-1");
    await waitFor(() => expect(paths).toHaveLength(1));
    const ws1 = sockets[0];
    act(() => {
      ws1.onopen?.();
      ws1.onmessage?.({ data: attachedFrame("7") } as MessageEvent);
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    act(() => {
      ws1.onmessage?.({ data: attachedFrame("7") } as MessageEvent);
    });
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "7", cols: 80, rows: 20 }),
    );
    ws1.send.mockClear();

    rerender(
      <RemoteTerminal
        sessionId="session-machine-1"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch="epoch-2"
        createWebSocket={createWebSocket}
      />,
    );
    await waitFor(() => expect(paths).toHaveLength(2));
    expect(paths[1]).toBe("/api/v1/terminal/session-machine-1?daemonEpoch=epoch-2&render=grid");

    const ws2 = sockets[1];
    act(() => {
      ws2.onopen?.();
      ws2.onmessage?.({ data: attachedFrame("9", "epoch-2") } as MessageEvent);
    });
    await waitFor(() => expect(connectionStatus()).toHaveTextContent("Live"));
    expect(ws2.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "9", cols: 80, rows: 20 }),
    );
    ws2.send.mockClear();

    // The replaced epoch's generation must not fence the new target: input stays binary.
    fireEvent.keyDown(surface(), { key: "b" });
    expect(ws2.send).toHaveBeenCalledTimes(1);
    expect(Object.prototype.toString.call(ws2.send.mock.calls[0][0])).toBe("[object Uint8Array]");
    expect(Array.from(ws2.send.mock.calls[0][0] as Uint8Array)).toEqual(
      Array.from(new TextEncoder().encode("b")),
    );

    // The replacement's boundary re-fences controls with its own generation.
    act(() => {
      ws2.onmessage?.({ data: attachedFrame("9", "epoch-2") } as MessageEvent);
    });
    expect(ws2.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "9", cols: 80, rows: 20 }),
    );
    expect(ws2.send).not.toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", generation: "7", cols: 80, rows: 20 }),
    );
    expect(ws2.send).not.toHaveBeenCalledWith(expect.stringContaining("remoteWrite"));
  });

  it("renders standard h-11 title bar with back button and title by default, and h-7 strip with status cluster only when showTitleBar is false", () => {
    const onBack = vi.fn();
    const { rerender } = render(
      <RemoteTerminal
        sessionId="session-titlebar-test"
        token="token-abc"
        title="feature-branch"
        onBack={onBack}
        showTitleBar={true}
      />,
    );

    // Default / showTitleBar=true checks
    expect(screen.getByText("Terminal")).toBeInTheDocument();
    expect(screen.getByText("feature-branch")).toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal-back")).toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal-menu-button")).toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();
    const titleBarTrue = screen.getByTestId("remote-terminal-back").closest("div.flex.shrink-0");
    expect(titleBarTrue?.className).toContain("h-11");
    expect(titleBarTrue?.className).toContain("justify-between");

    // showTitleBar=false checks
    rerender(
      <RemoteTerminal
        sessionId="session-titlebar-test"
        token="token-abc"
        title="feature-branch"
        onBack={onBack}
        showTitleBar={false}
      />,
    );

    expect(screen.queryByText("Terminal")).not.toBeInTheDocument();
    expect(screen.queryByText("feature-branch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-back")).not.toBeInTheDocument();
    // Status cluster remains with identical testids and roles
    expect(screen.getByTestId("remote-terminal-menu-button")).toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();
    const titleBarFalse = screen.getByTestId("remote-terminal-menu-button").closest("div.flex.shrink-0");
    expect(titleBarFalse?.className).toContain("h-7");
    expect(titleBarFalse?.className).toContain("justify-end");
  });

  describe("Task 7 normative terminal recovery and readiness contract", () => {
    it("rejects missing machine target session or epoch", async () => {
      const { sockets } = renderMachineTerminal("epoch-1");
      expect(sockets).toHaveLength(1);
      const ws = sockets[0];
      await ws.waitForHandlers();
      act(() => ws.onopen?.());
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Missing sessionId in target
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "attached",
            generation: "1",
            target: { daemonEpoch: "epoch-1" },
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Missing daemonEpoch in target
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "attached",
            generation: "2",
            target: { sessionId: "session-machine-1" },
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Missing target object entirely
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "attached",
            generation: "3",
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");
    });

    it("rejects mismatched machine target session or epoch", async () => {
      const { sockets } = renderMachineTerminal("epoch-1");
      expect(sockets).toHaveLength(1);
      const ws = sockets[0];
      await ws.waitForHandlers();
      act(() => ws.onopen?.());

      // Mismatched sessionId
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "attached",
            generation: "1",
            target: { sessionId: "wrong-session", daemonEpoch: "epoch-1" },
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Mismatched daemonEpoch
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "attached",
            generation: "2",
            target: { sessionId: "session-machine-1", daemonEpoch: "wrong-epoch" },
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");
    });

    it("rejects remoteStatus-only on legacy gateway socket until full grid frame arrives", () => {
      vi.stubGlobal("WebSocket", MockWebSocket);
      render(<RemoteTerminal sessionId="session-legacy" token="token-abc" />);
      const ws = socket();

      act(() => ws.onopen?.());
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // remoteStatus frame alone does NOT establish readiness
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "remoteStatus",
            state: "connected",
            generation: "101",
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Once valid full grid frame arrives, readiness is established
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 20,
            cursor: hiddenCursor,
            lines: [],
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
    });

    it("rejects stale socket grid frames after reconnect", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("WebSocket", MockWebSocket);
      render(<RemoteTerminal sessionId="session-stale" token="token-abc" />);
      const ws1 = socket();

      act(() => ws1.onopen?.());
      // Simulate drop on ws1
      act(() => ws1.onclose?.());

      // Advance 1s backoff to allow redial
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });

      // ws2 connects
      const ws2 = socket();
      expect(ws2).not.toBe(ws1);
      act(() => ws2.onopen?.());

      // Late grid frame arrives from ws1
      act(() => {
        ws1.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 20,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "stale data", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      // ws2 is still Connecting and has not adopted ws1 grid
      expect(connectionStatus()).toHaveTextContent("Connecting");
      expect(surface()).not.toHaveTextContent("stale data");

      // Grid on ws2 establishes readiness
      act(() => {
        ws2.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 20,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "fresh data", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
      expect(surface()).toHaveTextContent("fresh data");
    });

    it("legacy current full grid exposes confirmed observer without invented incarnation and stale grid or machine missing attached does not", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("__task9Wire", {});
      let socketNumber = 0;
      class ObservedWebSocket extends MockWebSocket {
        readonly __task9Id = `socket-legacy-${++socketNumber}`;
      }
      vi.stubGlobal("WebSocket", ObservedWebSocket);

      // 1. Legacy gateway: full grid exposes confirmed observer with incarnation null
      const legacyTarget = { sessionId: "session-legacy-observer", daemonEpoch: null };
      const legacyView = render(<RemoteTerminal sessionId={legacyTarget.sessionId} token="token-legacy" />);
      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error("Expected QA owner facade");

      const wsLegacy = socket();
      act(() => wsLegacy.onopen?.());

      // Pre-grid: identityConfirmed is false, currentTarget throws
      expect(qa.snapshot(legacyTarget)).toMatchObject({ identityConfirmed: false, incarnation: null });
      expect(() => qa.currentTarget()).toThrow("WEB_OWNER_CURRENT_BINDING_EVIDENCE_REQUIRED");

      // Full grid arrives on current legacy socket: identity confirmed, incarnation is strictly null
      act(() => {
        wsLegacy.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 24,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "ready", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
      expect(qa.snapshot(legacyTarget)).toMatchObject({
        identityConfirmed: true,
        incarnation: null,
      });
      expect(qa.currentTarget()).toMatchObject({
        sessionId: legacyTarget.sessionId,
        daemonEpoch: null,
        socketId: "socket-legacy-1",
        incarnation: null,
        outcome: "ready",
      });

      // 2. Stale socket grid does not confirm identity
      act(() => wsLegacy.onclose?.());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      const wsLegacy2 = socket();
      expect(wsLegacy2).not.toBe(wsLegacy);
      act(() => wsLegacy2.onopen?.());

      // Stale grid arrives from wsLegacy while wsLegacy2 is pending
      expect(qa.snapshot(legacyTarget)).toMatchObject({ identityConfirmed: false });
      act(() => {
        wsLegacy.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 24,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "stale-frame", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      expect(qa.snapshot(legacyTarget)).toMatchObject({ identityConfirmed: false });
      expect(() => qa.currentTarget()).toThrow("WEB_OWNER_CURRENT_BINDING_EVIDENCE_REQUIRED");

      legacyView.unmount();

      // 3. Machine transport missing attached: grid frame does NOT confirm identity
      const machineEpoch = "epoch-obs-check";
      const machineTarget = { sessionId: "session-machine-1", daemonEpoch: machineEpoch };
      const machineView = renderMachineTerminal(machineEpoch);
      const wsMachine = machineView.sockets[0];
      (wsMachine as any).__task9Id = "socket-machine-1";
      await wsMachine.waitForHandlers();
      act(() => wsMachine.onopen?.());

      act(() => {
        wsMachine.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 24,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "early-grid", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");
      expect(qa.snapshot(machineTarget)).toMatchObject({
        identityConfirmed: false,
        incarnation: null,
      });
      expect(() => qa.currentTarget()).toThrow("WEB_OWNER_CURRENT_BINDING_EVIDENCE_REQUIRED");

      machineView.unmount();
    });

    it("renders authoritative null-generation affirmative_exit and blocked with accurate status without inventing generation and preserves retry gate", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("__task9Wire", {});
      const sockets: MockWebSocket[] = [];
      const createWebSocket = vi.fn(() => {
        const ws = new SynchronousCloseWebSocket("mock://null-gen-authoritative");
        sockets.push(ws);
        return ws;
      });
      const sessionId = "paired-null-authoritative";
      const view = render(
        <RemoteTerminal
          sessionId={sessionId}
          token="token-null-auth"
          isAccountSession
          daemonEpoch="epoch-auth-1"
          createWebSocket={createWebSocket}
        />,
      );
      await sockets[0].waitForHandlers();
      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error("Expected QA owner facade");
      const target = { sessionId, daemonEpoch: "epoch-auth-1" };

      // 1. Authoritative blocked outcome with null generation displays typed reason
      const blockedFrame = {
        type: "terminalOutcome",
        target: { ...target, backendSessionId: sessionId },
        incarnation: "inc-auth-1",
        generation: null,
        state: "blocked",
        blockedReason: "permission_denied",
        exitCode: null,
      };
      act(() => sockets[0].onmessage?.({ data: JSON.stringify(blockedFrame) }));

      const outcomeBanner = screen.getByTestId("remote-terminal-outcome");
      expect(outcomeBanner).toHaveTextContent("Your computer refused access to this terminal.");
      expect(outcomeBanner).not.toHaveTextContent("permission_denied");
      expect(screen.queryByTestId("remote-terminal-outcome-unknown")).toBeNull();
      const blockedSnap = qa.snapshot(target);
      expect(blockedSnap).toMatchObject({
        outcome: "blocked",
        incarnation: "inc-auth-1",
        pendingTimers: 0,
        inFlight: false,
      });

      // Retry gate: background timers do not trigger automatic redial while blocked
      await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
      expect(createWebSocket).toHaveBeenCalledTimes(1);

      // Explicit Retry clears blocked outcome and redials
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      });
      expect(createWebSocket).toHaveBeenCalledTimes(2);
      await sockets[1].waitForHandlers();

      // 2. Authoritative affirmative_exit outcome with null generation displays exit code
      const exitFrame = {
        type: "terminalOutcome",
        target: { ...target, backendSessionId: sessionId },
        incarnation: "inc-auth-1",
        generation: null,
        state: "affirmative_exit",
        blockedReason: null,
        exitCode: 42,
      };
      act(() => sockets[1].onmessage?.({ data: JSON.stringify(exitFrame) }));

      const exitBanner = screen.getByTestId("remote-terminal-outcome");
      expect(exitBanner).toHaveTextContent("This terminal has exited (code 42).");
      expect(screen.queryByTestId("remote-terminal-outcome-unknown")).toBeNull();
      const exitSnap = qa.snapshot(target);
      expect(exitSnap).toMatchObject({
        outcome: "affirmative_exit",
        exitSettlements: 1,
        pendingTimers: 0,
        inFlight: false,
      });

      // Retry gate: background timers do not auto-redial on exit
      await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
      expect(createWebSocket).toHaveBeenCalledTimes(2);

      // Explicit Retry starts a new attempt before another outcome can be accepted.
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      });
      expect(createWebSocket).toHaveBeenCalledTimes(3);
      await sockets[2].waitForHandlers();

      // 3. Non-authoritative outcome on the new attempt retains Recovery outcome unknown
      const nonAuthFrame = {
        type: "terminalOutcome",
        target: { ...target, backendSessionId: sessionId },
        incarnation: "inc-auth-1",
        generation: null,
        state: "uncertain_state",
        blockedReason: null,
        exitCode: null,
      };
      act(() => sockets[2].onmessage?.({ data: JSON.stringify(nonAuthFrame) }));
      expect(screen.getByTestId("remote-terminal-outcome-unknown")).toHaveTextContent("We couldn't confirm whether this terminal recovered.");
      expect(screen.queryByTestId("remote-terminal-outcome")).toBeNull();

      view.unmount();
    });

    it("enforces 10s handshake deadline when socket opens but never attaches", async () => {
      vi.useFakeTimers();
      const { sockets } = renderMachineTerminal("epoch-1");
      expect(sockets).toHaveLength(1);
      const ws = sockets[0];
      await ws.waitForHandlers();

      act(() => ws.onopen?.());
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Advance 9.9s: still connecting
      await act(async () => {
        await vi.advanceTimersByTimeAsync(9900);
      });
      expect(ws.close).not.toHaveBeenCalled();

      // Advance past 10s: handshake deadline expires, closes socket, schedules retry
      await act(async () => {
        await vi.advanceTimersByTimeAsync(200);
      });
      expect(ws.close).toHaveBeenCalled();

      // Advance backoff delay to allow redial
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(sockets.length).toBeGreaterThanOrEqual(2);
    });

    it("rejects pre-readiness user input without buffering uncertain mutation replay", () => {
      vi.stubGlobal("WebSocket", MockWebSocket);
      const onInputDrop = vi.fn();
      render(
        <RemoteTerminal
          sessionId="session-input-gate"
          token="token-abc"
          onInputDrop={onInputDrop}
        />
      );
      const ws = socket();
      act(() => ws.onopen?.());

      // Type character 'x' before terminal is ready
      fireEvent.keyDown(surface(), { key: "x" });
      expect(onInputDrop).toHaveBeenCalled();
      expect(ws.send).not.toHaveBeenCalled();

      // Grid arrives establishing readiness
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 20,
            cursor: hiddenCursor,
            lines: [],
          }),
        });
      });
      expect(connectionStatus()).toHaveTextContent("Live");

      // Ensure pre-readiness input was NOT buffered and replayed on readiness!
      expect(ws.send).not.toHaveBeenCalledWith(expect.stringContaining("x"));

      // Now that terminal is ready, input is delivered
      fireEvent.keyDown(surface(), { key: "y" });
      expect(ws.send).toHaveBeenCalled();
    });

    it("executes deterministic fake-clock backoff over >20 failure cycles saturating at 10s cap", async () => {
      vi.useFakeTimers();
      vi.spyOn(Math, "random").mockReturnValue(0); // delay = min(10000, 1000 * 2^attempt)
      const { sockets } = renderMachineTerminal("epoch-1");
      expect(sockets).toHaveLength(1);

      // Run 25 consecutive failure cycles
      for (let cycle = 0; cycle < 25; cycle++) {
        const currentWs = sockets[sockets.length - 1];
        await currentWs.waitForHandlers();
        // Simulate socket failure
        act(() => {
          currentWs.onerror?.(new Event("error"));
          currentWs.onclose?.(new CloseEvent("close"));
        });
        // The backoff delay should be at most 10,000ms
        await act(async () => {
          await vi.advanceTimersByTimeAsync(10000);
        });
        expect(sockets.length).toBe(cycle + 2);
      }

      // On the 26th socket, valid attached frame arrives
      const finalWs = sockets[sockets.length - 1];
      await finalWs.waitForHandlers();
      act(() => {
        finalWs.onopen?.();
        finalWs.onmessage?.({ data: attachedFrame("100") });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
    });

    it("resumes terminal recovery ownership when parent fallback is a no-op or fails", async () => {
      vi.useFakeTimers();
      vi.spyOn(Math, "random").mockReturnValue(0);
      const noopFallback = vi.fn(); // parent fallback callback does not switch transport

      const { sockets } = renderMachineTerminal("epoch-1", { onTransportFailure: noopFallback });
      expect(sockets).toHaveLength(1);
      const ws1 = sockets[0];
      await ws1.waitForHandlers();

      // Trigger failure
      act(() => {
        ws1.onerror?.(new Event("error"));
        ws1.onclose?.(new CloseEvent("close"));
      });
      expect(noopFallback).toHaveBeenCalledTimes(1);

      // Terminal must not be stuck; recovery ownership resumes and schedules retry
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(sockets.length).toBe(2);
    });

    it("retires failed attempt: late deferred socket resolved after deadline and before redial closes exact socket and does not publish readiness", async () => {
      vi.useFakeTimers();
      let resolveDeferredSocket: ((ws: MockWebSocket) => void) | null = null;
      const lateSocket = new MockWebSocket("mock://machine-late");

      const createWebSocket = vi.fn(() => {
        return new Promise<MockWebSocket>((resolve) => {
          resolveDeferredSocket = resolve;
        });
      });

      render(
        <RemoteTerminal
          sessionId="session-late-socket"
          token="token-unused"
          isAccountSession={true}
          daemonEpoch="epoch-1"
          createWebSocket={createWebSocket}
        />,
      );

      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Advance past 10s handshake deadline without resolving socket promise
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10001);
      });

      // Handshake deadline has fired and failure was handled.
      // Now, before redial delay expires, the late deferred socket resolves!
      await act(async () => {
        resolveDeferredSocket?.(lateSocket);
      });

      // The late exact socket must be closed immediately
      expect(lateSocket.close).toHaveBeenCalled();

      // Late attached frame delivered to late socket must NOT publish readiness
      act(() => {
        lateSocket.onmessage?.({ data: attachedFrame("99") } as MessageEvent);
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");
    });

    it("onerror without close event immediately closes and nulls exact socket, retrying without duplicate timers or stale frame adoption", async () => {
      vi.useFakeTimers();
      const { sockets } = renderMachineTerminal("epoch-1");
      expect(sockets).toHaveLength(1);
      const ws1 = sockets[0];
      await ws1.waitForHandlers();

      act(() => ws1.onopen?.());

      // Trigger error WITHOUT close event
      act(() => {
        ws1.onerror?.(new Event("error"));
      });

      // Exact socket must be closed immediately
      expect(ws1.close).toHaveBeenCalled();
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Late frame on errored socket must not be adopted
      act(() => {
        ws1.onmessage?.({ data: attachedFrame("50") } as MessageEvent);
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Next dial succeeds on backoff
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(sockets.length).toBe(2);
      const ws2 = sockets[1];
      await ws2.waitForHandlers();

      act(() => {
        ws2.onopen?.();
        ws2.onmessage?.({ data: attachedFrame("51") } as MessageEvent);
      });
      expect(connectionStatus()).toHaveTextContent("Live");
    });

    it("recover() on online/visibility wake when socket is OPEN but unattached immediately closes stalled socket and redials without waiting 10s", async () => {
      vi.useFakeTimers();
      const { sockets } = renderMachineTerminal("1");
      expect(sockets).toHaveLength(1);
      const ws1 = sockets[0];
      await ws1.waitForHandlers();

      // Socket opens, readyState is OPEN, but attachment has not arrived (handshake pending)
      act(() => {
        ws1.onopen?.();
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Visibility change fires while handshake is pending
      Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // Stalled unattached socket must be closed and redial initiated without waiting for 10s deadline
      expect(ws1.close).toHaveBeenCalledTimes(1);
      expect(sockets).toHaveLength(2);

      const ws2 = sockets[1];
      await ws2.waitForHandlers();
      act(() => {
        ws2.onopen?.();
        ws2.onmessage?.({ data: attachedFrame("2", "1") });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
    });

    it("recover() on online event when ticket/createPromise is pending invalidates prior attempt and immediately closes late-resolved socket", async () => {
      vi.useFakeTimers();
      let resolveTicket1: ((ws: MockWebSocket) => void) | null = null;
      let resolveTicket2: ((ws: MockWebSocket) => void) | null = null;
      let callCount = 0;

      const createWebSocket = vi.fn(() => {
        callCount++;
        if (callCount === 1) {
          return new Promise<MockWebSocket>((res) => { resolveTicket1 = res; });
        }
        return new Promise<MockWebSocket>((res) => { resolveTicket2 = res; });
      });

      render(
        <RemoteTerminal
          sessionId="session-machine-1"
          token="token-unused"
          isAccountSession={true}
          daemonEpoch="1"
          createWebSocket={createWebSocket}
        />
      );

      expect(createWebSocket).toHaveBeenCalledTimes(1);
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // While ticket 1 is pending (socketRef.current is null), an online wake event fires
      act(() => {
        window.dispatchEvent(new Event("online"));
      });

      // Prior pending dial must be invalidated and a fresh dial initiated immediately
      expect(createWebSocket).toHaveBeenCalledTimes(2);

      // If the late ticket 1 resolves now, it must be closed immediately and discarded
      const lateSocket = new MockWebSocket("mock://late-ticket-1");
      await act(async () => {
        resolveTicket1?.(lateSocket);
      });
      expect(lateSocket.close).toHaveBeenCalledTimes(1);

      // Active ticket 2 resolves
      const activeSocket = new MockWebSocket("mock://active-ticket-2");
      await act(async () => {
        resolveTicket2?.(activeSocket);
      });
      await activeSocket.waitForHandlers();
      act(() => {
        activeSocket.onopen?.();
        activeSocket.onmessage?.({ data: attachedFrame("1", "1") });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
    });

    it("invalidates dial sequence BEFORE closing old socket on wake so synchronous close cannot trigger fallback or duplicate retries", async () => {
      vi.useFakeTimers();
      const onTransportFailure = vi.fn();
      const sockets: MockWebSocket[] = [];

      class SynchronousCloseWs extends MockWebSocket {
        override close = vi.fn(() => {
          this.readyState = 3;
          this.onclose?.(new CloseEvent("close"));
        });
      }

      const createWebSocket = vi.fn(() => {
        if (sockets.length === 0) {
          const ws = new SynchronousCloseWs("mock://sync-close-ws");
          sockets.push(ws);
          return ws;
        }
        const ws = new MockWebSocket("mock://next-ws");
        sockets.push(ws);
        return ws;
      });

      render(
        <RemoteTerminal
          sessionId="session-machine-1"
          token="token-unused"
          isAccountSession={true}
          daemonEpoch="1"
          createWebSocket={createWebSocket}
          onTransportFailure={onTransportFailure}
        />
      );

      expect(sockets).toHaveLength(1);
      const ws1 = sockets[0];
      await ws1.waitForHandlers();

      act(() => {
        ws1.onopen?.();
      });

      // Wake event fires
      act(() => {
        window.dispatchEvent(new Event("online"));
      });

      // Synchronous onclose on ws1 during close() must NOT invoke parent fallback or schedule extra retries
      expect(onTransportFailure).not.toHaveBeenCalled();
      expect(sockets).toHaveLength(2);
    });

    it("coalesces multiple wake events in the same microtask without spawning duplicate dials", async () => {
      vi.useFakeTimers();
      const { sockets } = renderMachineTerminal("1");
      expect(sockets).toHaveLength(1);
      const ws1 = sockets[0];
      await ws1.waitForHandlers();

      act(() => {
        ws1.onopen?.();
      });
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Dispatch online and visibilitychange in the same synchronous block
      Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
      act(() => {
        window.dispatchEvent(new Event("online"));
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // Exactly one fresh redial must be spawned, not two
      expect(ws1.close).toHaveBeenCalledTimes(1);
      expect(sockets).toHaveLength(2);
    });

    it("handles authoritative ticket 401/403 denial by gating retries, displaying Blocked, and recovering on explicit Retry", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("__task9Wire", {});
      const fetchMock = vi.fn(async (_input: RequestInfo | URL) => {
        return new Response(JSON.stringify({ error: "UNAUTHORIZED" }), { status: 401 });
      });
      vi.stubGlobal("fetch", fetchMock);

      const target = { sessionId: "session-auth-denial", daemonEpoch: "epoch-auth-denial" };
      const view = render(
        <RemoteTerminal
          sessionId={target.sessionId}
          token="real-device-token-abc"
          transportUrl="https://terminal-remote.example"
          daemonEpoch={target.daemonEpoch}
        />,
      );

      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error("Expected QA owner facade");

      // Wait for ticket fetch rejection to propagate
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      // Retries are cancelled (no pending timers, not inFlight)
      const snap = qa.snapshot(target);
      expect(snap).toMatchObject({
        outcome: "blocked",
        pendingTimers: 0,
        inFlight: false,
        identityConfirmed: false,
      });

      // Header status reflects Blocked, banner reflects unauthorized with Retry
      expect(connectionStatus()).toHaveTextContent("Blocked");
      expect(screen.getByTestId("remote-terminal-outcome")).toHaveTextContent("You were signed out.");
      expect(screen.getByTestId("remote-terminal-outcome")).not.toHaveTextContent("unauthorized");
      expect(screen.queryByTestId("remote-terminal-outcome-unknown")).toBeNull();

      // Timer progression does NOT auto-redial
      await act(async () => {
        await vi.advanceTimersByTimeAsync(30000);
      });
      const ticketCalls = () => fetchMock.mock.calls.filter(([input]) => String(input).endsWith("/api/v1/socket-ticket"));
      expect(ticketCalls()).toHaveLength(1);

      // Explicit Retry recovers: clears blocked gate and redials
      fetchMock.mockImplementationOnce(async () => {
        return new Response(JSON.stringify({ ticket: "fresh-ticket-123" }), { status: 200 });
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Retry" }));
      });
      expect(ticketCalls()).toHaveLength(2);

      view.unmount();
    });

    it("transitions current snapshot to retryable after formerly-ready socket disconnects", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("__task9Wire", {});
      vi.stubGlobal("WebSocket", MockWebSocket);

      const target = { sessionId: "session-disconnect-retryable", daemonEpoch: null };
      const view = render(<><button type="button">Other control</button><RemoteTerminal sessionId={target.sessionId} token="token-disc" /></>);
      const qa = window.__ferryxRecoveryQA;
      if (!qa) throw new Error("Expected QA owner facade");

      const ws = socket();
      (ws as any).__task9Id = "socket-disc-1";
      act(() => ws.onopen?.());
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "grid",
            cols: 80,
            rows: 24,
            cursor: hiddenCursor,
            lines: [{ index: 0, runs: [{ text: "ready-data", fg: null, bg: null, attrs: 0 }] }],
          }),
        });
      });
      expect(qa.snapshot(target)).toMatchObject({ outcome: "ready", identityConfirmed: true });
      const otherControl = screen.getByRole("button", { name: "Other control" });
      otherControl.focus();

      // Formerly-ready socket closes
      act(() => ws.onclose?.());

      // Advance into the redial attempt (dialSequence increments, new socket inFlight)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });

      // Snapshot must transition to retryable (stale ready outcome cleared), not stuck on ready
      const redialSnap = qa.snapshot(target);
      expect(redialSnap).toMatchObject({
        outcome: "retryable",
        inFlight: true,
        identityConfirmed: false,
        pendingTimers: 1,
      });

      const recovered = socket();
      act(() => {
        recovered.onopen?.();
        recovered.onmessage?.({ data: JSON.stringify({
          type: "grid", cols: 80, rows: 24, cursor: hiddenCursor, lines: [],
        }) });
      });
      expect(connectionStatus()).toHaveTextContent("Live");
      expect(document.activeElement).toBe(otherControl);

      view.unmount();
    });

    it("reflects validated blocked and affirmative_exit in connection status header instead of Connecting", async () => {
      vi.useFakeTimers();
      const { sockets } = renderMachineTerminal("epoch-header-test");
      const ws = sockets[0];
      await ws.waitForHandlers();
      act(() => ws.onopen?.());

      // While connecting, status is Connecting
      expect(connectionStatus()).toHaveTextContent("Connecting");

      // Validated affirmative_exit outcome reflects Exited in header
      const exitFrame = {
        type: "terminalOutcome",
        target: { sessionId: "session-machine-1", backendSessionId: "session-machine-1", daemonEpoch: "epoch-header-test" },
        incarnation: "inc-1",
        generation: "1",
        state: "affirmative_exit",
        blockedReason: null,
        exitCode: 0,
      };
      act(() => ws.onmessage?.({ data: JSON.stringify(exitFrame) }));
      expect(connectionStatus()).toHaveTextContent("Exited");

      // A new explicit attempt may report a different authoritative outcome.
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Retry" })); });
      const next = sockets[1];
      await next.waitForHandlers();
      const blockedFrame = {
        type: "terminalOutcome",
        target: { sessionId: "session-machine-1", backendSessionId: "session-machine-1", daemonEpoch: "epoch-header-test" },
        incarnation: "inc-1",
        generation: "1",
        state: "blocked",
        blockedReason: "auth_revoked",
        exitCode: null,
      };
      act(() => next.onmessage?.({ data: JSON.stringify(blockedFrame) }));
      expect(connectionStatus()).toHaveTextContent("Blocked");
    });

    it("renders replay gap indicator on current owner gap frame and resets on session/dial, ignoring stale gaps", async () => {
      vi.useFakeTimers();
      vi.stubGlobal("WebSocket", MockWebSocket);

      const target = { sessionId: "session-gap-test", daemonEpoch: null };
      const view = render(<RemoteTerminal sessionId={target.sessionId} token="token-gap" />);
      const ws = socket();
      act(() => ws.onopen?.());

      // Initially, no replay gap indicator
      expect(screen.queryByTestId("remote-terminal-replay-gap")).toBeNull();

      // Replay gap frame on current socket displays indicator
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "replayGap",
            tail: "task9-gap-tail",
          }),
        });
      });
      const gapIndicator = screen.getByTestId("remote-terminal-replay-gap");
      expect(gapIndicator).toHaveTextContent("Some output was skipped");

      // Stale gap frame from old socket after redial does not corrupt fresh socket
      act(() => ws.onclose?.());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      const ws2 = socket();
      expect(ws2).not.toBe(ws);

      // Redial resets replay gap state
      expect(screen.queryByTestId("remote-terminal-replay-gap")).toBeNull();

      // Stale message from ws is ignored
      act(() => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "replayGap",
            tail: "stale-tail",
          }),
        });
      });
      expect(screen.queryByTestId("remote-terminal-replay-gap")).toBeNull();

      view.unmount();
    });
  });
});
