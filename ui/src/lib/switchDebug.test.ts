import { describe, expect, it, vi } from "vitest";

import {
  createSwitchDebugLogger,
  resolveSwitchDebugEnabled,
} from "./switchDebug";

describe("switchDebug", () => {
  it("emits ordered structured entries through the configured sink", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: true,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select", { from: "alpha", to: "beta" })).toEqual({
      runId: "run-1",
      sequence: 1,
      event: "project.select",
      wallTimeMs: 1234,
      details: { from: "alpha", to: "beta" },
    });
    expect(log("workspace.swap")).toMatchObject({ sequence: 2 });
    expect(sink).toHaveBeenCalledTimes(2);
  });

  it("does not invoke the sink when disabled", () => {
    const sink = vi.fn();
    const log = createSwitchDebugLogger({
      enabled: false,
      runId: "run-1",
      now: () => 1234,
      sink,
    });

    expect(log("project.select")).toBeNull();
    expect(sink).not.toHaveBeenCalled();
  });

  describe("resolveSwitchDebugEnabled", () => {
    it("enables tracing in a dev build", () => {
      expect(
        resolveSwitchDebugEnabled({ DEV: true, MODE: "development" }),
      ).toBe(true);
    });

    it("stays disabled in a plain production build", () => {
      expect(
        resolveSwitchDebugEnabled({ DEV: false, MODE: "production" }),
      ).toBe(false);
    });

    it("opts a production build in when VITE_SWITCH_DEBUG is 1", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: false,
          MODE: "production",
          VITE_SWITCH_DEBUG: "1",
        }),
      ).toBe(true);
    });

    it("never traces under the test runner even when opted in", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: true,
          MODE: "test",
          VITE_SWITCH_DEBUG: "1",
        }),
      ).toBe(false);
    });

    it("ignores a VITE_SWITCH_DEBUG value that is not exactly 1", () => {
      expect(
        resolveSwitchDebugEnabled({
          DEV: false,
          MODE: "production",
          VITE_SWITCH_DEBUG: "true",
        }),
      ).toBe(false);
    });
  });
});
