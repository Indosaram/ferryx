import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useScreenWakeLock } from "./useScreenWakeLock";

describe("useScreenWakeLock", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reports a refused lock so the setting can say the screen may still turn off", async () => {
    vi.stubGlobal("navigator", { ...navigator, wakeLock: { request: vi.fn().mockRejectedValue(new Error("NotAllowedError")) } });
    const { result } = renderHook(() => useScreenWakeLock(true));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current).toBe("refused");
  });

  it("reports a held lock once the browser grants it", async () => {
    const sentinel = { released: false, release: vi.fn().mockResolvedValue(undefined), addEventListener: vi.fn() };
    vi.stubGlobal("navigator", { ...navigator, wakeLock: { request: vi.fn().mockResolvedValue(sentinel) } });
    const { result } = renderHook(() => useScreenWakeLock(true));
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current).toBe("held");
  });
});
