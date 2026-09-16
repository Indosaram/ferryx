import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane, resetNativeTerminalPaneForTest } from "./NativeTerminalPane";
import { Toaster, toast } from "./ui/sonner";

// Exercise the real active/session effect and DOM sink without native IPC.
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));

const session: TerminalSession = {
  id: "focus-owner", backendSessionId: "backend-a", cwd: "/repo",
  workspaceId: "ws", worktree: null, lifecycle: "working",
};

let nextFrame: number;
let frames: Map<number, FrameRequestCallback>;

beforeEach(() => {
  vi.useFakeTimers();
  resetNativeTerminalPaneForTest();
  nextFrame = 0;
  frames = new Map();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const id = ++nextFrame;
    frames.set(id, callback);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => { frames.delete(id); });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  resetNativeTerminalPaneForTest();
});

function flushFrames() {
  act(() => {
    const pending = [...frames.entries()];
    for (const [id, callback] of pending) {
      if (!frames.delete(id)) continue;
      callback(16);
    }
  });
}

function advanceFocusTimer() {
  act(() => { vi.advanceTimersByTime(40); });
}

function pane(backendSessionId: string, active: boolean) {
  return <>
    <button data-testid="other-focus-owner">Other focus owner</button>
    <NativeTerminalPane sessionId={session.id} session={{ ...session, backendSessionId }} active={active} />
  </>;
}

function mountActiveA() {
  const view = render(pane("backend-a", true));
  const sink = screen.getByTestId("native-terminal-focus-sink");
  const other = screen.getByTestId("other-focus-owner");
  expect(document.activeElement).toBe(sink);
  // Drain A's retries before testing B; frame scheduling is independent of time.
  flushFrames();
  advanceFocusTimer();
  expect(frames.size).toBe(0);
  other.focus();
  expect(document.activeElement).toBe(other);
  return { ...view, sink, other };
}

describe("NativeTerminalPane active non-null session focus ownership", () => {
  it("claims focus independently immediately, on frame, and at 40ms after active A becomes active B", () => {
    const { rerender, sink, other } = mountActiveA();
    // Spy delegates to the real DOM method: each phase must actually claim focus.
    const focus = vi.spyOn(sink, "focus");
    rerender(pane("backend-b", true));
    expect(screen.getByTestId("native-terminal-focus-sink")).toBe(sink);
    expect(focus).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(sink);

    other.focus();
    focus.mockClear();
    expect(frames.size).toBe(1);
    flushFrames();
    expect(focus).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(sink);

    other.focus();
    focus.mockClear();
    act(() => { vi.advanceTimersByTime(39); });
    expect(focus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(other);
    act(() => { vi.advanceTimersByTime(1); });
    expect(focus).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(sink);
    focus.mockRestore();
  });

  it.each(["frame", "timer"] as const)("cancels B's pending %s focus when B becomes inactive before deferred execution", (phase) => {
    const { rerender, sink, other } = mountActiveA();
    rerender(pane("backend-b", true));
    expect(document.activeElement).toBe(sink);
    expect(frames.size).toBe(1);
    const focus = vi.spyOn(sink, "focus");

    // For the timer case, prove B's frame works first, then deactivate before
    // the independent 40ms timer. For the frame case neither callback has run.
    if (phase === "timer") {
      other.focus();
      flushFrames();
      expect(focus).toHaveBeenCalledTimes(1);
      expect(document.activeElement).toBe(sink);
    }
    rerender(pane("backend-b", false));
    other.focus();
    focus.mockClear();
    expect(frames.size).toBe(0);
    if (phase === "frame") flushFrames();
    else advanceFocusTimer();
    expect(focus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(other);
    focus.mockRestore();
  });

  it("retains input enabled and interactive state on NativeTerminalPane when an error toast is displayed", async () => {
    render(
      <>
        <Toaster />
        <NativeTerminalPane sessionId={session.id} session={{ ...session, backendSessionId: "backend-a" }} active={true} />
      </>
    );
    const paneEl = screen.getByTestId("native-terminal-pane");
    expect(paneEl.getAttribute("data-native-terminal-input-enabled")).toBe("true");

    await act(async () => {
      toast.error("Persistent error occurred", { id: "test-error-toast", duration: Infinity });
      await vi.runAllTimersAsync();
    });

    // Error toast is mounted in the DOM
    expect(document.querySelector("[data-sonner-toast]")).not.toBeNull();

    // Terminal pane input MUST remain enabled so typing is never blocked
    expect(paneEl.getAttribute("data-native-terminal-input-enabled")).toBe("true");

    const closeButton = document.querySelector<HTMLButtonElement>("[data-sonner-toast] [data-close-button]");
    if (closeButton) {
      await act(async () => {
        closeButton.click();
        flushFrames();
        await vi.runAllTimersAsync();
        flushFrames();
      });
    }
    expect(paneEl.getAttribute("data-native-terminal-input-enabled")).toBe("true");
  });
});
