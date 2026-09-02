import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  RemoteTerminal,
} from "./RemoteTerminal";

class MockWebSocket {
  static readonly OPEN = 1;
  static latest: MockWebSocket | null = null;
  static instances: MockWebSocket[] = [];
  readonly url: string;
  binaryType = "arraybuffer";
  readyState = MockWebSocket.OPEN;
  send = vi.fn();
  close = vi.fn();
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.latest = this;
    MockWebSocket.instances.push(this);
  }
}

function socket(): MockWebSocket {
  const value = MockWebSocket.latest;
  if (!value) throw new Error("Expected remote terminal WebSocket");
  return value;
}

function surface(): HTMLElement {
  return screen.getByTestId("remote-terminal-grid");
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

function swipe(
  element: HTMLElement,
  from: { x: number; y: number },
  to: { x: number; y: number },
) {
  fireEvent.touchStart(element, {
    touches: [{ clientX: from.x, clientY: from.y }],
  });
  fireEvent.touchMove(element, {
    touches: [{ clientX: to.x, clientY: to.y }],
  });
  fireEvent.touchEnd(element, {
    touches: [],
    changedTouches: [{ clientX: to.x, clientY: to.y }],
  });
}

function pinch(
  element: HTMLElement,
  startTouches: [{ x: number; y: number }, { x: number; y: number }],
  moveTouches: [{ x: number; y: number }, { x: number; y: number }],
) {
  fireEvent.touchStart(element, {
    touches: [
      { clientX: startTouches[0].x, clientY: startTouches[0].y },
      { clientX: startTouches[1].x, clientY: startTouches[1].y },
    ],
  });
  fireEvent.touchMove(element, {
    touches: [
      { clientX: moveTouches[0].x, clientY: moveTouches[0].y },
      { clientX: moveTouches[1].x, clientY: moveTouches[1].y },
    ],
  });
  fireEvent.touchEnd(element, {
    touches: [],
    changedTouches: [
      { clientX: moveTouches[0].x, clientY: moveTouches[0].y },
      { clientX: moveTouches[1].x, clientY: moveTouches[1].y },
    ],
  });
}

describe("RemoteTerminal touch gestures", () => {
  beforeEach(() => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) {
        const parent = this.parentElement;
        const parsedFontSize = parent?.style.fontSize ? Number.parseFloat(parent.style.fontSize) : 13;
        const fontSize = Number.isFinite(parsedFontSize) && parsedFontSize > 0 ? parsedFontSize : 13;
        return rect(fontSize * 0.6, fontSize * 1.2);
      }
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });
  });

  afterEach(() => {
    cleanup();
    MockWebSocket.latest = null;
    MockWebSocket.instances = [];
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("fires onSwipeNextTab on horizontal left swipe past threshold and onSwipePreviousTab on right swipe", () => {
    const onSwipeNextTab = vi.fn();
    const onSwipePreviousTab = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSwipeNextTab={onSwipeNextTab}
        onSwipePreviousTab={onSwipePreviousTab}
      />,
    );

    const grid = surface();

    // Swipe left: finger moves from 200px to 100px (deltaX = -100px)
    swipe(grid, { x: 200, y: 100 }, { x: 100, y: 100 });
    expect(onSwipeNextTab).toHaveBeenCalledTimes(1);
    expect(onSwipePreviousTab).not.toHaveBeenCalled();

    onSwipeNextTab.mockClear();
    onSwipePreviousTab.mockClear();

    // Swipe right: finger moves from 100px to 200px (deltaX = +100px)
    swipe(grid, { x: 100, y: 100 }, { x: 200, y: 100 });
    expect(onSwipePreviousTab).toHaveBeenCalledTimes(1);
    expect(onSwipeNextTab).not.toHaveBeenCalled();
  });

  it("does not change tab on mostly-vertical swipes or sub-threshold horizontal swipes", () => {
    const onSwipeNextTab = vi.fn();
    const onSwipePreviousTab = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSwipeNextTab={onSwipeNextTab}
        onSwipePreviousTab={onSwipePreviousTab}
      />,
    );

    const grid = surface();

    // Pure vertical swipe (scrolling)
    swipe(grid, { x: 100, y: 200 }, { x: 100, y: 50 });
    expect(onSwipeNextTab).not.toHaveBeenCalled();
    expect(onSwipePreviousTab).not.toHaveBeenCalled();

    // Diagonal swipe with larger vertical delta than horizontal
    swipe(grid, { x: 100, y: 200 }, { x: 140, y: 80 });
    expect(onSwipeNextTab).not.toHaveBeenCalled();
    expect(onSwipePreviousTab).not.toHaveBeenCalled();

    // Sub-threshold horizontal swipe
    swipe(grid, { x: 100, y: 100 }, { x: 115, y: 100 });
    expect(onSwipeNextTab).not.toHaveBeenCalled();
    expect(onSwipePreviousTab).not.toHaveBeenCalled();
  });

  it("increases font size on pinch-out and decreases on pinch-in, clamped to min and max", () => {
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const grid = surface();

    const initialFontSize = Number.parseFloat(grid.style.fontSize);
    expect(initialFontSize).toBe(13);

    // Pinch-out: start distance 100 (100 to 200), end distance 200 (50 to 250) -> ratio 2.0
    pinch(
      grid,
      [{ x: 100, y: 100 }, { x: 200, y: 100 }],
      [{ x: 50, y: 100 }, { x: 250, y: 100 }],
    );

    const enlargedFontSize = Number.parseFloat(grid.style.fontSize);
    expect(enlargedFontSize).toBeGreaterThan(initialFontSize);
    expect(enlargedFontSize).toBe(26);

    // Pinch-in: start distance 200 (50 to 250), end distance 100 (100 to 200) -> ratio 0.5
    pinch(
      grid,
      [{ x: 50, y: 100 }, { x: 250, y: 100 }],
      [{ x: 100, y: 100 }, { x: 200, y: 100 }],
    );

    const reducedFontSize = Number.parseFloat(grid.style.fontSize);
    expect(reducedFontSize).toBeLessThan(enlargedFontSize);
    expect(reducedFontSize).toBe(13);

    // Extreme pinch-out: clamped to MAX_TERMINAL_FONT_SIZE
    pinch(
      grid,
      [{ x: 100, y: 100 }, { x: 200, y: 100 }],
      [{ x: 0, y: 100 }, { x: 600, y: 100 }],
    );
    expect(Number.parseFloat(grid.style.fontSize)).toBe(MAX_TERMINAL_FONT_SIZE);

    // Extreme pinch-in: clamped to MIN_TERMINAL_FONT_SIZE
    pinch(
      grid,
      [{ x: 0, y: 100 }, { x: 600, y: 100 }],
      [{ x: 295, y: 100 }, { x: 305, y: 100 }],
    );
    expect(Number.parseFloat(grid.style.fontSize)).toBe(MIN_TERMINAL_FONT_SIZE);
  });

  it("sends recomputed cols and rows through WebSocket resize path after pinch settles", async () => {
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const grid = surface();

    act(() => socket().onopen?.());

    // Socket open initializes geometry; clear initial call records
    socket().send.mockClear();

    // Pinch out to double font size
    pinch(
      grid,
      [{ x: 100, y: 100 }, { x: 200, y: 100 }],
      [{ x: 50, y: 100 }, { x: 250, y: 100 }],
    );

    await waitFor(() => {
      expect(socket().send).toHaveBeenCalledWith(
        expect.stringMatching(/"type":"resize"/),
      );
    });

    const resizeCalls = socket().send.mock.calls.filter(([arg]) =>
      typeof arg === "string" && arg.includes('"type":"resize"'),
    );
    expect(resizeCalls.length).toBeGreaterThan(0);
    const lastPayload = JSON.parse(resizeCalls[resizeCalls.length - 1][0] as string);
    // At font size 26 (2x initial 13): cell width is 26 * 0.6 = 15.6, height = 26 * 1.2 = 31.2
    // surface 800x400 -> cols = floor(800 / 15.6) = 51, rows = floor(400 / 31.2) = 12
    expect(lastPayload).toEqual({
      type: "resize",
      cols: Math.floor(800 / (26 * 0.6)),
      rows: Math.floor(400 / (26 * 1.2)),
    });
  });

  it("does not trigger tab change when releasing pinch fingers", () => {
    const onSwipeNextTab = vi.fn();
    const onSwipePreviousTab = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSwipeNextTab={onSwipeNextTab}
        onSwipePreviousTab={onSwipePreviousTab}
      />,
    );

    const grid = surface();

    pinch(
      grid,
      [{ x: 100, y: 100 }, { x: 200, y: 100 }],
      [{ x: 50, y: 100 }, { x: 250, y: 100 }],
    );

    expect(onSwipeNextTab).not.toHaveBeenCalled();
    expect(onSwipePreviousTab).not.toHaveBeenCalled();
  });

  it("sends scroll messages on vertical single-finger drag when socket is open", () => {
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const grid = surface();

    act(() => socket().onopen?.());
    socket().send.mockClear();

    // Drag up: finger moves from 200px to 140px (deltaY = -60px)
    // font size 13 -> cell height 13 * 1.2 = 15.6px
    // Math.trunc(-(-60) / 15.6) = Math.trunc(3.846) = 3 rows
    fireEvent.touchStart(grid, {
      touches: [{ clientX: 100, clientY: 200 }],
    });
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 140 }],
    });
    expect(socket().send).toHaveBeenCalledWith(JSON.stringify({ type: "scroll", rows: 3 }));

    fireEvent.touchEnd(grid, {
      touches: [],
      changedTouches: [{ clientX: 100, clientY: 140 }],
    });

    socket().send.mockClear();

    // Drag down: finger moves from 100px to 160px (deltaY = +60px)
    // Math.trunc(-(60) / 15.6) = Math.trunc(-3.846) = -3 rows
    fireEvent.touchStart(grid, {
      touches: [{ clientX: 100, clientY: 100 }],
    });
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 160 }],
    });
    expect(socket().send).toHaveBeenCalledWith(JSON.stringify({ type: "scroll", rows: -3 }));
  });

  it("does not send scroll messages during horizontal swipe tab-switch gesture", () => {
    const onSwipeNextTab = vi.fn();
    const onSwipePreviousTab = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSwipeNextTab={onSwipeNextTab}
        onSwipePreviousTab={onSwipePreviousTab}
      />,
    );
    const grid = surface();

    act(() => socket().onopen?.());
    socket().send.mockClear();

    swipe(grid, { x: 200, y: 100 }, { x: 100, y: 100 });
    expect(onSwipeNextTab).toHaveBeenCalledTimes(1);
    expect(socket().send).not.toHaveBeenCalledWith(expect.stringMatching(/"type":"scroll"/));
  });

  it("does not fire swipe callbacks on touch end after a vertical scroll gesture", () => {
    const onSwipeNextTab = vi.fn();
    const onSwipePreviousTab = vi.fn();

    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onSwipeNextTab={onSwipeNextTab}
        onSwipePreviousTab={onSwipePreviousTab}
      />,
    );
    const grid = surface();

    act(() => socket().onopen?.());

    // Single finger vertical drag that moves > 8px vertically
    fireEvent.touchStart(grid, {
      touches: [{ clientX: 100, clientY: 200 }],
    });
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 150 }],
    });
    // Even if it then moves horizontally past 40px, it was locked to scroll
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 30, clientY: 150 }],
    });
    fireEvent.touchEnd(grid, {
      touches: [],
      changedTouches: [{ clientX: 30, clientY: 150 }],
    });

    expect(onSwipeNextTab).not.toHaveBeenCalled();
    expect(onSwipePreviousTab).not.toHaveBeenCalled();
  });

  it("does not send scroll message on vertical touch drag when socket is not open", () => {
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const grid = surface();

    socket().readyState = 0;
    socket().send.mockClear();

    fireEvent.touchStart(grid, {
      touches: [{ clientX: 100, clientY: 200 }],
    });
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 140 }],
    });
    expect(socket().send).not.toHaveBeenCalledWith(expect.stringMatching(/"type":"scroll"/));
  });

  it("throttles touch scroll messages to at most one per 33ms", () => {
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const grid = surface();

    act(() => socket().onopen?.());
    socket().send.mockClear();

    const nowSpy = vi.spyOn(Date, "now");
    nowSpy.mockReturnValue(1000);

    // Initial move at t=1000 sends first scroll (delta -35px -> 2 rows @ 15.6px/row)
    fireEvent.touchStart(grid, {
      touches: [{ clientX: 100, clientY: 200 }],
    });
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 165 }],
    });
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: 2 }));

    // Second move at t=1010 (< 33ms elapsed): should NOT send yet, but delta accumulates
    nowSpy.mockReturnValue(1010);
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 130 }],
    });
    expect(socket().send).toHaveBeenCalledTimes(1);

    // Third move at t=1040 (>= 33ms elapsed): sends accumulated delta
    nowSpy.mockReturnValue(1040);
    fireEvent.touchMove(grid, {
      touches: [{ clientX: 100, clientY: 110 }],
    });
    expect(socket().send).toHaveBeenCalledTimes(2);
    expect(socket().send).toHaveBeenLastCalledWith(JSON.stringify({ type: "scroll", rows: 3 }));
  });
});
