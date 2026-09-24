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

  it("wraps multiline paste in bracketed paste mode markers to prevent prompt splitting", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const target = surface();

    fireEvent.paste(target, {
      clipboardData: { getData: () => "line 1\r\nline 2\nline 3" },
    });

    expect(socket().send).toHaveBeenCalledWith(
      new TextEncoder().encode("\x1b[200~line 1\nline 2\nline 3\x1b[201~"),
    );
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
    // A commit made only of jamo is unfinished text: the syllable can still grow, so it stays in the
    // overlay instead of being shipped a jamo at a time (see the mobile IME suite).
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("ㄱ");
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.input(sink, { target: { value: " " } });
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("ㄱ "));
  });

  it("shows the build stamp of the client it is running", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    expect(screen.getByTestId("remote-terminal-build-stamp").textContent).not.toBe("");
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

    // Two negative half-row pixel events produce one older-content row.
    fireEvent.wheel(surface(), { deltaY: -10 });
    expect(socket().send).toHaveBeenCalledTimes(1);
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

  it.each([
    [1, 1, 7, 3],
    [1, -1, 7, -3],
    [1, 100, 7, 10],
    [1, -100, 7, -10],
    [2, 1, 7, 7],
    [2, -1, 7, -7],
    [2, 1, 24, 10],
    [2, -1, 24, -10],
  ])("normalizes wheel mode %i delta %i using frame rows %i to %i wire rows", (deltaMode, deltaY, frameRows, rows) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onSocketLifecycle = vi.fn();
    render(<RemoteTerminal sessionId="session-123" token="token-abc" onSocketLifecycle={onSocketLifecycle} />);
    const current = socket();
    if (!current.onopen || !current.onmessage) throw new Error("Expected socket event subscriptions");
    act(() => {
      current.onopen?.();
      current.onmessage?.(new MessageEvent("message", { data: JSON.stringify({
        type: "grid", cols: 80, rows: frameRows, cursor: hiddenCursor, lines: [],
      }) }));
    });
    expect(onSocketLifecycle).toHaveBeenCalledWith("session-123", "open");
    expect(surface().querySelectorAll("[data-grid-line]")).toHaveLength(frameRows);

    fireEvent.wheel(surface(), { deltaMode, deltaY });

    expect(current.send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows })]]);
  });

  it.each([1, -1])("accumulates fractional pixels in direction %i without zero or horizontal input consuming them", (sign) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    act(() => socket().onopen?.());

    fireEvent.wheel(surface(), { deltaY: sign * 9.5 });
    fireEvent.wheel(surface(), { deltaY: 0, deltaMode: 2 });
    fireEvent.wheel(surface(), { deltaX: 100, deltaY: 0, deltaMode: 1 });
    fireEvent.wheel(surface(), { deltaY: sign * 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: sign * 0.5 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: sign })]]);
    socket().send.mockClear();
    fireEvent.wheel(surface(), { deltaY: sign * 500 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: sign * 10 })]]);
    socket().send.mockClear();
    fireEvent.wheel(surface(), { deltaY: sign * 1 });
    expect(socket().send).not.toHaveBeenCalled();
  });

  it.each(["session", "socket", "visibility"])("discards fractional wheel remainder across a %s transition", (transition) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const view = render(<RemoteTerminal sessionId="session-a" token="token-abc" />);
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    socket().send.mockClear();

    if (transition === "session") {
      view.rerender(<RemoteTerminal sessionId="session-b" token="token-abc" />);
      act(() => socket().onopen?.());
    } else if (transition === "socket") {
      view.rerender(<RemoteTerminal sessionId="session-a" token="token-abc" transportUrl="http://127.0.0.1" />);
      act(() => socket().onopen?.());
    } else {
      const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      fireEvent(document, new Event("visibilitychange"));
      fireEvent.wheel(surface(), { deltaY: 10 });
      expect(socket().send).not.toHaveBeenCalled();
      visibility.mockReturnValue("visible");
      fireEvent(document, new Event("visibilitychange"));
    }

    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: 1 })]]);
  });

  it.each([0, 2, 3])("does not accumulate or send wheel input with socket readyState %i", (readyState) => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    socket().send.mockClear();
    socket().readyState = readyState;
    fireEvent.wheel(surface(), { deltaY: 10 });
    fireEvent.wheel(surface(), { deltaY: 1, deltaMode: 1 });
    expect(socket().send).not.toHaveBeenCalled();
    socket().readyState = MockWebSocket.OPEN;
    act(() => socket().onopen?.());
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send).not.toHaveBeenCalled();
    fireEvent.wheel(surface(), { deltaY: 10 });
    expect(socket().send.mock.calls).toEqual([[JSON.stringify({ type: "scroll", rows: 1 })]]);
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

  it("pastes literal text that collides with key-command names verbatim, not as key commands", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const target = surface();

    const literals = ["up", "tab", "delete", "ctrl-c", "alt-x", "ctrl-d"];
    for (const literal of literals) {
      socket().send.mockClear();
      fireEvent.paste(target, { clipboardData: { getData: () => literal } });
      expect(socket().send).toHaveBeenCalledTimes(1);
      expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(literal));
    }
  });

  it("pastes multiline text whose lines collide with key names as verbatim bracketed paste", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.paste(surface(), {
      clipboardData: { getData: () => "up\r\ntab\rdelete" },
    });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(
      new TextEncoder().encode("\x1b[200~up\ntab\ndelete\x1b[201~"),
    );
  });

  it("pastes Object.prototype key names verbatim without leaking inherited members", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const target = surface();

    for (const literal of ["toString", "constructor", "hasOwnProperty"]) {
      socket().send.mockClear();
      fireEvent.paste(target, { clipboardData: { getData: () => literal } });
      expect(socket().send).toHaveBeenCalledTimes(1);
      expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(literal));
    }
  });

  it("pastes non-ASCII unicode literals verbatim without control-byte mangling", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.paste(surface(), { clipboardData: { getData: () => "안녕 ㅊ" } });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("안녕 ㅊ"));
  });

  it("maps Korean hardware Ctrl+C (key jamo, code KeyC) to a structured interrupt", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "ㅊ", code: "KeyC", ctrlKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(
      JSON.stringify({ type: "signal", signal: "interrupt" }),
    );
  });

  it("maps Korean hardware Ctrl+D / Ctrl+L to physical ASCII control codes", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "ㅇ", code: "KeyD", ctrlKey: true });
    expect(socket().send).toHaveBeenNthCalledWith(1, new Uint8Array([4]));

    fireEvent.keyDown(surface(), { key: "ㅣ", code: "KeyL", ctrlKey: true });
    expect(socket().send).toHaveBeenNthCalledWith(2, new Uint8Array([12]));
  });

  it("lets hardware Ctrl+V fall through to the browser paste event instead of sending a control byte", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const event = fireEvent.keyDown(surface(), { key: "ㅍ", code: "KeyV", ctrlKey: true });

    expect(socket().send).not.toHaveBeenCalled();
    // Not prevented -> the browser's native paste event can still run.
    expect(event).toBe(true);
  });

  it("preserves Ctrl+Shift+C for browser copy instead of sending an interrupt", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    const event = fireEvent.keyDown(surface(), {
      key: "C",
      code: "KeyC",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(socket().send).not.toHaveBeenCalled();
    expect(event).toBe(true);
  });

  it("does not send CR for Enter while an IME composition is active", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);
    const sink = screen.getByTestId("remote-terminal-input-sink");

    fireEvent.compositionStart(sink);
    // Platform where nativeEvent.isComposing is not reflected on the keydown:
    // the ref must still guard Enter from emitting a premature carriage return.
    fireEvent.keyDown(surface(), { key: "Enter" });

    expect(socket().send).not.toHaveBeenCalled();
  });

  it("does not send CR for Enter reported with IME keyCode 229", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "Enter", keyCode: 229 });

    expect(socket().send).not.toHaveBeenCalled();
  });

  it("encodes hardware Alt+letter as an ESC-prefixed meta chord instead of dropping it", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "x", code: "KeyX", altKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bx"));
  });

  it("encodes Korean hardware Alt+letter via the physical code, not the remapped jamo", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "ㅌ", code: "KeyX", altKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bx"));
  });

  it("preserves Shift case for Alt+letter meta chords (Alt+Shift+X -> ESC X)", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "X", code: "KeyX", altKey: true, shiftKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001bX"));
  });

  it("hands AltGraph printable keys to the input sink so the glyph is emitted once", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    // getModifierState('AltGraph') is the authoritative printable-glyph signal.
    // jsdom ignores getModifierState passed via the init dict, so define it on a
    // real event. ctrlKey is false here (a valid AltGr-only report).
    const event = new KeyboardEvent("keydown", {
      key: "@",
      code: "Digit2",
      altKey: true,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(event, "getModifierState", {
      value: (mod: string) => mod === "AltGraph",
    });
    fireEvent(surface(), event);

    // No synthesized chord and no direct send: the focused sink's InputEvent owns
    // the glyph, and the keystroke is not canceled.
    expect(socket().send).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(screen.getByTestId("remote-terminal-input-sink"));
  });

  it("encodes Shift+Tab as a CSI back-tab sequence rather than a plain Tab", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    fireEvent.keyDown(surface(), { key: "Tab", shiftKey: true });

    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode("\u001b[Z"));
  });

  it("never wraps an unsupported Ctrl+digit chord into a junk control byte", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    render(<RemoteTerminal sessionId="session-123" token="token-abc" />);

    // Ctrl+9 has no ASCII control code; it must emit nothing, not a garbage byte.
    fireEvent.keyDown(surface(), { key: "9", code: "Digit9", ctrlKey: true });
    expect(socket().send).not.toHaveBeenCalled();

    // Ctrl+3 maps to ESC (0x1b) in the standard control table.
    fireEvent.keyDown(surface(), { key: "3", code: "Digit3", ctrlKey: true });
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new Uint8Array([27]));
  });

  it("observably rejects input and displays overflow status when outbound buffer limit is exceeded while connecting", () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    const onInputOverflow = vi.fn();
    render(
      <RemoteTerminal
        sessionId="session-123"
        token="token-abc"
        onInputOverflow={onInputOverflow}
      />,
    );

    // Socket is in CONNECTING state (readyState = 0, before onopen)
    socket().readyState = 0;

    // Fill buffer up to MAX_OUTBOUND_BUFFER_BYTES (4096 bytes)
    const chunk4096 = "x".repeat(4096);
    fireEvent.paste(surface(), { clipboardData: { getData: () => chunk4096 } });

    // Verify buffered indicator is displayed and overflow indicator is not displayed yet
    expect(screen.getByTestId("remote-terminal-buffered-indicator")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).not.toBeInTheDocument();
    expect(onInputOverflow).not.toHaveBeenCalled();

    // Drive RemoteTerminal's own queue past the bound with an extra keystroke
    fireEvent.keyDown(surface(), { key: "a" });

    // Assert that overflow is now observable via both UI indicator and callback
    expect(screen.getByTestId("remote-terminal-overflow-indicator")).toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal-overflow-indicator")).toHaveTextContent("Input overflow");
    expect(onInputOverflow).toHaveBeenCalledWith(1);

    // Assert that socket send was not called while connecting
    expect(socket().send).not.toHaveBeenCalled();

    // When the socket transitions to OPEN, only the buffered 4096 bytes are flushed; the overflowed byte was dropped
    socket().readyState = MockWebSocket.OPEN;
    act(() => socket().onopen?.());
    expect(socket().send).toHaveBeenCalledTimes(1);
    expect(socket().send).toHaveBeenCalledWith(new TextEncoder().encode(chunk4096));
    expect(screen.queryByTestId("remote-terminal-overflow-indicator")).not.toBeInTheDocument();
  });

  it("tracks generation from remoteStatus, emits generation-fenced remoteResize on status and resize, and resets on reconnect", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);
    let resizeObserverCallback: (() => void) | null = null;
    class MockResizeObserver {
      constructor(cb: () => void) {
        resizeObserverCallback = cb;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    render(<RemoteTerminal sessionId="session-ssh" token="token-abc" />);

    // Initial WebSocket request contains initial geometry 80x20
    const ws1 = socket();
    act(() => ws1.onopen?.());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Live"));

    // Server sends remoteStatus with generation "101"
    act(() => {
      ws1.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "101",
        }),
      } as MessageEvent);
    });

    // RemoteTerminal should immediately send generation-fenced remoteResize with the measured geometry (80x20)
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "101",
        cols: 80,
        rows: 20,
      }),
    );

    // Viewport resizes to 1000x500 (100 cols, 25 rows)
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(1000, 500);
      return rect(0, 0);
    });

    act(() => {
      resizeObserverCallback?.();
    });

    // Verify remoteResize is sent with the new geometry and generation 101
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "101",
        cols: 100,
        rows: 25,
      }),
    );

    // Generation updates to "102"
    act(() => {
      ws1.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "102",
        }),
      } as MessageEvent);
    });

    // Should immediately send remoteResize with generation "102" and latest measured geometry (100x25)
    expect(ws1.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteResize",
        generation: "102",
        cols: 100,
        rows: 25,
      }),
    );
  });

  it("routes typed input and ctrl-c dock key through generation-fenced remoteWrite on SSH sessions while preserving raw binary input on local sessions", async () => {
    vi.stubGlobal("WebSocket", MockWebSocket);

    const sshView = render(<RemoteTerminal sessionId="session-ssh" token="token-abc" />);
    const sshWs = socket();
    act(() => sshWs.onopen?.());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Live"));

    // Server sends remoteStatus with generation "202"
    act(() => {
      sshWs.onmessage?.({
        data: JSON.stringify({
          type: "remoteStatus",
          state: "connected",
          generation: "202",
        }),
      } as MessageEvent);
    });

    sshWs.send.mockClear();

    // Simulate typed character "a" on SSH session
    fireEvent.keyDown(surface(), { key: "a" });
    expect(sshWs.send).toHaveBeenCalledTimes(1);
    expect(sshWs.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteWrite",
        generation: "202",
        data: "a",
      }),
    );

    sshWs.send.mockClear();

    // Assert the ctrl-c dock key sends data: "\u0003" as remoteWrite in the same generation state
    fireEvent.click(screen.getByRole("button", { name: "Ctrl-C" }));
    expect(sshWs.send).toHaveBeenCalledTimes(1);
    expect(sshWs.send).toHaveBeenCalledWith(
      JSON.stringify({
        type: "remoteWrite",
        generation: "202",
        data: "\u0003",
      }),
    );

    sshView.unmount();

    // Without any remoteStatus (local session), typed character is still sent as raw bytes
    render(<RemoteTerminal sessionId="session-local" token="token-abc" />);
    const localWs = socket();
    act(() => localWs.onopen?.());
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Live"));

    localWs.send.mockClear();
    fireEvent.keyDown(surface(), { key: "a" });
    expect(localWs.send).toHaveBeenCalledTimes(1);
    expect(localWs.send).toHaveBeenCalledWith(new TextEncoder().encode("a"));
  });
});
