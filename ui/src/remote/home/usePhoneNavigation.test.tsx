import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { PHONE_HISTORY_KEY, usePhoneNavigation } from "./usePhoneNavigation";

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

  it("pushes a history entry per tab switch and per opened session", () => {
    const { result } = renderHook(() => usePhoneNavigation(true));
    const start = window.history.length;
    act(() => result.current.selectTab("inbox"));
    act(() => result.current.openSession());
    expect(window.history.length).toBe(start + 2);
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

  it("resets to the Supervisor default when it is disabled again (sign-out)", () => {
    const { result, rerender } = renderHook(({ on }) => usePhoneNavigation(on), { initialProps: { on: true } });
    act(() => result.current.selectTab("inbox"));
    rerender({ on: false });
    expect(result.current.screen).toEqual({ kind: "home", destination: "supervisor" });
  });
});
