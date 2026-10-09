import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteTerminal } from "./RemoteTerminal";

class Socket {
  static OPEN = 1;
  static latest: Socket;
  url?: string;
  readyState = 1;
  binaryType = "";
  send = vi.fn();
  close = vi.fn();
  onopen?: () => void;
  onmessage?: (event: MessageEvent) => void;
  constructor(url?: string) {
    this.url = url;
    Socket.latest = this;
  }
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

  it("sends resize on ResizeObserver change and remoteResize on remoteStatus generation when followHostSize is false", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" />);
    expect(Socket.latest.url).toContain("cols=80&rows=20");
    act(() => Socket.latest.onopen?.());
    expect(Socket.latest.send).not.toHaveBeenCalled();

    height = 200;
    act(() => resize([], {} as ResizeObserver));
    expect(Socket.latest.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "resize", cols: 80, rows: 10 })
    );

    act(() => {
      Socket.latest.onmessage?.({
        data: JSON.stringify({ type: "remoteStatus", generation: "gen-1" }),
      } as MessageEvent);
    });
    expect(Socket.latest.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "remoteResize", generation: "gen-1", cols: 80, rows: 10 })
    );
  });

  it("never puts cols/rows in socket URL and never sends resize or remoteResize when followHostSize is true", () => {
    render(<RemoteTerminal sessionId="a" token="token-a" followHostSize={true} />);
    expect(Socket.latest.url).toMatch(/\/api\/v1\/terminal\/a\?token=token-a&render=grid$/);
    expect(Socket.latest.url).not.toContain("cols=");
    expect(Socket.latest.url).not.toContain("rows=");

    act(() => Socket.latest.onopen?.());
    expect(Socket.latest.send).not.toHaveBeenCalled();

    height = 200;
    act(() => resize([], {} as ResizeObserver));
    expect(Socket.latest.send).not.toHaveBeenCalled();

    act(() => {
      Socket.latest.onmessage?.({
        data: JSON.stringify({ type: "remoteStatus", generation: "gen-1" }),
      } as MessageEvent);
    });
    expect(Socket.latest.send).not.toHaveBeenCalled();

    expect(grid().className).toContain("overflow-x-auto");
    expect(grid().className).toContain("overflow-y-hidden");
    expect(grid().style.touchAction).toBe("pan-x");
  });

  describe("direct / line input mode", () => {
    const lineInput = () => screen.queryByTestId("remote-terminal-line-input");
    const modeToggle = () => screen.queryByTestId("remote-terminal-input-mode-toggle");
    const decodeSends = () =>
      Socket.latest.send.mock.calls.map(([data]) => new TextDecoder().decode(data as Uint8Array));

    /**
     * Switch modes the way a user does: press first (arming the guard that keeps a
     * mode switch from flushing the editor into the PTY), optionally deliver an
     * in-flight blur, then click. A sinkBlur argument exercises exactly that guard.
     */
    const switchMode = (sinkBlur?: HTMLTextAreaElement) => {
      const button = modeToggle();
      expect(button).not.toBeNull();
      fireEvent.mouseDown(button!);
      if (sinkBlur) fireEvent.blur(sinkBlur);
      fireEvent.click(button!);
    };

    it("defaults to direct, exposes the mode switch, and focuses the line editor only after an explicit toggle", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      expect(modeToggle()?.dataset.mode).toBe("direct");
      expect(lineInput()).toBeNull();
      expect(Socket.latest.send).not.toHaveBeenCalled();

      switchMode();
      expect(modeToggle()?.dataset.mode).toBe("line");
      expect(lineInput()).not.toBeNull();
      // The toggle is an explicit tap: it may summon the editor (and thus the keyboard).
      expect(document.activeElement).toBe(lineInput());
      expect(Socket.latest.send).not.toHaveBeenCalled();
    });

    it("keeps the line draft out of the PTY until Enter, then writes it exactly once with a trailing CR", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      switchMode();
      const line = lineInput()!;

      fireEvent.change(line, { target: { value: "모바일" } });
      expect(line).toHaveValue("모바일");
      expect(Socket.latest.send).not.toHaveBeenCalled();

      fireEvent.keyDown(line, { key: "Enter", keyCode: 13 });
      expect(decodeSends()).toEqual(["모바일\r"]);
      expect(lineInput()).toHaveValue("");

      // An empty line's Enter is a plain Return: one bare CR, never a duplicate of the last line.
      fireEvent.keyDown(lineInput()!, { key: "Enter", keyCode: 13 });
      expect(decodeSends()).toEqual(["모바일\r", "\r"]);
    });

    it("never submits the line while IME composition is active and never dispatches on compositionEnd", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      switchMode();
      const line = lineInput()!;

      fireEvent.change(line, { target: { value: "안녕" } });
      fireEvent.compositionStart(line);
      fireEvent.keyDown(line, { key: "Enter", keyCode: 229 });
      fireEvent.compositionEnd(line, { data: "안녕" });
      // Composition end alone commits nothing: only Enter dispatches the line.
      expect(Socket.latest.send).not.toHaveBeenCalled();
      // A native isComposing Enter (IME candidate confirm) is equally inert.
      fireEvent.keyDown(line, { key: "Enter", keyCode: 13, isComposing: true });
      expect(Socket.latest.send).not.toHaveBeenCalled();

      fireEvent.keyDown(line, { key: "Enter", keyCode: 13 });
      expect(decodeSends()).toEqual(["안녕\r"]);
    });

    it("never dispatches either mode's pending text across a mode switch (draft isolation)", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      const input = sink();
      fireEvent.input(input, { target: { value: "한글" }, inputType: "insertText" });
      expect(Socket.latest.send).not.toHaveBeenCalled();
      expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("한글");

      // Switch with an in-flight blur: the guard must swallow it instead of flushing.
      switchMode(input);
      expect(Socket.latest.send).not.toHaveBeenCalled();
      expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("한글");
      expect(lineInput()).toHaveValue("");

      const line = lineInput()!;
      fireEvent.change(line, { target: { value: "모바일" } });
      fireEvent.blur(line);
      expect(Socket.latest.send).not.toHaveBeenCalled();

      // Round trip: both drafts survive back and forth with zero dispatch.
      switchMode();
      expect(Socket.latest.send).not.toHaveBeenCalled();
      switchMode();
      expect(Socket.latest.send).not.toHaveBeenCalled();
      expect(lineInput()).toHaveValue("모바일");
      expect(screen.getByTestId("remote-terminal-preedit")).toHaveTextContent("한글");
    });

    it("still flushes the direct pending tail on a blur outside a mode switch", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      const input = sink();
      fireEvent.input(input, { target: { value: "한글" }, inputType: "insertText" });
      expect(Socket.latest.send).not.toHaveBeenCalled();

      // One full round trip arms and then releases the guard: the next plain blur behaves as ever.
      switchMode();
      switchMode();
      fireEvent.blur(input);
      expect(Socket.latest.send).toHaveBeenCalledWith(new TextEncoder().encode("한글"));
      expect(screen.queryByTestId("remote-terminal-preedit")).toBeNull();
    });

    it("summons the line editor from an explicit grid tap in line mode without dispatching", () => {
      render(<RemoteTerminal sessionId="a" token="token-a" />);
      switchMode();
      const line = lineInput()!;
      // Start away from the field so the tap is what summons it.
      line.blur();
      expect(document.activeElement).not.toBe(line);
      pointerDown("touch");
      fireEvent.touchStart(grid(), { touches: touch(10) });
      fireEvent.touchEnd(grid(), { touches: [], changedTouches: touch(14) });
      expect(document.activeElement).toBe(line);
      expect(Socket.latest.send).not.toHaveBeenCalled();
    });

    it("clears the line draft on a session change without dispatching it", () => {
      const view = render(<RemoteTerminal sessionId="a" token="token-a" />);
      switchMode();
      fireEvent.change(lineInput()!, { target: { value: "모바일" } });
      expect(Socket.latest.send).not.toHaveBeenCalled();

      view.rerender(<RemoteTerminal sessionId="session-456" token="token-a" />);
      expect(Socket.latest.send).not.toHaveBeenCalled();
      expect(lineInput()).toHaveValue("");
    });
  });
});
