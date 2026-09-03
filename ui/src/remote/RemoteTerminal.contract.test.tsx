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

    // 3rd socket successfully opens -> resets backoff!
    act(() => {
      thirdSocket.onopen?.();
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

  it("does not shatter IME jamo keydowns into individual PTY writes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

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
    const sink = screen.getByTestId("remote-terminal-input-sink");

    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    fireEvent.compositionStart(sink);
    fireEvent.input(sink, { target: { value: "ㄱ" } });
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("ㄱ");
    fireEvent.compositionEnd(sink, { data: "ㄱ" });
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("ㄱ"));
  });

  it("sends non-composing sink input as text without keydown duplication", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
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

    // Small negative deltaY (< 20) falls back to sign-only (-1)
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
});
