import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteTerminal } from "./RemoteTerminal";

class Socket {
  static OPEN = 1;
  static latest: Socket;
  readyState = 1;
  binaryType = "";
  send = vi.fn();
  close = vi.fn();
  onopen?: () => void;
  onmessage?: (event: MessageEvent) => void;
  constructor() { Socket.latest = this; }
}
const sink = () => screen.getByTestId("remote-terminal-input-sink") as HTMLTextAreaElement;
const grid = () => screen.getByTestId("remote-terminal-grid");
const touch = (y: number) => [{ clientX: 100, clientY: y }];
function pointerDown(pointerType: string) {
  const event = new Event("pointerdown", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerType", { value: pointerType });
  fireEvent(grid(), event);
  return event;
}
function frame(y: number) {
  act(() => Socket.latest.onmessage?.({ data: JSON.stringify({
    type: "grid", cols: 80, rows: 20,
    cursor: { x: 3, y, visible: true, blinking: false, wideTail: false, visualStyle: "block" },
    lines: [],
  }) } as MessageEvent));
}

describe("mobile terminal input lifecycle", () => {
  let resize: ResizeObserverCallback;
  let height: number;
  beforeEach(() => {
    height = 400;
    vi.stubGlobal("WebSocket", Socket);
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: true })));
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { resize = callback; }
      observe() {}
      disconnect() {}
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const cell = this.hasAttribute("data-terminal-cell-measure");
      return { width: cell ? 10 : 800, height: cell ? 20 : height } as DOMRect;
    });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("does not summon the keyboard on mobile mount, socket open, or tab change", () => {
    const view = render(<RemoteTerminal sessionId="a" token="token-a" activeTabId="a" />);
    expect(document.activeElement).not.toBe(sink());
    act(() => Socket.latest.onopen?.());
    expect(document.activeElement).not.toBe(sink());
    view.rerender(<RemoteTerminal sessionId="a" token="token-a" activeTabId="b" />);
    expect(document.activeElement).not.toBe(sink());
  });

  it("scrolls to the bottom without focusing, including compatibility mouse events", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    sink().blur();
    pointerDown("touch");
    fireEvent.touchStart(grid(), { touches: touch(200) });
    expect(document.activeElement).not.toBe(sink());
    fireEvent.touchMove(grid(), { touches: touch(120) });
    fireEvent.touchEnd(grid(), { touches: [], changedTouches: touch(120) });
    frame(19); // Server returns the bottom viewport with the prompt cursor visible.
    fireEvent.mouseDown(grid());
    fireEvent.click(grid());
    expect(Socket.latest.send).toHaveBeenCalledWith(JSON.stringify({ type: "scroll", rows: 4 }));
    expect(document.activeElement).not.toBe(sink());
    expect(grid().style.touchAction).toBe("none");
  });

  it("focuses a completed prompt tap and keeps the same stationary sink through keyboard resize and cursor output", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    sink().blur();
    const input = sink();
    pointerDown("touch");
    fireEvent.touchStart(grid(), { touches: touch(200) });
    expect(document.activeElement).not.toBe(input);
    fireEvent.touchEnd(grid(), { touches: [], changedTouches: touch(200) });
    expect(document.activeElement).toBe(input);
    // Model the browser default focus transfer, which jsdom does not perform.
    if (fireEvent.mouseDown(grid())) grid().focus();
    fireEvent.click(grid());
    act(() => Socket.latest.onopen?.());
    const transform = input.style.transform;
    height = 200;
    act(() => resize([], {} as ResizeObserver));
    frame(9);
    expect(sink()).toBe(input);
    expect(document.activeElement).toBe(input);
    expect(input.style.transform).toBe(transform);
    expect(Number.parseFloat(input.style.fontSize)).toBeGreaterThanOrEqual(16);
  });

  it("cancels touch without focusing and flushes a fast scroll's throttled tail on release", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    sink().blur();
    vi.spyOn(Date, "now").mockReturnValue(1000);
    fireEvent.touchStart(grid(), { touches: touch(200) });
    fireEvent.touchMove(grid(), { touches: touch(160) });
    fireEvent.touchMove(grid(), { touches: touch(100) });
    fireEvent.touchEnd(grid(), { touches: [], changedTouches: touch(100) });
    expect(Socket.latest.send.mock.calls.map(([data]) => JSON.parse(data).rows)).toEqual([2, 3]);
    fireEvent.touchStart(grid(), { touches: touch(200) });
    fireEvent.touchCancel(grid());
    fireEvent.touchEnd(grid(), { touches: [], changedTouches: touch(200) });
    expect(document.activeElement).not.toBe(sink());
  });

  it.each(["insertFromComposition", "insertText"])("sends Hangul once when %s follows compositionend", (inputType) => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    fireEvent.compositionStart(sink());
    fireEvent.input(sink(), { target: { value: "한" }, isComposing: true, inputType: "insertCompositionText" });
    expect(Socket.latest.send).not.toHaveBeenCalled();
    fireEvent.compositionEnd(sink(), { data: "한" });
    fireEvent.input(sink(), { target: { value: "한" }, data: "한", inputType });
    fireEvent.compositionStart(sink());
    fireEvent.input(sink(), { target: { value: "한" }, isComposing: true });
    fireEvent.compositionEnd(sink(), { data: "한" });
    expect(Socket.latest.send.mock.calls).toEqual([[new TextEncoder().encode("한")], [new TextEncoder().encode("한")]]);
  });

  it("does not emit canceled or blurred preedit as separated jamo", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    fireEvent.compositionStart(sink());
    fireEvent.input(sink(), { target: { value: "ㅎ" }, isComposing: true });
    fireEvent.compositionEnd(sink(), { data: "" });
    expect(Socket.latest.send).not.toHaveBeenCalled();
    fireEvent.compositionStart(sink());
    fireEvent.input(sink(), { target: { value: "ㄱ" }, isComposing: true });
    fireEvent.blur(sink());
    expect(Socket.latest.send).not.toHaveBeenCalled();
  });

  it("honors native composing input even before compositionstart and lets sink printable keydown reach the IME", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    expect(fireEvent.keyDown(sink(), { key: "g", code: "KeyG" })).toBe(true);
    expect(Socket.latest.send).not.toHaveBeenCalled();
    fireEvent.input(sink(), { target: { value: "ㅎ" }, isComposing: true, inputType: "insertCompositionText" });
    expect(Socket.latest.send).not.toHaveBeenCalled();
    fireEvent.compositionEnd(sink(), { data: "하" });
    expect(Socket.latest.send).toHaveBeenCalledWith(new TextEncoder().encode("하"));
  });
});
