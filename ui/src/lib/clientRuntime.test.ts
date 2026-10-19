import { afterEach, describe, expect, it, vi } from "vitest";
import { isDesktopShell, isMobileCompanion } from "./clientRuntime";

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
