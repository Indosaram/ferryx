import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RemoteTerminal } from "./RemoteTerminal";

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

describe("remote terminal grid contract", () => {
  beforeEach(() => {
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
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("includes viewport geometry in the initial grid socket request", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    render(<RemoteTerminal sessionId="session-123" token="token-abc" title="Remote Shell" />);

    expect(screen.getByText("Remote Shell")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");
    expect(socket().url).toMatch(/\/api\/v1\/terminal\/session-123\?token=token-abc&render=grid&cols=80&rows=20$/);

    act(() => socket().onopen?.());
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Live"));
    expect(socket().send).not.toHaveBeenCalled();
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

  it("preserves control modifiers for physical navigation keys", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "ArrowUp", ctrlKey: true });

    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001b[1;5A"));
  });

  it("keeps MobileKeyDock modified navigation wiring", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const buttons = screen.getAllByRole("button");
    fireEvent.click(buttons[0]);
    fireEvent.click(buttons[5]);

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
});
