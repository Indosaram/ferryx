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
import { RemoteBrowserWorkspace } from "./RemoteBrowserWorkspace";
import { useRemoteBrowserDriver } from "./useRemoteBrowserDriver";
import { RemoteBrowserSharingIndicator } from "../components/RemoteBrowserSharingIndicator";

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
  Object.defineProperty(document, "hidden", { value: false, configurable: true });
  Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
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
  Object.defineProperty(document, "hidden", { value: false, configurable: true });
  Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
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
        <button
          data-testid="ack-btn"
          onClick={() => {
            if (state.frame) {
              state.confirmPresented(state.frame.metadata.streamId, state.frame.seq);
            }
          }}
        >
          Confirm ACK
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

    // When frame arrives, image URL is created. Frame ACK is NOT sent yet (presentation-gated ACK)
    const frame = makeTestFrame(1, 1);
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame).buffer });
    });

    expect(screen.getByTestId("seq").textContent).toBe("1");
    expect(createdObjectUrls.length).toBe(1);

    // No premature ACK before presentation
    const acksBeforeConfirm = ws.sentMessages.filter((m) => {
      try {
        return JSON.parse(m as string).type === "browserFrameAck";
      } catch {
        return false;
      }
    });
    expect(acksBeforeConfirm.length).toBe(0);

    // Presentation confirmed via confirmPresented
    await act(async () => {
      fireEvent.click(screen.getByTestId("ack-btn"));
    });

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

    // Reset document visibility back to visible
    Object.defineProperty(document, "hidden", { value: false, configurable: true });
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
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
    await act(async () => {
      fireEvent.load(screen.getByAltText("Remote browser stream"));
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

  it("does not create a duplicate client when session is passed (P1-09 dedup)", () => {
    const mockSession = {
      status: "streaming" as const,
      frame: null,
      imageUrl: null,
      browserState: null,
      hello: null,
      error: null,
      client: null,
      reconnect: vi.fn(),
      sendAck: vi.fn(),
      confirmPresented: vi.fn(),
    };

    render(
      <RemoteBrowser
        baseUrl="http://localhost:8080"
        browserId="b1"
        deviceToken="mock-token"
        session={mockSession}
      />
    );

    // Exactly 0 WebSocket clients created by RemoteBrowser when session is passed
    expect(MockWebSocket.instances.length).toBe(0);
  });

  it("triggers presentation-gated ACK when image onLoad fires (P1-10)", async () => {
    const confirmPresented = vi.fn();
    const testFrame = makeTestFrame(1, 1, 640, 400);
    const mockSession = {
      status: "streaming" as const,
      frame: testFrame,
      imageUrl: "blob:mock-url-1",
      browserState: null,
      hello: null,
      error: null,
      client: null,
      reconnect: vi.fn(),
      sendAck: vi.fn(),
      confirmPresented,
    };

    render(
      <RemoteBrowser
        baseUrl="http://localhost:8080"
        browserId="b1"
        deviceToken="mock-token"
        session={mockSession}
      />
    );

    const img = screen.getByAltText("Remote browser stream");
    expect(img).toBeDefined();
    expect(confirmPresented).not.toHaveBeenCalled();

    // Image load completes presentation
    await act(async () => {
      fireEvent.load(img);
    });

    expect(confirmPresented).toHaveBeenCalledWith(1, 1);
  });

  it("ensures RemoteBrowserWorkspace creates exactly ONE WebSocket connection (P1-09 dedup)", async () => {
    await act(async () => {
      render(
        <RemoteBrowserWorkspace
          baseUrl="http://localhost:8080"
          browserId="b1"
          deviceToken="mock-token"
        />
      );
    });

    const ws = await waitForSocket(0);
    expect(ws).toBeDefined();
    // Exactly 1 WebSocket client created (no duplicate client from RemoteBrowser inside Workspace)
    expect(MockWebSocket.instances.length).toBe(1);
  });

  it("validates documentGeneration and viewportRevision in driver mutation commands (P1-10)", async () => {
    const mockClient = {
      sendCommand: vi.fn().mockResolvedValue({ ok: true }),
      onDriverChanged: vi.fn(() => () => {}),
      onError: vi.fn(() => () => {}),
      onClose: vi.fn(() => () => {}),
      heartbeat: vi.fn().mockResolvedValue({}),
      claimDriver: vi.fn().mockResolvedValue({ leaseEpoch: "10" }),
      releaseDriver: vi.fn().mockResolvedValue({}),
    } as unknown as BrowserClient;

    let capturedDriver!: ReturnType<typeof useRemoteBrowserDriver>;
    function DriverTestComponent() {
      const driver = useRemoteBrowserDriver({
        client: mockClient,
        browserId: "b1",
        browserInstanceId: "bi1",
        desktopEpoch: "1",
        documentGeneration: "gen1",
        viewportRevision: "vrev1",
      });
      capturedDriver = driver;

      return <button data-testid="claim" onClick={() => driver.claim()} />;
    }

    render(<DriverTestComponent />);

    // Claim driver first
    await act(async () => {
      await capturedDriver.claim();
    });

    // Valid guards pass through
    await act(async () => {
      await capturedDriver.click({
        u: 0.5,
        v: 0.5,
        documentGeneration: "gen1",
        viewportRevision: "vrev1",
      });
    });
    expect(mockClient.sendCommand).toHaveBeenCalledTimes(1);

    // Stale documentGeneration throws
    await expect(
      capturedDriver.click({
        u: 0.5,
        v: 0.5,
        documentGeneration: "stale_gen",
        viewportRevision: "vrev1",
      })
    ).rejects.toThrow(/stale documentGeneration/i);

    // Stale viewportRevision throws
    await expect(
      capturedDriver.click({
        u: 0.5,
        v: 0.5,
        documentGeneration: "gen1",
        viewportRevision: "stale_vrev",
      })
    ).rejects.toThrow(/stale viewportRevision/i);
  });

  it("releases driving lease on unmount (P1-12)", async () => {
    const releaseSpy = vi.fn().mockResolvedValue({});
    const mockClient = {
      sendCommand: vi.fn().mockResolvedValue({ ok: true }),
      onDriverChanged: vi.fn(() => () => {}),
      onError: vi.fn(() => () => {}),
      onClose: vi.fn(() => () => {}),
      heartbeat: vi.fn().mockResolvedValue({}),
      claimDriver: vi.fn().mockResolvedValue({ leaseEpoch: "epoch-99" }),
      releaseDriver: releaseSpy,
    } as unknown as BrowserClient;

    function DriverHarness() {
      const driver = useRemoteBrowserDriver({
        client: mockClient,
        browserId: "b1",
        browserInstanceId: "bi1",
        desktopEpoch: "1",
        documentGeneration: "1",
      });

      return <button data-testid="claim" onClick={() => driver.claim()} />;
    }

    let unmountFn: () => void = () => {};
    await act(async () => {
      const rendered = render(<DriverHarness />);
      unmountFn = rendered.unmount;
    });

    // Claim driver
    await act(async () => {
      fireEvent.click(screen.getByTestId("claim"));
    });

    expect(releaseSpy).not.toHaveBeenCalled();

    // Unmount triggers releaseDriver
    await act(async () => {
      unmountFn();
    });

    expect(releaseSpy).toHaveBeenCalledWith("epoch-99");
  });

  it("invokes browserRemoteReclaim when clicking Reclaim Control (P1-03)", async () => {
    const mockReclaim = vi.fn().mockResolvedValue(1);

    render(
      <RemoteBrowserSharingIndicator
        isSharing={true}
        onReclaim={mockReclaim}
      />
    );

    const indicator = screen.getByTestId("remote-browser-sharing-indicator");
    expect(indicator).toBeDefined();

    const reclaimBtn = screen.getByTestId("reclaim-control-btn");
    await act(async () => {
      fireEvent.click(reclaimBtn);
    });

    expect(mockReclaim).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("reclaim-feedback-msg").textContent).toContain("Control reclaimed");
  });

  it("supports browserDriverRevoked: updates client state to viewing and transitions driver to revoked (R3)", async () => {
    const client = new BrowserClient("ws://localhost:8080/remote");
    const ws = await waitForSocket(0);

    // Initial driver state is viewing
    expect(client.driverState).toBe("viewing");

    // Driver claims
    const claimPromise = client.claimDriver("b1", "sub1");
    const claimReq = JSON.parse(ws.sentMessages[0] as string);
    ws.onmessage?.({
      data: JSON.stringify({
        type: "browserDriverClaimed",
        requestId: claimReq.requestId,
        leaseEpoch: "epoch-100",
        expiresAt: Date.now() + 15000,
      }),
    });
    await claimPromise;
    expect(client.driverState).toBe("driving");

    // Hook listening to driver state
    let capturedDriver!: ReturnType<typeof useRemoteBrowserDriver>;
    function DriverComponent() {
      const d = useRemoteBrowserDriver({
        client,
        browserId: "b1",
        browserInstanceId: "bi1",
        desktopEpoch: "1",
        documentGeneration: "1",
      });
      capturedDriver = d;
      return null;
    }
    render(<DriverComponent />);

    const revokedListener = vi.fn();
    client.onDriverRevoked(revokedListener);

    // Server sends browserDriverRevoked
    await act(async () => {
      ws.onmessage?.({
        data: JSON.stringify({
          type: "browserDriverRevoked",
          reason: "Desktop owner reclaimed lease",
          leaseEpoch: "epoch-100",
        }),
      });
    });

    expect(revokedListener).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "browserDriverRevoked",
        reason: "Desktop owner reclaimed lease",
        leaseEpoch: "epoch-100",
      }),
    );
    expect(client.driverState).toBe("viewing");
    expect(capturedDriver.driverState).toBe("revoked");
    expect(capturedDriver.leaseEpoch).toBeNull();
  });

  it("strictly derives point click metadata from displayedFrame committed via img onLoad (R8)", async () => {
    const handlePointClick = vi.fn();
    const frame1 = makeTestFrame(1, 1, 640, 400);
    frame1.metadata.documentGeneration = "101";
    frame1.metadata.viewportRevision = "201";
    const frame2 = makeTestFrame(2, 1, 640, 400);
    frame2.metadata.documentGeneration = "102";
    frame2.metadata.viewportRevision = "202";

    render(
      <RemoteBrowser
        baseUrl="http://localhost:8080"
        browserId="b1"
        deviceToken="tok-1"
        onPointClick={handlePointClick}
      />,
    );

    const ws = await waitForSocket(0);
    await establishStreaming(ws);

    const viewport = screen.getByTestId("remote-browser-viewport");
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      right: 640,
      bottom: 400,
      width: 640,
      height: 400,
      x: 0,
      y: 0,
      toJSON: () => {},
    });

    // Frame 1 arrives over network
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame1).buffer });
    });

    const img = screen.getByAltText("Remote browser stream");

    // Click BEFORE onLoad fires: should NOT invoke onPointClick because displayedFrame is not yet committed!
    await act(async () => {
      fireEvent.click(viewport, { clientX: 320, clientY: 200 });
    });
    expect(handlePointClick).not.toHaveBeenCalled();

    // Now image onLoad fires for Frame 1
    await act(async () => {
      fireEvent.load(img);
    });

    // Click AFTER onLoad: now derived from frame 1
    await act(async () => {
      fireEvent.click(viewport, { clientX: 320, clientY: 200 });
    });
    expect(handlePointClick).toHaveBeenCalledTimes(1);
    expect(handlePointClick.mock.calls[0][0].streamId).toBe(1);
    expect(handlePointClick.mock.calls[0][0].seq).toBe(1);
    expect(handlePointClick.mock.calls[0][0].sequenceNumber).toBe(1);
    expect(handlePointClick.mock.calls[0][0].documentGeneration).toBe("101");
    expect(handlePointClick.mock.calls[0][0].viewportRevision).toBe("201");
    expect(handlePointClick.mock.calls[0][0].browserInstanceId).toBe("bi-1");

    // Frame 2 arrives over network, but has NOT yet loaded
    handlePointClick.mockClear();
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame2).buffer });
    });

    // Click before frame 2's onLoad: MUST strictly use displayedFrame (frame 1), NOT uncommitted network frame 2!
    await act(async () => {
      fireEvent.click(viewport, { clientX: 320, clientY: 200 });
    });
    expect(handlePointClick).toHaveBeenCalledTimes(1);
    expect(handlePointClick.mock.calls[0][0].streamId).toBe(1);
    expect(handlePointClick.mock.calls[0][0].seq).toBe(1);
    expect(handlePointClick.mock.calls[0][0].sequenceNumber).toBe(1);
    expect(handlePointClick.mock.calls[0][0].documentGeneration).toBe("101");
    expect(handlePointClick.mock.calls[0][0].viewportRevision).toBe("201");
    expect(handlePointClick.mock.calls[0][0].browserInstanceId).toBe("bi-1");

    // Now onLoad fires for frame 2
    await act(async () => {
      fireEvent.load(img);
    });

    // Click now uses frame 2
    handlePointClick.mockClear();
    await act(async () => {
      fireEvent.click(viewport, { clientX: 320, clientY: 200 });
    });
    expect(handlePointClick).toHaveBeenCalledTimes(1);
    expect(handlePointClick.mock.calls[0][0].streamId).toBe(1);
    expect(handlePointClick.mock.calls[0][0].seq).toBe(2);
    expect(handlePointClick.mock.calls[0][0].sequenceNumber).toBe(2);
    expect(handlePointClick.mock.calls[0][0].documentGeneration).toBe("102");
    expect(handlePointClick.mock.calls[0][0].viewportRevision).toBe("202");
    expect(handlePointClick.mock.calls[0][0].browserInstanceId).toBe("bi-1");
  });

  it("carries snapshotId and mapRevision on fill and click (R9)", async () => {
    const mockClient = {
      sendCommand: vi.fn().mockResolvedValue({ ok: true }),
      onDriverChanged: vi.fn(() => () => {}),
      onDriverRevoked: vi.fn(() => () => {}),
      onError: vi.fn(() => () => {}),
      onClose: vi.fn(() => () => {}),
      heartbeat: vi.fn().mockResolvedValue({}),
      claimDriver: vi.fn().mockResolvedValue({ leaseEpoch: "10" }),
      releaseDriver: vi.fn().mockResolvedValue({}),
    } as unknown as BrowserClient;

    let capturedDriver!: ReturnType<typeof useRemoteBrowserDriver>;
    function DriverTest() {
      const driver = useRemoteBrowserDriver({
        client: mockClient,
        browserId: "b1",
        browserInstanceId: "bi1",
        desktopEpoch: "1",
        documentGeneration: "1",
        snapshotId: "state-snapshot-42",
        mapRevision: "state-map-7",
      });
      capturedDriver = driver;
      return <button data-testid="claim" onClick={() => driver.claim()} />;
    }

    render(<DriverTest />);
    await act(async () => {
      await capturedDriver.claim();
    });

    // 1. click using driver's default snapshotId and mapRevision
    await act(async () => {
      await capturedDriver.click({ reference: "btn-1" });
    });
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "click",
        params: expect.objectContaining({
          reference: "btn-1",
          snapshotId: "state-snapshot-42",
          mapRevision: "state-map-7",
        }),
      }),
    );

    // 2. click with explicit override of snapshotId and mapRevision
    await act(async () => {
      await capturedDriver.click("btn-2", {
        snapshotId: "explicit-snap-99",
        mapRevision: "explicit-map-88",
      });
    });
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "click",
        params: expect.objectContaining({
          selector: "btn-2",
          snapshotId: "explicit-snap-99",
          mapRevision: "explicit-map-88",
        }),
      }),
    );

    // 3. fill using driver's default snapshotId and mapRevision
    await act(async () => {
      await capturedDriver.fill("input-1", "hello");
    });
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "fill",
        params: expect.objectContaining({
          selector: "input-1",
          value: "hello",
          snapshotId: "state-snapshot-42",
          mapRevision: "state-map-7",
        }),
      }),
    );

    // 4. fill with options carrying snapshotId and mapRevision
    await act(async () => {
      await capturedDriver.fill("input-2", "world", {
        snapshotId: "snap-custom",
        mapRevision: "map-custom",
      });
    });
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "fill",
        params: expect.objectContaining({
          selector: "input-2",
          value: "world",
          snapshotId: "snap-custom",
          mapRevision: "map-custom",
        }),
      }),
    );
  });

  it("drops unrendered frames, sends pause protocol in background, does NOT send frame ACKs while hidden, and resumes cleanly (R10)", async () => {
    function BackgroundResumeHarness() {
      const state = useRemoteBrowser({
        baseUrl: "http://localhost:8080",
        browserId: "b1",
        deviceToken: "mock-token",
      });
      return (
        <div>
          <div data-testid="status">{state.status}</div>
          <div data-testid="imageUrl">{state.imageUrl ?? "none"}</div>
        </div>
      );
    }

    render(<BackgroundResumeHarness />);
    const ws = await waitForSocket(0);
    await establishStreaming(ws);

    // Frame 1 arrives and is acknowledged
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(1, 1)).buffer });
    });
    expect(screen.getByTestId("imageUrl").textContent).not.toBe("none");

    const sentBefore = ws.sentMessages.length;

    // Document becomes hidden (background)
    Object.defineProperty(document, "hidden", { value: true, configurable: true });
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });

    // Buffer cleared and status paused
    expect(screen.getByTestId("imageUrl").textContent).toBe("none");
    expect(screen.getByTestId("status").textContent).toBe("paused");

    // Background pause protocol signal sent; NO frame ACKs sent while hidden
    const messagesWhileBackgrounding = ws.sentMessages.slice(sentBefore).map((m) => JSON.parse(m as string));
    expect(messagesWhileBackgrounding).toContainEqual(
      expect.objectContaining({ type: "browserPause", browserId: "b1", streamId: 1 }),
    );
    expect(messagesWhileBackgrounding.filter((m) => m.type === "browserFrameAck")).toHaveLength(0);

    // Unrendered frame arrives while hidden: should be dropped, and NO frame ACK sent
    const sentBeforeDrop = ws.sentMessages.length;
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(makeTestFrame(2, 1)).buffer });
    });
    // Still empty buffer
    expect(screen.getByTestId("imageUrl").textContent).toBe("none");
    const acksForDropped = ws.sentMessages.slice(sentBeforeDrop).map((m) => JSON.parse(m as string));
    expect(acksForDropped.filter((m) => m.type === "browserFrameAck")).toHaveLength(0);

    // Document returns to visible
    const sentBeforeResume = ws.sentMessages.length;
    Object.defineProperty(document, "hidden", { value: false, configurable: true });
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });

    // Cleanly resumes to streaming with resume signal and without duplicate subscribe
    expect(screen.getByTestId("status").textContent).toBe("streaming");
    const messagesWhileResuming = ws.sentMessages.slice(sentBeforeResume).map((m) => JSON.parse(m as string));
    expect(messagesWhileResuming).toContainEqual(
      expect.objectContaining({ type: "browserResume", browserId: "b1", streamId: 1 }),
    );
    const subscribes = ws.sentMessages.filter((m) => {
      try {
        return JSON.parse(m as string).type === "browserSubscribe";
      } catch {
        return false;
      }
    });
    // Only original subscribe, no double subscribe!
    expect(subscribes.length).toBe(1);
  });

  it("client supports takeSnapshot, pause, and resume protocol operations (R9, R10)", async () => {
    const client = new BrowserClient("ws://localhost:8080/ws");
    const ws = MockWebSocket.instances[MockWebSocket.instances.length - 1];
    await drainAsync();

    // 1. client.takeSnapshot
    const snapshotPromise = client.takeSnapshot("b-snap-test");
    const snapReq = JSON.parse(ws.sentMessages[ws.sentMessages.length - 1] as string);
    expect(snapReq.type).toBe("browserSnapshot");
    expect(snapReq.browserId).toBe("b-snap-test");
    expect(snapReq.requestId).toBeDefined();

    // Server responds with browserSnapshot message
    await act(async () => {
      ws.onmessage?.({
        data: JSON.stringify({
          type: "browserSnapshot",
          requestId: snapReq.requestId,
          snapshotId: "snap-client-1",
          mapRevision: "55",
          root: { tag: "BODY" },
          elements: [{ id: "e1" }],
        }),
      });
    });

    const snapshotResult = await snapshotPromise;
    expect(snapshotResult.snapshotId).toBe("snap-client-1");
    expect(snapshotResult.mapRevision).toBe("55");

    // 2. client.pause
    client.pause("b-snap-test", 42);
    const pauseMsg = JSON.parse(ws.sentMessages[ws.sentMessages.length - 1] as string);
    expect(pauseMsg).toEqual({
      type: "browserPause",
      browserId: "b-snap-test",
      streamId: 42,
    });

    // 3. client.resume
    client.resume("b-snap-test", 42);
    const resumeMsg = JSON.parse(ws.sentMessages[ws.sentMessages.length - 1] as string);
    expect(resumeMsg).toEqual({
      type: "browserResume",
      browserId: "b-snap-test",
      streamId: 42,
    });

    client.close();
  });

  it("acquires snapshot before click and fill when needed and cleanly reconciles integer mapRevision (R9)", async () => {
    const mockClient = {
      sendCommand: vi.fn().mockResolvedValue({ ok: true }),
      takeSnapshot: vi.fn().mockResolvedValue({
        type: "browserSnapshot",
        requestId: "req-auto-snap",
        snapshotId: "snap-auto-acquired",
        mapRevision: 77, // integer on the wire
        root: {},
        elements: [],
      }),
      onDriverChanged: vi.fn(() => () => {}),
      onDriverRevoked: vi.fn(() => () => {}),
      onError: vi.fn(() => () => {}),
      onClose: vi.fn(() => () => {}),
      heartbeat: vi.fn().mockResolvedValue({}),
      claimDriver: vi.fn().mockResolvedValue({ leaseEpoch: "10" }),
      releaseDriver: vi.fn().mockResolvedValue({}),
    } as unknown as BrowserClient;

    let capturedDriver!: ReturnType<typeof useRemoteBrowserDriver>;
    function AutoSnapDriverTest() {
      // Driver created WITHOUT snapshotId or mapRevision
      const driver = useRemoteBrowserDriver({
        client: mockClient,
        browserId: "b-auto",
        browserInstanceId: "bi-auto",
        desktopEpoch: "1",
        documentGeneration: "1",
      });
      capturedDriver = driver;
      return <div />;
    }

    render(<AutoSnapDriverTest />);
    await act(async () => {
      await capturedDriver.claim();
    });

    expect(capturedDriver.snapshotId).toBeNull();
    expect(capturedDriver.mapRevision).toBeNull();

    // 1. click on an element reference when snapshotId is missing: should automatically acquire snapshot
    await act(async () => {
      await capturedDriver.click({ reference: "e1" });
    });

    expect(mockClient.takeSnapshot).toHaveBeenCalledWith("b-auto");
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "click",
        params: expect.objectContaining({
          reference: "e1",
          snapshotId: "snap-auto-acquired",
          mapRevision: "77", // Integer 77 cleanly reconciled to decimal string "77"
        }),
      }),
    );
    expect(capturedDriver.snapshotId).toBe("snap-auto-acquired");
    expect(capturedDriver.mapRevision).toBe("77");

    // 2. fill should now carry the stored snapshotId and mapRevision
    await act(async () => {
      await capturedDriver.fill("e2", "typed text");
    });
    expect(mockClient.sendCommand).toHaveBeenLastCalledWith(
      expect.objectContaining({
        command: "fill",
        params: expect.objectContaining({
          reference: "e2",
          value: "typed text",
          snapshotId: "snap-auto-acquired",
          mapRevision: "77",
        }),
      }),
    );

    // 3. direct call to driver.takeSnapshot
    mockClient.takeSnapshot = vi.fn().mockResolvedValue({
      type: "browserSnapshot",
      requestId: "req-manual-snap",
      snapshotId: "snap-manual-99",
      mapRevision: "88",
    });
    let explicitSnapResult!: any;
    await act(async () => {
      explicitSnapResult = await capturedDriver.takeSnapshot();
    });
    expect(explicitSnapResult.snapshotId).toBe("snap-manual-99");
    expect(explicitSnapResult.mapRevision).toBe("88");
    expect(capturedDriver.snapshotId).toBe("snap-manual-99");
    expect(capturedDriver.mapRevision).toBe("88");
  });

  it("rejects late-loading frame with older seq and preserves committed frame (R8 exact image load binding)", async () => {
    const handlePointClick = vi.fn();
    const frame1 = makeTestFrame(1, 1, 640, 400);
    frame1.metadata.documentGeneration = "101";
    frame1.metadata.viewportRevision = "201";
    const frame2 = makeTestFrame(2, 1, 640, 400);
    frame2.metadata.documentGeneration = "102";
    frame2.metadata.viewportRevision = "202";

    render(
      <RemoteBrowser
        baseUrl="http://localhost:8080"
        browserId="b1"
        deviceToken="tok-1"
        onPointClick={handlePointClick}
      />,
    );

    const ws = await waitForSocket(0);
    await establishStreaming(ws);

    const viewport = screen.getByTestId("remote-browser-viewport");
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      right: 640,
      bottom: 400,
      width: 640,
      height: 400,
      x: 0,
      y: 0,
      toJSON: () => {},
    });

    // Frame 1 arrives over network
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame1).buffer });
    });
    // Frame 2 arrives before frame 1 finishes rendering
    await act(async () => {
      ws.onmessage?.({ data: encodeFrame(frame2).buffer });
    });

    const img = screen.getByAltText("Remote browser stream");
    // Frame 2's onLoad fires first (it loaded faster)
    await act(async () => {
      fireEvent.load(img);
    });

    // Clicks are now derived from frame 2
    await act(async () => {
      fireEvent.click(viewport, { clientX: 320, clientY: 200 });
    });
    expect(handlePointClick).toHaveBeenCalledTimes(1);
    expect(handlePointClick.mock.calls[0][0].seq).toBe(2);
    expect(handlePointClick.mock.calls[0][0].documentGeneration).toBe("102");
    expect(handlePointClick.mock.calls[0][0].viewportRevision).toBe("202");
  });
});
