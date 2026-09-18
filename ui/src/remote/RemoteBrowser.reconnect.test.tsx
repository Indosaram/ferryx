import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  encodeFrame,
  type BrowserFrame,
  type BrowserFrameMetadata,
} from "./browserProtocol";
import { BrowserClient } from "./browserClient";
import { useRemoteBrowser } from "./useRemoteBrowser";

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  binaryType: string = "blob";
  onopen: (() => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onerror: ((err: unknown) => void) | null = null;
  onmessage: ((event: { data: string | ArrayBuffer | ArrayBufferLike }) => void) | null = null;
  sentMessages: (string | ArrayBuffer)[] = [];
  readyState: number = 1;

  constructor(readonly url: string) {
    MockWebSocket.instances.push(this);
    queueMicrotask(() => {
      this.onopen?.();
    });
  }

  send(data: string | ArrayBuffer) {
    this.sentMessages.push(data);
  }

  close(code = 1000, reason = "Normal Closure") {
    this.readyState = 3;
    this.onclose?.({ code, reason });
  }
}

async function waitForSocket(index = 0): Promise<MockWebSocket> {
  while (MockWebSocket.instances.length <= index) {
    await new Promise((r) => setTimeout(r, 5));
  }
  return MockWebSocket.instances[index];
}

async function drainAsync(ms = 15) {
  await new Promise((r) => setTimeout(r, ms));
}

function makeTestJpeg(width = 640, height = 400): Uint8Array {
  return new Uint8Array([
    0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08,
    (height >> 8) & 0xff, height & 0xff,
    (width >> 8) & 0xff, width & 0xff,
    0x01, 0x01, 0x11, 0x00, 0xff, 0xd9,
  ]);
}

function makeTestFrame(
  seq: number,
  streamId = 1,
  width = 640,
  height = 400,
): BrowserFrame {
  const metadata: BrowserFrameMetadata = {
    offsetTop: 0,
    pageScaleFactor: 1,
    deviceWidth: 1280,
    deviceHeight: 800,
    imageWidth: width,
    imageHeight: height,
    scrollOffsetX: 0,
    scrollOffsetY: 0,
    timestamp: 1726560000,
    streamId,
    browserInstanceId: "bi-reconnect-1",
    browserServiceEpoch: "1",
    desktopEpoch: "1",
    documentGeneration: "1",
    viewportRevision: "1",
    captureRect: { x: 0, y: 0, width: 1280, height: 800 },
    geometrySource: "wkSnapshot",
  };
  return {
    format: "jpeg",
    seq,
    metadata,
    imageBytes: makeTestJpeg(width, height),
  };
}

async function establishStreaming(ws: MockWebSocket, streamId = 1) {
  await act(async () => {
    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserHello",
        browserId: "b-reconnect",
        browserInstanceId: "bi-reconnect-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        protocolVersion: 1,
        supportedCommands: ["navigate", "click", "fill"],
      }),
    });
    await drainAsync();
  });

  const subSent = JSON.parse(ws.sentMessages[ws.sentMessages.length - 1] as string);
  await act(async () => {
    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserSubscribed",
        requestId: subSent.requestId,
        subscriptionId: "sub-reconnect-1",
        streamId,
        browserId: "b-reconnect",
        browserInstanceId: "bi-reconnect-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        documentGeneration: "1",
        options: { format: "jpeg" },
      }),
    });
    await drainAsync();
  });
}

function ReconnectTestHarness({
  browserId = "b-reconnect",
  onStatus,
}: {
  browserId?: string | null;
  onStatus?: (st: string) => void;
}) {
  const state = useRemoteBrowser({
    baseUrl: "http://localhost:8080",
    browserId,
    deviceToken: "mock-reconnect-token",
  });

  onStatus?.(state.status);

  return (
    <div>
      <div data-testid="status">{state.status}</div>
      <div data-testid="streamId">{state.frame?.metadata.streamId ?? "none"}</div>
      <div data-testid="seq">{state.frame?.seq ?? "none"}</div>
      <div data-testid="imageUrl">{state.imageUrl ?? "none"}</div>
      <div data-testid="error">{state.error?.message ?? "none"}</div>
      <button data-testid="reconnect-btn" onClick={state.reconnect}>
        Reconnect
      </button>
    </div>
  );
}

