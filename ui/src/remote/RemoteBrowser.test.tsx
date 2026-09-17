import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  encodeFrame,
  type BrowserFrame,
  type BrowserFrameMetadata,
  type BrowserHelloMessage,
  type BrowserSubscribedMessage,
} from "./browserProtocol";
import { BrowserClient } from "./browserClient";
import { useRemoteBrowser } from "./useRemoteBrowser";
import { RemoteBrowser } from "./RemoteBrowser";

// Mock WebSocket
class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSING = 2;
  readonly CLOSED = 3;

  static instances: MockWebSocket[] = [];
  binaryType: string = "blob";
  onopen: (() => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onerror: ((err: unknown) => void) | null = null;
  onmessage: ((event: { data: string | ArrayBuffer | ArrayBufferLike }) => void) | null = null;
  sentMessages: (string | ArrayBuffer)[] = [];
  readyState: number = 1; // Start OPEN in tests

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

// Helpers for test frames
function makeTestJpeg(width = 640, height = 400): Uint8Array {
  return new Uint8Array([
    0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08,
    (height >> 8) & 0xff, height & 0xff,
    (width >> 8) & 0xff, width & 0xff,
    0x01, 0x01, 0x11, 0x00, 0xff, 0xd9,
  ]);
}

function makeTestFrame(seq: number, streamId = 1, width = 640, height = 400): BrowserFrame {
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
    browserInstanceId: "bi-1",
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

let createdObjectUrls: string[] = [];
let revokedObjectUrls: string[] = [];

beforeEach(() => {
  MockWebSocket.instances = [];
  createdObjectUrls = [];
  revokedObjectUrls = [];
  vi.stubGlobal("WebSocket", MockWebSocket);

  const OriginalURL = globalThis.URL;
  class MockURL extends OriginalURL {
    static createObjectURL = vi.fn((_blob: Blob) => {
      const url = `blob:mock-url-${createdObjectUrls.length + 1}`;
      createdObjectUrls.push(url);
      return url;
    });
    static revokeObjectURL = vi.fn((url: string) => {
      revokedObjectUrls.push(url);
    });
  }
  vi.stubGlobal("URL", MockURL);

  // Mock ticket fetch
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const urlStr = String(input instanceof Request ? input.url : input);
      if (urlStr.includes("/api/v1/socket-ticket")) {
        return new Response(
          JSON.stringify({ ticket: `ticket-${Math.random().toString(36).slice(2)}` }),
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

async function establishStreaming(ws: MockWebSocket) {
  await act(async () => {
    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserHello",
        browserId: "b1",
        browserInstanceId: "bi-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        protocolVersion: 1,
        supportedCommands: ["navigate"],
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
        subscriptionId: "sub-1",
        streamId: 1,
        browserId: "b1",
        browserInstanceId: "bi-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        documentGeneration: "1",
        options: { format: "jpeg" },
      }),
    });
    await drainAsync();
  });
}

describe("BrowserClient", () => {
  it("connects and receives hello message", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    expect(ws).toBeDefined();

    // Server sends hello
    const hello: BrowserHelloMessage = {
      type: "browserHello",
      browserId: "b1",
      browserInstanceId: "bi-1",
      browserServiceEpoch: "1",
      desktopEpoch: "1",
      protocolVersion: 1,
      supportedCommands: ["navigate", "click", "fill"],
    };

    let receivedHello: BrowserHelloMessage | null = null;
    client.onHello((h) => {
      receivedHello = h;
    });

    await drainAsync();
    ws.onmessage?.({ data: JSON.stringify(hello) });

    expect(receivedHello).toEqual(hello);
  });

  it("subscribes with options and receives subscription confirmation", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    await drainAsync();

    const subPromise = client.subscribe({ format: "jpeg", quality: 70 }, "viewer-1");

    // Check sent message
    expect(ws.sentMessages.length).toBe(1);
    const sent = JSON.parse(ws.sentMessages[0] as string);
    expect(sent.type).toBe("browserSubscribe");
    expect(sent.viewerInstanceId).toBe("viewer-1");
    expect(sent.options.format).toBe("jpeg");

    // Server responds with browserSubscribed
    const subscribed: BrowserSubscribedMessage = {
      type: "browserSubscribed",
      requestId: sent.requestId,
      subscriptionId: "sub-99",
      streamId: 1,
      browserId: "b1",
      browserInstanceId: "bi-1",
      browserServiceEpoch: "1",
      desktopEpoch: "1",
      documentGeneration: "1",
      options: { format: "jpeg", quality: 70 },
    };

    ws.onmessage?.({ data: JSON.stringify(subscribed) });
    const res = await subPromise;
    expect(res).toEqual(subscribed);
  });

  it("enforces NO auto-claim: client does not claim driver automatically", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    await drainAsync();

    client.subscribe({ format: "jpeg" });
    // Sent only browserSubscribe, NO browserDriverClaim
    const types = ws.sentMessages.map((m) => JSON.parse(m as string).type);
    expect(types).not.toContain("browserDriverClaim");
  });

  it("supports explicit claimDriver and releaseDriver", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    await drainAsync();

    const claimPromise = client.claimDriver("b1", "sub-1");
    const claimSent = JSON.parse(ws.sentMessages[0] as string);
    expect(claimSent.type).toBe("browserDriverClaim");

    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserDriverClaimed",
        requestId: claimSent.requestId,
        leaseEpoch: "10",
        expiresAt: Date.now() + 15000,
      }),
    });
    const claimed = await claimPromise;
    expect(claimed.leaseEpoch).toBe("10");

    // Release
    const releasePromise = client.releaseDriver("10");
    const releaseSent = JSON.parse(ws.sentMessages[1] as string);
    expect(releaseSent.type).toBe("browserDriverRelease");

    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserDriverReleased",
        requestId: releaseSent.requestId,
        leaseEpoch: "10",
      }),
    });
    const released = await releasePromise;
    expect(released.leaseEpoch).toBe("10");
  });

  it("sends commands and handles results and errors", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    await drainAsync();

    const cmdPromise = client.sendCommand({
      browserId: "b1",
      leaseEpoch: "10",
      browserInstanceId: "bi-1",
      desktopEpoch: "1",
      documentGeneration: "1",
      command: "navigate",
      params: { url: "https://example.com" },
    });

    const sent = JSON.parse(ws.sentMessages[0] as string);
    expect(sent.type).toBe("browserCommand");
    expect(sent.command).toBe("navigate");

    // Server responds with result
    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserResult",
        requestId: sent.requestId,
        result: { ok: true },
      }),
    });

    const res = await cmdPromise;
    expect(res.result).toEqual({ ok: true });
  });

  it("decodes incoming binary frames and dispatches to onFrame", async () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    await drainAsync();

    let receivedFrame: BrowserFrame | null = null;
    client.onFrame((f) => {
      receivedFrame = f;
    });

    const testFrame = makeTestFrame(1, 1);
    const encoded = encodeFrame(testFrame);
    ws.onmessage?.({ data: encoded.buffer });

    expect(receivedFrame).not.toBeNull();
    const frameResult = receivedFrame as BrowserFrame | null;
    expect(frameResult?.seq).toBe(1);
    expect(frameResult?.metadata.streamId).toBe(1);
  });

  it("never sends binary from client to server", () => {
    const client = new BrowserClient("ws://localhost:9000/browser/b1");
    const ws = MockWebSocket.instances[0];
    client.ackFrame(1, 5);

    // Everything sent must be a string, never an ArrayBuffer
    for (const msg of ws.sentMessages) {
      expect(typeof msg).toBe("string");
    }
  });
});

