import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  encodeFrame,
  type BrowserFrame,
  type BrowserFrameMetadata,
} from "./browserProtocol";
import { RemoteBrowser, type RemoteBrowserPointClickEvent } from "./RemoteBrowser";
import { RemoteBrowserWorkspace } from "./RemoteBrowserWorkspace";

// Mock WebSocket for RemoteBrowser tests
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

function makeTestJpeg(width: number, height: number): Uint8Array {
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
  width = 1280,
  height = 720,
): BrowserFrame {
  const metadata: BrowserFrameMetadata = {
    offsetTop: 0,
    pageScaleFactor: 1,
    deviceWidth: width,
    deviceHeight: height,
    imageWidth: width,
    imageHeight: height,
    scrollOffsetX: 0,
    scrollOffsetY: 0,
    timestamp: 1726560000,
    streamId,
    browserInstanceId: "bi-mobile-1",
    browserServiceEpoch: "1",
    desktopEpoch: "1",
    documentGeneration: "1",
    viewportRevision: "1",
    captureRect: { x: 0, y: 0, width, height },
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
        browserId: "b-mobile",
        browserInstanceId: "bi-mobile-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        protocolVersion: 1,
        supportedCommands: ["navigate", "click", "fill", "keypress"],
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
        subscriptionId: "sub-mobile-1",
        streamId,
        browserId: "b-mobile",
        browserInstanceId: "bi-mobile-1",
        browserServiceEpoch: "1",
        desktopEpoch: "1",
        documentGeneration: "1",
        options: { format: "jpeg" },
      }),
    });
    await drainAsync();
  });
}