describe("RemoteBrowser - Network Resilience & Reconnection Lifecycle (Phase 7B)", () => {
  let createdUrls: string[] = [];
  let revokedUrls: string[] = [];
  let ticketCounter = 0;
  let issuedTickets: string[] = [];

  beforeEach(() => {
    MockWebSocket.instances = [];
    createdUrls = [];
    revokedUrls = [];
    ticketCounter = 0;
    issuedTickets = [];

    vi.stubGlobal("WebSocket", MockWebSocket);

    const OriginalURL = globalThis.URL;
    class MockURL extends OriginalURL {
      static createObjectURL = vi.fn(() => {
        const u = `blob:reconnect-frame-${createdUrls.length + 1}`;
        createdUrls.push(u);
        return u;
      });
      static revokeObjectURL = vi.fn((u: string) => {
        revokedUrls.push(u);
      });
    }
    vi.stubGlobal("URL", MockURL);

    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const urlStr = String(input instanceof Request ? input.url : input);
        if (urlStr.includes("/api/v1/socket-ticket")) {
          ticketCounter += 1;
          const ticket = `ticket-single-use-${ticketCounter}-${Math.random().toString(36).slice(2)}`;
          issuedTickets.push(ticket);
          return new Response(
            JSON.stringify({ ticket }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          );
        }
        return new Response("Not found", { status: 404 });
      }),
    );
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    MockWebSocket.instances = [];
  });

  describe("Disconnection detection", () => {
    it("transitions state to closed on sudden WebSocket transport error or closure", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      const ws = await waitForSocket(0);
      await establishStreaming(ws);
      expect(screen.getByTestId("status").textContent).toBe("streaming");

      // WebSocket drops unexpectedly with abnormal closure 1006
      await act(async () => {
        ws.close(1006, "Abnormal Closure");
      });

      expect(screen.getByTestId("status").textContent).toBe("closed");
    });

    it("rejects all in-flight pending requests with 'WebSocket closed' error upon disconnection", async () => {
      const client = new BrowserClient("ws://localhost:9000/browser/b1");
      const ws = MockWebSocket.instances[0];
      await drainAsync();

      // Initiate a command while connected
      const cmdPromise = client.sendCommand({
        browserId: "b1",
        leaseEpoch: "1",
        browserInstanceId: "bi-1",
        desktopEpoch: "1",
        documentGeneration: "1",
        command: "click",
      });

      // Socket drops before server responds
      ws.close(1006, "Abnormal Closure");

      await expect(cmdPromise).rejects.toThrow(/WebSocket closed/i);
    });
  });

  describe("Fresh single-use ticket acquisition", () => {
    it("requests a fresh single-use ticket via remoteSocketUrl and NEVER reuses previous tickets", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      // Connection 1
      const ws1 = await waitForSocket(0);
      await establishStreaming(ws1);
      expect(issuedTickets.length).toBe(1);
      const ticket1 = issuedTickets[0];
      expect(ws1.url).toContain(`ticket=${ticket1}`);

      // Trigger reconnect
      await act(async () => {
        fireEvent.click(screen.getByTestId("reconnect-btn"));
      });

      // Connection 2
      const ws2 = await waitForSocket(1);
      expect(MockWebSocket.instances.length).toBe(2);
      expect(issuedTickets.length).toBe(2);
      const ticket2 = issuedTickets[1];

      // Must be a brand new ticket
      expect(ticket2).not.toBe(ticket1);
      expect(ws2.url).toContain(`ticket=${ticket2}`);
      expect(ws2.url).not.toContain(ticket1);

      // Trigger second reconnect
      await act(async () => {
        fireEvent.click(screen.getByTestId("reconnect-btn"));
      });

      // Connection 3
      const ws3 = await waitForSocket(2);
      expect(issuedTickets.length).toBe(3);
      const ticket3 = issuedTickets[2];
      expect(ticket3).not.toBe(ticket2);
      expect(ticket3).not.toBe(ticket1);
      expect(ws3.url).toContain(`ticket=${ticket3}`);
    });
  });

  describe("NO mutation auto-replay", () => {
    it("NEVER automatically replays previously pending or failed mutation commands upon reconnect", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      const ws1 = await waitForSocket(0);
      await establishStreaming(ws1);

      // Trigger reconnect
      await act(async () => {
        fireEvent.click(screen.getByTestId("reconnect-btn"));
      });

      const ws2 = await waitForSocket(1);
      await establishStreaming(ws2);

      // Check all messages sent on the new connection
      const sentTypes = ws2.sentMessages.map((m) => {
        try {
          return JSON.parse(m as string).type;
        } catch {
          return "raw";
        }
      });

      // Only browserSubscribe must be issued. Absolutely NO mutation commands!
      expect(sentTypes).toEqual(["browserSubscribe"]);
      expect(sentTypes).not.toContain("browserCommand");
    });
  });

  describe("Decode order enforcement", () => {
    it("drops frames arriving out of sequence (stale seq) and decodes only strictly increasing seq", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Send Frame seq 10
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(10, 1)).buffer as ArrayBuffer });
      });
      expect(screen.getByTestId("seq").textContent).toBe("10");

      // Frame seq 8 arrives late (stale! seq < 10)
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(8, 1)).buffer as ArrayBuffer });
      });
      // MUST BE DROPPED! Still seq 10
      expect(screen.getByTestId("seq").textContent).toBe("10");

      // Frame seq 10 arrives again (duplicate! seq == 10)
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(10, 1)).buffer as ArrayBuffer });
      });
      // MUST BE DROPPED! Still seq 10
      expect(screen.getByTestId("seq").textContent).toBe("10");

      // Frame seq 15 arrives (newer! seq 15 > 10)
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(15, 1)).buffer as ArrayBuffer });
      });
      expect(screen.getByTestId("seq").textContent).toBe("15");

      // Frame seq 12 arrives (stale! seq < 15)
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(12, 1)).buffer as ArrayBuffer });
      });
      // MUST BE DROPPED! Still seq 15
      expect(screen.getByTestId("seq").textContent).toBe("15");
    });

    it("ignores frames from old streamId if streamId changed after re-subscription", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      const ws = await waitForSocket(0);
      await establishStreaming(ws, 1); // streamId = 1

      // Frame from active streamId 1
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(1, 1)).buffer as ArrayBuffer });
      });
      expect(screen.getByTestId("seq").textContent).toBe("1");
      expect(screen.getByTestId("streamId").textContent).toBe("1");

      // Frame from phantom streamId 999 arrives
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(5, 999)).buffer as ArrayBuffer });
      });
      // Ignored!
      expect(screen.getByTestId("seq").textContent).toBe("1");
      expect(screen.getByTestId("streamId").textContent).toBe("1");
    });
  });

  describe("Memory cleanup and leak prevention", () => {
    it("revokes previous ObjectURL on every new frame and revokes the active URL on unmount", async () => {
      let unmount: () => void = () => {};
      await act(async () => {
        const res = render(<ReconnectTestHarness />);
        unmount = res.unmount;
      });

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Receive 5 consecutive frames
      for (let i = 1; i <= 5; i++) {
        await act(async () => {
          ws.onmessage?.({ data: encodeFrame(makeTestFrame(i, 1)).buffer as ArrayBuffer });
        });
      }

      // 5 ObjectURLs created
      expect(createdUrls.length).toBe(5);
      // Frames 1..4 must have been revoked as they were replaced
      expect(revokedUrls.length).toBe(4);
      expect(revokedUrls).toContain(createdUrls[0]);
      expect(revokedUrls).toContain(createdUrls[1]);
      expect(revokedUrls).toContain(createdUrls[2]);
      expect(revokedUrls).toContain(createdUrls[3]);
      // Frame 5 is currently active
      expect(revokedUrls).not.toContain(createdUrls[4]);

      // Unmount component
      await act(async () => {
        unmount();
      });

      // Frame 5 URL must now be revoked as well (all 5 cleaned up!)
      expect(revokedUrls.length).toBe(5);
      expect(revokedUrls).toContain(createdUrls[4]);
    });
  });

  describe("Background throttling", () => {
    it("clears frame buffer and pauses rendering when page visibility changes to hidden, and resumes cleanly", async () => {
      await act(async () => {
        render(<ReconnectTestHarness />);
      });

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Frame 1 arrives
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(1, 1)).buffer as ArrayBuffer });
      });
      expect(screen.getByTestId("imageUrl").textContent).not.toBe("none");
      expect(screen.getByTestId("status").textContent).toBe("streaming");

      // App backgrounded: document visibility changes to hidden
      Object.defineProperty(document, "hidden", { value: true, configurable: true });
      await act(async () => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // Frame buffer cleared and rendering paused
      expect(screen.getByTestId("imageUrl").textContent).toBe("none");
      expect(screen.getByTestId("status").textContent).toBe("paused");
      // Current frame URL revoked to save memory in background
      expect(revokedUrls).toContain(createdUrls[0]);

      // App returns to foreground: document visibility changes to visible
      Object.defineProperty(document, "hidden", { value: false, configurable: true });
      await act(async () => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // New frame arrives after returning to visible
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(makeTestFrame(2, 1)).buffer as ArrayBuffer });
      });

      // Streaming resumed cleanly
      expect(screen.getByTestId("imageUrl").textContent).not.toBe("none");
      expect(screen.getByTestId("seq").textContent).toBe("2");
    });
  });
});
