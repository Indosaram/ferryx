import { afterEach, describe, expect, it, vi } from "vitest";
import { haptic } from "./haptics";

describe("haptic", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("does nothing where the browser has no vibrate", () => {
    vi.stubGlobal("navigator", { ...navigator, vibrate: undefined });
    expect(() => haptic("selection")).not.toThrow();
  });

  it("swallows a vibrate that throws", () => {
    const vibrate = vi.fn(() => {
      throw new Error("NotAllowedError");
    });
    vi.stubGlobal("navigator", { ...navigator, vibrate });
    expect(() => haptic("warning")).not.toThrow();
    expect(vibrate).toHaveBeenCalledTimes(1);
  });

  it("vibrates with the kind's pattern", () => {
    const vibrate = vi.fn(() => true);
    vi.stubGlobal("navigator", { ...navigator, vibrate });
    haptic("selection");
    haptic("success");
    haptic("warning");
    expect(vibrate.mock.calls).toEqual([[8], [[10, 40, 10]], [20]]);
  });
});
