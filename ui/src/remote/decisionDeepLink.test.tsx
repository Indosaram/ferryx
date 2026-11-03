import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  DECISION_FOCUS_TTL_MS,
  DecisionFocusContext,
  backendSessionId,
  decisionForFocus,
  isSessionRef,
  parseDecisionLink,
  useDecisionDeepLink,
  useFocusedDecision,
  withoutDecisionParams,
} from "./decisionDeepLink";

const ORIGIN = "https://phone.example.test";

describe("parseDecisionLink", () => {
  it("reads the card from the exact link the gateway pushes", () => {
    expect(parseDecisionLink("/?panel=decision&session=mac%3As1", ORIGIN)).toEqual({ sessionRef: "mac:s1" });
    // A backend id may hold ':' itself: only the first colon splits host and session.
    expect(parseDecisionLink("/?panel=decision&session=win%3Aa%3Ab", ORIGIN)).toEqual({ sessionRef: "win:a:b" });
  });

  it("opens the panel alone when no valid session is named, never guessing a card", () => {
    expect(parseDecisionLink("/?panel=decision", ORIGIN)).toEqual({ sessionRef: null });
    for (const bad of ["", "nocolon", ":s1", "mac:"]) {
      expect(parseDecisionLink(`/?panel=decision&session=${encodeURIComponent(bad)}`, ORIGIN), bad).toEqual({ sessionRef: null });
    }
  });

  it("ignores anything that is not a decision link for this origin", () => {
    expect(parseDecisionLink("/?panel=inbox&session=mac%3As1", ORIGIN)).toBeNull();
    expect(parseDecisionLink("/?session=mac%3As1", ORIGIN)).toBeNull();
    expect(parseDecisionLink("https://evil.example/?panel=decision&session=mac%3As1", ORIGIN)).toBeNull();
    expect(parseDecisionLink("//evil.example/?panel=decision", ORIGIN)).toBeNull();
    expect(parseDecisionLink("http://[bad", ORIGIN)).toBeNull();
  });

  it("splits session refs the way the hub does", () => {
    expect(isSessionRef("mac:s1")).toBe(true);
    expect(isSessionRef("mac:")).toBe(false);
    expect(isSessionRef(":s1")).toBe(false);
    expect(isSessionRef("s1")).toBe(false);
    expect(backendSessionId("win:a:b")).toBe("a:b");
    expect(backendSessionId("nocolon")).toBeNull();
  });

  it("strips only the link parameters from the address bar", () => {
    expect(withoutDecisionParams("?panel=decision&session=mac%3As1&code=1")).toBe("?code=1");
    expect(withoutDecisionParams("?panel=decision&session=mac%3As1")).toBe("");
  });
});

describe("decisionForFocus", () => {
  it("matches the exact sessionRef and never a neighbouring card", () => {
    const cards = ["mac:s1", "mac:s10", "win:s1"];
    expect(decisionForFocus({ sessionRef: "mac:s1", seq: 1 }, cards)).toBe("mac:s1");
    expect(decisionForFocus({ sessionRef: "mac:s", seq: 1 }, cards)).toBeNull();
    expect(decisionForFocus({ sessionRef: "linux:s1", seq: 1 }, cards)).toBeNull();
    expect(decisionForFocus({ sessionRef: null, seq: 1 }, cards)).toBeNull();
    expect(decisionForFocus(null, cards)).toBeNull();
  });
});

type Listener = (event: MessageEvent) => void;

function installWorker() {
  const listeners = new Set<Listener>();
  const worker = {
    addEventListener: (_: string, listener: Listener) => listeners.add(listener),
    removeEventListener: (_: string, listener: Listener) => listeners.delete(listener),
  };
  Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: worker });
  return {
    post: (data: unknown) => act(() => listeners.forEach((listener) => listener({ data } as MessageEvent))),
    count: () => listeners.size,
  };
}

