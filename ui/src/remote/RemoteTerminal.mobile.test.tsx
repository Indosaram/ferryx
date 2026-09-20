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

  // WKWebView's Korean IME (iOS/iPadOS) fires no composition events at all - it rewrites the sink
  // in place through plain input events whose isComposing is false (xtermjs/xterm.js#6084).
  it("holds the mutable Hangul tail of a composition-free IME instead of shipping lone jamo", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const rewrite = (value: string) => fireEvent.input(input, { target: { value }, inputType: "insertText" });

    rewrite("ㅎ");
    rewrite("하");
    rewrite("한");
    rewrite("한ㄱ"); // the next syllable began; 한 can still absorb this consonant
    rewrite("한그");
    rewrite("한글");
    expect(Socket.latest.send).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("한글");

    rewrite("한글 "); // a non-Hangul insertion settles the run
    expect(Socket.latest.send).toHaveBeenCalledTimes(1);
    expect(Socket.latest.send).toHaveBeenLastCalledWith(new TextEncoder().encode("한글 "));
    expect(sink()).toHaveValue("");
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
  });

  it("commits a held Hangul run before the key that ends the line", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const rewrite = (value: string) => fireEvent.input(input, { target: { value }, inputType: "insertText" });

    rewrite("ㅁ");
    rewrite("모");
    rewrite("모바일");
    expect(Socket.latest.send).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Enter" });
    expect(Socket.latest.send.mock.calls.map(([data]) => new TextDecoder().decode(data as Uint8Array)))
      .toEqual(["모바일", "\r"]);
    expect(sink()).toHaveValue("");
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
  });

  it("lets Backspace edit a pending IME tail instead of deleting echoed terminal content", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    fireEvent.input(input, { target: { value: "한" }, inputType: "insertText" });
    expect(Socket.latest.send).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Backspace" });
    expect(Socket.latest.send).not.toHaveBeenCalled();
    fireEvent.input(input, { target: { value: "하" }, inputType: "deleteContentBackward" });
    fireEvent.input(input, { target: { value: "" }, inputType: "deleteContentBackward" });
    expect(Socket.latest.send).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Backspace" });
    expect(Socket.latest.send).toHaveBeenCalledWith(new TextEncoder().encode("\u007f"));
  });

  it("keeps settled ASCII immediate and never forwards WebKit's U+00A0 space", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();

    fireEvent.input(input, { target: { value: "l" }, inputType: "insertText" });
    expect(Socket.latest.send).toHaveBeenLastCalledWith(new TextEncoder().encode("l"));
    expect(input).toHaveValue("");

    fireEvent.input(input, { target: { value: "cd\u00a0" }, inputType: "insertText" });
    expect(Socket.latest.send).toHaveBeenLastCalledWith(new TextEncoder().encode("cd "));
    expect(input).toHaveValue("");
  });

  // Android WebKit and Gboard commit one jamo per composition and clear the field between commits;
  // emitting each commit as it arrives is what shards 이렇게 into ㅇㅣㄹㅓㅎㄱㅔ in the PTY.
  it("reassembles jamo an IME commits one at a time into syllables", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const commitJamo = (jamo: string) => {
      fireEvent.compositionStart(input);
      fireEvent.input(input, { target: { value: jamo }, isComposing: true, inputType: "insertCompositionText" });
      fireEvent.compositionEnd(input, { data: jamo });
      fireEvent.input(input, { target: { value: jamo }, inputType: "insertFromComposition" });
    };

    for (const jamo of "ㅇㅣㄹㅓㅎㄱㅔ") commitJamo(jamo);
    expect(Socket.latest.send).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("이렇게");

    fireEvent.input(input, { target: { value: " " }, inputType: "insertText" });
    expect(Socket.latest.send.mock.calls).toEqual([[new TextEncoder().encode("이렇게 ")]]);
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
  });

  it("sends the reassembled jamo run before the key that ends the line", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const commitJamo = (jamo: string) => {
      fireEvent.compositionStart(input);
      fireEvent.input(input, { target: { value: jamo }, isComposing: true, inputType: "insertCompositionText" });
      fireEvent.compositionEnd(input, { data: jamo });
      fireEvent.input(input, { target: { value: jamo }, inputType: "insertFromComposition" });
    };

    for (const jamo of "ㅁㅗㅂㅏㅇㅣㄹ") commitJamo(jamo);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(Socket.latest.send.mock.calls.map(([data]) => new TextDecoder().decode(data as Uint8Array)))
      .toEqual(["모바일", "\r"]);
  });

  it("deletes one committed jamo per Backspace while the sink is empty", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const commitJamo = (jamo: string) => {
      fireEvent.compositionStart(input);
      fireEvent.input(input, { target: { value: jamo }, isComposing: true, inputType: "insertCompositionText" });
      fireEvent.compositionEnd(input, { data: jamo });
      fireEvent.input(input, { target: { value: jamo }, inputType: "insertFromComposition" });
    };

    for (const jamo of "ㅇㅣㄹ") commitJamo(jamo);
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("일");

    fireEvent.keyDown(input, { key: "Backspace" });
    expect(Socket.latest.send).not.toHaveBeenCalled();
    expect(input).toHaveValue("");
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("이");

    fireEvent.keyDown(input, { key: "Backspace" });
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    expect(Socket.latest.send).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Backspace" });
    expect(Socket.latest.send).toHaveBeenCalledWith(new TextEncoder().encode("\u007f"));
  });

  it("keeps repeated jamo in a held run instead of treating them as duplicates", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const commitJamo = (jamo: string, inputType = "insertFromComposition") => {
      fireEvent.compositionStart(input);
      fireEvent.input(input, { target: { value: jamo }, isComposing: true, inputType: "insertCompositionText" });
      fireEvent.compositionEnd(input, { data: jamo });
      fireEvent.input(input, { target: { value: jamo }, inputType });
    };

    commitJamo("ㅋ");
    commitJamo("ㅋ");
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("ㅋㅋ");

    fireEvent.input(input, { target: { value: " " }, inputType: "insertText" });
    expect(Socket.latest.send.mock.calls).toEqual([[new TextEncoder().encode("ㅋㅋ ")]]);
  });

  it("does not double a commit whose mirror insertion carries no input type", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const commitJamo = (jamo: string) => {
      fireEvent.compositionStart(input);
      fireEvent.input(input, { target: { value: jamo }, isComposing: true, inputType: "insertCompositionText" });
      fireEvent.compositionEnd(input, { data: jamo });
      // WebKit reports some mirrors with an empty input type; the commit is still already held.
      fireEvent.input(input, { target: { value: jamo }, inputType: "" });
    };

    for (const jamo of "ㅁㅗㅂㅏㅇㅣㄹ") commitJamo(jamo);
    expect(sink()).toHaveValue("");
    expect(Socket.latest.send).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Enter" });
    expect(Socket.latest.send.mock.calls.map(([data]) => new TextDecoder().decode(data as Uint8Array)))
      .toEqual(["모바일", "\r"]);
  });

  it("composes jamo a composition-free IME accumulates in the sink", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    const input = sink();
    const jamo = "ㅇㅣㄹㅓㅎㄱㅔ";
    for (let end = 1; end <= jamo.length; end += 1) {
      fireEvent.input(input, { target: { value: jamo.slice(0, end) }, inputType: "insertText" });
    }
    expect(Socket.latest.send).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("이렇게");

    fireEvent.input(input, { target: { value: `${jamo} ` }, inputType: "insertText" });
    expect(Socket.latest.send.mock.calls).toEqual([[new TextEncoder().encode("이렇게 ")]]);
    expect(sink()).toHaveValue("");
  });
});
