import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { runPhoneTransition } from "./phoneTransition";

// TS's DOM lib already declares startViewTransition; jsdom does not implement it.
const doc = document as unknown as { startViewTransition?: unknown };

let originalMatchMedia: typeof window.matchMedia;

beforeEach(() => {
  originalMatchMedia = window.matchMedia;
});

function mockReducedMotion(reduce: boolean) {
  window.matchMedia = ((media: string) => ({
    media,
    matches: reduce && media.includes("reduce"),
    onchange: null,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

afterEach(() => {
  delete doc.startViewTransition;
  window.matchMedia = originalMatchMedia;
  delete document.documentElement.dataset.phoneTransition;
});

describe("runPhoneTransition", () => {
  it("runs the update synchronously where view transitions are missing", () => {
    const update = vi.fn();
    runPhoneTransition("push", update);
    expect(update).toHaveBeenCalledTimes(1);
    expect(document.documentElement.dataset.phoneTransition).toBeUndefined();
  });

  it("skips the view transition when reduced motion is requested", () => {
    mockReducedMotion(true);
    const start = vi.fn();
    doc.startViewTransition = start;
    const update = vi.fn();
    runPhoneTransition("pop", update);
    expect(start).not.toHaveBeenCalled();
    expect(update).toHaveBeenCalledTimes(1);
  });

  it("runs the update inside a view transition and clears the direction once it finishes", async () => {
    mockReducedMotion(false);
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const start = vi.fn((callback: () => void) => {
      callback();
      return { finished, ready: Promise.resolve() };
    });
    doc.startViewTransition = start;
    const update = vi.fn();
    runPhoneTransition("push", update);
    expect(start).toHaveBeenCalledTimes(1);
    expect(update).toHaveBeenCalledTimes(1);
    expect(document.documentElement.dataset.phoneTransition).toBe("push");
    finish();
    await finished;
    await Promise.resolve();
    expect(document.documentElement.dataset.phoneTransition).toBeUndefined();
  });
});