describe("useDecisionDeepLink", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    window.history.replaceState(null, "", "/");
  });
  afterEach(() => {
    vi.useRealTimers();
    Reflect.deleteProperty(navigator, "serviceWorker");
    window.history.replaceState(null, "", "/");
  });

  it("focuses the card a cold start opened on, reports it once and strips the link", () => {
    window.history.replaceState(null, "", "/?panel=decision&session=mac%3As1&keep=1#h");
    const onLink = vi.fn();
    installWorker();
    const { result } = renderHook(() => useDecisionDeepLink(onLink));
    expect(result.current.focus).toEqual({ sessionRef: "mac:s1", seq: 1 });
    expect(onLink).toHaveBeenCalledTimes(1);
    expect(onLink).toHaveBeenCalledWith({ sessionRef: "mac:s1", seq: 1 });
    expect(window.location.search).toBe("?keep=1");
    expect(window.location.hash).toBe("#h");
  });

  it("does nothing on a normal start", () => {
    const onLink = vi.fn();
    installWorker();
    const { result } = renderHook(() => useDecisionDeepLink(onLink));
    expect(result.current.focus).toBeNull();
    expect(onLink).not.toHaveBeenCalled();
  });

  it("follows ferryx:open-url from a warm window, newest link wins, same card twice focuses twice", () => {
    const worker = installWorker();
    const onLink = vi.fn();
    const { result } = renderHook(() => useDecisionDeepLink(onLink));
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As1" });
    expect(result.current.focus).toEqual({ sessionRef: "mac:s1", seq: 1 });
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=win%3As2" });
    expect(result.current.focus).toEqual({ sessionRef: "win:s2", seq: 2 });
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=win%3As2" });
    expect(result.current.focus?.seq).toBe(3);
    expect(onLink).toHaveBeenCalledTimes(3);
  });

  it("ignores foreign, malformed and other message types", () => {
    const worker = installWorker();
    const onLink = vi.fn();
    const { result } = renderHook(() => useDecisionDeepLink(onLink));
    for (const data of [
      { type: "ferryx:open-url", url: "https://evil.example/?panel=decision&session=mac%3As1" },
      { type: "ferryx:open-url", url: "/?panel=inbox" },
      { type: "ferryx:open-url", url: 5 },
      { type: "ferryx:open-session", sessionId: "s1" },
      null,
    ]) {
      worker.post(data);
    }
    expect(result.current.focus).toBeNull();
    expect(onLink).not.toHaveBeenCalled();
  });

  it("spends a focus on acknowledge, but only the focus it was acknowledged for", () => {
    const worker = installWorker();
    const { result } = renderHook(() => useDecisionDeepLink());
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As1" });
    const stale = result.current.focus!.seq;
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As2" });
    act(() => result.current.acknowledge(stale));
    expect(result.current.focus?.sessionRef).toBe("mac:s2");
    act(() => result.current.acknowledge(result.current.focus!.seq));
    expect(result.current.focus).toBeNull();
  });

  it("drops an unclaimed focus after the ttl and says so once, but not while signed out", () => {
    const worker = installWorker();
    const onExpire = vi.fn();
    const { result, rerender } = renderHook(({ armed }) => useDecisionDeepLink(undefined, onExpire, armed), {
      initialProps: { armed: false },
    });
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As1" });
    act(() => void vi.advanceTimersByTime(DECISION_FOCUS_TTL_MS * 3));
    expect(result.current.focus?.sessionRef, "signed out: the link waits for the login").toBe("mac:s1");
    expect(onExpire).not.toHaveBeenCalled();

    rerender({ armed: true });
    act(() => void vi.advanceTimersByTime(DECISION_FOCUS_TTL_MS - 1));
    expect(result.current.focus).not.toBeNull();
    act(() => void vi.advanceTimersByTime(1));
    expect(result.current.focus).toBeNull();
    expect(onExpire).toHaveBeenCalledTimes(1);
    expect(onExpire).toHaveBeenCalledWith({ sessionRef: "mac:s1", seq: 1 });
  });

  it("restarts the ttl for a newer link instead of expiring it early", () => {
    const worker = installWorker();
    const { result } = renderHook(() => useDecisionDeepLink());
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As1" });
    act(() => void vi.advanceTimersByTime(DECISION_FOCUS_TTL_MS - 1000));
    worker.post({ type: "ferryx:open-url", url: "/?panel=decision&session=mac%3As2" });
    act(() => void vi.advanceTimersByTime(2000));
    expect(result.current.focus?.sessionRef).toBe("mac:s2");
  });

  it("stops listening on unmount", () => {
    const worker = installWorker();
    const { unmount } = renderHook(() => useDecisionDeepLink());
    expect(worker.count()).toBe(1);
    unmount();
    expect(worker.count()).toBe(0);
  });
});

describe("useFocusedDecision", () => {
  function render(focus: { sessionRef: string | null; seq: number } | null, cards: string[], acknowledge = vi.fn()) {
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(DecisionFocusContext.Provider, { value: { focus, acknowledge } }, children);
    return { acknowledge, ...renderHook(() => useFocusedDecision(cards), { wrapper }) };
  }

  it("names the card of the exact session once it is on the panel, and spends the focus on done", () => {
    const { result, acknowledge } = render({ sessionRef: "mac:s1", seq: 4 }, ["win:s1", "mac:s1"]);
    expect(result.current?.sessionRef).toBe("mac:s1");
    expect(acknowledge).not.toHaveBeenCalled();
    act(() => result.current!.done());
    expect(acknowledge).toHaveBeenCalledWith(4);
  });

  it("names no card while its card is not there, and keeps the focus for it instead of spending it", () => {
    const { result, acknowledge } = render({ sessionRef: "mac:s1", seq: 4 }, ["win:s1", "mac:s10"]);
    expect(result.current).toBeNull();
    expect(acknowledge).not.toHaveBeenCalled();
  });

  it("spends a panel-only link as soon as the panel shows, without focusing any card", () => {
    const { result, acknowledge } = render({ sessionRef: null, seq: 2 }, ["mac:s1"]);
    expect(result.current).toBeNull();
    expect(acknowledge).toHaveBeenCalledWith(2);
  });

  it("does nothing without a focus", () => {
    const { result, acknowledge } = render(null, ["mac:s1"]);
    expect(result.current).toBeNull();
    expect(acknowledge).not.toHaveBeenCalled();
  });
});
