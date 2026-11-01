import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteTerminal, type WebSocketLike } from "./RemoteTerminal";
import type { TunnelCloseEvent, TunnelErrorEvent, TunnelMessageEvent } from "./attachTunnel";

class MockWebSocket implements WebSocketLike {
  static readonly OPEN = 1;
  readonly url: string;
  binaryType = "arraybuffer";
  readyState = MockWebSocket.OPEN;
  send = vi.fn();
  close = vi.fn();
  onopen: ((event: Event) => void) | null = null;
  onclose: ((event: TunnelCloseEvent) => void) | null = null;
  onmessage: ((event: TunnelMessageEvent) => void) | null = null;
  onerror: ((event: TunnelErrorEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
  }
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

function gridFrame(rows: number, cols: number): string {
  const cells = Array.from({ length: rows * cols }, () => ({ text: " ", fg: null, bg: null, flags: 0 }));
  return JSON.stringify({ type: "grid", rows, cols, cursor: hiddenCursor, cells });
}

describe("remote terminal agent state bridge", () => {
  let createdSockets: MockWebSocket[] = [];
  const fakeCreateWebSocket = vi.fn((pathAndQuery: string) => {
    const socket = new MockWebSocket(pathAndQuery);
    createdSockets.push(socket);
    return socket;
  });

  beforeEach(() => {
    createdSockets = [];
    fakeCreateWebSocket.mockClear();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });
  });

  afterEach(() => {
    cleanup();
    createdSockets = [];
    vi.restoreAllMocks();
  });

  it("acknowledges a legacy outcome once after presentation is ready, not merely socket open", async () => {
    await act(async () => {
      render(<RemoteTerminal sessionId="s-ack" token="token" createWebSocket={fakeCreateWebSocket} />);
    });
    const socket = createdSockets[0];
    expect(socket).toBeDefined();
    const presentation = JSON.stringify({ type: "grid", rows: 20, cols: 80, cursor: hiddenCursor, lines: [] });
    await act(async () => { socket.onopen?.(new Event("open")); });
    expect(socket.send).not.toHaveBeenCalled();
    await act(async () => { socket.onmessage?.({ data: presentation }); });
    const acknowledgement = JSON.stringify({ type: "acknowledgeAgentOutcome" });
    expect(socket.send.mock.calls).toEqual([[acknowledgement]]);
    await act(async () => {
      socket.onopen?.(new Event("open"));
      socket.onmessage?.({ data: presentation });
    });
    expect(socket.send.mock.calls).toEqual([[acknowledgement]]);
  });

  it("acknowledges only after an attached frame matches the requested session and epoch", async () => {
    let resolveConnected!: (socket: MockWebSocket) => void;
    const connected = new Promise<MockWebSocket>((resolve) => { resolveConnected = resolve; });
    await act(async () => {
      render(<RemoteTerminal
        sessionId="s-ack"
        token="tunnel-device-token"
        isAccountSession
        daemonEpoch="101"
        createWebSocket={(path) => {
          const socket = new MockWebSocket(path);
          resolveConnected(socket);
          return socket;
        }}
      />);
    });
    const socket = await connected;
    const frame = (sessionId: string, daemonEpoch: string) => ({
      data: JSON.stringify({ type: "attached", generation: "9", target: { sessionId, daemonEpoch } }),
    });
    await act(async () => {
      socket.onopen?.(new Event("open"));
      socket.onmessage?.(frame("other", "101"));
      socket.onmessage?.(frame("s-ack", "100"));
    });
    const acknowledgement = JSON.stringify({ type: "acknowledgeAgentOutcome", generation: "9" });
    expect(socket.send).not.toHaveBeenCalledWith(acknowledgement);
    await act(async () => { socket.onmessage?.(frame("s-ack", "101")); });
    expect(socket.send).toHaveBeenCalledWith(acknowledgement);
  });

  it.each([true, false])("acknowledges new outcomes only for its current ready target (machine=%s)", async (machine) => {
    await act(async () => {
      render(<RemoteTerminal sessionId="s-ack" token="token" isAccountSession={machine} daemonEpoch="101" createWebSocket={fakeCreateWebSocket} />);
    });
    const socket = createdSockets[0];
    const outcome = (sessionId: string, daemonEpoch: string, value: string | null = "done") => ({
      data: JSON.stringify({ type: "agent_state", state: "idle", stateSource: "protocol", outcome: value,
        target: { sessionId, daemonEpoch } }),
    });
    await act(async () => { socket.onmessage?.(outcome("s-ack", "101")); });
    expect(socket.send).not.toHaveBeenCalled();
    await act(async () => {
      socket.onmessage?.({ data: JSON.stringify(machine
        ? { type: "attached", generation: "9", target: { sessionId: "s-ack", daemonEpoch: "101" } }
        : { type: "grid", rows: 20, cols: 80, cursor: hiddenCursor, lines: [] }) });
    });
    socket.send.mockClear();
    await act(async () => {
      socket.onmessage?.(outcome("other", "101"));
      socket.onmessage?.(outcome("s-ack", "100"));
      socket.onmessage?.(outcome("s-ack", "101", null));
    });
    expect(socket.send).not.toHaveBeenCalled();
    await act(async () => { socket.onmessage?.(outcome("s-ack", "101", "error")); });
    expect(socket.send.mock.calls).toEqual([[JSON.stringify({
      type: "acknowledgeAgentOutcome", ...(machine ? { generation: "9" } : {}),
    })]]);
  });

  it("forwards non-grid string frames to the agent-state listener and keeps grid frames out of it", async () => {
    const frames: string[] = [];
    render(
      <RemoteTerminal
        sessionId="s-1b-beta"
        token="tunnel-device-token"
        isAccountSession={true}
        daemonEpoch={101}
        createWebSocket={fakeCreateWebSocket}
        onAgentStateFrame={(raw) => frames.push(raw)}
      />,
    );

    await waitFor(() => expect(createdSockets.length).toBeGreaterThan(0));
    const socket = createdSockets[0];
    const agentState = JSON.stringify({
      type: "agent_state",
      target: { machineId: "mach-parity-1", sessionId: "s-1b-beta", daemonEpoch: 101 },
      state: "working",
      agent: "claude",
    });

    act(() => {
      socket.onmessage?.({ data: agentState });
    });
    expect(frames).toEqual([agentState]);

    act(() => {
      socket.onmessage?.({ data: gridFrame(2, 20) });
    });
    expect(frames).toEqual([agentState]);
    expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
  });

  it.each([true, false])("defers outcome acknowledgement while hidden and sends the latest on return (machine=%s)", async (machine) => {
    let visibility: DocumentVisibilityState = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    try {
      await act(async () => {
        render(<RemoteTerminal sessionId="s-ack" token="token" isAccountSession={machine} daemonEpoch="101" createWebSocket={fakeCreateWebSocket} />);
      });
      const socket = createdSockets[0];
      await act(async () => {
        socket.onmessage?.({ data: JSON.stringify(machine
          ? { type: "attached", generation: "9", target: { sessionId: "s-ack", daemonEpoch: "101" } }
          : { type: "grid", rows: 20, cols: 80, cursor: hiddenCursor, lines: [] }) });
      });
      const acknowledgement = JSON.stringify({ type: "acknowledgeAgentOutcome", ...(machine ? { generation: "9" } : {}) });
      expect(socket.send).toHaveBeenCalledWith(acknowledgement);
      socket.send.mockClear();
      visibility = "hidden";
      await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
      const outcome = (value: string) => ({
        data: JSON.stringify({ type: "agent_state", state: "idle", stateSource: "protocol", outcome: value,
          target: { sessionId: "s-ack", daemonEpoch: "101" } }),
      });
      await act(async () => {
        socket.onmessage?.(outcome("done"));
        socket.onmessage?.(outcome("error"));
      });
      expect(socket.send).not.toHaveBeenCalledWith(acknowledgement);
      visibility = "visible";
      await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
      expect(socket.send.mock.calls.filter(([sent]) => sent === acknowledgement)).toHaveLength(1);
      await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
      expect(socket.send.mock.calls.filter(([sent]) => sent === acknowledgement)).toHaveLength(1);
    } finally {
      Reflect.deleteProperty(document, "visibilityState");
    }
  });

  it("passes unrelated control frames through without throwing and ignores binary frames", async () => {
    const frames: string[] = [];
    render(
      <RemoteTerminal
        sessionId="s-1b-beta"
        token="tunnel-device-token"
        isAccountSession={true}
        daemonEpoch={101}
        createWebSocket={fakeCreateWebSocket}
        onAgentStateFrame={(raw) => frames.push(raw)}
      />,
    );

    await waitFor(() => expect(createdSockets.length).toBeGreaterThan(0));
    const socket = createdSockets[0];

    const controlFrame = JSON.stringify({ type: "remoteStatus", generation: "g-1" });
    expect(() => {
      act(() => {
        socket.onmessage?.({ data: controlFrame });
      });
    }).not.toThrow();
    expect(frames).toEqual([controlFrame]);

    act(() => {
      socket.onmessage?.({ data: new TextEncoder().encode(JSON.stringify({ type: "agent_state" })) });
    });
    expect(frames).toEqual([controlFrame]);
  });

  it("works without the listener and keeps decoding grid frames", async () => {
    render(
      <RemoteTerminal
        sessionId="s-1a-alpha"
        token="tunnel-device-token"
        isAccountSession={true}
        daemonEpoch={101}
        createWebSocket={fakeCreateWebSocket}
      />,
    );

    await waitFor(() => expect(createdSockets.length).toBeGreaterThan(0));
    const socket = createdSockets[0];

    expect(() => {
      act(() => {
        socket.onmessage?.({ data: JSON.stringify({ type: "agent_state", target: { sessionId: "s-1a-alpha" }, state: "waiting" }) });
      });
    }).not.toThrow();
    act(() => {
      socket.onmessage?.({ data: gridFrame(2, 20) });
    });
    expect(screen.getByTestId("remote-terminal-grid")).toBeDefined();
  });
});
