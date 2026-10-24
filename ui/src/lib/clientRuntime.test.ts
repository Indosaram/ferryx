import { afterEach, describe, expect, it, vi } from "vitest";
import { isDesktopShell, isIosCompanion, isMobileCompanion } from "./clientRuntime";

afterEach(() => vi.unstubAllEnvs());

describe("client runtime routing", () => {
  it("routes the native phone shell to remote, even with native internals", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "1");
    expect(isMobileCompanion()).toBe(true);
    expect(isDesktopShell(true)).toBe(false);
    expect(isDesktopShell(false)).toBe(false);
  });
  it("keeps desktop native and browser remote", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", undefined);
    expect(isMobileCompanion()).toBe(false);
    expect(isDesktopShell(true)).toBe(true);
    expect(isDesktopShell(false)).toBe(false);
  });
  it("does not enable mobile for an arbitrary truthy value", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "false");
    expect(isDesktopShell(true)).toBe(true);
  });
});

const IPHONE_UA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15";

describe("isIosCompanion", () => {
  afterEach(() => vi.restoreAllMocks());
  it("is true for the mobile companion on an iPhone", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "1");
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE_UA);
    expect(isIosCompanion()).toBe(true);
  });
  it("is false for Safari on an iPhone outside the companion", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "");
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE_UA);
    expect(isIosCompanion()).toBe(false);
  });
  it("is false for the Android companion", () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "1");
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Linux; Android 15; Pixel 9)");
    expect(isIosCompanion()).toBe(false);
  });
});
