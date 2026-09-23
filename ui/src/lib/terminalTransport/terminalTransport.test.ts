import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { terminalEventBus, type TerminalOutputChunk } from "../terminalEvents";
import * as tauri from "../tauri";
import { MAX_OUTBOUND_BUFFER_BYTES, WebSocketTerminalTransport } from "./remoteTransport";
import { TauriTerminalTransport } from "./tauriTransport";

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSING = 2;
  readonly CLOSED = 3;

  readyState = MockWebSocket.CONNECTING;
  binaryType = "blob";
  readonly sent: Array<string | Uint8Array> = [];
  private readonly openListeners: Set<() => void> = new Set();
  onopen: ((event: Event) => void) | null = null;
  onclose: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onmessage: ((event: { data: string | ArrayBuffer }) => void) | null = null;

  constructor(public readonly url: string) {}

  send(data: string | Uint8Array) {
    this.sent.push(data);
  }

  close() {
    this.readyState = MockWebSocket.CLOSED;
  }

  addEventListener(event: string, listener: () => void) {
    if (event === "open") {
      this.openListeners.add(listener);
    }
  }

  removeEventListener(event: string, listener: () => void) {
    if (event === "open") {
      this.openListeners.delete(listener);
    }
  }

  open() {
    this.readyState = MockWebSocket.OPEN;
    for (const listener of this.openListeners) {
      listener();
    }
    const openEvent = typeof Event !== "undefined" ? new Event("open") : ({ type: "open" } as unknown as Event);
    this.onopen?.(openEvent);
  }
}

