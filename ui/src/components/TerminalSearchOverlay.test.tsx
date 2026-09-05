import type { NativeTerminalSearchResult } from "./TerminalSearchOverlay";
import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.HTMLInputElement = dom.window.HTMLInputElement;
  globalThis.HTMLTextAreaElement = dom.window.HTMLTextAreaElement;
  globalThis.MutationObserver = dom.window.MutationObserver;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.dispatchEvent = dom.window.dispatchEvent.bind(dom.window);
  globalThis.addEventListener = dom.window.addEventListener.bind(dom.window);
  globalThis.removeEventListener = dom.window.removeEventListener.bind(dom.window);
}

const { act, cleanup, fireEvent, render, screen, waitFor } = await import("@testing-library/react");
const { afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("../test/setup");

const { TerminalSearchOverlay } = await import("./TerminalSearchOverlay");

const tauriInvoke = vi.fn<(cmd: string, args?: any) => Promise<any>>();
const tauriIsTauri = vi.fn(() => true);

const tauriCoreMocks = {
  invoke: tauriInvoke,
  isTauri: tauriIsTauri,
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: any) => tauriInvoke(cmd, args),
  isTauri: () => tauriIsTauri(),
}));

describe("TerminalSearchOverlay native session search", () => {
  beforeEach(() => {
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders search input, navigation buttons, and close button", () => {
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    expect(screen.getByRole("search")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-input")).toBeInTheDocument();
    expect(screen.getByLabelText("Previous match")).toBeInTheDocument();
    expect(screen.getByLabelText("Next match")).toBeInTheDocument();
    expect(screen.getByLabelText("Close search")).toBeInTheDocument();
  });

  it("triggers native search on query input and parses camelCase Rust DTO object", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      matches: [
        { row: 0, startCol: 5, endCol: 10 },
        { row: 1, startCol: 2, endCol: 7 },
      ],
      totalMatches: 2,
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "error" } });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_search", {
        sessionId: "backend-term-1",
        query: "error",
        caseSensitive: false,
      });
    });

    await waitFor(() => {
      expect(screen.getByText("1/2")).toBeInTheDocument();
    });
  });

  it("triggers native search on query input and displays match counter", async () => {
    tauriCoreMocks.invoke.mockResolvedValue([
      [0, 5, 10],
      [1, 2, 7],
    ]);
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "error" } });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_search", {
        sessionId: "backend-term-1",
        query: "error",
        caseSensitive: false,
      });
    });

    await waitFor(() => {
      expect(screen.getByText("1/2")).toBeInTheDocument();
    });
  });

  it("handles zero matches DTO object cleanly with 0/0", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      matches: [],
      totalMatches: 0,
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "notfound" } });

    await waitFor(() => {
      expect(screen.getByText("0/0")).toBeInTheDocument();
    });
  });

  it("handles invoke rejection gracefully by showing 0/0", async () => {
    tauriCoreMocks.invoke.mockRejectedValue(new Error("search error"));
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "bad" } });

    await waitFor(() => {
      expect(screen.getByText("0/0")).toBeInTheDocument();
    });
  });

  it("parses snake_case DTO matches gracefully", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      matches: [
        { row: 1, start_col: 0, end_col: 4 },
      ],
      total_matches: 1,
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "test" } });

    await waitFor(() => {
      expect(screen.getByText("1/1")).toBeInTheDocument();
    });
  });

  it("navigates matches with Enter, Shift+Enter, and arrow buttons", async () => {
    tauriCoreMocks.invoke.mockResolvedValue({
      matches: [
        { row: 0, startCol: 5, endCol: 10 },
        { row: 1, startCol: 2, endCol: 7 },
        { row: 3, startCol: 0, endCol: 5 },
      ],
      totalMatches: 3,
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "warn" } });

    await waitFor(() => {
      expect(screen.getByText("1/3")).toBeInTheDocument();
    });

    // Next match via Enter
    fireEvent.keyDown(input, { key: "Enter", shiftKey: false });
    expect(screen.getByText("2/3")).toBeInTheDocument();

    // Next match via button
    fireEvent.click(screen.getByLabelText("Next match"));
    expect(screen.getByText("3/3")).toBeInTheDocument();

    // Wrap around to first match
    fireEvent.click(screen.getByLabelText("Next match"));
    expect(screen.getByText("1/3")).toBeInTheDocument();

    // Previous match via Shift+Enter
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(screen.getByText("3/3")).toBeInTheDocument();

    // Previous match via button
    fireEvent.click(screen.getByLabelText("Previous match"));
    expect(screen.getByText("2/3")).toBeInTheDocument();
  });

  it("ignores an out-of-order stale search result and keeps the newest query's counts", async () => {
    type Deferred = { promise: Promise<NativeTerminalSearchResult>; resolve: (value: NativeTerminalSearchResult) => void; reject: (error: unknown) => void };
    const calls: Array<{ args: { query?: string; sessionId?: string } | undefined; d: Deferred }> = [];
    tauriCoreMocks.invoke.mockImplementation((_cmd: string, args?: { query?: string; sessionId?: string }) => {
      let resolve: (value: NativeTerminalSearchResult) => void = () => {};
      let reject: (error: unknown) => void = () => {};
      const promise = new Promise<NativeTerminalSearchResult>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      calls.push({ args, d: { promise, resolve, reject } });
      return promise;
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "err" } });
    fireEvent.change(input, { target: { value: "error" } });

    expect(calls).toHaveLength(2);
    expect(calls[0].args?.query).toBe("err");
    expect(calls[1].args?.query).toBe("error");

    // Newest request ("error") resolves first with 2 matches.
    await act(async () => {
      calls[1].d.resolve({
        matches: [
          { row: 0, startCol: 0, endCol: 3 },
          { row: 2, startCol: 1, endCol: 4 },
        ],
        totalMatches: 2,
      });
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();

    // Stale request ("err") resolves late with 5 matches; must be ignored.
    await act(async () => {
      calls[0].d.resolve({
        matches: [0, 1, 2, 3, 4].map((row) => ({ row, startCol: 0, endCol: 3 })),
        totalMatches: 5,
      });
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();
    expect(screen.queryByText("1/5")).not.toBeInTheDocument();
  });

  it("does not let a stale rejection overwrite a newer successful result", async () => {
    type Deferred = { promise: Promise<NativeTerminalSearchResult>; resolve: (value: NativeTerminalSearchResult) => void; reject: (error: unknown) => void };
    const calls: Array<{ args: { query?: string; sessionId?: string } | undefined; d: Deferred }> = [];
    tauriCoreMocks.invoke.mockImplementation((_cmd: string, args?: { query?: string; sessionId?: string }) => {
      let resolve: (value: NativeTerminalSearchResult) => void = () => {};
      let reject: (error: unknown) => void = () => {};
      const promise = new Promise<NativeTerminalSearchResult>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      calls.push({ args, d: { promise, resolve, reject } });
      return promise;
    });
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "err" } });
    fireEvent.change(input, { target: { value: "error" } });
    expect(calls).toHaveLength(2);

    await act(async () => {
      calls[1].d.resolve({
        matches: [
          { row: 0, startCol: 0, endCol: 3 },
          { row: 2, startCol: 1, endCol: 4 },
        ],
        totalMatches: 2,
      });
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();

    await act(async () => {
      calls[0].d.reject(new Error("stale failure"));
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();
    expect(screen.queryByText("0/0")).not.toBeInTheDocument();
  });

  it("resets stale counts and re-searches the new session on sessionId change", async () => {
    type Deferred = { promise: Promise<NativeTerminalSearchResult>; resolve: (value: NativeTerminalSearchResult) => void; reject: (error: unknown) => void };
    const calls: Array<{ args: { query?: string; sessionId?: string } | undefined; d: Deferred }> = [];
    tauriCoreMocks.invoke.mockImplementation((_cmd: string, args?: { query?: string; sessionId?: string }) => {
      let resolve: (value: NativeTerminalSearchResult) => void = () => {};
      let reject: (error: unknown) => void = () => {};
      const promise = new Promise<NativeTerminalSearchResult>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      calls.push({ args, d: { promise, resolve, reject } });
      return promise;
    });
    const onClose = vi.fn();
    const { rerender } = render(
      <TerminalSearchOverlay sessionId="session-a" onClose={onClose} />,
    );

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "err" } });
    expect(calls).toHaveLength(1);
    expect(calls[0].args?.sessionId).toBe("session-a");

    await act(async () => {
      calls[0].d.resolve({
        matches: [
          { row: 0, startCol: 0, endCol: 3 },
          { row: 1, startCol: 0, endCol: 3 },
        ],
        totalMatches: 2,
      });
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();

    // Switch to a different backend session.
    await act(async () => {
      rerender(<TerminalSearchOverlay sessionId="session-b" onClose={onClose} />);
    });

    // Stale count from session-a must be gone immediately.
    expect(screen.queryByText("1/2")).not.toBeInTheDocument();
    // A fresh search must be issued for session-b using the retained query.
    expect(calls).toHaveLength(2);
    expect(calls[1].args?.sessionId).toBe("session-b");
    expect(calls[1].args?.query).toBe("err");

    await act(async () => {
      calls[1].d.resolve({
        matches: [
          { row: 0, startCol: 0, endCol: 3 },
          { row: 1, startCol: 0, endCol: 3 },
          { row: 2, startCol: 0, endCol: 3 },
        ],
        totalMatches: 3,
      });
    });
    expect(screen.getByText("1/3")).toBeInTheDocument();
  });

  it("closes on Escape and close button, triggering onFocusTerminal", () => {
    const onClose = vi.fn();
    const onFocusTerminal = vi.fn();
    const { unmount } = render(
      <TerminalSearchOverlay
        sessionId="backend-term-1"
        onClose={onClose}
        onFocusTerminal={onFocusTerminal}
      />,
    );

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onFocusTerminal).toHaveBeenCalledTimes(1);

    unmount();
    const onClose2 = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose2} />);

    fireEvent.click(screen.getByLabelText("Close search"));
    expect(onClose2).toHaveBeenCalledTimes(1);
  });
});
