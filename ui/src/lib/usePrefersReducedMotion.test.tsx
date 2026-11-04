import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { prefersReducedMotion, usePrefersReducedMotion } from "./usePrefersReducedMotion";

type Listener = (event: MediaQueryListEvent) => void;

/** A controllable reduced-motion media query: `set` flips it and fires `change` like a browser. */
function installReducedMotion(initial: boolean) {
  let matches = initial;
  const listeners = new Set<Listener>();
  const queries: string[] = [];
  window.matchMedia = ((media: string) => {
    queries.push(media);
    return {
      media,
      get matches() {
        return media === "(prefers-reduced-motion: reduce)" && matches;
      },
      onchange: null,
      addListener: (listener: Listener) => listeners.add(listener),
      removeListener: (listener: Listener) => listeners.delete(listener),
      addEventListener: (_type: string, listener: Listener) => listeners.add(listener),
      removeEventListener: (_type: string, listener: Listener) => listeners.delete(listener),
      dispatchEvent: () => false,
    };
  }) as unknown as typeof window.matchMedia;
  return {
    listeners,
    queries,
    set(next: boolean) {
      matches = next;
      for (const listener of [...listeners]) listener({ matches: next } as MediaQueryListEvent);
    },
  };
}

function Probe() {
  return <span data-testid="motion">{usePrefersReducedMotion() ? "reduce" : "full"}</span>;
}

let originalMatchMedia: typeof window.matchMedia;

beforeEach(() => {
  originalMatchMedia = window.matchMedia;
});

afterEach(() => {
  cleanup();
  window.matchMedia = originalMatchMedia;
});

describe("usePrefersReducedMotion", () => {
  it("reads the reduce query for event handlers", () => {
    const media = installReducedMotion(true);
    expect(prefersReducedMotion()).toBe(true);
    media.set(false);
    expect(prefersReducedMotion()).toBe(false);
    expect(media.queries).toContain("(prefers-reduced-motion: reduce)");
  });

  it("is false where matchMedia does not exist", () => {
    // jsdom and old WebViews: no matchMedia means no stated preference.
    (window as { matchMedia?: unknown }).matchMedia = undefined;
    expect(prefersReducedMotion()).toBe(false);
    render(<Probe />);
    expect(screen.getByTestId("motion")).toHaveTextContent("full");
  });

  it("re-renders when the preference changes and unsubscribes on unmount", () => {
    const media = installReducedMotion(false);
    const { unmount } = render(<Probe />);
    expect(screen.getByTestId("motion")).toHaveTextContent("full");

    act(() => media.set(true));
    expect(screen.getByTestId("motion")).toHaveTextContent("reduce");

    act(() => media.set(false));
    expect(screen.getByTestId("motion")).toHaveTextContent("full");

    unmount();
    expect(media.listeners.size).toBe(0);
  });
});