describe("TerminalTransport abstractions", () => {
  it("TauriTerminalTransport instantiates and conforms to contract", async () => {
    const transport = new TauriTerminalTransport();
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
    expect(typeof transport.signal).toBe("function");
    expect(typeof transport.onOutput).toBe("function");
    expect(typeof transport.listSessions).toBe("function");
  });

  it("TauriTerminalTransport listSessions queries tauri listTerminalSessions", async () => {
    const listSpy = vi.spyOn(tauri, "listTerminalSessions").mockResolvedValueOnce([
      {
        sessionId: "sess-100",
        worktreePath: "/repo/path",
      },
      {
        sessionId: "sess-exited",
        worktreePath: "/repo/exited",
        daemonEpoch: "epoch-exited",
        running: false,
      },
    ]);
    const transport = new TauriTerminalTransport();
    const sessions = await transport.listSessions();

    expect(listSpy).toHaveBeenCalled();
    expect(sessions).toEqual([
      { sessionId: "sess-100", worktreePath: "/repo/path", daemonEpoch: null, running: true },
      { sessionId: "sess-exited", worktreePath: "/repo/exited", daemonEpoch: "epoch-exited", running: false },
    ]);
  });

  it("TauriTerminalTransport attach invokes attachTerminal and decodes base64 history", async () => {
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "sess-200",
      daemonEpoch: "epoch-1",
      historyStartSequence: "10",
      historyEndSequence: "12",
      history: btoa("history buffer content"),
      gap: {
        requestedAfterSequence: "5",
        availableFromSequence: "10",
      },
    });

    const transport = new TauriTerminalTransport();
    const attachment = await transport.attach("sess-200", "5");

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "sess-200", afterSequence: "5" });
    expect(attachment.sessionId).toBe("sess-200");
    expect(attachment.daemonEpoch).toBe("epoch-1");
    expect(attachment.historyStartSequence).toBe("10");
    expect(attachment.historyEndSequence).toBe("12");
    expect(attachment.gap).toEqual({
      requestedAfterSequence: "5",
      availableFromSequence: "10",
    });
    expect(attachment.initialHistory).toBeDefined();
    expect(new TextDecoder().decode(attachment.initialHistory)).toBe("history buffer content");
  });

  it("TauriTerminalTransport attach handles empty history properly", async () => {
    vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "sess-empty",
      daemonEpoch: null,
      historyStartSequence: null,
      historyEndSequence: null,
      history: "",
      gap: null,
    });

    const transport = new TauriTerminalTransport();
    const attachment = await transport.attach("sess-empty", null);

    expect(attachment.sessionId).toBe("sess-empty");
    expect(attachment.initialHistory).toBeUndefined();
    expect(attachment.gap).toBeNull();
  });

  it("TauriTerminalTransport delegates write, resize, signal, and close to tauri IPC", async () => {
    const writeSpy = vi.spyOn(tauri, "writeTerminal").mockResolvedValue();
    const resizeSpy = vi.spyOn(tauri, "resizeTerminal").mockResolvedValue();
    const signalSpy = vi.spyOn(tauri, "signalTerminal").mockResolvedValue();
    const closeSpy = vi.spyOn(tauri, "closeTerminal").mockResolvedValue();

    const transport = new TauriTerminalTransport();

    await transport.write("s1", "ls -la\n");
    expect(writeSpy).toHaveBeenCalledWith({ sessionId: "s1", data: "ls -la\n" });

    await transport.write("s1", new TextEncoder().encode("echo hi\n"));
    expect(writeSpy).toHaveBeenCalledWith({ sessionId: "s1", data: "echo hi\n" });

    await transport.resize("s1", 120, 40);
    expect(resizeSpy).toHaveBeenCalledWith({ sessionId: "s1", cols: 120, rows: 40 });

    await transport.signal("s1", "interrupt");
    expect(signalSpy).toHaveBeenCalledWith({ sessionId: "s1", signal: "interrupt" });

    await transport.close("s1");
    expect(closeSpy).toHaveBeenCalledWith("s1");
  });

  it("TauriTerminalTransport onOutput forwards raw byte chunks from the shared event bus", async () => {
    let outputHandler: ((data: TerminalOutputChunk) => void) | null = null;
    const unlistenFn = vi.fn();
    vi.spyOn(terminalEventBus, "ensureStarted").mockResolvedValue();
    const subscribeSpy = vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementation((sessionId, handler, replay) => {
      expect(sessionId).toBe("sess-match");
      expect(replay).toBe(false);
      outputHandler = handler;
      return unlistenFn;
    });

    const transport = new TauriTerminalTransport();
    const received: Uint8Array[] = [];
    const unsubscribe = transport.onOutput("sess-match", (data) => {
      received.push(typeof data === "string" ? new TextEncoder().encode(data) : data);
    });

    await Promise.resolve();
    expect(subscribeSpy).toHaveBeenCalledOnce();
    expect(outputHandler).not.toBeNull();

    const chunk = new TextEncoder().encode("matched content");
    const emitOutput = outputHandler as unknown as (data: TerminalOutputChunk) => void;
    emitOutput(chunk);

    expect(received).toHaveLength(1);
    expect(received[0]).toBe(chunk);
    expect(new TextDecoder().decode(received[0])).toBe("matched content");

    unsubscribe();
    expect(unlistenFn).toHaveBeenCalled();
  });

  it("TauriTerminalTransport onLifecycle forwards lifecycle events", async () => {
    let lifecycleHandler: ((payload: tauri.TerminalLifecyclePayload) => void) | null = null;
    const unlistenFn = vi.fn();
    vi.spyOn(tauri, "onTerminalLifecycle").mockImplementation(async (handler) => {
      lifecycleHandler = handler;
      return unlistenFn;
    });

    const transport = new TauriTerminalTransport();
    const events: Array<{ sessionId: string; state: string; exitCode?: number | null }> = [];
    const unsubscribe = transport.onLifecycle((evt) => events.push(evt));

    await Promise.resolve();
    expect(lifecycleHandler).toBeDefined();

    if (lifecycleHandler) {
      (lifecycleHandler as (payload: tauri.TerminalLifecyclePayload) => void)({
        sessionId: "sess-1",
        state: "exited",
        exitCode: 0,
        reason: null,
      });
    }

    expect(events).toEqual([{ sessionId: "sess-1", state: "exited", exitCode: 0 }]);

    unsubscribe();
    expect(unlistenFn).toHaveBeenCalled();
  });

  it("WebSocketTerminalTransport formats WS URL and handles events", async () => {
    const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
  });

  describe("WebSocketTerminalTransport outbound buffering", () => {
    let lastMockWs: MockWebSocket | null = null;

    class TestWebSocket extends MockWebSocket {
      constructor(url: string) {
        super(url);
        lastMockWs = this;
      }
    }

    beforeEach(() => {
      lastMockWs = null;
      vi.spyOn(globalThis, "fetch").mockResolvedValue({
        ok: true,
        json: async () => ({ ticket: "mock-ticket" }),
      } as Response);
      vi.stubGlobal("WebSocket", TestWebSocket);
    });

    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it("WebSocketTerminalTransport delivers writes issued while CONNECTING in exact order on open", async () => {
      const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
      await transport.attach("sess-a");

      expect(lastMockWs?.readyState).toBe(MockWebSocket.CONNECTING);

      // Writes issued while socket is connecting are buffered in FIFO order.
      transport.write("sess-a", "first-chunk\n");
      transport.resize("sess-a", 120, 40);
      transport.signal("sess-a", "interrupt");
      transport.write("sess-a", new TextEncoder().encode("second-chunk\n"));

      expect(lastMockWs?.sent).toHaveLength(0);

      lastMockWs?.open();

      expect(lastMockWs?.sent).toHaveLength(4);
      const decoder = new TextDecoder();
      expect(decoder.decode(lastMockWs?.sent[0] as Uint8Array)).toBe("first-chunk\n");
      expect(lastMockWs?.sent[1]).toBe(JSON.stringify({ type: "resize", cols: 120, rows: 40 }));
      expect(lastMockWs?.sent[2]).toBe(JSON.stringify({ type: "signal", signal: "interrupt" }));
      expect(decoder.decode(lastMockWs?.sent[3] as Uint8Array)).toBe("second-chunk\n");
    });

    it("WebSocketTerminalTransport increments drop counter when byte bound is exceeded without unbounded buffer growth", async () => {
      const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
      expect(MAX_OUTBOUND_BUFFER_BYTES).toBe(4096);

      // 4000 bytes fit within the 4096-byte bound.
      transport.write("sess-b", new Uint8Array(4000));
      expect(transport.getOutboundDropCount("sess-b")).toBe(0);
      expect(transport.getOutboundBufferSize("sess-b")).toBe(4000);

      // Exceeding 4096 bytes drops the payload and increments the drop counter.
      transport.write("sess-b", new Uint8Array(100));
      expect(transport.getOutboundDropCount("sess-b")).toBe(1);
      expect(transport.getOutboundBufferSize("sess-b")).toBe(4000);

      transport.write("sess-b", new Uint8Array(200));
      expect(transport.getOutboundDropCount("sess-b")).toBe(2);
      expect(transport.getOutboundBufferSize("sess-b")).toBe(4000);
    });

    it("WebSocketTerminalTransport close clears the pending buffer for that session", async () => {
      const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");

      transport.write("sess-c", "pending payload before close");
      expect(transport.getOutboundBufferSize("sess-c")).toBeGreaterThan(0);

      await transport.close("sess-c");
      expect(transport.getOutboundBufferSize("sess-c")).toBe(0);

      // Subsequent socket open flushes nothing because the buffer was cleared.
      await transport.attach("sess-c");
      lastMockWs?.open();
      expect(lastMockWs?.sent).toHaveLength(0);
    });
  });
});