describe("useRemoteBrowser", () => {
  function TestHarness({
    browserId = "b1",
    onStatus,
  }: {
    browserId?: string | null;
    onStatus?: (st: string) => void;
  }) {
    const state = useRemoteBrowser({
      baseUrl: "http://localhost:8080",
      browserId,
      deviceToken: "mock-token",
    });

    onStatus?.(state.status);
    return (
      <div>
        <div data-testid="status">{state.status}</div>
        <div data-testid="streamId">{state.frame?.metadata.streamId ?? "none"}</div>
        <div data-testid="seq">{state.frame?.seq ?? "none"}</div>
        <div data-testid="imageUrl">{state.imageUrl ?? "none"}</div>
        <button data-testid="reconnect-btn" onClick={state.reconnect}>
          Reconnect
        </button>
      </div>
    );
  }

  it("transitions opening -> ready -> streaming on connection and subscription", async () => {
    let unmount: () => void = () => {};
    await act(async () => {
      const rendered = render(<TestHarness />);
      unmount = rendered.unmount;
    });

    expect(screen.getByTestId("status").textContent).toBe("opening");

    const ws = await waitForSocket(0);
    expect(ws).toBeDefined();

    // Establish streaming
    await establishStreaming(ws);

    expect(screen.getByTestId("status").textContent).toBe("streaming");

    // When frame arrives, image URL is created and frame ACK is sent
    const frame = makeTestFrame(1, 1);
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame).buffer });
    });

    expect(screen.getByTestId("seq").textContent).toBe("1");
    expect(createdObjectUrls.length).toBe(1);

    // Frame ack sent
    const ackMsg = JSON.parse(ws.sentMessages[ws.sentMessages.length - 1] as string);
    expect(ackMsg.type).toBe("browserFrameAck");
    expect(ackMsg.streamId).toBe(1);
    expect(ackMsg.seq).toBe(1);

    // Clean unmount
    await act(async () => {
      unmount();
    });
    // URL revoked on unmount
    expect(revokedObjectUrls).toContain(createdObjectUrls[0]);
  });

  it("discards stale seq frames (decode order enforcement)", async () => {
    await act(async () => {
      render(<TestHarness />);
    });
    const ws = await waitForSocket(0);

    // Establish streaming
    await establishStreaming(ws);

    // Receive frame seq 5
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(5, 1)).buffer });
    });
    expect(screen.getByTestId("seq").textContent).toBe("5");

    // Receive stale frame seq 3 (should be DISCARDED!)
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(3, 1)).buffer });
    });
    // Still seq 5!
    expect(screen.getByTestId("seq").textContent).toBe("5");

    // Receive newer frame seq 6
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(6, 1)).buffer });
    });
    expect(screen.getByTestId("seq").textContent).toBe("6");
  });

  it("releases previous object URL when new frame replaces it", async () => {
    await act(async () => {
      render(<TestHarness />);
    });
    const ws = await waitForSocket(0);

    // Establish streaming
    await establishStreaming(ws);

    // Frame 1
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(1, 1)).buffer });
    });
    const url1 = createdObjectUrls[0];
    expect(revokedObjectUrls).not.toContain(url1);

    // Frame 2 replaces Frame 1
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(2, 1)).buffer });
    });
    expect(createdObjectUrls.length).toBe(2);
    expect(revokedObjectUrls).toContain(url1);
  });

  it("clears frame buffer on background visibilitychange", async () => {
    await act(async () => {
      render(<TestHarness />);
    });
    const ws = await waitForSocket(0);

    // Establish streaming
    await establishStreaming(ws);

    // Frame 1 arrives
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(1, 1)).buffer });
    });
    expect(screen.getByTestId("imageUrl").textContent).not.toBe("none");

    // Trigger document hidden (background)
    Object.defineProperty(document, "hidden", { value: true, configurable: true });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });

    // Buffer cleared!
    expect(screen.getByTestId("imageUrl").textContent).toBe("none");
    expect(screen.getByTestId("status").textContent).toBe("paused");
  });

  it("reconnect requests a new ticket, creates a new subscription, and DOES NOT auto-replay mutations", async () => {
    await act(async () => {
      render(<TestHarness />);
    });
    const ws1 = await waitForSocket(0);
    await establishStreaming(ws1);
    expect(MockWebSocket.instances.length).toBe(1);

    // Trigger reconnect
    await act(async () => {
      fireEvent.click(screen.getByTestId("reconnect-btn"));
    });

    // New ticket was fetched and new WebSocket was instantiated
    const newWs = await waitForSocket(1);
    expect(MockWebSocket.instances.length).toBe(2);

    // Establish streaming on new connection
    await establishStreaming(newWs);

    // Must issue a new subscription
    const subscribeSent = newWs.sentMessages.filter((m) => {
      try {
        return JSON.parse(m as string).type === "browserSubscribe";
      } catch {
        return false;
      }
    });
    expect(subscribeSent.length).toBe(1);

    // Must NOT have replayed any mutations or commands!
    const allTypes = newWs.sentMessages.map((m) => JSON.parse(m as string).type);
    expect(allTypes).toEqual(["browserSubscribe"]);
  });
});

