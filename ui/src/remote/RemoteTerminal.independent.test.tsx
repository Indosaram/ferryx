import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteTerminal } from "./RemoteTerminal";

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static latest: MockWebSocket | null = null;
  static instances: MockWebSocket[] = [];
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

  get onopen() { return this._onopen; }
  set onopen(val) { this._onopen = val; this._checkInstalled(); }

  get onclose() { return this._onclose; }
  set onclose(val) { this._onclose = val; this._checkInstalled(); }

  get onmessage() { return this._onmessage; }
  set onmessage(val) { this._onmessage = val; this._checkInstalled(); }

  get onerror() { return this._onerror; }
  set onerror(val) { this._onerror = val; this._checkInstalled(); }

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
  }
}

class SynchronousCloseWebSocket extends MockWebSocket {
  override close = vi.fn(() => this.onclose?.());
}

function connectionStatus() {
  return screen.getByTestId("remote-terminal-connection-status");
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

function attachedFrame(generation: unknown, epoch = "1"): string {
  return JSON.stringify({
    type: "attached",
    target: { machineId: "mach-1", sessionId: "session-independent-1", daemonEpoch: epoch },
    generation,
    cols: 80,
    rows: 20,
    startSequence: 0,
    endSequence: 0,
    replayGap: null,
  });
}

describe("Task 7 Independent Verification — Terminal Wake and Recovery Invalidation", () => {
  beforeEach(() => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    vi.stubGlobal("WebSocket", MockWebSocket);
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
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("wakes pending deferred ticket promise on online event and closes late-resolved socket without adopting", async () => {
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
        sessionId="session-independent-1"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch="1"
        createWebSocket={createWebSocket}
      />
    );

    expect(createWebSocket).toHaveBeenCalledTimes(1);
    expect(connectionStatus()).toHaveTextContent("Connecting");

    // While ticket 1 is pending (socketRef.current is null), an online wake event fires!
    act(() => {
      window.dispatchEvent(new Event("online"));
    });

    // Normative contract: pending dial must be invalidated and a fresh dial initiated immediately
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

  it("wakes OPEN socket with unattached pending handshake on visibilitychange event", async () => {
    vi.useFakeTimers();
    const sockets: MockWebSocket[] = [];
    const createWebSocket = vi.fn(() => {
      const ws = new MockWebSocket("mock://socket");
      sockets.push(ws);
      return ws;
    });

    render(
      <RemoteTerminal
        sessionId="session-independent-1"
        token="token-unused"
        isAccountSession={true}
        daemonEpoch="1"
        createWebSocket={createWebSocket}
      />
    );

    expect(sockets).toHaveLength(1);
    const ws1 = sockets[0];
    await ws1.waitForHandlers();

    // Socket opens, readyState is OPEN, but attachment has not arrived (handshake pending)
    act(() => {
      ws1.readyState = MockWebSocket.OPEN;
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

  it("invalidates dial sequence BEFORE closing old socket on wake so synchronous close cannot trigger fallback or duplicate retries", async () => {
    vi.useFakeTimers();
    const onTransportFailure = vi.fn();
    const sockets: MockWebSocket[] = [];

    const createWebSocket = vi.fn(() => {
      // Use SynchronousCloseWebSocket for first attempt
      if (sockets.length === 0) {
        const ws = new SynchronousCloseWebSocket("mock://sync-close-ws");
        sockets.push(ws);
        return ws;
      }
      const ws = new MockWebSocket("mock://next-ws");
      sockets.push(ws);
      return ws;
    });

    render(
      <RemoteTerminal
        sessionId="session-independent-1"
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

    // Socket connects but does not attach
    act(() => {
      ws1.readyState = MockWebSocket.OPEN;
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
});

