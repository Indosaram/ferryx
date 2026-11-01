import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { OVERLAY_HISTORY_KEY, PHONE_HISTORY_KEY, useOverlayHistory, usePhoneNavigation } from "./usePhoneNavigation";

function popTo(state: unknown) {
  act(() => {
    window.dispatchEvent(new PopStateEvent("popstate", { state }));
  });
}

afterEach(() => {
  window.history.replaceState(null, "");
});

describe("usePhoneNavigation", () => {
  it("starts on the Supervisor home and anchors that entry in history", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    expect(result.current.screen).toEqual({ kind: "home", destination: "supervisor" });
    expect(window.history.state?.[PHONE_HISTORY_KEY]).toEqual({ kind: "home", destination: "supervisor" });
  });

  it("replaces the entry on a tab switch and pushes one per opened session", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    const start = window.history.length;
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.selectTab("worktrees"));
    act(() => result.current.selectTab("inbox"));
    expect(window.history.length).toBe(start);
    expect(window.history.state?.[PHONE_HISTORY_KEY]).toEqual({ kind: "home", destination: "inbox" });
    act(() => result.current.openSession());
    expect(window.history.length).toBe(start + 1);
    expect(result.current.screen).toEqual({ kind: "session", from: "inbox" });
    expect(result.current.destination).toBe("inbox");
  });

  it("returns to the destination the session came from when the platform back fires", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    act(() => result.current.selectTab("worktrees"));
    const homeState = window.history.state;
    act(() => result.current.openSession());
    popTo(homeState);
    expect(result.current.screen).toEqual({ kind: "home", destination: "worktrees" });
  });

  it("treats an entry it did not write as the home of the open session", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.openSession());
    popTo(null);
    expect(result.current.screen).toEqual({ kind: "home", destination: "inbox" });
  });

  it("neither reads nor writes history while disabled", () => {
    window.history.replaceState({ other: 1 }, "");
    const start = window.history.length;
    const { result } = renderHook(() => usePhoneNavigation(false));
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.openSession());
    expect(window.history.length).toBe(start);
    expect(window.history.state).toEqual({ other: 1 });
    popTo({ [PHONE_HISTORY_KEY]: { kind: "home", destination: "worktrees" } });
    expect(result.current.screen).toEqual({ kind: "session", from: "inbox" });
  });

  it("keeps the tab across a layout flip and drops only the pushed session", () => {
    const { result, rerender } = renderHook(({ on }) => usePhoneNavigation(on), { initialProps: { on: true } });
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.openSession());
    rerender({ on: false });
    expect(result.current.screen).toEqual({ kind: "home", destination: "inbox" });
  });

  it("resets to the Supervisor default on reset (sign-out)", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.reset());
    expect(result.current.screen).toEqual({ kind: "home", destination: "supervisor" });
    expect(window.history.state?.[PHONE_HISTORY_KEY]).toEqual({ kind: "home", destination: "supervisor" });
  });
});

describe("useOverlayHistory", () => {
  it("pushes an entry while open and closes on the platform back gesture", () => {
    const onClose = vi.fn();
    const start = window.history.length;
    renderHook(({ open }) => useOverlayHistory(open, onClose), { initialProps: { open: true } });
    expect(window.history.length).toBe(start + 1);
    expect(window.history.state?.[OVERLAY_HISTORY_KEY]).toBeDefined();
    popTo(null);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the page's own history key on the overlay entry", () => {
    window.history.replaceState({ [PHONE_HISTORY_KEY]: { kind: "home", destination: "worktrees" } }, "");
    renderHook(() => useOverlayHistory(true, vi.fn()));
    expect(window.history.state?.[PHONE_HISTORY_KEY]).toEqual({ kind: "home", destination: "worktrees" });
  });
});