describe("RemoteBrowser component", () => {
  it("renders controls slot and handles letterbox normalized coordinate clicks", async () => {
    const handlePointClick = vi.fn();
    const testFrame = makeTestFrame(1, 1, 640, 400);

    let unmount: () => void = () => {};
    await act(async () => {
      const res = render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b1"
          deviceToken="token-1"
          controls={<button data-testid="custom-control">Back</button>}
          onPointClick={handlePointClick}
        />,
      );
      unmount = res.unmount;
    });

    // Custom control is rendered
    expect(screen.getByTestId("custom-control")).toBeDefined();

    // Get socket and establish connection
    const ws = await waitForSocket(0);
    await establishStreaming(ws);

    // Render frame
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(testFrame).buffer });
    });

    const viewport = screen.getByTestId("remote-browser-viewport");
    expect(viewport).toBeDefined();

    // Mock viewport rect: 800x600 container.
    // Image is 640x400 (aspect 1.6).
    // In 800x600 container:
    // Fits with width 800, height 800 / 1.6 = 500.
    // Top letterbox margin = (600 - 500) / 2 = 50. Left = 0.
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      right: 800,
      bottom: 600,
      width: 800,
      height: 600,
      x: 0,
      y: 0,
      toJSON: () => {},
    });

    // Click inside the image area (e.g. at x=400, y=300):
    // Relative to image: x = 400 (0.5), y = 300 - 50 = 250 (250 / 500 = 0.5).
    await act(async () => {
      fireEvent.click(viewport, { clientX: 400, clientY: 300 });
    });

    expect(handlePointClick).toHaveBeenCalledTimes(1);
    const clickData = handlePointClick.mock.calls[0][0];
    expect(clickData.u).toBeCloseTo(0.5, 2);
    expect(clickData.v).toBeCloseTo(0.5, 2);
    expect(clickData.streamId).toBe(1);
    expect(clickData.seq).toBe(1);

    // Click inside the top letterbox (e.g. at x=400, y=20):
    // y=20 is before the image begins (top margin is 50).
    // Should NOT trigger click on document!
    handlePointClick.mockClear();
    await act(async () => {
      fireEvent.click(viewport, { clientX: 400, clientY: 20 });
    });
    expect(handlePointClick).not.toHaveBeenCalled();

    await act(async () => {
      unmount();
    });
  });
});