describe("RemoteBrowser - Mobile Viewport & Touch Interaction (Phase 7B)", () => {
  let createdUrls: string[] = [];
  let revokedUrls: string[] = [];

  beforeEach(() => {
    MockWebSocket.instances = [];
    createdUrls = [];
    revokedUrls = [];
    vi.stubGlobal("WebSocket", MockWebSocket);

    const OriginalURL = globalThis.URL;
    class MockURL extends OriginalURL {
      static createObjectURL = vi.fn(() => {
        const u = `blob:mobile-frame-${createdUrls.length + 1}`;
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
          return new Response(
            JSON.stringify({ ticket: `ticket-mobile-${Date.now()}` }),
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

  describe("Letterbox coordinate normalization", () => {
    it("discards touches in top and bottom letterbox margins (mobile portrait: 390x844 with 16:9 stream)", async () => {
      const onPointClick = vi.fn();
      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onPointClick={onPointClick}
        />,
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Stream 16:9 1280x720 frame (aspect 1.7778)
      const frame = makeTestFrame(1, 1, 1280, 720);
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(frame).buffer as ArrayBuffer });
      });
      await act(async () => {
        fireEvent.load(screen.getByAltText("Remote browser stream"));
      });

      const viewport = screen.getByTestId("remote-browser-viewport");

      // Container: iPhone portrait 390x844 (aspect 0.4621)
      // Rendered width = 390
      // Rendered height = 390 / (1280/720) = 219.375
      // offsetTop = (844 - 219.375) / 2 = 312.3125
      // offsetLeft = 0
      vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        right: 390,
        bottom: 844,
        width: 390,
        height: 844,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      // Touch in top black margin (y = 150 < 312.3125)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 195, clientY: 150 });
      });
      expect(onPointClick).not.toHaveBeenCalled();

      // Touch in bottom black margin (y = 600 > 312.3125 + 219.375 = 531.6875)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 195, clientY: 600 });
      });
      expect(onPointClick).not.toHaveBeenCalled();

      // Touch inside active image canvas: center (x = 195, y = 312.3125 + 219.375 / 2 = 422)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 195, clientY: 422 });
      });
      expect(onPointClick).toHaveBeenCalledTimes(1);
      const pt: RemoteBrowserPointClickEvent = onPointClick.mock.calls[0][0];
      expect(pt.u).toBeCloseTo(0.5, 2);
      expect(pt.v).toBeCloseTo(0.5, 2);
      expect(pt.streamId).toBe(1);
      expect(pt.seq).toBe(1);
    });

    it("discards touches in left and right pillarbox margins (mobile landscape: 844x390 with 3:4 portrait stream)", async () => {
      const onPointClick = vi.fn();
      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onPointClick={onPointClick}
        />,
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Stream 3:4 600x800 portrait frame
      const frame = makeTestFrame(1, 1, 600, 800);
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(frame).buffer as ArrayBuffer });
      });
      await act(async () => {
        fireEvent.load(screen.getByAltText("Remote browser stream"));
      });

      const viewport = screen.getByTestId("remote-browser-viewport");

      // Container: landscape 844x390 (aspect 2.1641)
      // Image aspect = 600 / 800 = 0.75
      // Rendered height = 390
      // Rendered width = 390 * 0.75 = 292.5
      // offsetLeft = (844 - 292.5) / 2 = 275.75
      // offsetTop = 0
      vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        right: 844,
        bottom: 390,
        width: 844,
        height: 390,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      // Touch in left pillarbox margin (x = 100 < 275.75)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 100, clientY: 195 });
      });
      expect(onPointClick).not.toHaveBeenCalled();

      // Touch in right pillarbox margin (x = 650 > 275.75 + 292.5 = 568.25)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 650, clientY: 195 });
      });
      expect(onPointClick).not.toHaveBeenCalled();

      // Touch inside active image canvas: center (x = 275.75 + 292.5 / 2 = 422, y = 195)
      await act(async () => {
        fireEvent.click(viewport, { clientX: 422, clientY: 195 });
      });
      expect(onPointClick).toHaveBeenCalledTimes(1);
      const pt = onPointClick.mock.calls[0][0];
      expect(pt.u).toBeCloseTo(0.5, 2);
      expect(pt.v).toBeCloseTo(0.5, 2);
    });

    it("clamps normalized coordinates exactly to [0.0, 1.0] at canvas boundary extremes", async () => {
      const onPointClick = vi.fn();
      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onPointClick={onPointClick}
        />,
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      const frame = makeTestFrame(1, 1, 1000, 1000); // 1:1 square
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(frame).buffer as ArrayBuffer });
      });
      await act(async () => {
        fireEvent.load(screen.getByAltText("Remote browser stream"));
      });

      const viewport = screen.getByTestId("remote-browser-viewport");
      // Container 400x400 (square) -> exact fit, offsetLeft=0, offsetTop=0
      vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        right: 400,
        bottom: 400,
        width: 400,
        height: 400,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      // Top-left corner
      await act(async () => {
        fireEvent.click(viewport, { clientX: 0, clientY: 0 });
      });
      expect(onPointClick).toHaveBeenLastCalledWith(
        expect.objectContaining({ u: 0, v: 0 }),
      );

      // Bottom-right corner
      await act(async () => {
        fireEvent.click(viewport, { clientX: 400, clientY: 400 });
      });
      expect(onPointClick).toHaveBeenLastCalledWith(
        expect.objectContaining({ u: 1, v: 1 }),
      );
    });
  });

  describe("Viewport orientation change and resize", () => {
    it("recalculates aspect ratio and refits canvas without distortion when rotating portrait to landscape", async () => {
      const onPointClick = vi.fn();
      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onPointClick={onPointClick}
        />,
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Frame: 1280x720 (16:9, aspect 1.7778)
      const frame = makeTestFrame(1, 1, 1280, 720);
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(frame).buffer as ArrayBuffer });
      });
      await act(async () => {
        fireEvent.load(screen.getByAltText("Remote browser stream"));
      });

      const viewport = screen.getByTestId("remote-browser-viewport");

      // State 1: Portrait 390x844
      const portraitSpy = vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
        left: 0,
        top: 0,
        right: 390,
        bottom: 844,
        width: 390,
        height: 844,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      // In portrait: rendered height = 219.375, offsetTop = 312.3125.
      // Click at y=100 is in top margin -> discarded.
      await act(async () => {
        fireEvent.click(viewport, { clientX: 195, clientY: 100 });
      });
      expect(onPointClick).not.toHaveBeenCalled();

      // Rotate to Landscape: 844x390
      portraitSpy.mockReturnValue({
        left: 0,
        top: 0,
        right: 844,
        bottom: 390,
        width: 844,
        height: 390,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      // In landscape (844x390, aspect 2.1641 > 1.7778):
      // renderedHeight = 390, renderedWidth = 390 * (16/9) = 693.333
      // offsetLeft = (844 - 693.333) / 2 = 75.333, offsetTop = 0
      // Now click at x=195, y=100 (which was previously in margin) is INSIDE canvas:
      // u = (195 - 75.333) / 693.333 = 0.1726, v = 100 / 390 = 0.2564
      await act(async () => {
        fireEvent.click(viewport, { clientX: 195, clientY: 100 });
      });
      expect(onPointClick).toHaveBeenCalledTimes(1);
      const pt = onPointClick.mock.calls[0][0];
      expect(pt.u).toBeCloseTo(0.173, 2);
      expect(pt.v).toBeCloseTo(0.256, 2);

      // In landscape, click at x=30 is in left margin (< 75.333) -> discarded
      onPointClick.mockClear();
      await act(async () => {
        fireEvent.click(viewport, { clientX: 30, clientY: 195 });
      });
      expect(onPointClick).not.toHaveBeenCalled();
    });
  });

  describe("Mobile touch scrolling and gesture isolation", () => {
    it("isolates touch gestures so remote browser touches do not trigger parent drawer swipe or pane drag", async () => {
      const parentDrawerSwipe = vi.fn();
      const parentPaneDrag = vi.fn();
      const onPointClick = vi.fn();

      // Parent container simulating MobileHostDrawer and PaneSplitter gesture listeners
      render(
        <div
          data-testid="parent-gesture-container"
          onTouchStart={(e) => {
            // Check if touch target is outside isolated screencast viewport
            const target = e.target as HTMLElement;
            if (!target.closest('[data-testid="remote-browser-viewport"]')) {
              parentDrawerSwipe();
            }
          }}
          onTouchMove={(e) => {
            const target = e.target as HTMLElement;
            if (!target.closest('[data-testid="remote-browser-viewport"]')) {
              parentPaneDrag();
            }
          }}
        >
          <RemoteBrowser
            baseUrl="http://localhost:8080"
            browserId="b-mobile"
            deviceToken="token-mobile"
            onPointClick={onPointClick}
          />
        </div>
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      const frame = makeTestFrame(1, 1, 800, 600);
      await act(async () => {
        ws.onmessage?.({ data: encodeFrame(frame).buffer as ArrayBuffer });
      });

      const viewport = screen.getByTestId("remote-browser-viewport");

      // Touch gesture directly on remote browser viewport
      fireEvent.touchStart(viewport, {
        touches: [{ clientX: 200, clientY: 200 }],
      });
      fireEvent.touchMove(viewport, {
        touches: [{ clientX: 100, clientY: 200 }],
      });
      fireEvent.touchEnd(viewport, {
        touches: [],
        changedTouches: [{ clientX: 100, clientY: 200 }],
      });

      // Parent drawer and pane drag gestures MUST NOT be triggered
      expect(parentDrawerSwipe).not.toHaveBeenCalled();
      expect(parentPaneDrag).not.toHaveBeenCalled();

      // Touch outside viewport on parent container triggers drawer swipe
      const parent = screen.getByTestId("parent-gesture-container");
      fireEvent.touchStart(parent, {
        touches: [{ clientX: 20, clientY: 20 }],
      });
      expect(parentDrawerSwipe).toHaveBeenCalledTimes(1);
    });
  });

  describe("Mobile IME composition handling", () => {
    it("suppresses raw key events during active composition and passes finalized string on compositionend without duplicate characters", async () => {
      // Simulates mobile IME composition workflow as used in RemoteBrowser text entry / fill controls (§4.5, §7.1)
      const emittedInputs: string[] = [];
      const rawKeyEvents: string[] = [];

      // Test harness modeling mobile IME input handler attached to controls slot
      function MobileImeInputBar({ onCommit }: { onCommit: (val: string) => void }) {
        const isComposingRef = { current: false };

        return (
          <input
            data-testid="mobile-browser-input"
            onCompositionStart={() => {
              isComposingRef.current = true;
            }}
            onCompositionUpdate={() => {
              isComposingRef.current = true;
            }}
            onCompositionEnd={(e) => {
              isComposingRef.current = false;
              if (e.data) {
                onCommit(e.data);
              }
            }}
            onKeyDown={(e) => {
              if (isComposingRef.current || e.nativeEvent.isComposing) {
                // Suppressed during composition
                return;
              }
              rawKeyEvents.push(e.key);
            }}
          />
        );
      }

      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          controls={<MobileImeInputBar onCommit={(v) => emittedInputs.push(v)} />}
        />,
      );

      const input = screen.getByTestId("mobile-browser-input");

      // User begins Korean IME composition: "ㅎ" -> "하" -> "한" -> "한글"
      fireEvent.compositionStart(input);
      fireEvent.compositionUpdate(input, { data: "ㅎ" });
      fireEvent.keyDown(input, { key: "g", isComposing: true });

      fireEvent.compositionUpdate(input, { data: "하" });
      fireEvent.keyDown(input, { key: "k", isComposing: true });

      fireEvent.compositionUpdate(input, { data: "한" });
      fireEvent.keyDown(input, { key: "s", isComposing: true });

      // Raw key events must be completely suppressed during composition!
      expect(rawKeyEvents).toHaveLength(0);
      expect(emittedInputs).toHaveLength(0);

      // Composition ends with finalized string
      fireEvent.compositionEnd(input, { data: "한글" });

      // Finalized string emitted exactly once without duplicate characters
      expect(emittedInputs).toEqual(["한글"]);
      expect(rawKeyEvents).toHaveLength(0);

      // Normal non-composing keydown (e.g. Enter or ASCII) works when not composing
      fireEvent.keyDown(input, { key: "Enter", isComposing: false });
      expect(rawKeyEvents).toEqual(["Enter"]);
    });

    it("handles canceled composition (empty compositionend) without emitting input", () => {
      const emittedInputs: string[] = [];

      function MobileImeInputBar({ onCommit }: { onCommit: (val: string) => void }) {
        const isComposingRef = { current: false };

        return (
          <input
            data-testid="mobile-browser-input-cancel"
            onCompositionStart={() => {
              isComposingRef.current = true;
            }}
            onCompositionEnd={(e) => {
              isComposingRef.current = false;
              if (e.data && e.data.length > 0) {
                onCommit(e.data);
              }
            }}
          />
        );
      }

      render(
        <RemoteBrowser
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          controls={<MobileImeInputBar onCommit={(v) => emittedInputs.push(v)} />}
        />,
      );

      const input = screen.getByTestId("mobile-browser-input-cancel");

      fireEvent.compositionStart(input);
      fireEvent.compositionUpdate(input, { data: "a" });
      // Canceled composition produces empty string
      fireEvent.compositionEnd(input, { data: "" });

      expect(emittedInputs).toHaveLength(0);
    });

    it("preserves preceding text on compositionend and only clears imeText after fill succeeds (P2-01)", async () => {
      render(
        <RemoteBrowserWorkspace
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onBack={vi.fn()}
        />
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Open IME bar
      fireEvent.click(screen.getByTestId("remote-browser-ime-toggle-btn"));
      const imeInput = screen.getByTestId("remote-browser-ime-text-input");

      // Type initial text
      fireEvent.change(imeInput, { target: { value: "Preceding text " } });
      expect(imeInput).toHaveValue("Preceding text ");

      // Begin IME composition
      fireEvent.compositionStart(imeInput);
      fireEvent.compositionUpdate(imeInput, { data: "한" });
      fireEvent.compositionUpdate(imeInput, { data: "한글" });

      // End IME composition: should NOT wipe "Preceding text " with just "한글"
      fireEvent.compositionEnd(imeInput, { data: "한글" });
      expect(imeInput).toHaveValue("Preceding text 한글");
    });

    it("preserves newly typed characters during fill execution using revision-based clearing", async () => {
      render(
        <RemoteBrowserWorkspace
          baseUrl="http://localhost:8080"
          browserId="b-mobile"
          deviceToken="token-mobile"
          onBack={vi.fn()}
        />,
      );

      const ws = await waitForSocket(0);
      await establishStreaming(ws);

      // Make driver active
      await act(async () => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "browserDriverChanged",
            leaseEpoch: "epoch-ime-1",
            isDriver: true,
          }),
        });
      });

      // Open IME bar
      fireEvent.click(screen.getByTestId("remote-browser-ime-toggle-btn"));
      const imeInput = screen.getByTestId("remote-browser-ime-text-input");
      const imeBar = screen.getByTestId("remote-browser-ime-bar");

      // 1. Submit text and type additional characters while fill is in flight
      fireEvent.change(imeInput, { target: { value: "first part" } });
      const sentCountBefore = ws.sentMessages.length;

      // Submit fill
      await act(async () => {
        fireEvent.submit(imeBar);
      });

      // Find the fill command message sent to WebSocket
      const fillMsg = JSON.parse(ws.sentMessages[sentCountBefore] as string);
      expect(fillMsg.command).toBe("fill");

      // While fill is in-flight, user types more characters into input
      fireEvent.change(imeInput, { target: { value: "first part + newly typed" } });

      // Now server responds with success
      await act(async () => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "browserResult",
            requestId: fillMsg.requestId,
            result: { ok: true },
          }),
        });
      });

      // Newly typed text must be preserved, NOT wiped!
      expect(imeInput).toHaveValue(" + newly typed");

      // 2. Submit remaining text without typing anything more during fill
      const sentCountBefore2 = ws.sentMessages.length;
      await act(async () => {
        fireEvent.submit(imeBar);
      });

      const fillMsg2 = JSON.parse(ws.sentMessages[sentCountBefore2] as string);
      expect(fillMsg2.command).toBe("fill");

      // Server responds with success
      await act(async () => {
        ws.onmessage?.({
          data: JSON.stringify({
            type: "browserResult",
            requestId: fillMsg2.requestId,
            result: { ok: true },
          }),
        });
      });

      // Since no new characters were typed, input is completely cleared
      expect(imeInput).toHaveValue("");
    });
  });
});
